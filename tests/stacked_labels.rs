//! Label-above-value pairs among loose texts, checked on G23, a title block
//! laid out as ISO 7200 draws its fields: each name a small text at the top
//! left of its field, its value a larger text beneath it.

use iron_scout_cad::{summarize, LabelDirection, LabelledText, Lookup, Summary};
use uncad_model::model::{Entity, EntityId, HorizontalJustification};
use uncad_model::CadDatabase;

fn g23() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g23.expected.json")).expect("G23 deserializes")
}

fn g7() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g7.expected.json")).expect("G7 deserializes")
}

/// Every pair as (label, value, direction, later lines), ordered by the
/// label's text so a test does not depend on entity order.
fn pairs(s: &Summary) -> Vec<(String, String, LabelDirection, Vec<String>)> {
    let mut out: Vec<_> = s
        .labelled_texts
        .iter()
        .map(|l| {
            (
                l.label.text.clone(),
                l.value.text.clone(),
                l.direction,
                l.continued_by.iter().map(|c| c.text.clone()).collect(),
            )
        })
        .collect();
    out.sort();
    out
}

/// The label texts that have a value beneath them.
fn named_with_a_value_beneath(s: &Summary) -> Vec<String> {
    s.labelled_texts
        .iter()
        .filter(|l| l.direction == LabelDirection::Below)
        .map(|l| l.label.text.clone())
        .collect()
}

fn text_mut<'a>(db: &'a mut CadDatabase, text: &str) -> &'a mut uncad_model::model::TextEntity {
    db.entities
        .iter_mut()
        .find_map(|e| match e {
            Entity::Text(t) if t.text == text => Some(t),
            _ => None,
        })
        .expect("G23 has that text")
}

