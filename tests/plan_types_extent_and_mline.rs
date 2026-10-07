//! An MLINE is pointed at by its parallel lines, and every type the hit test
//! measures in plan gives the summary's extent -- and a window selection --
//! the same points: what can be pointed at can be found by where it is.

use iron_scout_cad::{hit_test, select, summarize, Bounds, NotSearchedReason, Selection};
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

fn entity(kind: &str, id: u64, fields: Value) -> Value {
    let mut e = fields;
    e["type"] = json!(kind);
    e["common"] = common(id);
    e
}

fn xyz(x: f64, y: f64, z: f64) -> Value {
    json!({"x": x, "y": y, "z": z})
}

/// A model space holding `entities`, with the MLINE style `WALL` (offsets
/// 0.5 and -0.5).
fn model_space(entities: Vec<Value>) -> CadDatabase {
    let entities: Vec<uncad_model::model::Entity> =
        serde_json::from_value(Value::Array(entities)).expect("the entities deserialize");
    let mut db = CadDatabase {
        entities: entities.clone(),
        tables: Default::default(),
        header: Default::default(),
        read_diagnostics: Default::default(),
    };
    db.tables.block_records.insert(
        "*Model_Space".to_string(),
        BlockRecord {
            base_point: Default::default(),
            name: "*Model_Space".to_string(),
            entities,
            external_reference: None,
        },
    );
    db.tables
        .mlinestyles
        .insert("WALL".to_string(), vec![0.5, -0.5]);
    db
}

/// A wall 200 thick along x from (0, 0) to (1000, 0), drawn in `style` at
/// `scale`.
fn wall(style: &str, scale: Value) -> Value {
    entity(
        "MLINE",
        1,
        json!({
            "vertices": [
                {"point": xyz(0.0, 0.0, 0.0), "miter_direction": xyz(0.0, 1.0, 0.0)},
                {"point": xyz(1000.0, 0.0, 0.0), "miter_direction": xyz(0.0, 1.0, 0.0)}
            ],
            "closed": false,
            "mlinestyle_name": {"type": "RESOLVED", "data": style},
            "scale": scale
        }),
    )
}

#[test]
fn an_mline_is_its_lines_at_the_styles_offsets_times_its_scale() {
    let db = model_space(vec![wall("WALL", json!(200.0))]);
    for y in [100.0, -100.0] {
        let r = hit_test(&db, p(500.0, y), 1e-9);
        assert_eq!(r.hits.len(), 1, "{r:?}");
        assert_eq!(r.hits[0].distance, 0.0);
    }
    // Its centerline is not drawn.
    let r = hit_test(&db, p(500.0, 0.0), 1.0);
    assert!(r.hits.is_empty() && r.unsupported.is_empty(), "{r:?}");
}

#[test]
fn an_mline_without_its_style_or_scale_is_not_guessed() {
    for db in [
        model_space(vec![wall("GONE", json!(200.0))]),
        model_space(vec![wall("WALL", Value::Null)]),
    ] {
        let r = hit_test(&db, p(500.0, 100.0), 1.0);
        assert!(r.hits.is_empty());
        assert_eq!(r.not_searched.len(), 1, "{r:?}");
        assert_eq!(r.not_searched[0].reason, NotSearchedReason::StyleUndefined);
    }
}

fn bounds_of(db: &CadDatabase) -> (Point2D, Point2D) {
    let extents = summarize(db).extents;
    assert_eq!(extents.len(), 1);
    assert!(extents[0].not_measured.is_empty(), "{extents:?}");
    let b = extents[0].bounds.expect("the space has an extent");
    (b.min, b.max)
}

#[test]
fn an_mlines_extent_is_its_lines() {
    assert_eq!(
        bounds_of(&model_space(vec![wall("WALL", json!(200.0))])),
        (p(0.0, -100.0), p(1000.0, 100.0))
    );
}

#[test]
fn a_hatchs_extent_holds_its_arc_edges() {
    // The right half of a circle of radius 5 and the diameter back: the arc
    // reaches x = 5, beyond both of its ends.
    let hatch = entity(
        "HATCH",
        1,
        json!({
            "boundary_paths": [{"type": "EDGES", "data": [
                {"type": "ARC", "center": {"x": 0.0, "y": 0.0}, "radius": 5.0,
                 "start_angle": -std::f64::consts::FRAC_PI_2,
                 "end_angle": std::f64::consts::FRAC_PI_2, "is_ccw": true},
                {"type": "LINE", "start": {"x": 0.0, "y": 5.0}, "end": {"x": 0.0, "y": -5.0}}
            ]}],
            "solid_fill": true, "gradient": null, "pattern_lines": [],
            "elevation": 0.0, "extrusion": xyz(0.0, 0.0, 1.0), "style": null
        }),
    );
    let (min, max) = bounds_of(&model_space(vec![hatch]));
    assert!(
        (min.x - 0.0).abs() < 1e-12 && (min.y + 5.0).abs() < 1e-12,
        "{min:?}"
    );
    assert!(
        (max.x - 5.0).abs() < 1e-12 && (max.y - 5.0).abs() < 1e-12,
        "{max:?}"
    );
}

#[test]
fn the_plan_types_give_their_points() {
    let db = model_space(vec![
        entity(
            "POLYLINE_3D",
            1,
            json!({"vertices": [xyz(0.0, 0.0, 9.0), xyz(10.0, 20.0, -9.0)], "closed": false}),
        ),
        entity(
            "LIGHT",
            3,
            json!({"position": xyz(5.0, 40.0, 3.0), "target": xyz(0.0, 0.0, 0.0), "light_type": null}),
        ),
        entity(
            "REGION",
            4,
            json!({"wireframe_edges": [[xyz(50.0, 0.0, 1.0), xyz(60.0, 0.0, 1.0)]]}),
        ),
    ]);
    assert_eq!(bounds_of(&db), (p(0.0, 0.0), p(60.0, 40.0)));
}

#[test]
fn a_body_with_depth_is_still_not_measured() {
    let db = model_space(vec![entity(
        "3DSOLID",
        1,
        json!({"wireframe_edges": [[xyz(0.0, 0.0, 0.0), xyz(10.0, 0.0, 5.0)]]}),
    )]);
    let extents = summarize(&db).extents;
    assert_eq!(extents[0].not_measured, ["3DSOLID"]);
    assert!(extents[0].bounds.is_none());
}

#[test]
fn a_window_finds_what_a_point_finds() {
    // An image frame 10 x 5 from (20, 30): a window over its corner finds it.
    let db = model_space(vec![entity(
        "IMAGE",
        7,
        json!({
            "insertion_point": xyz(20.0, 30.0, 0.0),
            "u_vector": xyz(0.1, 0.0, 0.0), "v_vector": xyz(0.0, 0.1, 0.0),
            "size_pixels": {"x": 100.0, "y": 50.0},
            "definition": {"type": "ABSENT"},
            "display_flags": null, "clipping": null, "brightness": null,
            "contrast": null, "fade": null, "clip_outside": null, "boundary": []
        }),
    )]);
    assert_eq!(hit_test(&db, p(20.0, 30.0), 1e-9).hits.len(), 1);
    let picked = select(
        &db,
        &Selection::new().crossing(Bounds {
            min: p(29.0, 34.0),
            max: p(40.0, 40.0),
        }),
    );
    assert_eq!(picked.total, 1, "{picked:?}");
    assert!(picked.not_measured.is_empty(), "{picked:?}");
}
