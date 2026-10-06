//! The extent of what block references draw: the entities of their blocks,
//! placed where the reference puts them, measured exactly under any
//! placement -- and named when they cannot be measured.

use iron_scout_cad::{select, summarize, Selection, SpaceExtent};
use serde_json::{json, Value};
use std::f64::consts::FRAC_PI_4;
use uncad_model::model::EntityId;
use uncad_model::tables::BlockRecord;
use uncad_model::{CadDatabase, Point2D};

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

fn xyz(x: f64, y: f64) -> Value {
    json!({"x": x, "y": y, "z": 0.0})
}

fn insert(id: u64, block: &str, at: (f64, f64), scale: (f64, f64), rotation: f64) -> Value {
    entity(
        "INSERT",
        id,
        json!({
            "block_name": {"type": "RESOLVED", "data": block},
            "insertion_point": xyz(at.0, at.1),
            "scale": {"x": scale.0, "y": scale.1, "z": 1.0}, "rotation": rotation,
            "attribs": []
        }),
    )
}

fn circle(id: u64, at: (f64, f64), radius: f64) -> Value {
    entity(
        "CIRCLE",
        id,
        json!({"center": xyz(at.0, at.1), "radius": radius}),
    )
}

/// A drawing whose model space holds `model`, with the named blocks beside it
/// (each based at the origin unless its name says otherwise).
fn drawing(model: Vec<Value>, blocks: Vec<(&str, Vec<Value>)>) -> CadDatabase {
    let mut db = CadDatabase {
        entities: Vec::new(),
        tables: Default::default(),
        header: Default::default(),
        read_diagnostics: Default::default(),
    };
    let model: Vec<uncad_model::model::Entity> =
        serde_json::from_value(Value::Array(model)).expect("the entities deserialize");
    db.entities.extend(model.iter().cloned());
    for (name, entities) in std::iter::once(("*Model_Space", model)).chain(blocks.into_iter().map(
        |(name, entities)| {
            let entities = serde_json::from_value(Value::Array(entities))
                .expect("the block's entities deserialize");
            (name, entities)
        },
    )) {
        db.tables.block_records.insert(
            name.to_string(),
            BlockRecord {
                base_point: Default::default(),
                name: name.to_string(),
                entities,
            },
        );
    }
    db
}