#[test]
fn a_value_beneath_its_name_is_paired_and_neighbouring_fields_are_not() {
    let s = summarize(&g23());
    let below = |l: &str, v: &str, more: &[&str]| {
        (
            l.to_string(),
            v.to_string(),
            LabelDirection::Below,
            more.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        pairs(&s),
        [
            below("Responsible dept.", "ABC 2", &[]),
            below("Rev.", "A", &[]),
            below("Technical reference", "Patricia Johnson", &[]),
            below("Title", "Mounting bracket", &["for motor M2"]),
        ]
    );
    // The row rule reads these four texts as two names and two values side
    // by side: a name paired with the name beside it, a value with the value
    // beside it. None of that is left.
    assert!(!pairs(&s)
        .iter()
        .any(|(l, v, ..)| l == "Responsible dept." && v == "Technical reference"));
    assert!(!pairs(&s)
        .iter()
        .any(|(l, v, ..)| l == "ABC 2" && v == "Patricia Johnson"));
}

#[test]
fn the_lookup_answers_one_line_values_and_withholds_a_cut_value() {
    let s = summarize(&g23());
    assert_eq!(s.labelled("Responsible dept."), Lookup::Unique("ABC 2"));
    assert_eq!(s.labelled("Rev."), Lookup::Unique("A"));
    // Its first line alone would read as the whole title: it is listed, not
    // given as the answer.
    assert_eq!(
        s.labelled("Title"),
        Lookup::Ambiguous(vec!["Mounting bracket"])
    );
    let title = s
        .labelled_texts
        .iter()
        .find(|l| l.label.text == "Title")
        .unwrap();
    assert_eq!(title.value.text, "Mounting bracket");
    assert_eq!(title.continued_by[0].text, "for motor M2");
}

#[test]
fn what_nothing_but_position_cannot_settle_is_left_unpaired() {
    let s = summarize(&g23());
    let in_a_pair = |t: &str| {
        s.labelled_texts.iter().any(|l| {
            l.label.text == t || l.value.text == t || l.continued_by.iter().any(|c| c.text == t)
        })
    };
    // Two lines of one height, 5 apart: no size tells the name from the value.
    assert!(!in_a_pair("SHEET") && !in_a_pair("1 OF 2"));
    // A name above a value centred on its point: the value is where its
    // alignment point puts it, not at a column the name can be matched to.
    assert!(!in_a_pair("Scale") && !in_a_pair("1:2"));
}

#[test]
fn the_pairs_do_not_depend_on_the_order_the_texts_are_stored_in() {
    let mut db = g23();
    let before = summarize(&db);
    db.entities.reverse();
    let after = summarize(&db);
    assert_eq!(pairs(&before), pairs(&after));
    assert_eq!(before.labelled_texts, after.labelled_texts);
}

#[test]
fn a_text_between_a_name_and_the_value_under_it_blocks_the_pair() {
    let mut db = g23();
    let mut between = text_mut(&mut db, "ABC 2").clone();
    between.common.id = EntityId::new(9_000);
    between.text = "in the way".to_string();
    between.start_point.y += 2.5; // level with neither, in the same column
    between.text_height = 1.0;
    db.entities.push(Entity::Text(between));
    let s = summarize(&db);
    assert!(
        !named_with_a_value_beneath(&s).contains(&"Responsible dept.".to_string()),
        "{:?}",
        pairs(&s)
    );
}

#[test]
fn a_turned_value_is_not_read_as_standing_in_a_column() {
    let mut db = g23();
    text_mut(&mut db, "ABC 2").rotation = 0.5;
    let s = summarize(&db);
    assert!(!named_with_a_value_beneath(&s).contains(&"Responsible dept.".to_string()));
    // Nor is the name paired along its row with a text far to the right,
    // past the field that stands in the way.
    for gone in ["Responsible dept.", "ABC 2"] {
        assert!(
            !s.labelled_texts.iter().any(|l| l.label.text == gone),
            "{gone}: {:?}",
            pairs(&s)
        );
    }
}

/// G23 with a copy of the text `of` placed at (`x`, `y`) reading `text`, at
/// height `height`.
fn with_text(
    mut db: CadDatabase,
    of: &str,
    id: u64,
    at: (f64, f64),
    height: f64,
    text: &str,
) -> CadDatabase {
    let mut t = text_mut(&mut db, of).clone();
    t.common.id = EntityId::new(id);
    t.text = text.to_string();
    t.start_point.x = at.0;
    t.start_point.y = at.1;
    t.text_height = height;
    db.entities.push(Entity::Text(t));
    db
}

#[test]
fn a_value_continued_by_two_equally_near_lines_is_withheld_not_cut_short() {
    // A second "for motor M2" drawn exactly over the first: where the title
    // goes on is not told by position.
    let db = with_text(
        g23(),
        "for motor M2",
        9_000,
        (102.0, -68.0),
        3.5,
        "for motor M2",
    );
    let s = summarize(&db);
    assert!(
        !s.labelled_texts.iter().any(|l| l.label.text == "Title"),
        "{:?}",
        pairs(&s)
    );
    assert_eq!(s.labelled("Title"), Lookup::Absent);
    // Its texts stay out of the row rule, which would pair them with whatever
    // lies to their right.
    assert!(!s
        .labelled_texts
        .iter()
        .any(|l| l.label.text == "Mounting bracket"));
}

#[test]
fn a_cut_value_beside_a_whole_one_is_ambiguous_not_unique() {
    // A second field called "Title" with a one-line value.
    let mut db = g23();
    text_mut(&mut db, "Rev.").text = "Title".to_string();
    let s = summarize(&db);
    match s.labelled("Title") {
        Lookup::Ambiguous(values) => {
            assert_eq!(values, ["A", "Mounting bracket"]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_long_column_is_a_list_and_stays_with_the_row_rule() {
    // A header over six lines of one height: no field goes on that long.
    let mut db = with_text(g23(), "Rev.", 9_000, (190.0, -58.0), 2.0, "Hdr");
    for (i, y) in [-63.0, -68.0, -73.0, -78.0, -83.0, -88.0]
        .into_iter()
        .enumerate()
    {
        db = with_text(
            db,
            "A",
            9_001 + i as u64,
            (190.0, y),
            3.5,
            &format!("row {i}"),
        );
    }
    let s = summarize(&db);
    assert!(!named_with_a_value_beneath(&s).contains(&"Hdr".to_string()));
}

#[test]
fn a_line_that_names_a_larger_text_beneath_it_ends_the_value_above() {
    // "Hdr" over "v1", then "v2" of the same height that is itself the name
    // of a bigger "big" beneath it.
    let mut db = with_text(g23(), "Rev.", 9_000, (190.0, -58.0), 2.0, "Hdr");
    db = with_text(db, "A", 9_001, (190.0, -63.0), 3.5, "v1");
    db = with_text(db, "A", 9_002, (190.0, -68.0), 3.5, "v2");
    db = with_text(db, "A", 9_003, (190.0, -73.0), 5.0, "big");
    let s = summarize(&db);
    let hdr = s
        .labelled_texts
        .iter()
        .find(|l| l.label.text == "Hdr")
        .unwrap();
    assert_eq!(hdr.value.text, "v1");
    assert!(hdr.continued_by.is_empty(), "{hdr:?}");
    let v2 = s
        .labelled_texts
        .iter()
        .find(|l| l.label.text == "v2")
        .unwrap();
    assert_eq!(v2.value.text, "big");
}

#[test]
fn a_text_drawn_over_another_is_not_the_next_line() {
    let mut db = g23();
    // A fake bold: the value drawn again a hundredth lower.
    db = with_text(
        db,
        "Mounting bracket",
        9_000,
        (102.0, -63.01),
        3.5,
        "Mounting bracket",
    );
    let s = summarize(&db);
    // The nearer one is the value; the overprint is not its second line.
    let title = s
        .labelled_texts
        .iter()
        .find(|l| l.label.text == "Title")
        .unwrap();
    assert!(
        !title
            .continued_by
            .iter()
            .any(|c| c.text == "Mounting bracket"),
        "{title:?}"
    );
}

#[test]
fn a_value_that_is_not_left_justified_is_not_paired_beneath() {
    let mut db = g23();
    text_mut(&mut db, "A").horizontal_justification = HorizontalJustification::Right;
    let s = summarize(&db);
    assert!(!named_with_a_value_beneath(&s).contains(&"Rev.".to_string()));
}

#[test]
fn a_name_barely_smaller_than_its_value_is_not_paired() {
    let mut db = g23();
    // 0.8 of the value is the limit; 3.0 of 3.5 is past it.
    text_mut(&mut db, "Rev.").text_height = 3.0;
    let s = summarize(&db);
    assert!(!named_with_a_value_beneath(&s).contains(&"Rev.".to_string()));
}

#[test]
fn rows_of_one_height_keep_the_row_rule_and_g7_is_unchanged() {
    let s = summarize(&g7());
    assert!(s
        .labelled_texts
        .iter()
        .all(|l| l.direction == LabelDirection::Right && l.continued_by.is_empty()));
    assert_eq!(s.labelled_texts.len(), 3);
}

#[test]
fn a_text_in_a_pair_beneath_takes_no_part_in_the_row_rule() {
    // "Responsible dept." and "Technical reference" share a row, and so do
    // their values: the row rule would pair each with its neighbour. A text
    // in a pair beneath is left out of it, so no row pair is made.
    let s = summarize(&g23());
    let row: Vec<_> = s
        .labelled_texts
        .iter()
        .filter(|l| l.direction == LabelDirection::Right)
        .map(|l| (l.label.text.as_str(), l.value.text.as_str()))
        .collect();
    assert!(row.is_empty(), "{row:?}");
}

#[test]
fn the_direction_and_continuation_serialize_and_old_pairs_still_read() {
    let s = summarize(&g23());
    let json = serde_json::to_value(&s).unwrap();
    let pairs = json["labelled_texts"].as_array().unwrap();
    let title = pairs
        .iter()
        .find(|p| p["label"]["text"] == "Title")
        .unwrap();
    assert_eq!(title["direction"], "below");
    assert_eq!(title["continued_by"][0]["text"], "for motor M2");
    let rev = pairs.iter().find(|p| p["label"]["text"] == "Rev.").unwrap();
    assert!(
        rev.get("continued_by").is_none(),
        "an empty list is left out"
    );

    // A pair written before the direction existed reads as a row pair.
    let mut old = rev.clone();
    old.as_object_mut().unwrap().remove("direction");
    let read: LabelledText = serde_json::from_value(old).unwrap();
    assert_eq!(read.direction, LabelDirection::Right);
    assert!(read.continued_by.is_empty());
}
