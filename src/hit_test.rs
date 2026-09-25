//! Pointing: a coordinate becomes the entity references at that spot.
//!
//! A hit is an entity whose geometry passes within `tolerance` of the
//! point. Every such entity is a hit -- two coincident lines are two hits,
//! and no one of them is preferred. Closed shapes that enclose the point
//! without passing near it are listed separately: a point in the middle of
//! a hole is inside that circle, and saying so is not the same as saying
//! the circle's edge is there.
//!
//! What a block reference draws is searched too: each entity of the block
//! definition is placed through the INSERT's transform (nested references
//! compose), and a hit inside a block names the chain of INSERTs it was
//! reached through, since the same definition entity can sit at several
//! places in the drawing. What could not be searched is said so, with the
//! reason.

use crate::geometry::{
    distance, distance_to_arc, distance_to_segment, distance_to_segments, flat_plane, in_plane,
    shape_contains, world_angle, world_xy, xy,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uncad_model::bulge::{self, Segment};
use uncad_model::model::{
    Confidence, DimensionEntity, Entity, EntityId, HorizontalJustification, LeaderPath, Ref,
};
use uncad_model::{Affine2, CadDatabase, Point2D, Point3D, PolylineVertex};

/// How deep block references may nest before the search stops following
/// them. Real drawings nest a few levels; a definition that references
/// itself would otherwise never end.
const MAX_BLOCK_REF_DEPTH: usize = 20;

/// How many block references one hit test follows in total, across every
/// level. The depth cap bounds nesting but not breadth: many INSERTs per
/// block at every level fan out combinatorially long before the depth cap
/// engages.
const BLOCK_REF_BUDGET: usize = 1_000_000;

/// One entity at the point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub id: EntityId,
    /// The DXF type name the model reports for the entity.
    pub entity_type: String,
    /// Distance from the point to the entity's geometry, in the drawing's
    /// own units; `0` when the point is on it. For an entity whose extent
    /// the model does not carry (text, a block reference) this is the
    /// distance to its anchor, and `anchored` says so: a block reference's
    /// insertion point; a text's start point, or, for a text aligned on an
    /// alignment point, the nearer of the two -- and for aligned or fit text,
    /// which runs from one to the other, the baseline between them; a text
    /// block's or a feature control frame's insertion point. A dimension's
    /// distance is to the nearest of what its block draws, anchored when
    /// that nearest is a text.
    pub distance: f64,
    /// `true` when `distance` is to an anchor rather than to the entity's
    /// drawn geometry.
    pub anchored: bool,
    pub confidence: Confidence,
    /// `true` when the drawing hides what was hit: the entity is marked
    /// invisible (DXF 60), or a block reference it was reached through is --
    /// a hidden reference hides everything it draws. The entity is still
    /// reported, since the file does contain it there; whether a hidden
    /// entity counts is the caller's decision.
    #[serde(default)]
    pub invisible: bool,
    /// The block references the entity was reached through, outermost
    /// first: empty for an entity of the drawing's own space, one ID per
    /// INSERT for an entity of a block definition. The entity's geometry
    /// was placed through these INSERTs' transforms before measuring.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<EntityId>,
}

/// Why an entity, or what a block reference draws, was not searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotSearchedReason {
    /// The INSERT carries no block reference.
    BlockReferenceAbsent,
    /// The INSERT's block reference points at nothing the drawing answers to.
    BlockReferenceUnresolved,
    /// The block name resolves but no block record of that name exists.
    BlockUndefined,
    /// The placement of this entity is not one this crate measures in: the
    /// composed block placement is not a similarity (it scales the axes
    /// differently, or mirrors) for a circle, an arc or a polyline with arc
    /// segments -- a circle would be drawn as an ellipse -- or the circle,
    /// arc, polyline, text or block reference is written on a plane tilted out of
    /// the world's (a plane facing up or down, a mirror copy's included, is
    /// measured). This crate does not guess where it is drawn.
    NonSimilarPlacement,
    /// Block references nest deeper than the search follows.
    NestingTooDeep,
    /// The search followed as many block references as it allows in one call.
    BlockReferenceBudgetExhausted,
}

