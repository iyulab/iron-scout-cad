//! Pointing at what a drawing annotates with: dimensions (golden G5, and a
//! dimension whose block is missing), text blocks and feature control
//! frames, filled and masked areas, faces, and leaders.

use iron_scout_cad::hit_test;
use serde_json::{json, Value};
use uncad_model::model::EntityId;
use uncad_model::{CadDatabase, Point2D};

fn p(x: f64, y: f64) -> Point2D {
    Point2D { x, y }
}

fn g5() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g5.expected.json")).expect("G5 deserializes")
}

fn common(id: u64) -> Value {
    json!({
        "id": id, "origin": "VECTOR", "confidence": "HIGH",
        "source_handle": {"type": "ABSENT"}, "layer": {"type": "RESOLVED", "data": "0"},
        "color_index": 256, "true_color": null, "invisible": false,
        "linetype": {"type": "BY_LAYER"}, "linetype_scale": 1.0,
        "lineweight": -1, "transparency": 0
    })
}

fn xyz(x: f64, y: f64) -> Value {
    json!({"x": x, "y": y, "z": 0.0})
}

fn xy(x: f64, y: f64) -> Value {
    json!({"x": x, "y": y})
}

/// A drawing of the given entities, each already carrying its `type` tag.
fn drawing(entities: Vec<Value>) -> CadDatabase {
    CadDatabase {
        entities: serde_json::from_value(Value::Array(entities)).expect("the entities deserialize"),
        tables: Default::default(),
        read_diagnostics: Default::default(),
    }
}

fn entity(kind: &str, id: u64, fields: Value) -> Value {
    let mut e = fields;
    e["type"] = json!(kind);
    e["common"] = common(id);
    e
}

#[test]
fn a_dimension_is_hit_where_its_block_draws_it() {
    let db = g5();
    // *D1 draws its dimension line from (0, -15) to (120, -15).
    let r = hit_test(&db, p(60.0, -15.0), 1e-9);
    let dims: Vec<_> = r
        .hits
        .iter()
        .filter(|h| h.entity_type == "DIMENSION")
        .collect();
    assert_eq!(dims.len(), 1, "{:?}", r.hits);
    assert_eq!(dims[0].id, EntityId::new(298));
    assert_eq!(dims[0].distance, 0.0);
    assert!(!dims[0].anchored, "measured to the drawn line");
    assert!(!r.unsupported.iter().any(|t| t == "DIMENSION"));
}

#[test]
fn a_dimension_nearest_to_its_text_is_measured_to_the_texts_anchor() {
    let db = g5();
    // *D1's text starts at (60, -14); the dimension line is 1 away.
    let r = hit_test(&db, p(60.0, -14.0), 0.5);
    let dim = r
        .hits
        .iter()
        .find(|h| h.id == EntityId::new(298))
        .expect("the dimension is hit at its text");
    assert_eq!(dim.distance, 0.0);
    assert!(dim.anchored);
}

#[test]
fn every_dimension_of_the_dense_case_is_hit_on_its_own_line_only() {
    let db = g5();
    for (id, y) in [
        (298, -15.0),
        (299, 55.0),
        (300, 95.0),
        (301, 135.0),
        (302, 175.0),
    ] {
        let r = hit_test(&db, p(30.0, y), 1e-9);
        let dims: Vec<EntityId> = r
            .hits
            .iter()
            .filter(|h| h.entity_type == "DIMENSION")
            .map(|h| h.id)
            .collect();
        assert_eq!(dims, [EntityId::new(id)], "at y = {y}");
    }
}

#[test]
fn a_dimension_whose_block_is_missing_is_measured_to_its_text_midpoint() {
    let db = drawing(vec![entity(
        "DIMENSION",
        1,
        json!({
            "block_name": {"type": "UNRESOLVED", "data": "*D9"},
            "kind": "ROTATED", "measurement": 120.0,
            "text_override": {"type": "MEASURED"},
            "definition_point": xyz(0.0, -15.0),
            "text_midpoint": xy(60.0, -14.0),
            "points": {"extension1": null, "extension2": null, "radial": null, "arc": null},
            "rotation": 0.0, "text_rotation": 0.0,
            "style_name": {"type": "ABSENT"},
            "ordinate_axis": null
        }),
    )]);
    let r = hit_test(&db, p(60.0, -14.0), 1e-9);
    assert_eq!(r.hits.len(), 1, "{r:?}");
    assert!(r.hits[0].anchored);
    let r = hit_test(&db, p(0.0, -15.0), 1e-9);
    assert_eq!(r.hits.len(), 1, "its definition point: {r:?}");
}

