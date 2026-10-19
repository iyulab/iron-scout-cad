//! Relating: what each dimension's definition points sit on.
//!
//! A dimension is built on points of the geometry it measures -- the
//! origins of its extension lines, the point on the circle it gives the
//! radius of. Whether such a point is one the drawing's geometry defines is
//! a matter of coordinates, not of inference: it either coincides with a
//! point the format defines for an entity (a line's end, an arc's centre, a
//! polyline's vertex -- [`Anchor`]), or lies on an entity's curve, or
//! neither. Every entity a point is related to is reported; none is
//! preferred.
//!
//! A point related to nothing is reported with no relations. Whether that
//! means the geometry was changed or that it lies where this crate cannot
//! look is said beside it, for the whole drawing: block references to
//! external drawings (whose content is not in this file), entity types the
//! model knows no geometry of, and what could not be searched, with the
//! reason.

use crate::geometry::{
    distance, distance_to_arc, distance_to_segments, flat_plane, world_angle, world_xy, xy,
};
use crate::hit_test::{spaces, NotSearched, NotSearchedReason};
use crate::limits::{NotEntered, Walk};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uncad_model::bulge::{self, Segment};
use uncad_model::model::{DimensionEntity, DimensionKind, Entity, EntityId, Ref};
use uncad_model::{Affine2, CadDatabase, Point2D, Point3D, PolylineVertex};

/// A point the format defines for an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Anchor {
    /// A LINE's start point; an ARC's start, at its start angle.
    Start,
    /// A LINE's end point; an ARC's end, at its end angle.
    End,
    /// A CIRCLE's, an ARC's or an ELLIPSE's centre.
    Center,
    /// The vertex at this index of a polyline (LWPOLYLINE, POLYLINE_2D,
    /// POLYLINE_3D) or an MLINE's centerline, in file order; for a mesh
    /// (POLYLINE_MESH, POLYLINE_PFACE), its distinct edge ends in the order
    /// the model's wireframe first meets them.
    Vertex(usize),
    /// The corner at this index of a 3DFACE, SOLID or TRACE, from 0 for the
    /// file's first.
    Corner(usize),
    /// A SPLINE's fit point at this index.
    Fit(usize),
    /// A SPLINE's first control point -- where a spline not closed starts.
    ControlStart,
    /// A SPLINE's last control point.
    ControlEnd,
    /// The end of an ELLIPSE's major axis the file states.
    AxisEnd,
    /// The other end of an ELLIPSE's major axis.
    AxisEndOpposite,
    /// An XLINE's or a RAY's base point.
    Base,
    /// A POINT's position.
    Position,
}

/// How a definition point is related to an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum RelationKind {
    /// The point is one the format defines for the entity -- the relation's
    /// [`Relation::anchor`] names which.
    AtPoint,
    /// The point lies on the entity's curve, at none of its defined points:
    /// a LINE, an ARC, a CIRCLE, or a segment of a polyline (a bulge's arc
    /// included).
    OnCurve,
}

/// A definition point of a dimension by the DXF group the file carries it
/// in, as [`uncad_model::model::DimensionPoints`] names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PointRole {
    /// DXF 13.
    Extension1,
    /// DXF 14.
    Extension2,
    /// DXF 15.
    Radial,
    /// DXF 10, for the kinds where the format puts a point of the geometry
    /// there: a radius dimension's centre, the far point of a diameter, the
    /// end of a two-line angular dimension's second line.
    Definition,
}

/// One entity a definition point is related to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Relation {
    pub entity: EntityId,
    /// The DXF type name the model reports for the entity.
    pub entity_type: String,
    pub relation: RelationKind,
    /// For [`RelationKind::AtPoint`], which of the entity's points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    /// How far the point is from the anchor or the curve, in the drawing's
    /// units -- at most [`Relate::tolerance`].
    pub distance: f64,
    /// The block references the entity was reached through, outermost
    /// first, as [`crate::Hit::via`]: its geometry was placed through them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<EntityId>,
}

