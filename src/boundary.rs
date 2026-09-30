//! A HATCH's boundary, placed where it is drawn: each boundary path as the
//! segments it runs through, in order.
//!
//! Everything a HATCH states is in its own plane (its extrusion and
//! elevation), so the whole boundary is placed through one map from that
//! plane to the world's XY. Straight edges and bulged polyline segments are
//! exact, and so is an arc edge: it becomes bulged segments of a quarter turn
//! or less. An elliptical or spline edge is cut into chords the way an
//! ELLIPSE or SPLINE entity is, and says how far the curve can be from them.

use crate::curve::{self, Curve};
use crate::geometry::{distance_to_segments, in_plane, shape_contains};
use crate::hit_test::NotSearchedReason;
use std::f64::consts::{FRAC_PI_2, TAU};
use uncad_model::bulge::{self, Segment};
use uncad_model::model::{
    EllipseEntity, HatchBoundaryPath, HatchEdge, HatchEntity, PolylineVertex, SplineEntity,
};
use uncad_model::{Affine2, Point2D, Point3D};

/// A HATCH's boundary paths, placed.
pub(crate) struct Boundary {
    /// Each path's segments, in the order the path runs.
    pub paths: Vec<Vec<Segment>>,
    /// How far the chords of an elliptical or spline edge can be from the
    /// curve; `None` when every edge is measured exactly.
    pub within: Option<f64>,
}

impl Boundary {
    /// Distance from `p` to the nearest segment of any path; `None` when no
    /// path has a segment.
    pub fn distance(&self, p: Point2D) -> Option<f64> {
        self.paths
            .iter()
            .filter_map(|path| distance_to_segments(p, path))
            .reduce(f64::min)
    }

    /// Whether `p` is inside the area the paths bound, by the even-odd rule
    /// across all of them -- the area a fill covers when islands alternate.
    pub fn contains(&self, p: Point2D) -> bool {
        self.paths
            .iter()
            .filter(|path| shape_contains(p, path))
            .count()
            % 2
            == 1
    }
}

/// The boundary of `h` placed through `t`, its chords good to `tolerance`.
/// Not placed when the hatch's plane is tilted out of the world's, or when
/// an arc would not stay an arc (`NonSimilarPlacement`); not measured when an
/// edge's curve is not defined (`CurveUndefined`) or not bounded
/// (`CurveBoundUnknown`) -- the rest of the boundary would be only part of
/// what is drawn.
pub(crate) fn hatch(
    h: &HatchEntity,
    t: &Affine2,
    tolerance: f64,
) -> Result<Boundary, NotSearchedReason> {
    let m = in_plane(h.extrusion, t).ok_or(NotSearchedReason::NonSimilarPlacement)?;
    let turning = turning(&m);
    let mut within: Option<f64> = None;
    let mut paths = Vec::new();
    for path in &h.boundary_paths {
        let segments = match path {
            HatchBoundaryPath::Polyline(vertices) => {
                if vertices.iter().any(|v| v.bulge != 0.0) && turning.is_none() {
                    return Err(NotSearchedReason::NonSimilarPlacement);
                }
                let placed: Vec<PolylineVertex> = vertices
                    .iter()
                    .map(|v| PolylineVertex {
                        point: m.apply(v.point),
                        bulge: v.bulge * turning.unwrap_or(1.0),
                        ..*v
                    })
                    .collect();
                bulge::segments(&placed, true).collect()
            }
            HatchBoundaryPath::Edges(edges) => {
                let mut segments = Vec::new();
                for edge in edges {
                    let (edge_segments, edge_within) =
                        edge_segments(h, edge, &m, turning, tolerance)?;
                    segments.extend(edge_segments);
                    if let Some(w) = edge_within {
                        within = Some(within.map_or(w, |v| v.max(w)));
                    }
                }
                segments
            }
        };
        paths.push(segments);
    }
    Ok(Boundary { paths, within })
}

/// `1` when `m` keeps a circle a circle turning the same way, `-1` when it
/// keeps it a circle but mirrors it, `None` when a circle would not stay
/// one.
fn turning(m: &Affine2) -> Option<f64> {
    if m.similarity_scale().is_some() {
        return Some(1.0);
    }
    let mirrored = Affine2 {
        a: -m.a,
        b: -m.b,
        ..*m
    };
    mirrored.similarity_scale().map(|_| -1.0)
}

