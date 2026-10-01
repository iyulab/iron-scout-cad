//! Picking entities out of a summary: by type, layer, window, space, ID.

use iron_scout_cad::{select, summarize, summarize_with, Bounds, Selection, SpaceFilter};
use serde_json::{json, Value};
use uncad_model::model::EntityId;
use uncad_model::tables::BlockRecord;
use uncad_model::{CadDatabase, Point2D};

fn common(id: u64, layer: Value) -> Value {
    json!({
        "id": id, "origin": "VECTOR", "confidence": "HIGH",
        "source_handle": {"type": "ABSENT"}, "layer": layer,
        "color_index": 256, "true_color": null, "invisible": false,
        "linetype": {"type": "BY_LAYER"}, "linetype_scale": 1.0,
        "lineweight": -1, "transparency": 0
    })
}

fn named(layer: &str) -> Value {
    json!({"type": "RESOLVED", "data": layer})
}

fn circle(id: u64, layer: Value, x: f64, y: f64, r: f64) -> Value {
    json!({
        "type": "CIRCLE", "common": common(id, layer),
        "center": {"x": x, "y": y, "z": 0.0}, "radius": r,
        "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0}
    })
}

fn line(id: u64, layer: Value, from: (f64, f64), to: (f64, f64)) -> Value {
    json!({
        "type": "LINE", "common": common(id, layer),
        "start_point": {"x": from.0, "y": from.1, "z": 0.0},
        "end_point": {"x": to.0, "y": to.1, "z": 0.0}
    })
}

/// A construction line: it runs without end, so it gives no point to
/// measure.
fn xline(id: u64) -> Value {
    json!({
        "type": "XLINE", "common": common(id, named("0")),
        "point": {"x": 0.0, "y": 0.0, "z": 0.0}, "vector": {"x": 1.0, "y": 0.0, "z": 0.0}
    })
}

fn drawing(spaces: Vec<(&str, Vec<Value>)>) -> CadDatabase {
    let mut db = CadDatabase {
        entities: Vec::new(),
        tables: Default::default(),
        header: Default::default(),
        read_diagnostics: Default::default(),
    };
    for (name, entities) in spaces {
        let entities: Vec<uncad_model::model::Entity> =
            serde_json::from_value(Value::Array(entities)).expect("the entities deserialize");
        db.entities.extend(entities.iter().cloned());
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

/// Three circles on two layers, one of them far off, a line, a construction
/// line, and a circle on a sheet.
fn plan() -> CadDatabase {
    drawing(vec![
        (
            "*Model_Space",
            vec![
                circle(10, named("Pipes"), 0.0, 0.0, 1.75),
                circle(11, named("Pipes"), 0.0, 0.0, 2.0),
                circle(12, named("FURNITURE"), 100.0, 0.0, 8.0),
                line(13, named("Pipes"), (0.0, 0.0), (50.0, 0.0)),
                xline(14),
                circle(
                    15,
                    json!({"type": "UNRESOLVED", "data": "2A"}),
                    0.0,
                    0.0,
                    3.0,
                ),
            ],
        ),
        (
            "*Paper_Space",
            vec![circle(20, named("Pipes"), 5.0, 5.0, 1.0)],
        ),
    ])
}

fn window(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
    Bounds {
        min: Point2D { x: x0, y: y0 },
        max: Point2D { x: x1, y: y1 },
    }
}

fn ids(selection: &iron_scout_cad::Selected) -> Vec<u64> {
    selection
        .entities
        .iter()
        .map(|e| serde_json::to_value(e.id).unwrap().as_u64().unwrap())
        .collect()
}

#[test]
fn a_type_selects_as_many_as_the_summary_counts() {
    let db = plan();
    let by_type = summarize(&db).by_type;
    for (name, count) in by_type {
        let picked = select(&db, &Selection::new().of_type(&name));
        assert_eq!(picked.total, count, "{name}");
    }
    // Names are matched as CAD programs match them.
    let circles = select(&db, &Selection::new().of_type("circle"));
    assert_eq!(ids(&circles), [10, 11, 12, 15, 20]);
}

#[test]
fn every_filter_given_must_hold() {
    let db = plan();
    let picked = select(
        &db,
        &Selection::new()
            .of_type("CIRCLE")
            .on_layer("pipes")
            .in_space(SpaceFilter::Model),
    );
    assert_eq!(ids(&picked), [10, 11]);
    let e = &picked.entities[0];
    assert_eq!(e.space.as_deref(), Some("*Model_Space"));
    assert_eq!(e.bounds, Some(window(-1.75, -1.75, 1.75, 1.75)));
    assert!(e.record.is_none(), "no record unless asked");
}

#[test]
fn a_layer_the_drawing_does_not_hold_matches_no_name() {
    let db = plan();
    for name in ["2A", ""] {
        let picked = select(&db, &Selection::new().on_layer(name));
        assert!(
            !ids(&picked).contains(&15),
            "an unresolved layer is on no named layer: {name:?}"
        );
    }
}

#[test]
fn a_window_keeps_what_reaches_into_it_and_names_what_it_cannot_judge() {
    let db = plan();
    // Crossing: the line from (0,0) to (50,0) reaches into a window around
    // (40,0); the circles at the origin do not; the far circle does not.
    let picked = select(
        &db,
        &Selection::new().crossing(window(39.0, -1.0, 41.0, 1.0)),
    );
    assert_eq!(ids(&picked), [13]);
    assert_eq!(picked.not_measured, ["XLINE"]);

    // Without a window the construction line is selected, with no box.
    let all = select(&db, &Selection::new().of_type("XLINE"));
    assert_eq!(ids(&all), [14]);
    assert_eq!(all.entities[0].bounds, None);
    assert_eq!(all.not_measured, ["XLINE"]);
}

#[test]
fn a_limit_keeps_the_first_by_id_and_the_total_counts_all() {
    let db = plan();
    let picked = select(&db, &Selection::new().of_type("CIRCLE").limit(2));
    assert_eq!(picked.total, 5);
    assert_eq!(ids(&picked), [10, 11]);
}

#[test]
fn ids_pick_those_entities_and_detail_carries_the_model_record() {
    let db = plan();
    let id: EntityId = serde_json::from_value(json!(12)).unwrap();
    let picked = select(&db, &Selection::new().with_ids([id]).with_detail());
    assert_eq!(ids(&picked), [12]);
    // The record is the model's own form -- the field names an edit uses.
    let record = serde_json::to_value(picked.entities[0].record.as_ref().unwrap()).unwrap();
    assert_eq!(record["type"], "CIRCLE");
    assert_eq!(record["radius"], 8.0);
    assert_eq!(record["common"]["layer"], named("FURNITURE"));
}

#[test]
fn a_summary_carries_a_selection_only_when_asked() {
    let db = plan();
    let plain = serde_json::to_value(summarize(&db)).unwrap();
    assert!(plain.get("selection").is_none(), "{plain}");

    let with = summarize_with(&db, &Selection::new().of_type("LINE"));
    let selection = with.selection.as_ref().expect("the selection");
    assert_eq!(ids(selection), [13]);
    // The rest of the summary is the plain one.
    let mut without = serde_json::to_value(&with).unwrap();
    without.as_object_mut().unwrap().remove("selection");
    assert_eq!(without, plain);
}