/// A definition point and every entity it is related to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DefinitionPoint {
    pub role: PointRole,
    /// Where the point is, in the coordinates of the dimension's space --
    /// placed through the dimension's own block references when it is drawn
    /// by one.
    pub at: Point2D,
    /// By entity reference ID, then chain, then relation and anchor. Empty
    /// when the point is related to nothing this crate could look at.
    pub relations: Vec<Relation>,
}

/// One dimension and what its definition points sit on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DimensionRelations {
    pub id: EntityId,
    /// What the dimension measures, as the file states it; `None` when it
    /// does not -- and then which of its points are points of the geometry
    /// is not known, so `points` is empty.
    pub kind: Option<DimensionKind>,
    /// The block references the dimension was reached through, as
    /// [`Relation::via`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<EntityId>,
    /// The space the dimension is in, as [`crate::Hit::space`]. Only
    /// geometry of the same space is related to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    /// The points of the geometry the dimension is built on that the file
    /// carries, in the order of [`PointRole`]. For each kind: a linear or
    /// aligned dimension's extension line origins; a radius dimension's
    /// point on the curve and its centre; a diameter's two points on the
    /// curve; a two-line angular dimension's four line points; a three-point
    /// angular dimension's two extension origins and its vertex; an
    /// ordinate's feature; an arc length's two extension origins and the
    /// arc's centre.
    pub points: Vec<DefinitionPoint>,
}

/// A block standing for another drawing, as its record states it, and the
/// references that place it. What it draws is not in this file -- beyond
/// whatever was bound into it -- so a point within it is related to
/// nothing here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExternalReferenceBlock {
    pub block: String,
    /// The referenced drawing's path, as the file spells it.
    pub path: String,
    /// An overlay rather than an attachment.
    pub overlay: bool,
    /// How many entities the block carries in this file -- often none.
    pub entities: usize,
    /// The block references that place it, by reference ID then chain.
    pub inserts: Vec<PlacedReference>,
}

/// A block reference and the chain of references it was reached through.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlacedReference {
    pub id: EntityId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<EntityId>,
}

/// What each dimension's definition points sit on. See [`relate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Relate {
    /// How close a point must be to be related, in the drawing's units:
    /// the caller's, or -- when none is given -- a millionth of the larger
    /// side of the box around every defined point of the geometry, and at
    /// least a millionth.
    pub tolerance: f64,
    /// Blocks standing for other drawings that the drawing places, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_references: Vec<ExternalReferenceBlock>,
    /// Entity types the drawing holds whose geometry the model does not
    /// know, with how many of each were met. Sorted by type.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unmodeled: BTreeMap<String, usize>,
    /// Entities and block contents that were not looked at, each with its
    /// reason, by reference ID then chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_searched: Vec<NotSearched>,
    /// Every dimension of the drawing -- of its spaces and of what its block
    /// references draw -- by reference ID, then chain.
    pub dimensions: Vec<DimensionRelations>,
}

impl Relate {
    /// Keeps the dimension with this reference ID -- every chain it was
    /// reached through -- and drops the others. What the answer says about
    /// the whole drawing (the tolerance, external references, unmodeled
    /// types, what could not be searched) is kept as it is: it is what a
    /// point related to nothing is read against.
    pub fn of_dimension(mut self, id: EntityId) -> Relate {
        self.dimensions.retain(|d| d.id == id);
        self
    }
}

/// A tolerance that is not a positive, finite number: no point is that
/// close to anything, or every point is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InvalidTolerance(pub f64);

impl std::fmt::Display for InvalidTolerance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tolerance {} is not a positive finite number", self.0)
    }
}

impl std::error::Error for InvalidTolerance {}

/// What a defined point or a curve belongs to.
#[derive(Debug, Clone)]
struct Owner {
    id: EntityId,
    entity_type: String,
    via: Vec<EntityId>,
    space: Option<String>,
}