#[test]
fn a_text_block_and_a_feature_control_frame_are_hit_at_their_insertion_points() {
    let db = drawing(vec![
        entity(
            "MTEXT",
            1,
            json!({
                "insertion_point": xyz(10.0, 20.0), "text": "NOTE", "text_height": 2.5,
                "rotation": 0.0, "line_spacing_factor": 1.0, "attachment": "TOP_LEFT"
            }),
        ),
        entity(
            "TOLERANCE",
            2,
            json!({
                "insertion_point": xyz(40.0, 20.0), "text_height": 2.5,
                "text_value": "{\\Fgdt;j}%%v0.1", "direction": xyz(1.0, 0.0),
                "style_name": {"type": "ABSENT"}
            }),
        ),
    ]);
    for (x, id, kind) in [(10.0, 1, "MTEXT"), (40.0, 2, "TOLERANCE")] {
        let r = hit_test(&db, p(x, 20.0), 1e-9);
        assert_eq!(r.hits.len(), 1, "{r:?}");
        assert_eq!(r.hits[0].id, EntityId::new(id));
        assert_eq!(r.hits[0].entity_type, kind);
        assert!(r.hits[0].anchored);
    }
    assert!(hit_test(&db, p(0.0, 0.0), 1e-9).unsupported.is_empty());
}

fn square_solid() -> CadDatabase {
    // Corners in DXF order: the fill runs 1-2-4-3, so this is a square.
    drawing(vec![entity(
        "SOLID",
        1,
        json!({
            "corner1": xy(0.0, 0.0), "corner2": xy(10.0, 0.0),
            "corner3": xy(0.0, 10.0), "corner4": xy(10.0, 10.0)
        }),
    )])
}

#[test]
fn a_solid_is_filled_through_its_corners_in_the_formats_order() {
    let db = square_solid();
    // On its right edge, which runs from corner 2 to corner 4.
    let r = hit_test(&db, p(10.0, 5.0), 1e-9);
    assert_eq!(r.hits.len(), 1, "{r:?}");
    assert!(!r.hits[0].anchored);
    // Its middle is inside the square, not on a diagonal of a bow tie.
    let r = hit_test(&db, p(5.0, 5.0), 1e-9);
    assert!(r.hits.is_empty(), "{r:?}");
    assert_eq!(r.enclosing.len(), 1);
}

#[test]
fn a_faces_hidden_edge_is_not_hit() {
    let db = drawing(vec![entity(
        "3DFACE",
        1,
        json!({
            "corner1": xyz(0.0, 0.0), "corner2": xyz(10.0, 0.0),
            "corner3": xyz(10.0, 10.0), "corner4": xyz(0.0, 10.0),
            "invisible_edges": [false, true, false, false]
        }),
    )]);
    assert_eq!(hit_test(&db, p(5.0, 0.0), 1e-9).hits.len(), 1);
    // The second edge, from corner 2 to corner 3, is hidden.
    assert!(hit_test(&db, p(10.0, 5.0), 1e-9).hits.is_empty());
}

#[test]
fn a_wipeout_encloses_what_it_masks() {
    let db = drawing(vec![entity(
        "WIPEOUT",
        1,
        json!({"boundary": [xy(0.0, 0.0), xy(4.0, 0.0), xy(4.0, 4.0), xy(0.0, 4.0)]}),
    )]);
    let r = hit_test(&db, p(2.0, 2.0), 1e-9);
    assert_eq!(r.enclosing.len(), 1, "{r:?}");
    assert_eq!(hit_test(&db, p(4.0, 2.0), 1e-9).hits.len(), 1);
}

fn leader(path_type: Value) -> CadDatabase {
    drawing(vec![entity(
        "LEADER",
        1,
        json!({
            "vertices": [xyz(0.0, 0.0), xyz(10.0, 10.0), xyz(20.0, 10.0)],
            "has_arrowhead": true, "path_type": path_type, "annotation": "NOTHING",
            "annotation_id": {"type": "ABSENT"}, "style_name": {"type": "ABSENT"}
        }),
    )])
}

#[test]
fn a_straight_leader_is_hit_along_its_segments() {
    let db = leader(json!("STRAIGHT"));
    let r = hit_test(&db, p(15.0, 10.0), 1e-9);
    assert_eq!(r.hits.len(), 1, "{r:?}");
    assert!(r.unsupported.is_empty());
}

#[test]
fn a_leader_that_is_not_known_to_run_straight_is_not_measured_as_if_it_did() {
    for path_type in [json!("SPLINE"), Value::Null] {
        let db = leader(path_type);
        let r = hit_test(&db, p(15.0, 10.0), 1e-9);
        assert!(r.hits.is_empty(), "{r:?}");
        assert_eq!(r.unsupported, ["LEADER"]);
    }
}

#[test]
fn a_multileader_is_hit_on_its_leader_lines() {
    let db = drawing(vec![entity(
        "MULTILEADER",
        1,
        json!({"lines": [[xyz(0.0, 0.0), xyz(5.0, 5.0)], [xyz(0.0, 10.0), xyz(5.0, 5.0)]]}),
    )]);
    assert_eq!(hit_test(&db, p(2.5, 7.5), 1e-9).hits.len(), 1);
    assert!(hit_test(&db, p(5.0, 0.0), 1e-9).hits.is_empty());
}
