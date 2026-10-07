//! Tables (ACAD_TABLE): what their cells say, in the summary, and what their
//! blocks draw, under the point.

use iron_scout_cad::{hit_test, summarize, NotSearchedReason};
use serde_json::{json, Value};
use uncad_model::model::EntityId;
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

fn xyz(x: f64, y: f64) -> Value {
    json!({"x": x, "y": y, "z": 0.0})
}

fn entity(kind: &str, id: u64, fields: Value) -> Value {
    let mut e = fields;
    e["type"] = json!(kind);
    e["common"] = common(id);
    e
}

fn cell(text: Option<&str>, covered: bool, span_columns: u32) -> Value {
    json!({
        "kind": "TEXT", "text": text, "covered": covered,
        "span_columns": span_columns, "span_rows": 1
    })
}

/// A two by two parts list: a title over both columns, then one row of
/// values -- one written with an MTEXT font switch.
fn grid() -> Value {
    json!({
        "column_widths": [20.0, 10.0],
        "rows": [
            {"height": 5.0, "cells": [cell(Some("PARTS"), false, 2), cell(None, true, 1)]},
            {"height": 4.0, "cells": [cell(Some(r"{\fArial;S45C}"), false, 1), cell(Some("4"), false, 1)]},
        ]
    })
}

fn table(id: u64, block: &str, at: (f64, f64), rotation: f64, grid: Value) -> Value {
    entity(
        "ACAD_TABLE",
        id,
        json!({
            "block_name": {"type": "RESOLVED", "data": block},
            "insertion_point": xyz(at.0, at.1),
            "scale": {"x": 1.0, "y": 1.0, "z": 1.0},
            "rotation": rotation,
            "grid": grid,
        }),
    )
}

/// A drawing of `entities` and one block `*T1` drawing a line from (0, 0)
/// to (30, 0), whose base point is not the origin.
fn drawing(entities: Vec<Value>) -> CadDatabase {
    let block: BlockRecord = serde_json::from_value(json!({
        "name": "*T1",
        "base_point": xyz(100.0, 100.0),
        "entities": [entity("LINE", 900, json!({"start_point": xyz(0.0, 0.0), "end_point": xyz(30.0, 0.0)}))],
    }))
    .expect("the block deserializes");
    let mut db = CadDatabase {
        entities: serde_json::from_value(Value::Array(entities)).expect("the entities deserialize"),
        tables: Default::default(),
        header: Default::default(),
        read_diagnostics: Default::default(),
    };
    db.tables.block_records.insert(block.name.clone(), block);
    db
}

#[test]
fn a_table_is_listed_with_what_its_cells_say() {
    let s = summarize(&drawing(vec![table(7, "*T1", (10.0, 20.0), 0.0, grid())]));
    assert_eq!(s.tables.len(), 1);
    let t = &s.tables[0];
    assert_eq!(t.id, EntityId::new(7));
    assert_eq!(t.insertion_point, p(10.0, 20.0));
    let cells = t.cells.as_ref().expect("the cells were read");
    assert_eq!((cells.rows, cells.columns), (2, 2));
    // The covered cell under the title says nothing and is not listed.
    let listed: Vec<_> = cells
        .texts
        .iter()
        .map(|c| (c.row, c.column, c.plain.as_str(), c.span_columns))
        .collect();
    assert_eq!(
        listed,
        [(0, 0, "PARTS", 2), (1, 0, "S45C", 1), (1, 1, "4", 1)]
    );
    // The text as written stays beside the plain one.
    assert_eq!(cells.texts[1].text, r"{\fArial;S45C}");
}

#[test]
fn a_table_whose_cells_were_not_read_says_so() {
    let s = summarize(&drawing(vec![table(
        7,
        "*T1",
        (0.0, 0.0),
        0.0,
        Value::Null,
    )]));
    assert_eq!(s.tables[0].cells, None);
    let value = serde_json::to_value(&s).unwrap();
    assert!(value["tables"][0]["cells"].is_null());
}

#[test]
fn a_drawing_without_tables_lists_none() {
    let s = summarize(&drawing(vec![]));
    let value = serde_json::to_value(&s).unwrap();
    assert!(value.get("tables").is_none(), "{value}");
}

#[test]
fn what_a_tables_block_draws_is_hit_through_the_table() {
    // Inserted at (10, 20) and turned a quarter: the block's line runs from
    // (10, 20) up to (10, 50) -- placed from the origin, as the renderer
    // draws it, not from the block's base point.
    let db = drawing(vec![table(
        7,
        "*T1",
        (10.0, 20.0),
        std::f64::consts::FRAC_PI_2,
        grid(),
    )]);
    let r = hit_test(&db, p(10.0, 40.0), 1e-9);
    let line = r
        .hits
        .iter()
        .find(|h| h.entity_type == "LINE")
        .expect("the line the table's block draws");
    assert_eq!(line.id, EntityId::new(900));
    assert_eq!(line.via, [EntityId::new(7)]);
    assert!(
        !r.unsupported.contains(&"ACAD_TABLE".to_string()),
        "{:?}",
        r.unsupported
    );
}

#[test]
fn a_table_is_found_at_its_insertion_point() {
    let db = drawing(vec![table(7, "*T1", (10.0, 20.0), 0.0, grid())]);
    let r = hit_test(&db, p(10.0, 20.0), 1e-9);
    let own = r
        .hits
        .iter()
        .find(|h| h.entity_type == "ACAD_TABLE")
        .expect("the table itself");
    assert!(own.anchored);
    assert_eq!(own.id, EntityId::new(7));
}

