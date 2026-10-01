//! Pointing at the straight shapes a drawing shows in plan: construction
//! lines, 3D polylines, a viewport's frame, a raster image's frame, and a
//! body whose profile lies flat. Each is measured where it is drawn, seen
//! from above; a body with depth is not in plan, and stays unsupported.
//! One of these that the model gives nothing to measure -- no vertices, no
//! edges, a frame enclosing nothing -- is not searched, with that reason.

use iron_scout_cad::hit_test;
use serde_json::{json, Value};
use uncad_model::tables::BlockRecord;
use uncad_model::{CadDatabase, Point2D};

fn p(x: f64, y: f64) -> Point2D {
    Point2D { x, y }
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

fn xyz(x: f64, y: f64, z: f64) -> Value {
    json!({"x": x, "y": y, "z": z})
}

fn drawing(entities: Vec<Value>) -> CadDatabase {
    CadDatabase {
        entities: serde_json::from_value(Value::Array(entities)).expect("the entities deserialize"),
        tables: Default::default(),
        header: Default::default(),
        read_diagnostics: Default::default(),
    }
}

fn entity(kind: &str, id: u64, fields: Value) -> Value {
    let mut e = fields;
    e["type"] = json!(kind);
    e["common"] = common(id);
    e
}

/// A construction line from (10, 0) towards +x.
fn line(kind: &str) -> CadDatabase {
    drawing(vec![entity(
        kind,
        1,
        json!({"point": xyz(10.0, 0.0, 0.0), "vector": xyz(1.0, 0.0, 0.0)}),
    )])
}

/// The entity is not searched because the model gives it nothing to
/// measure -- its type is not unsupported.
fn assert_no_geometry(r: &iron_scout_cad::HitTest) {
    assert!(r.hits.is_empty() && r.unsupported.is_empty(), "{r:?}");
    assert_eq!(r.not_searched.len(), 1, "{r:?}");
    assert_eq!(
        r.not_searched[0].reason,
        iron_scout_cad::NotSearchedReason::NoGeometry
    );
}

fn distances(db: &CadDatabase, at: Point2D, tolerance: f64) -> Vec<f64> {
    let r = hit_test(db, at, tolerance);
    assert!(r.unsupported.is_empty(), "{r:?}");
    r.hits.iter().map(|h| h.distance).collect()
}

#[test]
fn a_ray_runs_one_way_from_its_base_point() {
    let db = line("RAY");
    // Far along it, and beside it.
    assert_eq!(distances(&db, p(1000.0, 0.0), 1e-9), [0.0]);
    assert_eq!(distances(&db, p(500.0, 2.0), 3.0), [2.0]);
    // Behind its base point it is as far as the base point.
    assert_eq!(distances(&db, p(0.0, 0.0), 11.0), [10.0]);
    assert!(distances(&db, p(0.0, 0.0), 9.0).is_empty());
}

#[test]
fn an_xline_runs_both_ways() {
    let db = line("XLINE");
    assert_eq!(distances(&db, p(-1000.0, 0.0), 1e-9), [0.0]);
    assert_eq!(distances(&db, p(-500.0, -2.0), 3.0), [2.0]);
}

#[test]
fn a_construction_line_straight_up_is_its_base_point_in_plan() {
    let db = drawing(vec![entity(
        "XLINE",
        1,
        json!({"point": xyz(3.0, 4.0, 0.0), "vector": xyz(0.0, 0.0, 1.0)}),
    )]);
    assert_eq!(distances(&db, p(0.0, 0.0), 5.0), [5.0]);
    assert!(distances(&db, p(0.0, 0.0), 4.0).is_empty());
}

#[test]
fn a_3d_polyline_is_seen_from_above() {
    let db = drawing(vec![entity(
        "POLYLINE_3D",
        1,
        json!({
            "vertices": [xyz(0.0, 0.0, 0.0), xyz(10.0, 0.0, 5.0), xyz(10.0, 10.0, -3.0)],
            "closed": false
        }),
    )]);
    assert_eq!(distances(&db, p(5.0, 0.0), 1e-9), [0.0]);
    assert_eq!(distances(&db, p(11.0, 5.0), 2.0), [1.0]);
    // Open: the closing side from the last vertex back to the first is
    // not drawn.
    assert!(distances(&db, p(5.0, 5.0), 1.0).is_empty());
}

#[test]
fn a_closed_3d_polyline_draws_its_closing_side() {
    let db = drawing(vec![entity(
        "POLYLINE_3D",
        1,
        json!({
            "vertices": [xyz(0.0, 0.0, 0.0), xyz(10.0, 0.0, 5.0), xyz(10.0, 10.0, -3.0)],
            "closed": true
        }),
    )]);
    assert_eq!(distances(&db, p(5.0, 5.0), 1e-9), [0.0]);
}

#[test]
fn a_viewport_is_its_frame_and_encloses_what_it_frames() {
    let db = drawing(vec![entity(
        "VIEWPORT",
        1,
        json!({
            "center": xyz(50.0, 40.0, 0.0), "width": 100.0, "height": 80.0,
            "view": null, "on": true, "viewport_id": 2
        }),
    )]);
    assert_eq!(distances(&db, p(100.0, 40.0), 1e-9), [0.0]);
    assert_eq!(distances(&db, p(50.0, 81.0), 1.0), [1.0]);
    let inside = hit_test(&db, p(50.0, 40.0), 1.0);
    assert!(inside.hits.is_empty());
    assert_eq!(inside.enclosing.len(), 1);
    assert_eq!(inside.enclosing[0].distance, 40.0);
}

fn image(clipping: Option<bool>, boundary: Value) -> CadDatabase {
    // 100 x 50 pixels, each 0.1 units: a 10 x 5 frame from (20, 30).
    drawing(vec![entity(
        "IMAGE",
        1,
        json!({
            "insertion_point": xyz(20.0, 30.0, 0.0),
            "u_vector": xyz(0.1, 0.0, 0.0), "v_vector": xyz(0.0, 0.1, 0.0),
            "size_pixels": {"x": 100.0, "y": 50.0},
            "definition": {"type": "ABSENT"},
            "display_flags": 7, "clipping": clipping, "brightness": 50,
            "contrast": 50, "fade": 0, "clip_outside": null,
            "boundary": boundary
        }),
    )])
}

#[test]
fn an_image_is_its_frame() {
    let db = image(None, json!([]));
    assert_eq!(distances(&db, p(30.0, 32.0), 1e-9), [0.0]);
    let inside = hit_test(&db, p(25.0, 32.0), 1.0);
    assert_eq!(inside.enclosing.len(), 1);
}

#[test]
fn a_clipped_image_is_its_clip_boundary() {
    let boundary = json!([
        {"x": 21.0, "y": 31.0}, {"x": 24.0, "y": 31.0}, {"x": 24.0, "y": 34.0}
    ]);
    let db = image(Some(true), boundary.clone());
    // On the clip boundary, not on the frame.
    assert_eq!(distances(&db, p(22.0, 31.0), 1e-9), [0.0]);
    assert!(distances(&db, p(30.0, 32.0), 1e-9).is_empty());
    // With clipping off the same boundary is not where the image is.
    let db = image(Some(false), boundary);
    assert_eq!(distances(&db, p(30.0, 32.0), 1e-9), [0.0]);
}

#[test]
fn an_image_with_no_size_is_not_measured() {
    let db = drawing(vec![entity(
        "IMAGE",
        1,
        json!({
            "insertion_point": xyz(0.0, 0.0, 0.0),
            "u_vector": xyz(0.0, 0.0, 0.0), "v_vector": xyz(0.0, 0.0, 0.0),
            "size_pixels": {"x": 0.0, "y": 0.0},
            "definition": {"type": "ABSENT"},
            "display_flags": null, "clipping": null, "brightness": null,
            "contrast": null, "fade": null, "clip_outside": null,
            "boundary": []
        }),
    )]);
    assert_no_geometry(&hit_test(&db, p(0.0, 0.0), 1.0));
}

fn body(kind: &str, z_far: f64) -> CadDatabase {
    // A 10 x 10 square profile at z = 2, one corner lifted to `z_far`.
    drawing(vec![entity(
        kind,
        1,
        json!({"wireframe_edges": [
            [xyz(0.0, 0.0, 2.0), xyz(10.0, 0.0, 2.0)],
            [xyz(10.0, 0.0, 2.0), xyz(10.0, 10.0, z_far)],
            [xyz(10.0, 10.0, z_far), xyz(0.0, 10.0, 2.0)],
            [xyz(0.0, 10.0, 2.0), xyz(0.0, 0.0, 2.0)]
        ]}),
    )])
}

#[test]
fn a_flat_profile_is_measured_where_it_is_drawn() {
    for kind in ["REGION", "3DSOLID", "POLYLINE_PFACE", "POLYLINE_MESH"] {
        let db = body(kind, 2.0);
        assert_eq!(distances(&db, p(5.0, 0.0), 1e-9), [0.0], "{kind}");
        assert_eq!(distances(&db, p(5.0, 11.0), 1.0), [1.0], "{kind}");
    }
}

#[test]
fn a_body_with_depth_is_not_in_plan() {
    for kind in ["REGION", "3DSOLID", "POLYLINE_PFACE", "POLYLINE_MESH"] {
        let r = hit_test(&body(kind, 7.0), p(5.0, 0.0), 1.0);
        assert!(r.hits.is_empty(), "{kind}");
        assert_eq!(r.unsupported, [kind]);
    }
}

#[test]
fn a_body_with_no_edges_is_not_measured() {
    let db = drawing(vec![entity(
        "3DSOLID",
        1,
        json!({"wireframe_edges": [], "skipped_edges": 3}),
    )]);
    assert_no_geometry(&hit_test(&db, p(0.0, 0.0), 1.0));
}

#[test]
fn a_construction_line_in_a_block_is_placed_with_it() {
    // An XLINE along x through the block's origin, the block placed at
    // (4, 5) and turned a quarter: the line runs along y through x = 4.
    let mut db = drawing(vec![entity(
        "INSERT",
        2,
        json!({
            "block_name": {"type": "RESOLVED", "data": "B"},
            "insertion_point": xyz(4.0, 5.0, 0.0),
            "scale": xyz(1.0, 1.0, 1.0), "rotation": std::f64::consts::FRAC_PI_2,
            "attribs": []
        }),
    )]);
    let entities = serde_json::from_value(json!([entity(
        "XLINE",
        1,
        json!({"point": xyz(0.0, 0.0, 0.0), "vector": xyz(1.0, 0.0, 0.0)})
    )]))
    .expect("the entities deserialize");
    db.tables.block_records.insert(
        "B".to_string(),
        BlockRecord {
            base_point: Default::default(),
            name: "B".to_string(),
            entities,
        },
    );
    let r = hit_test(&db, p(7.0, 100.0), 3.5);
    assert_eq!(r.hits.len(), 1, "{r:?}");
    assert_eq!(r.hits[0].entity_type, "XLINE");
    assert!((r.hits[0].distance - 3.0).abs() < 1e-9, "{r:?}");
    assert_eq!(r.hits[0].via.len(), 1);
}

#[test]
fn a_light_is_where_it_sits_not_where_it_aims() {
    let db = drawing(vec![entity(
        "LIGHT",
        1,
        json!({
            "position": xyz(2.0, 3.0, 9.0), "target": xyz(20.0, 3.0, 0.0),
            "light_type": "SPOT"
        }),
    )]);
    assert_eq!(distances(&db, p(2.0, 3.0), 1e-9), [0.0]);
    assert!(distances(&db, p(20.0, 3.0), 1.0).is_empty());
}

#[test]
fn geometry_the_model_does_not_carry_is_named_per_entity() {
    // A 2D polyline with no vertex, a multileader whose one line is a single
    // point, a 3D polyline of one vertex: each type is measured, but not
    // these entities.
    let one = |kind: &str, fields: Value| {
        let db = drawing(vec![entity(kind, 1, fields)]);
        assert_no_geometry(&hit_test(&db, p(0.0, 0.0), 1.0));
    };
    one(
        "POLYLINE_2D",
        json!({"vertices": [], "closed": false, "const_width": 0.0,
               "elevation": 0.0, "extrusion": xyz(0.0, 0.0, 1.0)}),
    );
    one(
        "MULTILEADER",
        json!({"leaders": [{"lines": [[xyz(1.0, 2.0, 0.0)]], "last_point": null, "dogleg": null}]}),
    );
    one(
        "POLYLINE_3D",
        json!({"vertices": [xyz(1.0, 2.0, 3.0)], "closed": false}),
    );
}

#[test]
fn a_multileader_line_runs_on_to_its_roots_last_point_and_dogleg() {
    // One vertex at the origin, the root's last point at (10, 0), a dogleg
    // of 2 along +y: drawn as (0,0)-(10,0) and (10,0)-(10,2).
    let db = drawing(vec![entity(
        "MULTILEADER",
        1,
        json!({"leaders": [{
            "lines": [[xyz(0.0, 0.0, 0.0)]],
            "last_point": xyz(10.0, 0.0, 0.0),
            "dogleg": {"direction": xyz(0.0, 1.0, 0.0), "length": 2.0}
        }]}),
    )]);
    assert_eq!(distances(&db, p(5.0, 0.0), 1e-9), [0.0]);
    assert_eq!(distances(&db, p(10.0, 1.5), 1e-9), [0.0]);
    assert!(distances(&db, p(10.0, 3.0), 0.5).is_empty());
}