/// A curve a point can lie on, placed in its space.
enum Shape {
    /// Straight segments and bulge arcs, as a polyline's.
    Segments(Vec<Segment>),
    Circle {
        center: Point2D,
        radius: f64,
    },
    Arc {
        center: Point2D,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

impl Shape {
    fn distance(&self, p: Point2D) -> Option<f64> {
        match self {
            Shape::Segments(s) => distance_to_segments(p, s),
            Shape::Circle { center, radius } => Some((distance(p, *center) - radius).abs()),
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => Some(distance_to_arc(p, *center, *radius, *start, *sweep)),
        }
    }

    /// The box the curve lies in.
    fn bounds(&self) -> (Point2D, Point2D) {
        let grow = |(lo, hi): (Point2D, Point2D), c: Point2D, r: f64| {
            (
                Point2D {
                    x: lo.x.min(c.x - r),
                    y: lo.y.min(c.y - r),
                },
                Point2D {
                    x: hi.x.max(c.x + r),
                    y: hi.y.max(c.y + r),
                },
            )
        };
        let empty = (
            Point2D {
                x: f64::INFINITY,
                y: f64::INFINITY,
            },
            Point2D {
                x: f64::NEG_INFINITY,
                y: f64::NEG_INFINITY,
            },
        );
        match self {
            Shape::Segments(segments) => segments.iter().fold(empty, |b, s| {
                let b = grow(grow(b, s.from, 0.0), s.to, 0.0);
                match &s.arc {
                    Some(arc) => grow(b, arc.center, arc.radius),
                    None => b,
                }
            }),
            Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => {
                grow(empty, *center, *radius)
            }
        }
    }
}

struct Curve {
    owner: usize,
    shape: Shape,
    lo: Point2D,
    hi: Point2D,
}

struct Defined {
    owner: usize,
    anchor: Anchor,
    at: Point2D,
}

struct Dimension {
    id: EntityId,
    kind: Option<DimensionKind>,
    via: Vec<EntityId>,
    space: Option<String>,
    points: Vec<(PointRole, Point2D)>,
}

struct Collect<'a> {
    db: &'a CadDatabase,
    spaces: BTreeMap<EntityId, &'a str>,
    blocks: Walk,
    owners: Vec<Owner>,
    defined: Vec<Defined>,
    curves: Vec<Curve>,
    dimensions: Vec<Dimension>,
    unmodeled: BTreeMap<String, usize>,
    not_searched: Vec<NotSearched>,
    external: BTreeMap<String, ExternalReferenceBlock>,
}

impl Collect<'_> {
    fn space_of(&self, id: EntityId, via: &[EntityId]) -> Option<String> {
        let top = via.first().copied().unwrap_or(id);
        self.spaces.get(&top).map(|s| s.to_string())
    }

    fn not_searched(&mut self, entity: &Entity, via: &[EntityId], reason: NotSearchedReason) {
        let id = entity.common().id;
        self.not_searched.push(NotSearched {
            id,
            entity_type: entity.type_name().to_string(),
            via: via.to_vec(),
            reason,
            space: self.space_of(id, via),
        });
    }

    fn entities(&mut self, entities: &[Entity], t: &Affine2, via: &mut Vec<EntityId>) {
        for entity in entities {
            self.entity(entity, t, via);
        }
    }

