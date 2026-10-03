//! Where a drawing is: per space, the box around the points this crate
//! measures entities by -- a coordinate range to point into.

use crate::boundary::hatch_points;
use crate::geometry::{
    flat_in_xy, flat_plane, image_frame, in_plane, mline_lines, world_angle, world_xy, xy,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uncad_model::bulge;
use uncad_model::model::{Entity, EntityId, LeaderLineType};
use uncad_model::tables::Tables;
use uncad_model::{Affine2, BulgeArc, CadDatabase, Point2D, Point3D};

/// An axis-aligned box, in the drawing's own units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min: Point2D,
    pub max: Point2D,
}

/// The extent of one space of the drawing: model space, or one paper-space
/// sheet -- each has coordinates of its own, so they are never mixed.
///
/// The box holds every point this crate measures an entity by, in world
/// coordinates: a line's ends, a circle's, an arc's or an ellipse's reach in
/// x and y (exactly, its sweep considered), a polyline's vertices and the
/// reach of its arc segments, a spline's control points (whose box holds the
/// curve) or, for one stored only by the points it passes through, those
/// points, a text's anchor, a block reference's insertion point, a
/// dimension's text and the points it was built on, and the corners,
/// boundaries and vertices of the rest, and a viewport's frame on its sheet --
/// a hatch's boundary (the reach of its arcs and ellipses, a spline edge's
/// control points), an MLINE's lines, a raster image's frame, a light's
/// position, and a body's edges when they lie at one height.
/// What a block reference draws, and a text's glyphs, can reach past it --
/// the model carries no extent for either. Entity types it takes no point
/// from are named.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpaceExtent {
    /// The space's block: `*Model_Space`, or a `*Paper_Space` sheet.
    pub space: String,
    /// `None` when the space holds no entity this crate takes a point from.
    pub bounds: Option<Bounds>,
    /// Entity types of this space that gave no point: not measured by this
    /// crate, or written on a plane tilted out of the world's. Sorted, each
    /// once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_measured: Vec<String>,
    /// The entity each side of `bounds` is set by -- the one with the
    /// point farthest out on that side, the lowest reference ID on a tie.
    /// One stray entity far from the rest sets a side of the box by
    /// itself; this names it, so a caller can tell the drawing's extent
    /// from one entity's. `None` exactly when `bounds` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounded_by: Option<BoundedBy>,
}

/// The entity that sets each side of a [`SpaceExtent`]'s box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BoundedBy {
    pub min_x: EntityId,
    pub min_y: EntityId,
    pub max_x: EntityId,
    pub max_y: EntityId,
}

fn is_space(name: &str) -> bool {
    name.eq_ignore_ascii_case("*Model_Space")
        || name.to_ascii_uppercase().starts_with("*PAPER_SPACE")
}

/// Every space of `db`, by block name.
pub(crate) fn space_extents(db: &CadDatabase) -> Vec<SpaceExtent> {
    db.tables
        .block_records
        .values()
        .filter(|b| is_space(&b.name))
        .map(|block| {
            let mut points = Vec::new();
            let mut not_measured = BTreeSet::new();
            // Each entity's own box, to name the one that sets each side.
            let mut boxes: Vec<(EntityId, Bounds)> = Vec::new();
            for e in &block.entities {
                let from = points.len();
                if points_of(e, &db.tables, &mut points) {
                    if let Some(b) = bounds(&points[from..]) {
                        boxes.push((e.common().id, b));
                    }
                } else {
                    not_measured.insert(e.type_name().to_string());
                }
            }
            SpaceExtent {
                space: block.name.clone(),
                bounds: bounds(&points),
                not_measured: not_measured.into_iter().collect(),
                bounded_by: bounded_by(&boxes),
            }
        })
        .collect()
}