/// An entity, or a block reference's contents, that the search did not
/// look at -- so that "no hit" is never silently "not looked at".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotSearched {
    pub id: EntityId,
    pub entity_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub via: Vec<EntityId>,
    pub reason: NotSearchedReason,
}

/// What is at a point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitTest {
    /// Entities whose geometry is within the tolerance, nearest first, then
    /// by reference ID, then by the chain they were reached through. Never
    /// narrowed to one: a caller that needs a single entity decides, or
    /// asks with a smaller tolerance.
    pub hits: Vec<Hit>,
    /// Closed entities that enclose the point (a circle, a closed polyline)
    /// and are not already in `hits`, by reference ID. Their `distance` is
    /// to the boundary.
    pub enclosing: Vec<Hit>,
    /// Entity types present in the drawing that this crate cannot
    /// hit-test yet, so that "no hit" is never silently "not looked at".
    /// Sorted, each once.
    pub unsupported: Vec<String>,
    /// Entities and block contents the search did not look at, each with
    /// its reason, by reference ID then chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_searched: Vec<NotSearched>,
}

/// Where a point's geometry is, for one entity.
enum Where {
    /// Distance to the drawn geometry, and whether the point is inside a
    /// closed shape.
    Geometry {
        distance: f64,
        inside: bool,
    },
    /// Distance to an anchor (the model carries no extent).
    Anchor(f64),
    Unsupported,
    NotSearched(NotSearchedReason),
}

/// Distance from `p` to where a line of text is anchored, placed through
/// `t`. Aligned or fit text runs from its start point to its alignment
/// point, so its anchor is the baseline between them. Any other aligned text
/// answers to its alignment point, while its start point is where the
/// writing program computed it to begin -- both are where the text is, and
/// the nearer counts. Text with no alignment point has its start point.
fn text_anchor_distance(
    p: Point2D,
    t: &Affine2,
    start: Point2D,
    alignment_point: Option<Point2D>,
    horizontal: HorizontalJustification,
) -> f64 {
    let start = t.apply(start);
    match alignment_point.map(|a| t.apply(a)) {
        None => distance(p, start),
        Some(end)
            if matches!(
                horizontal,
                HorizontalJustification::Aligned | HorizontalJustification::Fit
            ) =>
        {
            distance_to_segment(p, start, end)
        }
        Some(point) => distance(p, start).min(distance(p, point)),
    }
}