    fn entity(&mut self, entity: &Entity, t: &Affine2, via: &mut Vec<EntityId>) {
        let at = |q: Point2D| t.apply(q);
        let scale = t.similarity_scale();
        let mut defined: Vec<(Anchor, Point2D)> = Vec::new();
        let mut shape = None;
        match entity {
            Entity::Line(l) => {
                let (a, b) = (at(xy(l.start_point)), at(xy(l.end_point)));
                defined.extend([(Anchor::Start, a), (Anchor::End, b)]);
                shape = Some(Shape::Segments(straight(&[a, b], false)));
            }
            Entity::Circle(c) => {
                let (Some(plane), Some(s)) = (flat_plane(c.extrusion), scale) else {
                    return self.not_searched(entity, via, NotSearchedReason::NonSimilarPlacement);
                };
                let center = at(world_xy(plane, c.center));
                defined.push((Anchor::Center, center));
                shape = Some(Shape::Circle {
                    center,
                    radius: c.radius * s,
                });
            }
            Entity::Arc(a) => {
                let (Some(plane), Some(s)) = (flat_plane(a.extrusion), scale) else {
                    return self.not_searched(entity, via, NotSearchedReason::NonSimilarPlacement);
                };
                let on = |angle: f64| {
                    at(world_xy(
                        plane,
                        Point3D {
                            x: a.center.x + a.radius * angle.cos(),
                            y: a.center.y + a.radius * angle.sin(),
                            z: a.center.z,
                        },
                    ))
                };
                let center = at(world_xy(plane, a.center));
                defined.extend([
                    (Anchor::Center, center),
                    (Anchor::Start, on(a.start_angle)),
                    (Anchor::End, on(a.end_angle)),
                ]);
                // An arc whose angles are equal is the whole circle or
                // nothing; its points are still the file's, its curve is not
                // known.
                if let Some(sweep) = a.sweep() {
                    // Seen from below (a mirror copy's plane) the arc's own
                    // sweep runs clockwise, so its world start is its end.
                    let start = if plane.z_axis().z < 0.0 {
                        world_angle(plane, a.end_angle)
                    } else {
                        world_angle(plane, a.start_angle)
                    };
                    shape = Some(Shape::Arc {
                        center,
                        radius: a.radius * s,
                        start: start + t.rotation(),
                        sweep,
                    });
                }
            }
            Entity::LwPolyline(pl) | Entity::Polyline2D(pl) => {
                let Some(plane) = flat_plane(pl.extrusion) else {
                    return self.not_searched(entity, via, NotSearchedReason::NonSimilarPlacement);
                };
                let has_arcs = pl.vertices.iter().any(|v| v.bulge != 0.0);
                if has_arcs && scale.is_none() {
                    return self.not_searched(entity, via, NotSearchedReason::NonSimilarPlacement);
                }
                let turning = plane.z_axis().z.signum();
                let placed: Vec<PolylineVertex> = pl
                    .vertices
                    .iter()
                    .map(|v| PolylineVertex {
                        point: at(world_xy(
                            plane,
                            Point3D {
                                x: v.point.x,
                                y: v.point.y,
                                z: pl.elevation,
                            },
                        )),
                        bulge: v.bulge * turning,
                        ..*v
                    })
                    .collect();
                defined.extend(
                    placed
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (Anchor::Vertex(i), v.point)),
                );
                shape = Some(Shape::Segments(
                    bulge::segments(&placed, pl.closed).collect(),
                ));
            }
            Entity::Polyline3D(pl) => {
                let points: Vec<Point2D> = pl.vertices.iter().map(|&v| at(xy(v))).collect();
                defined.extend(
                    points
                        .iter()
                        .enumerate()
                        .map(|(i, &p)| (Anchor::Vertex(i), p)),
                );
                shape = Some(Shape::Segments(straight(&points, pl.closed)));
            }
            Entity::MLine(m) => defined.extend(
                m.vertices
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (Anchor::Vertex(i), at(xy(v.point)))),
            ),
            Entity::Face3D(f) => defined.extend(
                [f.corner1, f.corner2, f.corner3, f.corner4]
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| (Anchor::Corner(i), at(xy(c)))),
            ),
            Entity::Solid(s) | Entity::Trace(s) => {
                let Some(plane) = flat_plane(s.extrusion) else {
                    return self.not_searched(entity, via, NotSearchedReason::NonSimilarPlacement);
                };
                defined.extend(
                    [s.corner1, s.corner2, s.corner3, s.corner4]
                        .into_iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let c = Point3D {
                                x: c.x,
                                y: c.y,
                                z: s.elevation,
                            };
                            (Anchor::Corner(i), at(world_xy(plane, c)))
                        }),
                );
            }
            Entity::PolylineMesh(m) | Entity::PolylinePFace(m) => {
                let mut seen: Vec<Point2D> = Vec::new();
                for p in m.wireframe_edges.iter().flatten() {
                    let p = at(xy(*p));
                    if !seen.contains(&p) {
                        seen.push(p);
                    }
                }
                defined.extend(
                    seen.into_iter()
                        .enumerate()
                        .map(|(i, p)| (Anchor::Vertex(i), p)),
                );
            }
            Entity::Spline(s) => {
                defined.extend(
                    s.fit_points
                        .iter()
                        .enumerate()
                        .map(|(i, &p)| (Anchor::Fit(i), at(xy(p)))),
                );
                if let (Some(&first), Some(&last)) =
                    (s.control_points.first(), s.control_points.last())
                {
                    defined.extend([
                        (Anchor::ControlStart, at(xy(first))),
                        (Anchor::ControlEnd, at(xy(last))),
                    ]);
                }
            }
            Entity::Ellipse(e) => {
                let (c, m) = (e.center, e.major_axis_endpoint);
                defined.extend([
                    (Anchor::Center, at(xy(c))),
                    (
                        Anchor::AxisEnd,
                        at(Point2D {
                            x: c.x + m.x,
                            y: c.y + m.y,
                        }),
                    ),
                    (
                        Anchor::AxisEndOpposite,
                        at(Point2D {
                            x: c.x - m.x,
                            y: c.y - m.y,
                        }),
                    ),
                ]);
            }
            Entity::Ray(r) | Entity::XLine(r) => defined.push((Anchor::Base, at(xy(r.point)))),
            Entity::Point(p) => defined.push((Anchor::Position, at(xy(p.position)))),
            Entity::Dimension(d) => self.dimension(d, t, via),
            Entity::Unknown { type_name, .. } => {
                *self.unmodeled.entry(type_name.clone()).or_default() += 1;
            }
            _ => {}
        }
        if !defined.is_empty() || shape.is_some() {
            let common = entity.common();
            let owner = self.owners.len();
            self.owners.push(Owner {
                id: common.id,
                entity_type: entity.type_name().to_string(),
                via: via.clone(),
                space: self.space_of(common.id, via),
            });
            self.defined
                .extend(
                    defined
                        .into_iter()
                        .map(|(anchor, at)| Defined { owner, anchor, at }),
                );
            if let Some(shape) = shape {
                let (lo, hi) = shape.bounds();
                self.curves.push(Curve {
                    owner,
                    shape,
                    lo,
                    hi,
                });
            }
        }
        match entity {
            Entity::Insert(insert) => self.follow(
                entity,
                &insert.block_name,
                |base| insert.world_transform(base),
                t,
                via,
            ),
            Entity::AcadTable(table) => self.follow(
                entity,
                &table.block_name,
                |_| {
                    Some(Affine2::placement(
                        xy(table.insertion_point),
                        table.scale.x,
                        table.scale.y,
                        table.rotation,
                    ))
                },
                t,
                via,
            ),
            _ => {}
        }
    }

    /// Keeps a dimension's points of the geometry, placed through `t`.
    fn dimension(&mut self, d: &DimensionEntity, t: &Affine2, via: &[EntityId]) {
        let p = &d.points;
        let roles: &[(PointRole, Option<Point3D>)] = match d.kind {
            Some(DimensionKind::Rotated | DimensionKind::Aligned) => &[
                (PointRole::Extension1, p.extension1),
                (PointRole::Extension2, p.extension2),
            ],
            Some(DimensionKind::Radius | DimensionKind::Diameter) => &[
                (PointRole::Radial, p.radial),
                (PointRole::Definition, d.definition_point),
            ],
            Some(DimensionKind::Angular2Line) => &[
                (PointRole::Extension1, p.extension1),
                (PointRole::Extension2, p.extension2),
                (PointRole::Radial, p.radial),
                (PointRole::Definition, d.definition_point),
            ],
            Some(DimensionKind::Angular3Point | DimensionKind::ArcLength) => &[
                (PointRole::Extension1, p.extension1),
                (PointRole::Extension2, p.extension2),
                (PointRole::Radial, p.radial),
            ],
            Some(DimensionKind::Ordinate) => &[(PointRole::Extension1, p.extension1)],
            None => &[],
        };
        let id = d.common.id;
        self.dimensions.push(Dimension {
            id,
            kind: d.kind,
            via: via.to_vec(),
            space: self.space_of(id, via),
            points: roles
                .iter()
                .filter_map(|&(role, q)| Some((role, t.apply(xy(q?)))))
                .collect(),
        });
    }

    /// Collects what the block reference `owner` draws, placed through its
    /// own transform and then `t`, and notes a block standing for another
    /// drawing.
    fn follow(
        &mut self,
        owner: &Entity,
        block_name: &Ref<String>,
        own: impl FnOnce(Point3D) -> Option<Affine2>,
        t: &Affine2,
        via: &mut Vec<EntityId>,
    ) {
        let id = owner.common().id;
        if let Ref::Resolved(name) = block_name {
            if let Some(block) = self.db.tables.block_records.get(name) {
                if let Some(x) = &block.external_reference {
                    self.external
                        .entry(name.clone())
                        .or_insert_with(|| ExternalReferenceBlock {
                            block: name.clone(),
                            path: x.path.clone(),
                            overlay: x.overlay,
                            entities: block.entities.len(),
                            inserts: Vec::new(),
                        })
                        .inserts
                        .push(PlacedReference {
                            id,
                            via: via.clone(),
                        });
                }
            }
        }
        let (block, own) = match self
            .blocks
            .enter(&self.db.tables, block_name, |b| own(b.base_point))
        {
            Ok(entered) => entered,
            Err(why) => {
                let reason = match why {
                    NotEntered::Absent => NotSearchedReason::BlockReferenceAbsent,
                    NotEntered::Unresolved => NotSearchedReason::BlockReferenceUnresolved,
                    NotEntered::Undefined => NotSearchedReason::BlockUndefined,
                    NotEntered::Cycle => NotSearchedReason::BlockReferenceCycle,
                    NotEntered::TooDeep => NotSearchedReason::NestingTooDeep,
                    NotEntered::Tilted => NotSearchedReason::NonSimilarPlacement,
                    NotEntered::BudgetExhausted => NotSearchedReason::BlockReferenceBudgetExhausted,
                };
                return self.not_searched(owner, via, reason);
            }
        };
        let placed = own.then(t);
        via.push(id);
        self.entities(&block.entities, &placed, via);
        via.pop();
        self.blocks.leave();
    }
}