/// The entity farthest out on each side, the lowest reference ID on a tie.
fn bounded_by(boxes: &[(EntityId, Bounds)]) -> Option<BoundedBy> {
    let side = |key: fn(&Bounds) -> f64, outward: f64| {
        boxes
            .iter()
            .min_by(|(ia, a), (ib, b)| {
                (outward * key(b))
                    .total_cmp(&(outward * key(a)))
                    .then(ia.cmp(ib))
            })
            .map(|(id, _)| *id)
    };
    Some(BoundedBy {
        min_x: side(|b| b.min.x, -1.0)?,
        min_y: side(|b| b.min.y, -1.0)?,
        max_x: side(|b| b.max.x, 1.0)?,
        max_y: side(|b| b.max.y, 1.0)?,
    })
}

pub(crate) fn bounds(points: &[Point2D]) -> Option<Bounds> {
    let finite = points.iter().filter(|p| p.x.is_finite() && p.y.is_finite());
    finite.fold(None, |b: Option<Bounds>, p| {
        Some(match b {
            None => Bounds { min: *p, max: *p },
            Some(b) => Bounds {
                min: Point2D {
                    x: b.min.x.min(p.x),
                    y: b.min.y.min(p.y),
                },
                max: Point2D {
                    x: b.max.x.max(p.x),
                    y: b.max.y.max(p.y),
                },
            },
        })
    })
}

/// The reach of an arc in world coordinates: its two ends and its extreme
/// points within the sweep.
fn arc_reach(arc: &BulgeArc, out: &mut Vec<Point2D>) {
    out.push(arc.at(arc.start_angle));
    out.push(arc.at(arc.start_angle + arc.sweep));
    out.extend(arc.extremes());
}