/// Measures `entity`, placed through `t`, against `p`. `scale` is the
/// similarity scale of `t` when it has one.
fn locate(entity: &Entity, p: Point2D, t: &Affine2, scale: Option<f64>) -> Where {
    let at = |q| t.apply(q);
    match entity {
        Entity::Line(l) => Where::Geometry {
            distance: crate::geometry::distance_to_segment(
                p,
                at(xy(l.start_point)),
                at(xy(l.end_point)),
            ),
            inside: false,
        },
        Entity::Circle(c) => {
            let (Some(plane), Some(s)) = (flat_plane(c.extrusion), scale) else {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            };
            let r = c.radius * s;
            let d = distance(p, at(world_xy(plane, c.center)));
            Where::Geometry {
                distance: (d - r).abs(),
                inside: d < r,
            }
        }
        Entity::Arc(a) => {
            let (Some(plane), Some(s)) = (flat_plane(a.extrusion), scale) else {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            };
            // Seen from below (a mirror copy's plane) the arc's own
            // counter-clockwise sweep runs clockwise in the world, so its
            // world start is where its own end is.
            let (start, end) = (
                world_angle(plane, a.start_angle),
                world_angle(plane, a.end_angle),
            );
            let (start, end) = if plane.z_axis().z < 0.0 {
                (end, start)
            } else {
                (start, end)
            };
            let turn = t.rotation();
            Where::Geometry {
                distance: distance_to_arc(
                    p,
                    at(world_xy(plane, a.center)),
                    a.radius * s,
                    start + turn,
                    end + turn,
                ),
                inside: false,
            }
        }
        Entity::LwPolyline(pl) | Entity::Polyline2D(pl) => {
            let Some(plane) = flat_plane(pl.extrusion) else {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            };
            let has_arcs = pl.vertices.iter().any(|v| v.bulge != 0.0);
            if has_arcs && scale.is_none() {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            }
            // Each vertex taken to the world and through the placement. A
            // bulge's sign is its arc's turning direction, which a mirror
            // copy's plane reverses and a similarity keeps.
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
            let segments: Vec<Segment> = bulge::segments(&placed, pl.closed).collect();
            match distance_to_segments(p, &segments) {
                Some(distance) => Where::Geometry {
                    distance,
                    inside: pl.closed && shape_contains(p, &segments),
                },
                None => Where::Unsupported,
            }
        }
        Entity::Point(pt) => Where::Geometry {
            distance: distance(p, at(xy(pt.position))),
            inside: false,
        },
        Entity::Text(e) => match in_plane(e.extrusion, t) {
            Some(t) => Where::Anchor(text_anchor_distance(
                p,
                &t,
                e.start_point,
                e.alignment_point,
                e.horizontal_justification,
            )),
            None => Where::NotSearched(NotSearchedReason::NonSimilarPlacement),
        },
        Entity::Attrib(a) => match in_plane(a.extrusion, t) {
            Some(t) => Where::Anchor(text_anchor_distance(
                p,
                &t,
                a.start_point,
                a.alignment_point,
                a.horizontal_justification,
            )),
            None => Where::NotSearched(NotSearchedReason::NonSimilarPlacement),
        },
        Entity::Attdef(a) => Where::Anchor(text_anchor_distance(
            p,
            t,
            a.start_point,
            a.alignment_point,
            a.horizontal_justification,
        )),
        Entity::Insert(i) => Where::Anchor(distance(p, at(xy(i.insertion_point)))),
        // MTEXT and TOLERANCE carry their insertion point in world
        // coordinates; the extent of what they draw depends on a font the
        // model does not carry.
        Entity::MText(m) => Where::Anchor(distance(p, at(xy(m.insertion_point)))),
        Entity::Tolerance(f) => Where::Anchor(distance(p, at(xy(f.insertion_point)))),
        Entity::Solid(s) | Entity::Trace(s) => {
            let Some(plane) = flat_plane(s.extrusion) else {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            };
            let corner = |c: Point2D| {
                at(world_xy(
                    plane,
                    Point3D {
                        x: c.x,
                        y: c.y,
                        z: s.elevation,
                    },
                ))
            };
            // Filled through its corners in 1-2-4-3 order.
            outline(
                p,
                &[
                    corner(s.corner1),
                    corner(s.corner2),
                    corner(s.corner4),
                    corner(s.corner3),
                ],
                true,
            )
        }
        Entity::Face3D(f) => {
            // Only the edges the file draws; seen from above, as a LINE is.
            let corners = [f.corner1, f.corner2, f.corner3, f.corner4].map(|c| at(xy(c)));
            let distance = (0..4)
                .filter(|&i| !f.invisible_edges[i])
                .map(|i| distance_to_segment(p, corners[i], corners[(i + 1) % 4]))
                .fold(f64::INFINITY, f64::min);
            Where::Geometry {
                distance,
                inside: false,
            }
        }
        Entity::Wipeout(w) => {
            let boundary: Vec<Point2D> = w.boundary.iter().map(|&q| at(q)).collect();
            outline(p, &boundary, true)
        }
        Entity::Leader(l) if l.path_type == Some(LeaderPath::Straight) => {
            let vertices: Vec<Point2D> = l.vertices.iter().map(|&v| at(xy(v))).collect();
            outline(p, &vertices, false)
        }
        Entity::MultiLeader(m) if m.lines.iter().any(|line| line.len() >= 2) => {
            let distance = m
                .lines
                .iter()
                .map(|line| line.iter().map(|&v| at(xy(v))).collect::<Vec<_>>())
                .filter_map(|line| straight_distance(p, &line, false))
                .fold(f64::INFINITY, f64::min);
            Where::Geometry {
                distance,
                inside: false,
            }
        }
        _ => Where::Unsupported,
    }
}

