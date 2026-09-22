//! The summary, checked on the golden cases whose specs are the oracle: a
//! general part with a title block (G1), the same title block twice with
//! two drawing numbers (G9), and a title block of loose texts (G7).

use iron_scout_cad::{summarize, Lookup};
use uncad_model::model::{Confidence, Ref};
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
