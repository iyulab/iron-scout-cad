//! A HATCH's boundary: each boundary path as the pieces it runs through, in
//! order, in the hatch's own plane -- then placed where it is drawn, to point
//! at, or taken to the world as points, for its extent.
//!
//! Everything a HATCH states is in its own plane (its extrusion and
//! elevation), so the whole boundary is placed through one map from that
//! plane to the world's XY. Straight edges and bulged polyline segments are
//! exact, and so is an arc edge: it becomes bulged segments of a quarter turn
//! or less. An elliptical or spline edge is the same curve as an ELLIPSE or
//! SPLINE entity, cut into chords to point at, and says how far the curve
//! can be from them.

use crate::curve::{self, Curve};
use crate::extent::{conic_reach, conic_through, linear};
use crate::geometry::{distance_to_segments, in_plane, shape_contains, xy};
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

/// One stretch of a boundary path, in the hatch's own plane.
enum Piece {
    /// Straight or bulged segments through `vertices` -- closed back to the
    /// first vertex for a polyline path.
    Run {
        vertices: Vec<PolylineVertex>,
        closed: bool,
    },
    /// An elliptical edge, as an ELLIPSE running counter-clockwise over the
    /// edge's span; `reversed` when the path runs it the other way.
    Ellipse {
        ellipse: EllipseEntity,
        reversed: bool,
    },
    /// A spline edge, as a SPLINE.
    Spline(SplineEntity),
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
    for path in pieces(h) {
        let mut segments = Vec::new();
        for piece in path {
            match piece {
                Piece::Run { vertices, closed } => {
                    let placed = place(&vertices, &m, turning)
                        .ok_or(NotSearchedReason::NonSimilarPlacement)?;
                    segments.extend(bulge::segments(&placed, closed));
                }
                Piece::Ellipse { ellipse, reversed } => {
                    let (chords, w) = chords(curve::ellipse(&ellipse, &m, tolerance), reversed)?;
                    segments.extend(chords);
                    within = Some(within.map_or(w, |v| v.max(w)));
                }
                Piece::Spline(spline) => {
                    let (chords, w) = chords(curve::spline(&spline, &m, tolerance), false)?;
                    segments.extend(chords);
                    within = Some(within.map_or(w, |v| v.max(w)));
                }
            }
        }
        paths.push(segments);
    }
    Ok(Boundary { paths, within })
}

/// Adds the points `h`'s boundary reaches, placed through `t`, to `out`: the
/// ends of its segments and arcs and where each arc and ellipse turns in x
/// or y -- exactly, under any placement -- and a spline's control points,
/// whose box holds the curve (or, for one the file does not define, the
/// points it passes through). `false` when it adds none -- a hatch on a
/// tilted plane has no exact place in the world's XY.
pub(crate) fn hatch_points(h: &HatchEntity, t: &Affine2, out: &mut Vec<Point2D>) -> bool {
    let before = out.len();
    let Some(m) = in_plane(h.extrusion, t) else {
        return false;
    };
    for piece in pieces(h).into_iter().flatten() {
        match piece {
            Piece::Run { vertices, closed } => {
                // Measured in the hatch's own plane, where each bulge turns
                // the way the file says; `m` takes the curve to the world.
                for s in bulge::segments(&vertices, closed) {
                    out.extend([m.apply(s.from), m.apply(s.to)]);
                    if let Some(arc) = &s.arc {
                        conic_through(&m, arc.center, arc.radius, arc.start_angle, arc.sweep, out);
                    }
                }
                if let [only] = vertices.as_slice() {
                    out.push(m.apply(only.point));
                }
            }
            Piece::Ellipse { ellipse, .. } => {
                if let Some(minor) = ellipse.minor_axis() {
                    conic_reach(
                        m.apply(xy(ellipse.center)),
                        linear(&m, xy(ellipse.major_axis_endpoint)),
                        linear(&m, xy(minor)),
                        ellipse.start_angle,
                        ellipse.sweep(),
                        out,
                    );
                }
            }
            Piece::Spline(spline) => {
                let hull = match spline.nurbs() {
                    Some(_) => &spline.control_points,
                    None => &spline.fit_points,
                };
                out.extend(hull.iter().map(|&q| m.apply(xy(q))));
            }
        }
    }
    out.len() > before
}