/// Distance from `p` to the straight segments through `points`, closing the
/// last back to the first when `closed`; `None` for fewer than two points.
fn straight_distance(p: Point2D, points: &[Point2D], closed: bool) -> Option<f64> {
    let vertices: Vec<PolylineVertex> = points
        .iter()
        .map(|&q| PolylineVertex::straight(q))
        .collect();
    let segments: Vec<Segment> = bulge::segments(&vertices, closed).collect();
    distance_to_segments(p, &segments)
}

/// The straight outline through `points` as a [`Where`]: a closed outline
/// encloses what lies inside it. Fewer than two points draw nothing this
/// crate can measure.
fn outline(p: Point2D, points: &[Point2D], closed: bool) -> Where {
    let vertices: Vec<PolylineVertex> = points
        .iter()
        .map(|&q| PolylineVertex::straight(q))
        .collect();
    let segments: Vec<Segment> = bulge::segments(&vertices, closed).collect();
    match distance_to_segments(p, &segments) {
        Some(distance) => Where::Geometry {
            distance,
            inside: closed && shape_contains(p, &segments),
        },
        None => Where::Unsupported,
    }
}

struct Search<'a> {
    db: &'a CadDatabase,
    point: Point2D,
    tolerance: f64,
    hits: Vec<Hit>,
    enclosing: Vec<Hit>,
    unsupported: BTreeSet<String>,
    not_searched: Vec<NotSearched>,
    /// Block references followed so far, across every level.
    followed: usize,
    /// How many of the block references on the current path are marked
    /// invisible.
    hidden_refs: usize,
}

