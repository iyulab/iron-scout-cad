//! Pointing: a coordinate becomes the entity references at that spot.
//!
//! A hit is an entity whose geometry passes within `tolerance` of the
//! point. Every such entity is a hit -- two coincident lines are two hits,
//! and no one of them is preferred. Closed shapes that enclose the point
//! without passing near it are listed separately: a point in the middle of
//! a hole is inside that circle, and saying so is not the same as saying
//! the circle's edge is there.

use crate::geometry::{distance, distance_to_arc, distance_to_polyline, polygon_contains, xy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uncad_model::model::{Confidence, Entity, EntityId};
use uncad_model::{CadDatabase, Point2D};

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
}

/// What is at a point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitTest {
    /// Entities whose geometry is within the tolerance, nearest first, then
    /// by reference ID. Never narrowed to one: a caller that needs a single
    /// entity decides, or asks with a smaller tolerance.
    pub hits: Vec<Hit>,
    /// Closed entities that enclose the point (a circle, a closed polyline)
    /// and are not already in `hits`, by reference ID. Their `distance` is
    /// to the boundary.
    pub enclosing: Vec<Hit>,
    /// Entity types present in the drawing that this crate cannot
    /// hit-test yet, so that "no hit" is never silently "not looked at".
    /// Sorted, each once.
    pub unsupported: Vec<String>,
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
}

fn locate(entity: &Entity, p: Point2D) -> Where {
    match entity {
        Entity::Line(l) => Where::Geometry {
            distance: crate::geometry::distance_to_segment(p, xy(l.start_point), xy(l.end_point)),
            inside: false,
        },
        Entity::Circle(c) => {
            let d = distance(p, xy(c.center));
            Where::Geometry {
                distance: (d - c.radius).abs(),
                inside: d < c.radius,
            }
        }
        Entity::Arc(a) => Where::Geometry {
            distance: distance_to_arc(p, xy(a.center), a.radius, a.start_angle, a.end_angle),
            inside: false,
        },
        Entity::LwPolyline(pl) | Entity::Polyline2D(pl) => {
            match distance_to_polyline(p, &pl.vertices, pl.closed) {
                Some(distance) => Where::Geometry {
                    distance,
                    inside: pl.closed && polygon_contains(p, &pl.vertices),
                },
                None => Where::Unsupported,
            }
        }
        Entity::Point(pt) => Where::Geometry {
            distance: distance(p, xy(pt.position)),
            inside: false,
        },
        Entity::Text(t) => Where::Anchor(distance(p, t.start_point)),
        Entity::Attrib(a) => Where::Anchor(distance(p, a.start_point)),
        Entity::Attdef(a) => Where::Anchor(distance(p, a.start_point)),
        Entity::Insert(i) => Where::Anchor(distance(p, xy(i.insertion_point))),
        _ => Where::Unsupported,
    }
}

/// The entities of the drawing's own space at `point`, within `tolerance`.
///
/// Only the drawing's top-level entities are looked at; what a block
/// reference draws through its definition is not transformed and searched
/// yet -- the reference itself is anchored at its insertion point. The
/// input is never modified.
pub fn hit_test(db: &CadDatabase, point: Point2D, tolerance: f64) -> HitTest {
    let mut hits = Vec::new();
    let mut enclosing = Vec::new();
    let mut unsupported = BTreeSet::new();
    for entity in &db.entities {
        let common = entity.common();
        let hit = |distance: f64, anchored: bool| Hit {
            id: common.id,
            entity_type: entity.type_name().to_string(),
            distance,
            anchored,
            confidence: common.confidence,
        };
        match locate(entity, point) {
            Where::Geometry { distance, inside } => {
                if distance <= tolerance {
                    hits.push(hit(distance, false));
                } else if inside {
                    enclosing.push(hit(distance, false));
                }
            }
            Where::Anchor(distance) => {
                if distance <= tolerance {
                    hits.push(hit(distance, true));
                }
            }
            Where::Unsupported => {
                unsupported.insert(entity.type_name().to_string());
            }
        }
    }
    hits.sort_by(|a, b| a.distance.total_cmp(&b.distance).then(a.id.cmp(&b.id)));
    enclosing.sort_by_key(|h| h.id);
    HitTest {
        hits,
        enclosing,
        unsupported: unsupported.into_iter().collect(),
    }
}