/// Where an arc or elliptical-arc edge starts, counter-clockwise from +x,
/// and its signed sweep, in the direction the path runs. A
/// counter-clockwise edge runs from `start_angle` up to `end_angle`; the
/// format writes a clockwise edge's angles measured clockwise -- the
/// complements of the counter-clockwise ones -- so it starts at
/// `-start_angle` and runs down to `-end_angle`. Equal angles are the whole
/// turn.
fn edge_span(start_angle: f64, end_angle: f64, is_ccw: bool) -> (f64, f64) {
    if is_ccw {
        let mut sweep = end_angle - start_angle;
        if sweep <= 0.0 {
            sweep += TAU;
        }
        (start_angle, sweep)
    } else {
        let (from, to) = (-start_angle, -end_angle);
        let mut sweep = to - from;
        if sweep >= 0.0 {
            sweep -= TAU;
        }
        (from, sweep)
    }
}

fn straight(from: Point2D, to: Point2D) -> Segment {
    Segment {
        from,
        to,
        arc: None,
    }
}

fn in_3d(p: Point2D) -> Point3D {
    Point3D {
        x: p.x,
        y: p.y,
        z: 0.0,
    }
}

/// One edge's segments, placed through `m`, in the direction the path runs,
/// and how far its chords can be from its curve when it is cut into chords.
fn edge_segments(
    h: &HatchEntity,
    edge: &HatchEdge,
    m: &Affine2,
    turning: Option<f64>,
    tolerance: f64,
) -> Result<(Vec<Segment>, Option<f64>), NotSearchedReason> {
    match edge {
        HatchEdge::Line { start, end } => {
            Ok((vec![straight(m.apply(*start), m.apply(*end))], None))
        }
        HatchEdge::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            is_ccw,
        } => {
            let turning = turning.ok_or(NotSearchedReason::NonSimilarPlacement)?;
            let (from, sweep) = edge_span(*start_angle, *end_angle, *is_ccw);
            // Pieces of a quarter turn or less, each a bulged segment: a
            // bulge is the tangent of a quarter of its sweep, finite below a
            // whole turn.
            let pieces = ((sweep.abs() / FRAC_PI_2).ceil() as usize).max(1);
            let piece = sweep / pieces as f64;
            let bulge = if *radius > 0.0 {
                (piece / 4.0).tan() * turning
            } else {
                0.0
            };
            let vertices: Vec<PolylineVertex> = (0..=pieces)
                .map(|i| {
                    let a = from + piece * i as f64;
                    PolylineVertex {
                        bulge: if i < pieces { bulge } else { 0.0 },
                        ..PolylineVertex::straight(m.apply(Point2D {
                            x: center.x + radius * a.cos(),
                            y: center.y + radius * a.sin(),
                        }))
                    }
                })
                .collect();
            Ok((bulge::segments(&vertices, false).collect(), None))
        }
        HatchEdge::Ellipse {
            center,
            end,
            minor_major_ratio,
            start_angle,
            end_angle,
            is_ccw,
        } => {
            // The same ellipse as an ELLIPSE entity in the hatch's plane,
            // running counter-clockwise over the edge's span; a clockwise
            // edge's chords are then walked back so the path keeps its
            // direction.
            let (from, sweep) = edge_span(*start_angle, *end_angle, *is_ccw);
            let (start, end_parameter) = if sweep >= 0.0 {
                (from, from + sweep)
            } else {
                (from + sweep, from)
            };
            let ellipse = EllipseEntity {
                common: h.common.clone(),
                center: in_3d(*center),
                major_axis_endpoint: in_3d(*end),
                axis_ratio: *minor_major_ratio,
                start_angle: start,
                end_angle: end_parameter,
                extrusion: Point3D {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            };
            chord_segments(curve::ellipse(&ellipse, m, tolerance), sweep < 0.0)
        }
        HatchEdge::Spline {
            degree,
            rational,
            periodic,
            knots,
            control_points,
            weights,
            fit_points,
            start_tangent,
            end_tangent,
        } => {
            let spline = SplineEntity {
                common: h.common.clone(),
                degree: *degree,
                closed: None,
                periodic: Some(*periodic),
                knots: knots.clone(),
                weights: if *rational {
                    weights.clone()
                } else {
                    Vec::new()
                },
                fit_points: fit_points.iter().copied().map(in_3d).collect(),
                control_points: control_points.iter().copied().map(in_3d).collect(),
                start_tangent: start_tangent.map(in_3d),
                end_tangent: end_tangent.map(in_3d),
            };
            chord_segments(curve::spline(&spline, m, tolerance), false)
        }
    }
}

/// A curve's chords as straight segments, walked back when `reversed`.
fn chord_segments(
    curve: Curve,
    reversed: bool,
) -> Result<(Vec<Segment>, Option<f64>), NotSearchedReason> {
    match curve {
        Curve::Chords(c) => {
            let mut points = c.points;
            if reversed {
                points.reverse();
            }
            let segments = points.windows(2).map(|w| straight(w[0], w[1])).collect();
            Ok((segments, Some(c.within)))
        }
        Curve::Undefined => Err(NotSearchedReason::CurveUndefined),
        Curve::BoundUnknown => Err(NotSearchedReason::CurveBoundUnknown),
    }
}
