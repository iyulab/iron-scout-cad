//! A block reference with a grid (a multiple insert) places its block in every
//! cell of it: each answer that follows block references -- the extent, the
//! signature, the search for what is at a point, the relations of a drawing --
//! reads every cell, and a reference that cannot be followed or has no cell is
//! said so once.

use iron_scout_cad::{hit_test, relate, signature, summarize, NotSearchedReason};
use serde_json::{json, Value};
use uncad_model::tables::BlockRecord;
use uncad_model::{CadDatabase, Point2D};

fn common(id: u64) -> Value {
    json!({
        "id": id.to_string(), "origin": "VECTOR", "confidence": "HIGH",
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
                external_reference: None,
            },
        );
    }
    db
}

/// The reference `insert` with a grid.
fn gridded(
    mut insert: Value,
    columns: u32,
    rows: u32,
    column_spacing: f64,
    row_spacing: f64,
) -> Value {
    insert["array"] = json!({
        "columns": columns, "rows": rows,
        "column_spacing": column_spacing, "row_spacing": row_spacing,
    });
    insert
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A circle of radius 1 at the block's origin, placed at (100, 0) in three
/// columns 50 apart and two rows 30 apart: it reaches from x 99 to 201 and
/// from y -1 to 31.
#[test]
fn the_extent_reaches_every_cell_of_a_grid() {
    let db = drawing(
        vec![gridded(
            insert(5, "B", (100.0, 0.0), (1.0, 1.0), 0.0),
            3,
            2,
            50.0,
            30.0,
        )],
        vec![("B", vec![circle(1, (0.0, 0.0), 1.0)])],
    );
    let extents = summarize(&db).extents;
    assert_eq!(extents.len(), 1, "{extents:?}");
    let b = extents[0].bounds.expect("a box");
    assert!(near(b.min.x, 99.0) && near(b.max.x, 201.0), "{b:?}");
    assert!(near(b.min.y, -1.0) && near(b.max.y, 31.0), "{b:?}");
}

#[test]
fn the_signature_counts_the_block_once_for_each_cell() {
    let one = drawing(
        vec![insert(5, "B", (100.0, 0.0), (1.0, 1.0), 0.0)],
        vec![("B", vec![circle(1, (0.0, 0.0), 1.0)])],
    );
    let grid = drawing(
        vec![gridded(
            insert(5, "B", (100.0, 0.0), (1.0, 1.0), 0.0),
            4,
            3,
            20.0,
            20.0,
        )],
        vec![("B", vec![circle(1, (0.0, 0.0), 1.0)])],
    );
    assert_eq!(signature(&one).entities.get("CIRCLE"), Some(&1));
    assert_eq!(signature(&grid).entities.get("CIRCLE"), Some(&12));
}

/// The circle of the cell at column 1 -- centred (150, 0) -- is found where
/// it is drawn, through the reference; that cell's insertion point is an
/// anchor of the reference.
#[test]
fn a_point_is_found_on_the_circle_of_any_cell_and_at_any_cells_insertion_point() {
    let db = drawing(
        vec![gridded(
            insert(5, "B", (100.0, 0.0), (1.0, 1.0), 0.0),
            2,
            1,
            50.0,
            0.0,
        )],
        vec![("B", vec![circle(1, (0.0, 0.0), 5.0)])],
    );
    let on_circle = hit_test(&db, Point2D { x: 155.0, y: 0.0 }, 0.1);
    assert!(
        on_circle
            .hits
            .iter()
            .any(|h| h.id.value() == 1 && !h.anchored),
        "{:?}",
        on_circle.hits
    );
    let at_cell = hit_test(&db, Point2D { x: 150.0, y: 0.0 }, 0.1);
    assert!(
        at_cell.hits.iter().any(|h| h.id.value() == 5 && h.anchored),
        "{:?}",
        at_cell.hits
    );
    // A point where no cell draws anything finds nothing.
    assert!(hit_test(&db, Point2D { x: 125.0, y: 0.0 }, 0.1)
        .hits
        .is_empty());
}

/// A reference that cannot be followed is said so once, not once per cell;
/// a grid with no cell is said so, in each answer that names what it left out.
#[test]
fn a_grid_that_is_not_followed_is_named_once_and_an_empty_one_is_named() {
    let missing = drawing(
        vec![gridded(
            insert(5, "NOWHERE", (0.0, 0.0), (1.0, 1.0), 0.0),
            6,
            6,
            5.0,
            5.0,
        )],
        vec![],
    );
    let found = hit_test(&missing, Point2D { x: 0.0, y: 0.0 }, 1.0);
    let missing_blocks = found
        .not_searched
        .iter()
        .filter(|n| n.reason == NotSearchedReason::BlockUndefined)
        .count();
    assert_eq!(missing_blocks, 1, "{:?}", found.not_searched);
    assert_eq!(
        signature(&missing).not_measured.get("INSERT_UNRESOLVED"),
        Some(&1)
    );
    let related = relate(&missing, None).unwrap();
    let missing_blocks = related
        .not_searched
        .iter()
        .filter(|n| n.reason == NotSearchedReason::BlockUndefined)
        .count();
    assert_eq!(missing_blocks, 1, "{:?}", related.not_searched);

    let empty = drawing(
        vec![gridded(
            insert(5, "B", (0.0, 0.0), (1.0, 1.0), 0.0),
            0,
            2,
            5.0,
            5.0,
        )],
        vec![("B", vec![circle(1, (0.0, 0.0), 1.0)])],
    );
    assert_eq!(
        signature(&empty).not_measured.get("INSERT_EMPTY_GRID"),
        Some(&1)
    );
    assert!(summarize(&empty).extents[0]
        .not_measured
        .iter()
        .any(|n| n == "INSERT_EMPTY_GRID"));
    let found = hit_test(&empty, Point2D { x: 0.0, y: 0.0 }, 1.0);
    assert!(found
        .not_searched
        .iter()
        .any(|n| n.reason == NotSearchedReason::BlockReferenceEmptyGrid));
    let related = relate(&empty, None).unwrap();
    assert!(related
        .not_searched
        .iter()
        .any(|n| n.reason == NotSearchedReason::BlockReferenceEmptyGrid));
}
