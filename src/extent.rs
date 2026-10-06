//! Where a drawing is: per space, the box around the points this crate
//! measures entities by -- a coordinate range to point into.

use crate::boundary::hatch_points;
use crate::geometry::{flat_in_xy, image_frame, in_plane, mline_lines, xy};
use crate::limits::{NotEntered, Walk};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::f64::consts::{PI, TAU};
use uncad_model::bulge;
use uncad_model::model::{
    DimensionKind, Entity, EntityId, LeaderLineType, MultiLeaderContent, Ref,
};
use uncad_model::tables::Tables;
use uncad_model::{Affine2, CadDatabase, Point2D};

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
/// points, a text's anchor, a dimension's text and the points it was built
/// on, and the corners, boundaries and vertices of the rest, and a
/// viewport's frame on its sheet -- a hatch's boundary (the reach of its arcs
/// and ellipses, a spline edge's control points), an MLINE's lines, a raster
/// image's frame, a light's position, and a body's edges when they lie at
/// one height.
///
/// A block reference is measured by its insertion point and by what it
/// draws: the entities of its block, placed where the reference puts them
/// (nested references compose), measured the same way -- exactly under any
/// placement, rotated, mirrored or scaled differently along each axis. A
/// table (ACAD_TABLE) is a block reference too, and a dimension's block
/// holds the lines, arrows and text it draws. An ordinate dimension's datum
/// (DXF 10) is the origin it measures from, not a point it draws, and is not
/// measured, nor is anything on the DEFPOINTS layer, which drawing programs
/// never plot and where a dimension keeps its definition points. A text's
/// glyphs can reach past its anchor -- the model carries no extent for them.
///
/// What gives no point is named, see [`Self::not_measured`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SpaceExtent {
    /// The space's block: `*Model_Space`, or a `*Paper_Space` sheet.
    pub space: String,
    /// `None` when the space holds no entity this crate takes a point from.
    pub bounds: Option<Bounds>,
    /// Entity types of this space that gave no point -- not measured by this
    /// crate, or written on a plane tilted out of the world's -- including
    /// those inside the blocks its references draw. A block reference whose
    /// block was not measured is named by its type and the reason:
    /// `_UNRESOLVED` (the block it names is not in the drawing), `_CYCLE`
    /// (it draws itself), `_TOO_DEEP` (inside 20 other references),
    /// `_BUDGET_EXHAUSTED` (ten million entities already met inside expanded
    /// blocks) or `_TILTED` (placed on a plane tilted out of the world's) --
    /// `INSERT_UNRESOLVED`, say; its insertion point is still measured.
    /// Sorted, each once.
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
    // One budget for the whole drawing: a block placed in every space is
    // still the same work each time it is expanded.
    let mut reach = Reach::drawn(&db.tables);
    db.tables
        .block_records
        .values()
        .filter(|b| is_space(&b.name))
        .map(|block| {
            let mut points = Vec::new();
            let mut not_measured = BTreeSet::new();
            // Each entity's own box -- what it draws included -- to name the
            // one that sets each side.
            let mut boxes: Vec<(EntityId, Bounds)> = Vec::new();
            for e in &block.entities {
                let from = points.len();
                reach.entity(e, &Affine2::IDENTITY, &mut points, &mut not_measured);
                if let Some(b) = bounds(&points[from..]) {
                    boxes.push((e.common().id, b));
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

/// The linear part of `t` applied to the direction `v`.
pub(crate) fn linear(t: &Affine2, v: Point2D) -> Point2D {
    Point2D {
        x: t.a * v.x + t.c * v.y,
        y: t.b * v.x + t.d * v.y,
    }
}

/// The reach of the curve `c + u·cos θ + v·sin θ` for θ from `start`
/// through `sweep` (radians, signed): its two ends, and the points within it
/// where x or y turns. Exact for a circle, an ellipse or an arc of either,
/// and it stays so through any affine placement -- a placement moves `c`,
/// `u` and `v` and leaves θ alone ([`conic_through`]).
pub(crate) fn conic_reach(
    c: Point2D,
    u: Point2D,
    v: Point2D,
    start: f64,
    sweep: f64,
    out: &mut Vec<Point2D>,
) {
    let at = |a: f64| Point2D {
        x: c.x + u.x * a.cos() + v.x * a.sin(),
        y: c.y + u.y * a.cos() + v.y * a.sin(),
    };
    out.push(at(start));
    out.push(at(start + sweep));
    // x turns where -u.x sin θ + v.x cos θ = 0, half a turn apart; y alike.
    for base in [v.x.atan2(u.x), v.y.atan2(u.y)] {
        for half_turn in [0.0, PI] {
            let along = ((base + half_turn - start) * sweep.signum()).rem_euclid(TAU);
            if along <= sweep.abs() || sweep.abs() >= TAU {
                out.push(at(start + along * sweep.signum()));
            }
        }
    }
}

/// [`conic_reach`] of the circle or arc of `radius` about `center`, both in
/// the coordinates `t` places into the world.
pub(crate) fn conic_through(
    t: &Affine2,
    center: Point2D,
    radius: f64,
    start: f64,
    sweep: f64,
    out: &mut Vec<Point2D>,
) {
    let u = linear(t, Point2D { x: radius, y: 0.0 });
    let v = linear(t, Point2D { x: 0.0, y: radius });
    conic_reach(t.apply(center), u, v, start, sweep, out);
}

/// One walk over what a drawing's entities draw: the points each is measured
/// by, with every block reference followed into its block. The bounds on
/// following (depth, budget) are shared across the walk.
pub(crate) struct Reach<'a> {
    tables: &'a Tables,
    walk: Walk,
    /// The layer each open block's reference is effectively on, outermost
    /// first: an entity of the block on layer 0 takes it as its own.
    reference_layers: Vec<String>,
    /// Whether a top-level entity on DEFPOINTS is left out too -- for an
    /// extent, which holds what is drawn; a selection judges every entity
    /// it is asked about by where it is.
    skip_defpoints_at_top: bool,
}

/// The layer drawing programs never plot: a dimension keeps its definition
/// points there.
const DEFPOINTS: &str = "DEFPOINTS";

impl<'a> Reach<'a> {
    /// A walk for an extent: what is drawn.
    pub(crate) fn drawn(tables: &'a Tables) -> Self {
        Reach {
            tables,
            walk: Walk::default(),
            reference_layers: Vec::new(),
            skip_defpoints_at_top: true,
        }
    }

    /// A walk for a selection: each entity asked about by where it is, a
    /// block reference by what it draws.
    pub(crate) fn per_entity(tables: &'a Tables) -> Self {
        Reach {
            skip_defpoints_at_top: false,
            ..Reach::drawn(tables)
        }
    }

    /// Adds the points `e` is measured by, placed through `t`, to `out` --
    /// what it draws included -- and names in `not_measured` what gave none.
    /// An entity on the DEFPOINTS layer is not drawn, and gives nothing --
    /// inside a block always, at the top level for a walk of what is
    /// [`drawn`](Self::drawn).
    pub(crate) fn entity(
        &mut self,
        e: &Entity,
        t: &Affine2,
        out: &mut Vec<Point2D>,
        not_measured: &mut BTreeSet<String>,
    ) {
        let own = e.common().layer.name();
        let layer = match self.reference_layers.last() {
            Some(reference) if own == "0" => reference.clone(),
            _ => own.to_string(),
        };
        let at_top = self.walk.at_top();
        if layer.eq_ignore_ascii_case(DEFPOINTS) && (!at_top || self.skip_defpoints_at_top) {
            return;
        }
        if !points_of(e, self.tables, t, out) {
            not_measured.insert(e.type_name().to_string());
        }
        match e {
            Entity::Insert(insert) => {
                let placed = |block: &uncad_model::tables::BlockRecord| {
                    insert.world_transform(block.base_point)
                };
                self.follow(e, &layer, &insert.block_name, placed, t, out, not_measured);
            }
            // A table's block holds what the table draws, placed as though
            // based at the origin -- where the renderer places it.
            Entity::AcadTable(table) => {
                let placed = |_: &uncad_model::tables::BlockRecord| {
                    Some(Affine2::placement(
                        xy(table.insertion_point),
                        table.scale.x,
                        table.scale.y,
                        table.rotation,
                    ))
                };
                self.follow(e, &layer, &table.block_name, placed, t, out, not_measured);
            }
            // A dimension's block is already in world coordinates: it is
            // placed by `t` alone. A dimension with no block is measured by
            // the points the file states beside it.
            Entity::Dimension(d) if matches!(d.block_name, Ref::Resolved(_)) => {
                let placed = |_: &uncad_model::tables::BlockRecord| Some(Affine2::IDENTITY);
                self.follow(e, &layer, &d.block_name, placed, t, out, not_measured);
            }
            _ => {}
        }
    }

    /// Measures the block `owner` (effectively on `layer`) names, placed
    /// through its own transform (`own`, given the block) and then `t`; when
    /// it cannot, names `owner`'s type with the reason in `not_measured`.
    #[allow(clippy::too_many_arguments)]
    fn follow(
        &mut self,
        owner: &Entity,
        layer: &str,
        block_name: &Ref<String>,
        own: impl FnOnce(&uncad_model::tables::BlockRecord) -> Option<Affine2>,
        t: &Affine2,
        out: &mut Vec<Point2D>,
        not_measured: &mut BTreeSet<String>,
    ) {
        let (block, own) = match self.walk.enter(self.tables, block_name, own) {
            Ok(entered) => entered,
            Err(why) => {
                let reason = match why {
                    NotEntered::Absent | NotEntered::Unresolved | NotEntered::Undefined => {
                        "UNRESOLVED"
                    }
                    NotEntered::Cycle => "CYCLE",
                    NotEntered::TooDeep => "TOO_DEEP",
                    NotEntered::Tilted => "TILTED",
                    NotEntered::BudgetExhausted => "BUDGET_EXHAUSTED",
                };
                not_measured.insert(format!("{}_{reason}", owner.type_name()));
                return;
            }
        };
        let placed = own.then(t);
        self.reference_layers.push(layer.to_string());
        for inner in &block.entities {
            self.entity(inner, &placed, out, not_measured);
        }
        self.reference_layers.pop();
        self.walk.leave();
    }
}

/// Adds the points `e` itself is measured by, placed through `t`, to `out`
/// -- not what a block reference draws, which [`Reach`] follows; `false`
/// when it gives none.
fn points_of(e: &Entity, tables: &Tables, t: &Affine2, out: &mut Vec<Point2D>) -> bool {
    let before = out.len();
    let at = |q: Point2D| t.apply(q);
    match e {
        Entity::Line(l) => out.extend([at(xy(l.start_point)), at(xy(l.end_point))]),
        Entity::Circle(c) => {
            if let Some(m) = in_plane(c.extrusion, t) {
                conic_through(&m, xy(c.center), c.radius, 0.0, TAU, out);
            }
        }
        Entity::Arc(a) => {
            // Measured in its own plane, where it runs counter-clockwise
            // from its start; the placement -- a mirror copy's plane
            // included -- takes the curve where it is drawn. An arc whose
            // angles are equal has no sweep: the file does not say whether
            // it is the whole circle or nothing.
            let Some(sweep) = a.sweep() else {
                return false;
            };
            if let Some(m) = in_plane(a.extrusion, t) {
                conic_through(&m, xy(a.center), a.radius, a.start_angle, sweep, out);
            }
        }
        Entity::LwPolyline(pl) | Entity::Polyline2D(pl) => {
            if let Some(m) = in_plane(pl.extrusion, t) {
                out.extend(pl.vertices.iter().map(|v| m.apply(v.point)));
                for segment in bulge::segments(&pl.vertices, pl.closed) {
                    if let Some(arc) = segment.arc {
                        conic_through(&m, arc.center, arc.radius, arc.start_angle, arc.sweep, out);
                    }
                }
            }
        }
        Entity::Point(p) => out.push(at(xy(p.position))),
        Entity::Viewport(v) => {
            // The frame on the sheet: its centre and size, in paper space.
            let (w, h) = (v.width / 2.0, v.height / 2.0);
            out.extend([(-w, -h), (w, -h), (w, h), (-w, h)].map(|(dx, dy)| {
                at(Point2D {
                    x: v.center.x + dx,
                    y: v.center.y + dy,
                })
            }));
        }
        Entity::Text(x) => {
            if let Some(m) = in_plane(x.extrusion, t) {
                out.push(m.apply(x.start_point));
                out.extend(x.alignment_point.map(|a| m.apply(a)));
            }
        }
        Entity::Attrib(x) => {
            if let Some(m) = in_plane(x.extrusion, t) {
                out.push(m.apply(x.start_point));
                out.extend(x.alignment_point.map(|a| m.apply(a)));
            }
        }
        Entity::Attdef(x) => {
            out.push(at(x.start_point));
            out.extend(x.alignment_point.map(at));
        }
        Entity::Insert(i) => out.push(at(xy(i.insertion_point))),
        Entity::AcadTable(table) => out.push(at(xy(table.insertion_point))),
        Entity::MText(x) => out.push(at(xy(x.insertion_point))),
        Entity::Tolerance(f) => out.push(at(xy(f.insertion_point))),
        Entity::Dimension(d) => {
            out.push(at(d.text_midpoint));
            // An ordinate dimension's DXF 10 is its datum -- the origin it
            // measures from -- not a point of what it draws.
            let datum = match d.kind {
                Some(DimensionKind::Ordinate) => None,
                _ => d.definition_point,
            };
            let p = &d.points;
            out.extend(
                [datum, p.extension1, p.extension2, p.radial, p.arc]
                    .into_iter()
                    .flatten()
                    .map(|q| at(xy(q))),
            );
        }
        Entity::Solid(s) | Entity::Trace(s) => {
            if let Some(m) = in_plane(s.extrusion, t) {
                out.extend([s.corner1, s.corner2, s.corner3, s.corner4].map(|c| m.apply(c)));
            }
        }
        Entity::Face3D(f) => {
            out.extend([f.corner1, f.corner2, f.corner3, f.corner4].map(|c| at(xy(c))))
        }
        Entity::Wipeout(w) => out.extend(w.boundary.iter().map(|&q| at(q))),
        Entity::Ellipse(el) => {
            // Its two ends, and wherever x or y turns within its sweep.
            let Some(minor) = el.minor_axis() else {
                return false;
            };
            conic_reach(
                at(xy(el.center)),
                linear(t, xy(el.major_axis_endpoint)),
                linear(t, xy(minor)),
                el.start_angle,
                el.sweep(),
                out,
            );
        }
        // A spline lies in the hull of its control points, so their box
        // holds it -- and an affine placement keeps the hull. One stored
        // only by the points it passes through gives those.
        Entity::Spline(s) => match s.nurbs() {
            Some(_) => out.extend(s.control_points.iter().map(|&c| at(xy(c)))),
            None => out.extend(s.fit_points.iter().map(|&c| at(xy(c)))),
        },
        Entity::Leader(l) => out.extend(l.vertices.iter().map(|&v| at(xy(v)))),
        Entity::MultiLeader(m) => {
            // Lines of no type are not drawn.
            if m.line_type != Some(LeaderLineType::Invisible) {
                out.extend(m.drawn_lines().iter().flatten().map(|&v| at(xy(v))));
            }
            out.extend(m.doglegs().iter().flatten().map(|&v| at(xy(v))));
            // Where what it points out is placed, as an MTEXT's insertion
            // point or a block reference's is.
            match &m.content {
                Some(MultiLeaderContent::MText(c)) => out.push(at(xy(c.location))),
                Some(MultiLeaderContent::Block(b)) => out.push(at(xy(b.location))),
                None => {}
            }
        }
        Entity::Polyline3D(p) => out.extend(p.vertices.iter().map(|&v| at(xy(v)))),
        Entity::Image(i) => out.extend(image_frame(i).unwrap_or_default().into_iter().map(at)),
        Entity::Light(l) => out.push(at(xy(l.position))),
        // A profile flat in the world's XY; one with depth is not drawn in
        // plan.
        Entity::Solid3D(s)
        | Entity::Region(s)
        | Entity::PolylinePFace(s)
        | Entity::PolylineMesh(s)
            if flat_in_xy(&s.wireframe_edges) =>
        {
            out.extend(s.wireframe_edges.iter().flatten().map(|&q| at(xy(q))))
        }
        Entity::Hatch(h) => {
            hatch_points(h, t, out);
        }
        Entity::MLine(l) => out.extend(
            mline_lines(l, tables)
                .into_iter()
                .flatten()
                .flatten()
                .map(at),
        ),
        // A construction line (RAY, XLINE) has no end, so no box holds it:
        // whether it reaches into a window is not told by points.
        _ => {}
    }
    out.len() > before
}
