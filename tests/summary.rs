//! The summary, checked on the golden cases whose specs are the oracle: a
//! general part with a title block (G1), the same title block twice with
//! two drawing numbers (G9), a title block of loose texts (G7), and a
//! drawing that is mostly dimensions (G5).

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

fn g8() -> CadDatabase {
    golden(include_str!("golden/g8.expected.json"))
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
fn a_text_on_a_tilted_plane_is_listed_as_unplaced_not_paired() {
    // G7 has none: its mirror copy faces down, which is measured.
    assert!(summarize(&g7()).unplaced_texts.is_empty());
    let mut db = g7();
    let mut tilted = None;
    for e in &mut db.entities {
        if let Entity::Text(t) = e {
            if t.text == "BP-1042" {
                t.extrusion = uncad_model::Point3D {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                };
                tilted = Some(t.common.id);
            }
        }
    }
    let s = summarize(&db);
    let ids: Vec<EntityId> = s.unplaced_texts.iter().map(|t| t.id).collect();
    assert_eq!(ids, [tilted.expect("G7 has the value")]);
    assert_eq!(s.unplaced_texts[0].plain, "BP-1042");
    // It is no longer the label's value; the label has none left on its row.
    assert!(s
        .labelled_texts
        .iter()
        .all(|l| l.value.text != "BP-1042" && l.label.text != "BP-1042"));
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
fn labels_are_matched_and_values_returned_as_plain_text() {
    // The model carries text with its codes in it. A label is found by what
    // it reads, not by how it is written: an underlined label answers to its
    // words, and a diameter value comes back as the sign it names -- with
    // the text as written beside it.
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
    assert_eq!(s.labelled("DWG NO"), Lookup::Unique("\u{2300}32"));
    assert_eq!(s.labelled("%%uDWG NO"), Lookup::Absent);
    let pair = s
        .labelled_texts
        .iter()
        .find(|l| l.label.plain == "DWG NO")
        .expect("the pair");
    assert_eq!(pair.label.text, "%%uDWG NO");
    assert_eq!(pair.value.text, "%%c32");
}

#[test]
fn plain_text_drops_the_codes_and_keeps_what_depends_on_the_font() {
    use iron_scout_cad::plain_text;
    assert_eq!(plain_text("%%uA%%u %%d%%p0.1"), "A \u{b0}\u{b1}0.1");
    // Which character %%nnn draws depends on the font: kept as written.
    assert_eq!(plain_text("90%%127"), "90%%127");
    assert_eq!(plain_text("100%%%"), "100%");
    assert_eq!(plain_text("plain"), "plain");
}

fn g5() -> CadDatabase {
    golden(include_str!("golden/g5.expected.json"))
}

#[test]
fn every_dimension_is_listed_as_the_file_states_it() {
    use uncad_model::model::{DimensionKind, TextOverride};
    let s = summarize(&g5());
    let ids: Vec<u64> = s.dimensions.iter().map(|d| d.id.value()).collect();
    assert_eq!(ids, [298, 299, 300, 301, 302, 303, 304]);
    let d = |id: u64| s.dimensions.iter().find(|d| d.id.value() == id).unwrap();
    // A literal that disagrees with the recorded measurement: both, as
    // written -- neither is corrected to the other.
    assert_eq!(d(301).measurement, Some(120.0));
    assert_eq!(d(301).text, TextOverride::Literal("125".into()));
    assert_eq!(d(301).plain.as_deref(), Some("125"));
    // A literal's codes read as plain text beside it.
    assert_eq!(d(303).kind, Some(DimensionKind::Diameter));
    assert_eq!(d(303).plain.as_deref(), Some("\u{2300}20"));
    // A suppressed text, a missing measurement and an undeclared style are
    // said, not filled in.
    assert_eq!(d(300).text, TextOverride::Suppressed);
    assert_eq!(d(300).plain, None);
    assert_eq!(d(302).measurement, None);
    assert_eq!(d(302).style, Ref::Unresolved("NOT-DECLARED".into()));
}

#[test]
fn a_dimensions_text_midpoint_points_back_at_it() {
    // The summary and the hit test close a loop: where the summary says a
    // dimension's text is, pointing finds that dimension.
    let db = g5();
    for d in summarize(&db).dimensions {
        let r = iron_scout_cad::hit_test(&db, d.text_midpoint, 1e-9);
        assert!(
            r.hits.iter().any(|h| h.id == d.id),
            "{:?} not found at {:?}: {:?}",
            d.id,
            d.text_midpoint,
            r.hits
        );
    }
}

/// Lowers the confidence of every copy of `id` (model space is listed at
/// the top level and in its block record).
fn lower(db: &mut CadDatabase, id: EntityId, to: Confidence) {
    db.entities
        .iter_mut()
        .chain(
            db.tables
                .block_records
                .values_mut()
                .flat_map(|b| b.entities.iter_mut()),
        )
        .filter(|e| e.common().id == id)
        .for_each(|e| e.common_mut().confidence = to);
}

/// One entity the reader was unsure of lowers the whole summary to its
/// confidence, and the pair it takes part in; the pairs it does not touch
/// keep theirs. Nothing summarized reads higher than what it was built from.
#[test]
fn a_low_confidence_entity_lowers_the_summary_and_its_own_pair_only() {
    let clean = summarize(&g7());
    assert_eq!(clean.confidence, Confidence::High);
    let pair = &clean.labelled_texts[0];
    let (low_value, label) = (pair.value.id, pair.label.id);

    let mut db = g7();
    lower(&mut db, low_value, Confidence::Low);
    let s = summarize(&db);
    assert_eq!(s.confidence, Confidence::Low);
    for p in &s.labelled_texts {
        let expected = if p.value.id == low_value || p.label.id == low_value {
            Confidence::Low
        } else {
            Confidence::High
        };
        assert_eq!(p.confidence, expected, "pair of {:?}", p.label.plain);
    }
    assert!(s.labelled_texts.iter().any(|p| p.label.id == label));

    // Unknown is lower still, and wins over Low.
    lower(&mut db, label, Confidence::Unknown);
    let s = summarize(&db);
    assert_eq!(s.confidence, Confidence::Unknown);
    let p = s
        .labelled_texts
        .iter()
        .find(|p| p.label.id == label)
        .expect("the pair stays listed");
    assert_eq!(p.confidence, Confidence::Unknown);
}

/// A title block in Korean (G8 -- CP949 in the file, UTF-8 in the model):
/// the attribute values come back as the text the file states, layer names
/// likewise, and a lone loose text with nothing beside it is not paired.
#[test]
fn g8_korean_title_block_values_are_read_by_tag() {
    let s = summarize(&g8());
    assert_eq!(s.attribute("DWGNO"), Lookup::Unique("BP-1042"));
    assert_eq!(
        s.attribute("MATERIAL"),
        Lookup::Unique(
            "SS400 \u{C77C}\u{BC18}\u{AD6C}\u{C870}\u{C6A9} \u{C555}\u{C5F0}\u{AC15}\u{C7AC}"
        )
    );
    assert_eq!(
        s.attribute("DRAWN"),
        Lookup::Unique("\u{D64D}\u{AE38}\u{B3D9}")
    );
    let layers: Vec<&str> = s.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(layers.contains(&"\u{D45C}\u{C81C}\u{B780}"), "{layers:?}");
    assert!(layers.contains(&"\u{C678}\u{D615}\u{C120}"), "{layers:?}");
    assert!(s.labelled_texts.is_empty(), "{:?}", s.labelled_texts);
    assert_eq!(s.confidence, Confidence::High);
}