fn only(db: &CadDatabase) -> SpaceExtent {
    let mut extents = summarize(db).extents;
    assert_eq!(extents.len(), 1, "{extents:?}");
    extents.remove(0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_circle_in_a_turned_and_scaled_block_reaches_where_it_is_drawn() {
    // A circle of radius 1 at (10, 0) in the block; the block placed at
    // (100, 0), doubled and turned a quarter: the circle is drawn at
    // (100, 20) with radius 2.
    let db = drawing(
        vec![insert(
            5,
            "B",
            (100.0, 0.0),
            (2.0, 2.0),
            std::f64::consts::FRAC_PI_2,
        )],
        vec![("B", vec![circle(1, (10.0, 0.0), 1.0)])],
    );
    let b = only(&db).bounds.expect("a box");
    // The insertion point (100, 0) and the circle (98..102, 18..22).
    assert!(near(b.min.x, 98.0) && near(b.max.x, 102.0), "{b:?}");
    assert!(near(b.min.y, 0.0) && near(b.max.y, 22.0), "{b:?}");
}

#[test]
fn a_circle_in_a_block_scaled_differently_along_each_axis_is_measured_as_the_ellipse_drawn() {
    // A unit circle, scaled (2, 1) and turned an eighth: an ellipse whose
    // reach in x is sqrt((2 cos 45)^2 + (sin 45)^2) = sqrt(2.5).
    let db = drawing(
        vec![insert(5, "B", (0.0, 0.0), (2.0, 1.0), FRAC_PI_4)],
        vec![("B", vec![circle(1, (0.0, 0.0), 1.0)])],
    );
    let b = only(&db).bounds.expect("a box");
    assert!(
        near(b.max.x, 2.5f64.sqrt()) && near(b.min.x, -(2.5f64.sqrt())),
        "{b:?}"
    );
    assert!(near(b.max.y, 2.5f64.sqrt()), "{b:?}");
}

#[test]
fn an_arc_in_a_mirrored_block_reaches_where_it_is_drawn() {
    // A quarter arc from +x to +y, mirrored in x: drawn from -x to +y.
    let arc = entity(
        "ARC",
        1,
        json!({
            "center": xyz(0.0, 0.0), "radius": 10.0,
            "start_angle": 0.0, "end_angle": std::f64::consts::FRAC_PI_2,
        }),
    );
    let db = drawing(
        vec![insert(5, "B", (0.0, 0.0), (-1.0, 1.0), 0.0)],
        vec![("B", vec![arc])],
    );
    let b = only(&db).bounds.expect("a box");
    assert!(near(b.min.x, -10.0) && near(b.max.x, 0.0), "{b:?}");
    assert!(near(b.min.y, 0.0) && near(b.max.y, 10.0), "{b:?}");
}

#[test]
fn a_reference_is_what_sets_a_side_not_what_it_holds() {
    let db = drawing(
        vec![
            circle(1, (0.0, 0.0), 1.0),
            insert(5, "B", (50.0, 0.0), (1.0, 1.0), 0.0),
        ],
        vec![("B", vec![circle(2, (10.0, 0.0), 1.0)])],
    );
    let e = only(&db);
    let by = e.bounded_by.expect("sides");
    assert!(near(e.bounds.expect("a box").max.x, 61.0));
    assert_eq!(by.max_x, EntityId::new(5));
    assert_eq!(by.min_x, EntityId::new(1));
}

#[test]
fn a_table_is_measured_by_what_its_block_draws() {
    // The table's block is placed as though based at the origin, whatever
    // its base point says -- where the renderer places it.
    let table = entity(
        "ACAD_TABLE",
        7,
        json!({
            "block_name": {"type": "RESOLVED", "data": "*T1"},
            "insertion_point": xyz(10.0, 20.0),
            "scale": {"x": 1.0, "y": 1.0, "z": 1.0}, "rotation": 0.0,
            "grid": null,
        }),
    );
    let line = entity(
        "LINE",
        900,
        json!({"start_point": xyz(0.0, 0.0), "end_point": xyz(30.0, -8.0)}),
    );
    let mut db = drawing(vec![table], vec![("*T1", vec![line])]);
    db.tables
        .block_records
        .get_mut("*T1")
        .expect("the block")
        .base_point = uncad_model::Point3D {
        x: 100.0,
        y: 100.0,
        z: 0.0,
    };
    let e = only(&db);
    let b = e.bounds.expect("a box");
    assert!(near(b.min.x, 10.0) && near(b.max.x, 40.0), "{b:?}");
    assert!(near(b.min.y, 12.0) && near(b.max.y, 20.0), "{b:?}");
    assert!(!e.not_measured.contains(&"ACAD_TABLE".to_string()), "{e:?}");
}

#[test]
fn an_ordinate_dimensions_datum_is_not_measured() {
    // DXF 10 of an ordinate dimension is the origin it measures from.
    let dimension = |kind: &str| {
        entity(
            "DIMENSION",
            3,
            json!({
                "block_name": {"type": "ABSENT"},
                "kind": kind, "measurement": 50.0,
                "text_override": {"type": "MEASURED"},
                "definition_point": xyz(0.0, 0.0),
                "text_midpoint": {"x": 26.0, "y": 50.0},
                "points": {"extension1": xyz(24.0, 50.0), "extension2": xyz(25.0, 50.0), "radial": null, "arc": null},
                "rotation": 0.0, "text_rotation": 0.0,
                "style_name": {"type": "ABSENT"},
                "ordinate_axis": null
            }),
        )
    };
    let b = only(&drawing(vec![dimension("ORDINATE")], vec![]))
        .bounds
        .expect("a box");
    assert!(near(b.min.x, 24.0) && near(b.min.y, 50.0), "{b:?}");
    // Another kind's DXF 10 is on the line it draws, and is measured.
    let b = only(&drawing(vec![dimension("ROTATED")], vec![]))
        .bounds
        .expect("a box");
    assert!(near(b.min.x, 0.0) && near(b.min.y, 0.0), "{b:?}");
}

#[test]
fn a_reference_whose_block_is_missing_is_named_and_measured_by_its_insertion_point() {
    let db = drawing(vec![insert(5, "GONE", (3.0, 4.0), (1.0, 1.0), 0.0)], vec![]);
    let e = only(&db);
    assert_eq!(e.not_measured, ["INSERT_UNRESOLVED"]);
    let b = e.bounds.expect("a box");
    assert!(near(b.min.x, 3.0) && near(b.max.y, 4.0), "{b:?}");
}

#[test]
fn a_block_that_draws_itself_is_named_and_measured_once() {
    let db = drawing(
        vec![insert(5, "A", (0.0, 0.0), (1.0, 1.0), 0.0)],
        vec![(
            "A",
            vec![
                circle(1, (0.0, 0.0), 1.0),
                insert(2, "A", (10.0, 0.0), (1.0, 1.0), 0.0),
            ],
        )],
    );
    let e = only(&db);
    assert_eq!(e.not_measured, ["INSERT_CYCLE"]);
    let b = e.bounds.expect("a box");
    // The circle, and the inner reference's insertion point.
    assert!(near(b.min.x, -1.0) && near(b.max.x, 10.0), "{b:?}");
}

#[test]
fn what_a_block_holds_that_gives_no_point_is_named() {
    let ray = entity(
        "RAY",
        1,
        json!({"point": xyz(0.0, 0.0), "vector": xyz(1.0, 0.0)}),
    );
    let db = drawing(
        vec![insert(5, "B", (0.0, 0.0), (1.0, 1.0), 0.0)],
        vec![("B", vec![ray, circle(2, (0.0, 0.0), 1.0)])],
    );
    assert_eq!(only(&db).not_measured, ["RAY"]);
}

#[test]
fn a_window_over_what_a_reference_draws_selects_the_reference() {
    // Nothing of the reference but what it draws reaches the window.
    let db = drawing(
        vec![insert(5, "B", (0.0, 0.0), (1.0, 1.0), 0.0)],
        vec![("B", vec![circle(1, (100.0, 0.0), 1.0)])],
    );
    let window = iron_scout_cad::Bounds {
        min: Point2D { x: 99.0, y: -0.5 },
        max: Point2D { x: 99.5, y: 0.5 },
    };
    let s = select(&db, &Selection::default().crossing(window));
    let ids: Vec<_> = s.entities.iter().map(|e| e.id).collect();
    assert_eq!(ids, [EntityId::new(5)], "{s:?}");
}

#[test]
fn a_polyline_arc_in_a_turned_block_reaches_its_turned_extreme() {
    // A half circle below (0, 0)-(2, 0) about (1, 0), turned an eighth: its
    // circle's rightmost point (angle 0 after the turn) lies on the arc, at
    // x = cos 45 + 1. Turning the arc's own extremes would miss it.
    let polyline = entity(
        "LWPOLYLINE",
        1,
        json!({
            "vertices": [
                {"point": {"x": 0.0, "y": 0.0}, "bulge": 1.0, "start_width": 0.0, "end_width": 0.0},
                {"point": {"x": 2.0, "y": 0.0}, "bulge": 0.0, "start_width": 0.0, "end_width": 0.0},
            ],
            "closed": false, "elevation": 0.0,
            "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
        }),
    );
    let db = drawing(
        vec![insert(5, "B", (0.0, 0.0), (1.0, 1.0), FRAC_PI_4)],
        vec![("B", vec![polyline])],
    );
    let b = only(&db).bounds.expect("a box");
    assert!(near(b.max.x, FRAC_PI_4.cos() + 1.0), "{b:?}");
}

fn on_layer(mut e: Value, layer: &str) -> Value {
    e["common"]["layer"] = json!({"type": "RESOLVED", "data": layer});
    e
}

#[test]
fn what_sits_on_defpoints_is_not_drawn_and_not_measured() {
    // A dimension's block keeps its definition points on DEFPOINTS; a
    // block's layer-0 entity takes the layer of the reference it is in.
    let point = |id, x| entity("POINT", id, json!({"position": xyz(x, 0.0)}));
    let db = drawing(
        vec![
            circle(1, (50.0, 0.0), 1.0),
            on_layer(point(2, -100.0), "Defpoints"),
            insert(5, "B", (0.0, 0.0), (1.0, 1.0), 0.0),
            on_layer(insert(6, "C", (0.0, 0.0), (1.0, 1.0), 0.0), "DEFPOINTS"),
        ],
        vec![
            (
                "B",
                vec![
                    on_layer(point(3, -200.0), "defpoints"),
                    circle(4, (60.0, 0.0), 1.0),
                ],
            ),
            ("C", vec![point(7, -300.0)]),
        ],
    );
    let b = only(&db).bounds.expect("a box");
    assert!(near(b.min.x, 0.0) && near(b.max.x, 61.0), "{b:?}");
    // A selection judges an entity it is asked about by where it is.
    let window = iron_scout_cad::Bounds {
        min: Point2D { x: -101.0, y: -1.0 },
        max: Point2D { x: -99.0, y: 1.0 },
    };
    let s = select(&db, &Selection::default().crossing(window));
    let ids: Vec<_> = s.entities.iter().map(|e| e.id).collect();
    assert_eq!(ids, [EntityId::new(2)], "{s:?}");
}