/// Adds the points `e` is measured by to `out`; `false` when it gives none.
pub(crate) fn points_of(e: &Entity, tables: &Tables, out: &mut Vec<Point2D>) -> bool {
    let before = out.len();
    match e {
        Entity::Line(l) => out.extend([xy(l.start_point), xy(l.end_point)]),
        Entity::Circle(c) => {
            if let Some(plane) = flat_plane(c.extrusion) {
                let at = world_xy(plane, c.center);
                arc_reach(
                    &BulgeArc {
                        center: at,
                        radius: c.radius,
                        start_angle: 0.0,
                        sweep: std::f64::consts::TAU,
                    },
                    out,
                );
            }
        }
        Entity::Arc(a) => {
            if let Some(plane) = flat_plane(a.extrusion) {
                // Seen from below (a mirror copy's plane) the arc runs
                // clockwise in the world, from its own end to its own start.
                // An arc whose angles are equal has no sweep: the file does
                // not say whether it is the whole circle or nothing.
                let Some(sweep) = a.sweep() else {
                    return false;
                };
                let start = if plane.z_axis().z < 0.0 {
                    world_angle(plane, a.end_angle)
                } else {
                    world_angle(plane, a.start_angle)
                };
                arc_reach(
                    &BulgeArc {
                        center: world_xy(plane, a.center),
                        radius: a.radius,
                        start_angle: start,
                        sweep,
                    },
                    out,
                );
            }
        }
        Entity::LwPolyline(pl) | Entity::Polyline2D(pl) => {
            if let Some(plane) = flat_plane(pl.extrusion) {
                let turning = plane.z_axis().z.signum();
                let placed: Vec<_> = pl
                    .vertices
                    .iter()
                    .map(|v| uncad_model::PolylineVertex {
                        point: world_xy(
                            plane,
                            Point3D {
                                x: v.point.x,
                                y: v.point.y,
                                z: pl.elevation,
                            },
                        ),
                        bulge: v.bulge * turning,
                        ..*v
                    })
                    .collect();
                out.extend(placed.iter().map(|v| v.point));
                for segment in bulge::segments(&placed, pl.closed) {
                    if let Some(arc) = segment.arc {
                        out.extend(arc.extremes());
                    }
                }
            }
        }
        Entity::Point(p) => out.push(xy(p.position)),
        Entity::Viewport(v) => {
            // The frame on the sheet: its centre and size, in paper space.
            let (w, h) = (v.width / 2.0, v.height / 2.0);
            out.extend([
                Point2D {
                    x: v.center.x - w,
                    y: v.center.y - h,
                },
                Point2D {
                    x: v.center.x + w,
                    y: v.center.y + h,
                },
            ]);
        }
        Entity::Text(t) => {
            if let Some(t2) = in_plane(t.extrusion, &Affine2::IDENTITY) {
                out.push(t2.apply(t.start_point));
                out.extend(t.alignment_point.map(|a| t2.apply(a)));
            }
        }
        Entity::Attrib(t) => {
            if let Some(t2) = in_plane(t.extrusion, &Affine2::IDENTITY) {
                out.push(t2.apply(t.start_point));
                out.extend(t.alignment_point.map(|a| t2.apply(a)));
            }
        }
        Entity::Attdef(t) => {
            out.push(t.start_point);
            out.extend(t.alignment_point);
        }
        Entity::Insert(i) => out.push(xy(i.insertion_point)),
        Entity::MText(m) => out.push(xy(m.insertion_point)),
        Entity::Tolerance(f) => out.push(xy(f.insertion_point)),
        Entity::Dimension(d) => {
            out.push(d.text_midpoint);
            let p = &d.points;
            out.extend(
                [
                    d.definition_point,
                    p.extension1,
                    p.extension2,
                    p.radial,
                    p.arc,
                ]
                .into_iter()
                .flatten()
                .map(xy),
            );
        }
        Entity::Solid(s) | Entity::Trace(s) => {
            if let Some(plane) = flat_plane(s.extrusion) {
                for c in [s.corner1, s.corner2, s.corner3, s.corner4] {
                    out.push(world_xy(
                        plane,
                        Point3D {
                            x: c.x,
                            y: c.y,
                            z: s.elevation,
                        },
                    ));
                }
            }
        }
        Entity::Face3D(f) => out.extend([f.corner1, f.corner2, f.corner3, f.corner4].map(xy)),
        Entity::Wipeout(w) => out.extend(w.boundary.iter().copied()),
        Entity::Ellipse(el) => {
            // Its two ends, and wherever x or y turns within its sweep.
            let Some(turns) = el.extremes() else {
                return false;
            };
            let ends = [el.start_angle, el.start_angle + el.sweep()];
            out.extend(ends.into_iter().filter_map(|t| el.point_at(t)).map(xy));
            out.extend(turns.into_iter().map(xy));
        }
        // A spline lies in the hull of its control points, so their box
        // holds it. One stored only by the points it passes through gives
        // those.
        Entity::Spline(s) => match s.nurbs() {
            Some(_) => out.extend(s.control_points.iter().map(|&c| xy(c))),
            None => out.extend(s.fit_points.iter().map(|&c| xy(c))),
        },
        Entity::Leader(l) => out.extend(l.vertices.iter().map(|&v| xy(v))),
        Entity::MultiLeader(m) => {
            // Lines of no type are not drawn.
            if m.line_type != Some(LeaderLineType::Invisible) {
                out.extend(m.drawn_lines().iter().flatten().map(|&v| xy(v)));
            }
            out.extend(m.doglegs().iter().flatten().map(|&v| xy(v)));
        }
        Entity::Polyline3D(p) => out.extend(p.vertices.iter().map(|&v| xy(v))),
        Entity::Image(i) => out.extend(image_frame(i).unwrap_or_default()),
        Entity::Light(l) => out.push(xy(l.position)),
        // A profile flat in the world's XY; one with depth is not drawn in
        // plan.
        Entity::Solid3D(s)
        | Entity::Region(s)
        | Entity::PolylinePFace(s)
        | Entity::PolylineMesh(s)
            if flat_in_xy(&s.wireframe_edges) =>
        {
            out.extend(s.wireframe_edges.iter().flatten().map(|&q| xy(q)))
        }
        Entity::Hatch(h) => {
            hatch_points(h, out);
        }
        Entity::MLine(l) => out.extend(mline_lines(l, tables).into_iter().flatten().flatten()),
        // A construction line (RAY, XLINE) has no end, so no box holds it:
        // whether it reaches into a window is not told by points.
        _ => {}
    }
    out.len() > before
}