impl Search<'_> {
    fn entities(&mut self, entities: &[Entity], t: &Affine2, via: &mut Vec<EntityId>) {
        let scale = t.similarity_scale();
        for entity in entities {
            let common = entity.common();
            let invisible = common.invisible || self.hidden_refs > 0;
            let hit = |distance: f64, anchored: bool| Hit {
                id: common.id,
                entity_type: entity.type_name().to_string(),
                distance,
                anchored,
                confidence: common.confidence,
                invisible,
                via: via.clone(),
            };
            let place = match entity {
                Entity::Dimension(d) => self.locate_dimension(d, t, scale),
                _ => locate(entity, self.point, t, scale),
            };
            match place {
                Where::Geometry { distance, inside } => {
                    if distance <= self.tolerance {
                        self.hits.push(hit(distance, false));
                    } else if inside {
                        self.enclosing.push(hit(distance, false));
                    }
                }
                Where::Anchor(distance) => {
                    if distance <= self.tolerance {
                        self.hits.push(hit(distance, true));
                    }
                }
                Where::Unsupported => {
                    self.unsupported.insert(entity.type_name().to_string());
                }
                Where::NotSearched(reason) => self.not_searched.push(NotSearched {
                    id: common.id,
                    entity_type: entity.type_name().to_string(),
                    via: via.clone(),
                    reason,
                }),
            }
            if let Entity::Insert(insert) = entity {
                self.follow(insert, t, via);
            }
        }
    }

    /// A dimension is where it is drawn and where the file says it is: the
    /// entities of its block, which the file already places in world
    /// coordinates, measured through `t` alone, and the middle of its text
    /// (DXF 11) and the point it was built on (DXF 10), which the file states
    /// beside the block. The nearest of these is the dimension's distance --
    /// an anchor when it is one of the two stated points or a text of the
    /// block.
    fn locate_dimension(&self, d: &DimensionEntity, t: &Affine2, scale: Option<f64>) -> Where {
        let at = |q| t.apply(q);
        let drawn = match &d.block_name {
            Ref::Resolved(name) => self.db.tables.block_records.get(name),
            _ => None,
        };
        let text = (distance(self.point, at(d.text_midpoint)), true);
        let built_on = d
            .definition_point
            .map(|q| (distance(self.point, at(xy(q))), true));
        let nearer = |a: (f64, bool), b: (f64, bool)| {
            // At equal distance the drawn geometry wins, so a point on a line
            // is not reported as merely near an anchor.
            if b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)).is_lt() {
                b
            } else {
                a
            }
        };
        let nearest = drawn
            .into_iter()
            .flat_map(|block| &block.entities)
            .filter_map(|e| match locate(e, self.point, t, scale) {
                Where::Geometry { distance, .. } => Some((distance, false)),
                Where::Anchor(distance) => Some((distance, true)),
                Where::Unsupported | Where::NotSearched(_) => None,
            })
            .chain(built_on)
            .fold(text, nearer);
        match nearest {
            (distance, false) => Where::Geometry {
                distance,
                inside: false,
            },
            (distance, true) => Where::Anchor(distance),
        }
    }

    /// Searches what `insert` draws: its block's entities, placed through
    /// the INSERT's own transform and then `t`.
    fn follow(
        &mut self,
        insert: &uncad_model::model::InsertEntity,
        t: &Affine2,
        via: &mut Vec<EntityId>,
    ) {
        let not = |reason| NotSearched {
            id: insert.common.id,
            entity_type: "INSERT".to_string(),
            via: via.clone(),
            reason,
        };
        let name = match &insert.block_name {
            Ref::Resolved(name) => name,
            Ref::Absent => {
                self.not_searched
                    .push(not(NotSearchedReason::BlockReferenceAbsent));
                return;
            }
            Ref::Unresolved(_) => {
                self.not_searched
                    .push(not(NotSearchedReason::BlockReferenceUnresolved));
                return;
            }
        };
        let Some(block) = self.db.tables.block_records.get(name) else {
            self.not_searched
                .push(not(NotSearchedReason::BlockUndefined));
            return;
        };
        if via.len() >= MAX_BLOCK_REF_DEPTH {
            self.not_searched
                .push(not(NotSearchedReason::NestingTooDeep));
            return;
        }
        if self.followed >= BLOCK_REF_BUDGET {
            self.not_searched
                .push(not(NotSearchedReason::BlockReferenceBudgetExhausted));
            return;
        }
        // A block placed in a plane tilted out of the world's has no exact
        // 2D placement, so its contents are not measured -- and said so.
        let Some(own) = insert.world_transform() else {
            self.not_searched
                .push(not(NotSearchedReason::NonSimilarPlacement));
            return;
        };
        self.followed += 1;
        let placed = own.then(t);
        let hides = usize::from(insert.common.invisible);
        self.hidden_refs += hides;
        via.push(insert.common.id);
        self.entities(&block.entities, &placed, via);
        via.pop();
        self.hidden_refs -= hides;
    }
}

/// The entities at `point`, within `tolerance`: the drawing's own space,
/// and what its block references draw, placed where they are drawn.
///
/// The input is never modified.
pub fn hit_test(db: &CadDatabase, point: Point2D, tolerance: f64) -> HitTest {
    let mut search = Search {
        db,
        point,
        tolerance,
        hits: Vec::new(),
        enclosing: Vec::new(),
        unsupported: BTreeSet::new(),
        not_searched: Vec::new(),
        followed: 0,
        hidden_refs: 0,
    };
    search.entities(&db.entities, &Affine2::IDENTITY, &mut Vec::new());
    let Search {
        mut hits,
        mut enclosing,
        unsupported,
        mut not_searched,
        ..
    } = search;
    hits.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then(a.id.cmp(&b.id))
            .then(a.via.cmp(&b.via))
    });
    enclosing.sort_by(|a, b| a.id.cmp(&b.id).then(a.via.cmp(&b.via)));
    not_searched.sort_by(|a, b| a.id.cmp(&b.id).then(a.via.cmp(&b.via)));
    HitTest {
        hits,
        enclosing,
        unsupported: unsupported.into_iter().collect(),
        not_searched,
    }
}
