//! Where a drawing is: the extent of each space in the summary.

use iron_scout_cad::{summarize, Bounds, SpaceExtent};
use serde_json::{json, Value};
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

/// A drawing whose spaces hold the given entities.
fn drawing(spaces: Vec<(&str, Vec<Value>)>) -> CadDatabase {
    let mut db = CadDatabase {
        entities: Vec::new(),
        tables: Default::default(),
        read_diagnostics: Default::default(),
    };
    for (name, entities) in spaces {
        let entities: Vec<uncad_model::model::Entity> =
            serde_json::from_value(Value::Array(entities)).expect("the entities deserialize");
        db.entities.extend(entities.iter().cloned());
        db.tables.block_records.insert(
            name.to_string(),
            BlockRecord {
                name: name.to_string(),
                entities,
            },
        );
    }
    db
}

fn p(x: f64, y: f64) -> Point2D {
    Point2D { x, y }
}

fn arc(id: u64, facing: f64) -> Value {
    entity(
        "ARC",
        id,
        json!({
            "center": {"x": 0.0, "y": 0.0, "z": 0.0}, "radius": 10.0,
            "start_angle": 0.0, "end_angle": std::f64::consts::FRAC_PI_2,
            "extrusion": {"x": 0.0, "y": 0.0, "z": facing}
        }),
    )
}

fn only(db: &CadDatabase) -> SpaceExtent {
    let mut extents = summarize(db).extents;
    assert_eq!(extents.len(), 1, "{extents:?}");
    extents.remove(0)
}

fn close(a: Point2D, b: Point2D) -> bool {
    (a.x - b.x).abs() < 1e-12 && (a.y - b.y).abs() < 1e-12
}

#[test]
fn an_arc_reaches_as_far_as_its_sweep_not_its_circle() {
    let e = only(&drawing(vec![("*Model_Space", vec![arc(1, 1.0)])]));
    let b = e.bounds.unwrap();
    assert!(
        close(b.min, p(0.0, 0.0)) && close(b.max, p(10.0, 10.0)),
        "{b:?}"
    );
}

#[test]
fn a_mirror_copys_arc_is_where_it_is_drawn() {
    // Seen from below, the quarter from 0 to 90 degrees is drawn in the
    // world's second quadrant.
    let e = only(&drawing(vec![("*Model_Space", vec![arc(1, -1.0)])]));
    let b = e.bounds.unwrap();
    assert!(
        close(b.min, p(-10.0, 0.0)) && close(b.max, p(0.0, 10.0)),
        "{b:?}"
    );
}

#[test]
fn each_space_has_its_own_extent_and_says_what_it_did_not_measure() {
    let line = |id, x: f64| {
        entity(
            "LINE",
            id,
            json!({"start_point": {"x": x, "y": 0.0, "z": 0.0}, "end_point": {"x": x + 1.0, "y": 2.0, "z": 0.0}}),
        )
    };
    let spline = entity(
        "SPLINE",
        3,
        json!({"degree": 3, "closed": false, "periodic": false, "knots": [], "weights": [],
               "fit_points": [], "control_points": []}),
    );
    let db = drawing(vec![
        ("*Model_Space", vec![line(1, 1000.0), spline]),
        ("*Paper_Space", vec![line(2, 0.0)]),
    ]);
    let extents = summarize(&db).extents;
    let names: Vec<&str> = extents.iter().map(|e| e.space.as_str()).collect();
    assert_eq!(names, ["*Model_Space", "*Paper_Space"]);
    assert_eq!(
        extents[0].bounds,
        Some(Bounds {
            min: p(1000.0, 0.0),
            max: p(1001.0, 2.0)
        })
    );
    assert_eq!(extents[0].not_measured, ["SPLINE"]);
    assert_eq!(
        extents[1].bounds,
        Some(Bounds {
            min: p(0.0, 0.0),
            max: p(1.0, 2.0)
        })
    );
    assert!(extents[1].not_measured.is_empty());
}

#[test]
fn the_general_parts_extent_holds_every_hole_and_every_dimension() {
    let db: CadDatabase = serde_json::from_str(include_str!("golden/g1.expected.json")).unwrap();
    let s = summarize(&db);
    let model = s
        .extents
        .iter()
        .find(|e| e.space == "*Model_Space")
        .expect("G1 has model space");
    let b = model.bounds.expect("G1 has measured entities");
    for e in &db.entities {
        if let uncad_model::model::Entity::Circle(c) = e {
            assert!(b.min.x <= c.center.x - c.radius && c.center.x + c.radius <= b.max.x);
            assert!(b.min.y <= c.center.y - c.radius && c.center.y + c.radius <= b.max.y);
        }
    }
    for d in &s.dimensions {
        let t = d.text_midpoint;
        assert!(b.min.x <= t.x && t.x <= b.max.x && b.min.y <= t.y && t.y <= b.max.y);
    }
}
