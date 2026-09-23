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

use crate::geometry::{distance, distance_to_arc, distance_to_polyline, polygon_contains, xy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uncad_model::model::{Confidence, Entity, EntityId, Ref};
use uncad_model::{Affine2, CadDatabase, Point2D};

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
    /// distance to its anchor point, and `anchored` says so.
    pub distance: f64,
    /// `true` when `distance` is to an anchor point rather than to the
    /// entity's drawn geometry.
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
    /// The placement of this entity's circle or arc is not one this crate
    /// measures in: the composed block placement is not a similarity (it
    /// scales the axes differently, or mirrors), or the circle or arc is
    /// written in its own coordinate system rather than the world's (an
    /// extrusion other than the world Z axis -- a mirror copy, or a tilted
    /// plane). This crate does not guess where it is drawn.
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

/// Whether an entity written in its own coordinate system is in the world's:
/// its extrusion is the world Z axis, give or take the rounding files write
/// it with.
fn in_world_plane(extrusion: uncad_model::Point3D) -> bool {
    extrusion.z > 0.0 && extrusion.x.abs().max(extrusion.y.abs()) <= 1e-9 * extrusion.z
}

/// Where a point's geometry is, for one entity.
enum Where {
    /// Distance to the drawn geometry, and whether the point is inside a
    /// closed shape.
    Geometry {
        distance: f64,
        inside: bool,
    },
    /// Distance to an anchor point (the model carries no extent).
    Anchor(f64),
    Unsupported,
    NotSearched(NotSearchedReason),
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
            let Some(s) = scale.filter(|_| in_world_plane(c.extrusion)) else {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            };
            let r = c.radius * s;
            let d = distance(p, at(xy(c.center)));
            Where::Geometry {
                distance: (d - r).abs(),
                inside: d < r,
            }
        }
        Entity::Arc(a) => {
            let Some(s) = scale.filter(|_| in_world_plane(a.extrusion)) else {
                return Where::NotSearched(NotSearchedReason::NonSimilarPlacement);
            };
            let turn = t.rotation();
            Where::Geometry {
                distance: distance_to_arc(
                    p,
                    at(xy(a.center)),
                    a.radius * s,
                    a.start_angle + turn,
                    a.end_angle + turn,
                ),
                inside: false,
            }
        }
        Entity::LwPolyline(pl) | Entity::Polyline2D(pl) => {
            // TODO(bulge): arc segments are still measured as their chords.
            let vertices: Vec<Point2D> = pl.vertices.iter().map(|v| at(v.point)).collect();
            match distance_to_polyline(p, &vertices, pl.closed) {
                Some(distance) => Where::Geometry {
                    distance,
                    inside: pl.closed && polygon_contains(p, &vertices),
                },
                None => Where::Unsupported,
            }
        }
        Entity::Point(pt) => Where::Geometry {
            distance: distance(p, at(xy(pt.position))),
            inside: false,
        },
        Entity::Text(e) => Where::Anchor(distance(p, at(e.start_point))),
        Entity::Attrib(a) => Where::Anchor(distance(p, at(a.start_point))),
        Entity::Attdef(a) => Where::Anchor(distance(p, at(a.start_point))),
        Entity::Insert(i) => Where::Anchor(distance(p, at(xy(i.insertion_point)))),
        _ => Where::Unsupported,
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
            match locate(entity, self.point, t, scale) {
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
        self.followed += 1;
        let placed = insert.transform().then(t);
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
