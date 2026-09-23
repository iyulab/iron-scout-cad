//! The summary, checked on the golden cases whose specs are the oracle: a
//! general part with a title block (G1), the same title block twice with
//! two drawing numbers (G9), and a title block of loose texts (G7).

use iron_scout_cad::{summarize, Lookup};
use uncad_model::model::{Confidence, Entity, EntityId, Ref};
use uncad_model::CadDatabase;

fn golden(json: &str) -> CadDatabase {
    serde_json::from_str(json).expect("the golden model deserializes")
}

fn g1() -> CadDatabase {
    golden(include_str!("golden/g1.expected.json"))
}

fn g7() -> CadDatabase {
    golden(include_str!("golden/g7.expected.json"))
}

fn g9() -> CadDatabase {
    golden(include_str!("golden/g9.expected.json"))
}

#[test]
fn g1_counts_what_the_spec_says_it_contains() {
    let s = summarize(&g1());
    assert_eq!(s.entity_count, 13);
    let by_type: Vec<(&str, usize)> = s.by_type.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(
        by_type,
        [
            ("ATTRIB", 3),
            ("CIRCLE", 4),
            ("DIMENSION", 4),
            ("INSERT", 1),
            ("LWPOLYLINE", 1),
        ]
    );
    let layers: Vec<(&str, i16, usize)> = s
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.color_index, l.entity_count))
        .collect();
    assert_eq!(
        layers,
        [
            ("0", 7, 0),
            ("DIMS", 3, 4),
            ("HOLES", 1, 4),
            ("OUTLINE", 7, 1),
            ("TITLE", 2, 4),
        ]
    );
    let blocks: Vec<(&str, usize, usize)> = s
        .blocks
        .iter()
        .map(|b| (b.name.as_str(), b.entity_count, b.insert_count))
        .collect();
    assert_eq!(
        blocks,
        [
            ("*D1", 4, 0),
            ("*D2", 4, 0),
            ("*D3", 4, 0),
            ("*D4", 2, 0),
            ("TITLEBLOCK", 4, 1),
        ]
    );
    assert_eq!(s.confidence, Confidence::High);
    assert!(s.warnings.is_empty());
}

#[test]
fn g1_title_block_values_are_read_by_tag() {
    let s = summarize(&g1());
    let values: Vec<(&str, &str)> = s
        .attributes
        .iter()
        .map(|a| (a.tag.as_str(), a.value.as_str()))
        .collect();
    assert_eq!(
        values,
        [("DWGNO", "BP-1042"), ("REV", "B"), ("MATERIAL", "SS400")]
    );
    assert_eq!(
        s.attributes[0].block,
        Ref::Resolved("TITLEBLOCK".to_string())
    );
    assert_eq!(s.attribute("DWGNO"), Lookup::Unique("BP-1042"));
    assert_eq!(s.attribute("REV"), Lookup::Unique("B"));
    assert_eq!(s.attribute("SCALE"), Lookup::Absent);
    assert!(s.labelled_texts.is_empty(), "no loose texts in G1");
}

#[test]
fn g9_two_drawing_numbers_are_ambiguous_not_a_pick() {
    let s = summarize(&g9());
    assert_eq!(s.attributes.len(), 2);
    assert_eq!(
        s.attribute("DWGNO"),
        Lookup::Ambiguous(vec!["BP-1042", "BP-2077"])
    );
    let title = s.blocks.iter().find(|b| b.name == "TITLEBLOCK").unwrap();
    assert_eq!(title.insert_count, 2);
}

#[test]
fn g7_loose_texts_pair_up_by_position() {
    let s = summarize(&g7());
    assert!(s.attributes.is_empty(), "no block attributes in G7");
    let pairs: Vec<(&str, &str)> = s
        .labelled_texts
        .iter()
        .map(|l| (l.label.text.as_str(), l.value.text.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [("DWG NO", "BP-1042"), ("REV", "B"), ("MATERIAL", "SS400")]
    );
    assert_eq!(s.labelled("DWG NO"), Lookup::Unique("BP-1042"));
    assert_eq!(
        s.labelled("SS400"),
        Lookup::Absent,
        "a value is not a label"
    );
}

#[test]
fn the_summary_serializes_with_the_lookup_in_the_models_convention() {
    let s = summarize(&g9());
    let json = serde_json::to_string(&s.attribute("DWGNO")).unwrap();
    assert_eq!(json, r#"{"type":"AMBIGUOUS","data":["BP-1042","BP-2077"]}"#);
    let json = serde_json::to_string(&s.attribute("NONE")).unwrap();
    assert_eq!(json, r#"{"type":"ABSENT"}"#);
}

/// G7 with a second text drawn exactly on top of its value `target`, under
/// a fresh reference ID.
fn g7_with_a_text_over(target: &str, text: &str) -> CadDatabase {
    let mut db = g7();
    let twin = db
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Text(t) if t.text == target => Some(t.clone()),
            _ => None,
        })
        .expect("G7 has that text");
    let mut over = twin;
    over.common.id = EntityId::new(9_000);
    over.text = text.to_string();
    db.entities.push(Entity::Text(over));
    db
}

#[test]
fn two_texts_at_the_same_nearest_distance_are_both_listed_and_the_lookup_is_ambiguous() {
    // The value of "DWG NO" in G7 is "BP-1042"; a different text drawn on
    // top of it is just as near, and neither is picked.
    let s = summarize(&g7_with_a_text_over("BP-1042", "BP-2077"));
    let dwg_no: Vec<&str> = s
        .labelled_texts
        .iter()
        .filter(|l| l.label.text == "DWG NO")
        .map(|l| l.value.text.as_str())
        .collect();
    assert_eq!(
        dwg_no,
        ["BP-1042", "BP-2077"],
        "listed once per candidate, by ID"
    );
    assert_eq!(
        s.labelled("DWG NO"),
        Lookup::Ambiguous(vec!["BP-1042", "BP-2077"])
    );
    // The other rows are untouched.
    assert_eq!(s.labelled("REV"), Lookup::Unique("B"));
}

#[test]
fn the_same_text_drawn_twice_is_still_one_value() {
    let s = summarize(&g7_with_a_text_over("BP-1042", "BP-1042"));
    assert_eq!(s.labelled("DWG NO"), Lookup::Unique("BP-1042"));
    assert_eq!(
        s.labelled_texts
            .iter()
            .filter(|l| l.label.text == "DWG NO")
            .count(),
        2,
        "both carriers are listed; the value is one"
    );
}

#[test]
fn labels_are_matched_and_values_returned_as_the_file_wrote_them() {
    // The model carries text as the file wrote it, control codes included,
    // and so does the summary: an underlined label and a diameter value are
    // found by their codes, not by what they draw.
    let mut db = g7();
    for e in &mut db.entities {
        if let Entity::Text(t) = e {
            match t.text.as_str() {
                "DWG NO" => t.text = "%%uDWG NO".to_string(),
                "BP-1042" => t.text = "%%c32".to_string(),
                _ => {}
            }
        }
    }
    let s = summarize(&db);
    assert_eq!(s.labelled("%%uDWG NO"), Lookup::Unique("%%c32"));
    assert_eq!(s.labelled("DWG NO"), Lookup::Absent);
}