/// The straight segments through `points`, closing back to the first when
/// `closed`.
fn straight(points: &[Point2D], closed: bool) -> Vec<Segment> {
    let vertices: Vec<PolylineVertex> = points
        .iter()
        .map(|&q| PolylineVertex::straight(q))
        .collect();
    bulge::segments(&vertices, closed).collect()
}

/// Relates every dimension's definition points to the geometry of its space
/// within `tolerance` -- the caller's, or the default [`Relate::tolerance`]
/// describes. The drawing's own spaces are searched, and what their block
/// references draw, placed where it is drawn; a dimension's own block (the
/// lines and arrows it draws) is not geometry it measures.
///
/// The input is never modified, and the same drawing gives the same answer
/// whatever order its entities are in.
pub fn relate(db: &CadDatabase, tolerance: Option<f64>) -> Result<Relate, InvalidTolerance> {
    if let Some(t) = tolerance {
        if !(t.is_finite() && t > 0.0) {
            return Err(InvalidTolerance(t));
        }
    }
    let mut c = Collect {
        db,
        spaces: spaces(db),
        blocks: Walk::default(),
        owners: Vec::new(),
        defined: Vec::new(),
        curves: Vec::new(),
        dimensions: Vec::new(),
        unmodeled: BTreeMap::new(),
        not_searched: Vec::new(),
        external: BTreeMap::new(),
    };
    c.entities(&db.entities, &Affine2::IDENTITY, &mut Vec::new());
    let Collect {
        owners,
        defined,
        curves,
        dimensions,
        unmodeled,
        mut not_searched,
        external,
        ..
    } = c;

    let tolerance = tolerance.unwrap_or_else(|| default_tolerance(&defined));
    let grid = Grid::new(&defined, tolerance);
    // The curves by the left side of their box, so that a point looks only
    // at those that can reach it.
    let mut by_left: Vec<usize> = (0..curves.len()).collect();
    by_left.sort_by(|&a, &b| curves[a].lo.x.total_cmp(&curves[b].lo.x).then(a.cmp(&b)));
    let widest = curves.iter().map(|c| c.hi.x - c.lo.x).fold(0.0, f64::max);

    let relations_at = |at: Point2D, space: &Option<String>| -> Vec<Relation> {
        let mut out = Vec::new();
        let mut at_point: BTreeSet<usize> = BTreeSet::new();
        for d in grid.near(at) {
            let d = &defined[d];
            let owner = &owners[d.owner];
            if &owner.space != space {
                continue;
            }
            let distance = distance(at, d.at);
            if distance <= tolerance {
                at_point.insert(d.owner);
                out.push(relation(
                    owner,
                    RelationKind::AtPoint,
                    Some(d.anchor),
                    distance,
                ));
            }
        }
        let first = by_left.partition_point(|&i| curves[i].lo.x < at.x - tolerance - widest);
        for &i in &by_left[first..] {
            let curve = &curves[i];
            if curve.lo.x > at.x + tolerance {
                break;
            }
            if at.x > curve.hi.x + tolerance
                || at.y < curve.lo.y - tolerance
                || at.y > curve.hi.y + tolerance
            {
                continue;
            }
            let owner = &owners[curve.owner];
            if &owner.space != space || at_point.contains(&curve.owner) {
                continue;
            }
            if let Some(distance) = curve.shape.distance(at).filter(|&d| d <= tolerance) {
                out.push(relation(owner, RelationKind::OnCurve, None, distance));
            }
        }
        out.sort_by(|a, b| {
            (a.entity, &a.via, a.relation, a.anchor)
                .cmp(&(b.entity, &b.via, b.relation, b.anchor))
                .then(a.distance.total_cmp(&b.distance))
        });
        out
    };

    let mut dimensions: Vec<DimensionRelations> = dimensions
        .into_iter()
        .map(|d| DimensionRelations {
            points: d
                .points
                .iter()
                .map(|&(role, at)| DefinitionPoint {
                    role,
                    at,
                    relations: relations_at(at, &d.space),
                })
                .collect(),
            id: d.id,
            kind: d.kind,
            via: d.via,
            space: d.space,
        })
        .collect();
    dimensions.sort_by(|a, b| (a.id, &a.via).cmp(&(b.id, &b.via)));
    not_searched.sort_by(|a, b| a.id.cmp(&b.id).then(a.via.cmp(&b.via)));
    let external_references = external
        .into_values()
        .map(|mut x| {
            x.inserts.sort();
            x
        })
        .collect();
    Ok(Relate {
        tolerance,
        external_references,
        unmodeled,
        not_searched,
        dimensions,
    })
}