#[test]
fn a_table_naming_a_block_the_drawing_lacks_is_not_searched_and_says_so() {
    let db = drawing(vec![table(7, "*T9", (0.0, 0.0), 0.0, grid())]);
    let r = hit_test(&db, p(50.0, 50.0), 1e-9);
    assert_eq!(r.not_searched.len(), 1, "{:?}", r.not_searched);
    let n = &r.not_searched[0];
    assert_eq!(
        (n.id, n.entity_type.as_str()),
        (EntityId::new(7), "ACAD_TABLE")
    );
    assert_eq!(n.reason, NotSearchedReason::BlockUndefined);
}

/// The parts list with the way its rows run: `flow` is the model's
/// `"DOWN"` or `"UP"`, or `None` when it is not known.
fn flowing(id: u64, at: (f64, f64), rotation: f64, flow: Option<&str>) -> Value {
    let mut t = table(id, "*T1", at, rotation, grid());
    t["flow"] = json!(flow);
    t
}

/// The (row, column) of each cell under `at`.
fn cells(db: &CadDatabase, at: Point2D) -> Vec<(usize, usize)> {
    hit_test(db, at, 1e-9)
        .table_cells
        .iter()
        .map(|c| (c.row, c.column))
        .collect()
}

#[test]
fn a_point_in_a_table_names_its_row_and_column() {
    // Columns 20 and 10 wide from x = 10; rows 5 and 4 high running down
    // from y = 20. The title spans both columns.
    let db = drawing(vec![flowing(7, (10.0, 20.0), 0.0, Some("DOWN"))]);
    assert_eq!(cells(&db, p(15.0, 18.0)), [(0, 0)]);
    // Under the title's span: the title cell, by its first column.
    assert_eq!(cells(&db, p(35.0, 18.0)), [(0, 0)]);
    assert_eq!(cells(&db, p(15.0, 13.0)), [(1, 0)]);
    assert_eq!(cells(&db, p(35.0, 13.0)), [(1, 1)]);
    // Outside the grid: no cell.
    assert!(cells(&db, p(15.0, 10.0)).is_empty());
    assert!(cells(&db, p(41.0, 13.0)).is_empty());
    let hit = &hit_test(&db, p(15.0, 13.0), 1e-9).table_cells[0];
    assert_eq!(hit.table, EntityId::new(7));
    assert!(hit.via.is_empty());
}

#[test]
fn a_point_on_the_edge_between_cells_names_each() {
    let db = drawing(vec![flowing(7, (10.0, 20.0), 0.0, Some("DOWN"))]);
    assert_eq!(cells(&db, p(30.0, 13.0)), [(1, 0), (1, 1)]);
}

#[test]
fn a_table_whose_rows_run_up_counts_its_first_row_at_the_bottom() {
    let db = drawing(vec![flowing(7, (10.0, 20.0), 0.0, Some("UP"))]);
    assert_eq!(cells(&db, p(15.0, 22.0)), [(0, 0)]);
    assert_eq!(cells(&db, p(15.0, 27.0)), [(1, 0)]);
    assert!(cells(&db, p(15.0, 18.0)).is_empty());
}

#[test]
fn a_turned_table_counts_its_cells_along_its_own_axes() {
    // Turned a quarter: its columns run up from (10, 20), its rows to the
    // right. The table-local point (25, -7) -- row 1, column 1 -- is at
    // (10 + 7, 20 + 25).
    let db = drawing(vec![flowing(
        7,
        (10.0, 20.0),
        std::f64::consts::FRAC_PI_2,
        Some("DOWN"),
    )]);
    assert_eq!(cells(&db, p(17.0, 45.0)), [(1, 1)]);
}

#[test]
fn a_table_drawn_by_a_block_reference_names_the_chain() {
    let mut db = drawing(vec![entity(
        "INSERT",
        3,
        json!({
            "block_name": {"type": "RESOLVED", "data": "W"},
            "insertion_point": xyz(100.0, 0.0),
            "scale": {"x": 1.0, "y": 1.0, "z": 1.0}, "rotation": 0.0,
            "attribs": []
        }),
    )]);
    let inner: Vec<uncad_model::model::Entity> =
        serde_json::from_value(json!([flowing(7, (10.0, 20.0), 0.0, Some("DOWN"))]))
            .expect("the table deserializes");
    db.tables.block_records.insert(
        "W".to_string(),
        BlockRecord {
            base_point: Default::default(),
            name: "W".to_string(),
            entities: inner,
            external_reference: None,
        },
    );
    let r = hit_test(&db, p(115.0, 13.0), 1e-9);
    let found: Vec<_> = r
        .table_cells
        .iter()
        .map(|c| (c.table, c.via.clone(), c.row, c.column))
        .collect();
    assert_eq!(found, [(EntityId::new(7), vec![EntityId::new(3)], 1, 0)]);
}

#[test]
fn a_table_whose_rows_way_is_unknown_says_so_where_the_point_is_at_it() {
    let db = drawing(vec![flowing(7, (10.0, 20.0), 0.0, None)]);
    // At its insertion point the table itself is found: which cell is not
    // known, and said so.
    let r = hit_test(&db, p(10.0, 20.0), 1e-9);
    assert!(r.table_cells.is_empty());
    let unknown: Vec<_> = r
        .not_searched
        .iter()
        .filter(|n| n.reason == NotSearchedReason::TableCellsUnknown)
        .map(|n| n.id)
        .collect();
    assert_eq!(unknown, [EntityId::new(7)]);
    // Far from it, nothing is said about it.
    let r = hit_test(&db, p(500.0, 500.0), 1e-9);
    assert!(!r
        .not_searched
        .iter()
        .any(|n| n.reason == NotSearchedReason::TableCellsUnknown));
}