/// `vertices` placed through `m`, each bulge turned the way `m` turns a
/// circle; `None` when the run has an arc and `m` would not keep it one.
fn place(
    vertices: &[PolylineVertex],
    m: &Affine2,
    turning: Option<f64>,
) -> Option<Vec<PolylineVertex>> {
    let has_arcs = vertices.iter().any(|v| v.bulge != 0.0);
    let turning = match turning {
        Some(t) => t,
        None if has_arcs => return None,
        None => 1.0,
    };
    Some(
        vertices
            .iter()
            .map(|v| PolylineVertex {
                point: m.apply(v.point),
                bulge: v.bulge * turning,
                ..*v
            })
            .collect(),
    )
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

/// Each boundary path of `h` as its pieces, in the hatch's own plane.
fn pieces(h: &HatchEntity) -> Vec<Vec<Piece>> {
    h.boundary_paths
        .iter()
        .map(|path| match path {
            HatchBoundaryPath::Polyline(vertices) => vec![Piece::Run {
                vertices: vertices.clone(),
                closed: true,
            }],
            HatchBoundaryPath::Edges(edges) => edges.iter().map(|e| piece(h, e)).collect(),
        })
        .collect()
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

fn in_3d(p: Point2D) -> Point3D {
    Point3D {
        x: p.x,
        y: p.y,
        z: 0.0,
    }
}

/// One edge as a piece, in the direction the path runs.
fn piece(h: &HatchEntity, edge: &HatchEdge) -> Piece {
    match edge {
        HatchEdge::Line { start, end } => Piece::Run {
            vertices: vec![
                PolylineVertex::straight(*start),
                PolylineVertex::straight(*end),
            ],
            closed: false,
        },
        HatchEdge::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            is_ccw,
        } => {
            let (from, sweep) = edge_span(*start_angle, *end_angle, *is_ccw);
            // Pieces of a quarter turn or less, each a bulged segment: a
            // bulge is the tangent of a quarter of its sweep, finite below a
            // whole turn.
            let count = ((sweep.abs() / FRAC_PI_2).ceil() as usize).max(1);
            let step = sweep / count as f64;
            let bulge = if *radius > 0.0 {
                (step / 4.0).tan()
            } else {
                0.0
            };
            let vertices = (0..=count)
                .map(|i| {
                    let a = from + step * i as f64;
                    PolylineVertex {
                        bulge: if i < count { bulge } else { 0.0 },
                        ..PolylineVertex::straight(Point2D {
                            x: center.x + radius * a.cos(),
                            y: center.y + radius * a.sin(),
                        })
                    }
                })
                .collect();
            Piece::Run {
                vertices,
                closed: false,
            }
        }
        HatchEdge::Ellipse {
            center,
            end,
            minor_major_ratio,
            start_angle,
            end_angle,
            is_ccw,
        } => {
            // The ELLIPSE runs counter-clockwise over the edge's span; a
            // clockwise edge's chords are walked back so the path keeps its
            // direction. The reference fields are the hatch's own: the
            // ellipse is a measure, never part of an answer.
            let (from, sweep) = edge_span(*start_angle, *end_angle, *is_ccw);
            let (start, end_parameter) = if sweep >= 0.0 {
                (from, from + sweep)
            } else {
                (from + sweep, from)
            };
            Piece::Ellipse {
                ellipse: EllipseEntity {
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
                },
                reversed: sweep < 0.0,
            }
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
        } => Piece::Spline(SplineEntity {
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
        }),
    }
}

/// A curve's chords as straight segments, walked back when `reversed`, and
/// how far they may be from the curve.
fn chords(curve: Curve, reversed: bool) -> Result<(Vec<Segment>, f64), NotSearchedReason> {
    match curve {
        Curve::Chords(c) => {
            let mut points = c.points;
            if reversed {
                points.reverse();
            }
            let segments = points
                .windows(2)
                .map(|w| Segment {
                    from: w[0],
                    to: w[1],
                    arc: None,
                })
                .collect();
            Ok((segments, c.within))
        }
        Curve::Undefined => Err(NotSearchedReason::CurveUndefined),
        Curve::BoundUnknown => Err(NotSearchedReason::CurveBoundUnknown),
    }
}