fn relation(
    owner: &Owner,
    relation: RelationKind,
    anchor: Option<Anchor>,
    distance: f64,
) -> Relation {
    Relation {
        entity: owner.id,
        entity_type: owner.entity_type.clone(),
        relation,
        anchor,
        distance,
        via: owner.via.clone(),
    }
}

/// A millionth of the larger side of the box around the defined points --
/// at least a millionth.
fn default_tolerance(defined: &[Defined]) -> f64 {
    let points: Vec<Point2D> = defined.iter().map(|d| d.at).collect();
    let side =
        crate::extent::bounds(&points).map_or(0.0, |b| (b.max.x - b.min.x).max(b.max.y - b.min.y));
    1e-6 * side.max(1.0)
}

/// The defined points by grid cell, a cell four tolerances wide: a point
/// within the tolerance of another is in its cell or a neighbouring one.
struct Grid {
    cell: f64,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
}

impl Grid {
    fn new(defined: &[Defined], tolerance: f64) -> Grid {
        let cell = tolerance * 4.0;
        let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (i, d) in defined.iter().enumerate() {
            if let Some(key) = Self::key(d.at, cell) {
                cells.entry(key).or_default().push(i);
            }
        }
        Grid { cell, cells }
    }

    fn key(p: Point2D, cell: f64) -> Option<(i64, i64)> {
        let (x, y) = ((p.x / cell).floor(), (p.y / cell).floor());
        (x.is_finite() && y.is_finite() && x.abs() < 9e18 && y.abs() < 9e18)
            .then_some((x as i64, y as i64))
    }

    fn near(&self, p: Point2D) -> impl Iterator<Item = usize> + '_ {
        let key = Self::key(p, self.cell);
        key.into_iter().flat_map(move |(gx, gy)| {
            (-1..=1).flat_map(move |dx| {
                (-1..=1).flat_map(move |dy| {
                    self.cells
                        .get(&(gx + dx, gy + dy))
                        .into_iter()
                        .flatten()
                        .copied()
                })
            })
        })
    }
}
