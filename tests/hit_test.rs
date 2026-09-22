//! Pointing, checked on the golden cases: a hole of the general part (G1)
//! and two coincident lines (G6).

use iron_scout_cad::hit_test;
use uncad_model::model::{Confidence, Entity, EntityId};
use uncad_model::{CadDatabase, Point2D};

fn golden(json: &str) -> CadDatabase {
    serde_json::from_str(json).expect("the golden model deserializes")
}

fn g1() -> CadDatabase {
    golden(include_str!("golden/g1.expected.json"))
}

fn g6() -> CadDatabase {
    golden(include_str!("golden/g6.expected.json"))
}

fn p(x: f64, y: f64) -> Point2D {
    Point2D { x, y }
}

/// The hole centred at (20, 20), radius 5.
fn first_hole(db: &CadDatabase) -> EntityId {
    db.entities
        .iter()
        .find_map(|e| match e {
            Entity::Circle(c) if c.center.x == 20.0 && c.center.y == 20.0 => Some(c.common.id),
            _ => None,
        })
        .expect("G1 has a hole at (20, 20)")
}

#[test]
fn a_point_on_a_circles_edge_hits_it_at_distance_zero() {
    let db = g1();
    let result = hit_test(&db, p(25.0, 20.0), 1e-9);
    assert_eq!(result.hits.len(), 1, "{:?}", result.hits);
    assert_eq!(result.hits[0].id, first_hole(&db));
    assert_eq!(result.hits[0].entity_type, "CIRCLE");
    assert_eq!(result.hits[0].distance, 0.0);
    assert!(!result.hits[0].anchored);
    assert_eq!(result.hits[0].confidence, Confidence::High);
    // The point is inside the part's outline, which encloses it without
    // being near it.
    let enclosing: Vec<&str> = result
        .enclosing
        .iter()
        .map(|h| h.entity_type.as_str())
        .collect();
    assert_eq!(enclosing, ["LWPOLYLINE"]);
}

#[test]
fn a_point_inside_a_hole_is_enclosed_by_it_not_on_it() {
    let db = g1();
    let result = hit_test(&db, p(20.0, 20.0), 0.5);
    assert!(result.hits.is_empty(), "{:?}", result.hits);
    // Enclosed by the hole and by the outline around it, by reference ID
    // (the outline was written first).
    assert_eq!(result.enclosing.len(), 2, "{:?}", result.enclosing);
    let hole = result
        .enclosing
        .iter()
        .find(|h| h.id == first_hole(&db))
        .expect("the hole encloses its centre");
    assert_eq!(hole.distance, 5.0);
    assert_eq!(result.enclosing[0].entity_type, "LWPOLYLINE");
    assert_eq!(result.enclosing[1].entity_type, "CIRCLE");
}

#[test]
fn a_point_on_the_outline_is_inside_no_hole_and_on_the_outline() {
    let db = g1();
    let result = hit_test(&db, p(100.0, 0.0), 1e-9);
    let types: Vec<&str> = result.hits.iter().map(|h| h.entity_type.as_str()).collect();
    assert_eq!(types, ["LWPOLYLINE"]);
    assert!(
        result.enclosing.is_empty(),
        "the outline is hit, not merely enclosing"
    );
    // A point well inside the outline and outside every hole encloses only
    // the outline.
    let result = hit_test(&db, p(100.0, 50.0), 1e-9);
    assert!(result.hits.is_empty());
    let types: Vec<&str> = result
        .enclosing
        .iter()
        .map(|h| h.entity_type.as_str())
        .collect();
    assert_eq!(types, ["LWPOLYLINE"]);
}

#[test]
fn what_cannot_be_hit_tested_is_named_never_skipped_in_silence() {
    let db = g1();
    let result = hit_test(&db, p(0.0, 0.0), 1e-9);
    assert_eq!(result.unsupported, ["DIMENSION"]);
}

#[test]
fn two_coincident_lines_are_two_hits_neither_preferred() {
    let db = g6();
    let ids: Vec<EntityId> = db.entities.iter().map(|e| e.common().id).collect();
    let result = hit_test(&db, p(25.0, 0.0), 1e-9);
    let hit_ids: Vec<EntityId> = result.hits.iter().map(|h| h.id).collect();
    assert_eq!(hit_ids, ids, "both lines, by reference ID");
    assert!(result.hits.iter().all(|h| h.distance == 0.0));
    // Nearest first when the distances differ.
    let result = hit_test(&db, p(25.0, 0.5), 1.0);
    assert_eq!(result.hits.len(), 2);
    assert_eq!(result.hits[0].distance, 0.5);
}

#[test]
fn a_text_is_hit_at_its_anchor_and_says_so() {
    let db = g1();
    // The first ATTRIB of the title block sits at (125, -45).
    let result = hit_test(&db, p(125.0, -45.0), 1e-9);
    let attribs: Vec<&iron_scout_cad::Hit> = result
        .hits
        .iter()
        .filter(|h| h.entity_type == "ATTRIB")
        .collect();
    assert_eq!(attribs.len(), 1);
    assert!(attribs[0].anchored);
}
