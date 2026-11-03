//! A dimension names the space it is drawn in, as a table and a text do: its
//! points and its text's middle are only places on a picture of that space.

use iron_scout_cad::summarize;
use uncad_model::tables::BlockRecord;
use uncad_model::CadDatabase;

/// G25's dimensions, none of them listed by a space record: a model as
/// built by hand names no space for them.
fn unplaced() -> CadDatabase {
    let mut db: CadDatabase = serde_json::from_str(include_str!("golden/g25.expected.json"))
        .expect("the golden model deserializes");
    db.tables
        .block_records
        .retain(|name, _| !name.starts_with('*') || name.starts_with("*D"));
    db
}

/// The same drawing with the first dimension moved onto a sheet and the
/// rest left in model space, the way a file lists them.
fn split_between_spaces() -> CadDatabase {
    let mut db = unplaced();
    let (first, rest) = db
        .entities
        .split_first()
        .map(|(f, r)| (f.clone(), r.to_vec()))
        .expect("the case has entities");
    for (name, entities) in [("*Model_Space", rest), ("*Paper_Space", vec![first])] {
        let record: BlockRecord = serde_json::from_value(serde_json::json!({
            "name": name,
            "base_point": {"x": 0.0, "y": 0.0, "z": 0.0},
            "entities": entities,
        }))
        .expect("the space block deserializes");
        db.tables.block_records.insert(record.name.clone(), record);
    }
    db
}

#[test]
fn a_dimension_names_the_space_it_is_drawn_in() {
    let summary = summarize(&split_between_spaces());
    let spaces: Vec<_> = summary
        .dimensions
        .iter()
        .map(|d| d.space.as_deref())
        .collect();
    assert_eq!(spaces[0], Some("*Paper_Space"));
    assert!(spaces[1..].iter().all(|s| *s == Some("*Model_Space")));
    assert_eq!(spaces.len(), 20);
    let value = serde_json::to_value(&summary).unwrap();
    assert_eq!(value["dimensions"][0]["space"], "*Paper_Space");
}

#[test]
fn a_dimension_no_space_lists_names_no_space() {
    // Not guessed to be in the model, and left out of the serialized answer.
    let summary = summarize(&unplaced());
    assert!(summary.dimensions.iter().all(|d| d.space.is_none()));
    let value = serde_json::to_value(&summary).unwrap();
    assert!(value["dimensions"][0].get("space").is_none());
}
