//! What a dimension requires, in one place: the nominal value and where it
//! came from, the length in millimetres, a count and a tolerance written in
//! the text, and what the reading did not turn into a field. The texts are
//! the ones of golden case G25; the lengths come from case g5's style.

use iron_scout_cad::{summarize, DimensionValue, ToleranceRole, ValueSource, ValueUnknown};
use serde_json::Value;
use uncad_model::CadDatabase;

fn g25() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g25.expected.json"))
        .expect("the golden model deserializes")
}

fn g5_json() -> Value {
    serde_json::from_str(include_str!("golden/g5.expected.json")).unwrap()
}

fn entity_at(v: &mut Value, id: u64) -> &mut Value {
    v["entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["common"]["id"].as_str() == Some(id.to_string().as_str()))
        .unwrap()
}

fn value_of(db: &CadDatabase, id: u64) -> Option<DimensionValue> {
    summarize(db)
        .dimensions
        .into_iter()
        .find(|d| d.id.value() == id)
        .unwrap()
        .value
}

/// (plain text, nominal, source, count, tolerance, what was not read).
type Row = (
    String,
    Option<f64>,
    Option<ValueSource>,
    Option<u32>,
    Option<ToleranceRole>,
    Vec<ValueUnknown>,
);

fn row(
    plain: &str,
    nominal: Option<f64>,
    source: Option<ValueSource>,
    count: Option<u32>,
    tolerance: Option<ToleranceRole>,
    unknown: &[ValueUnknown],
) -> Row {
    (
        plain.to_string(),
        nominal,
        source,
        count,
        tolerance,
        unknown.to_vec(),
    )
}

#[test]
fn every_text_of_g25_reads_to_the_value_it_writes_and_nothing_more() {
    use ValueSource::{Measured, Typed};
    use ValueUnknown::{FitNotRead, TextFormNotRead};
    let got: Vec<Row> = summarize(&g25())
        .dimensions
        .iter()
        .map(|d| {
            let v = d.value.as_ref().expect("a linear dimension has a value");
            // These dimensions name no style, so the factor is not known and
            // no length is given -- said once, not repeated in every row.
            assert_eq!(v.length_mm, None);
            assert!(v.unknown.contains(&ValueUnknown::FactorUnknown));
            (
                d.plain.clone().unwrap(),
                v.nominal,
                v.source,
                v.count.as_ref().map(|c| c.n),
                v.tolerance.clone(),
                v.unknown
                    .iter()
                    .copied()
                    .filter(|u| *u != ValueUnknown::FactorUnknown)
                    .collect(),
            )
        })
        .collect();
    let dev = |upper, lower| Some(ToleranceRole::Deviation { upper, lower });
    assert_eq!(
        got,
        [
            // A fit is named and left in the text.
            row(
                "\u{2300}25 H7/g6",
                Some(25.0),
                Some(Typed),
                None,
                None,
                &[FitNotRead]
            ),
            row(
                "\u{2300}25 H7+0.021/0",
                Some(25.0),
                Some(Typed),
                None,
                dev(0.021, 0.0),
                &[FitNotRead]
            ),
            row(
                "\u{2300}25 g6",
                Some(25.0),
                Some(Typed),
                None,
                None,
                &[FitNotRead]
            ),
            row(
                "\u{2300}25H7/g6",
                Some(25.0),
                Some(Typed),
                None,
                None,
                &[FitNotRead]
            ),
            // A count in front is not a part of the value.
            row("4X \u{2300}6", Some(6.0), Some(Typed), Some(4), None, &[]),
            row("4-\u{2300}6", Some(6.0), Some(Typed), Some(4), None, &[]),
            row(
                "4X \u{2300}6 THRU",
                Some(6.0),
                Some(Typed),
                Some(4),
                None,
                &[]
            ),
            // A thread has no nominal value to take.
            row("M8x1.25-6H", None, None, None, None, &[TextFormNotRead]),
            // `±` after a typed value and after the placeholder.
            row(
                "50 \u{b1}0.05",
                Some(50.0),
                Some(Typed),
                None,
                Some(ToleranceRole::Symmetric { value: 0.05 }),
                &[]
            ),
            row(
                "<>\u{b1}0.1",
                Some(50.0),
                Some(Measured),
                None,
                Some(ToleranceRole::Symmetric { value: 0.1 }),
                &[]
            ),
            // Two limits and no nominal value.
            row(
                "25.021/24.980",
                None,
                None,
                None,
                Some(ToleranceRole::Limits {
                    upper: 25.021,
                    lower: 24.98
                }),
                &[]
            ),
            // A reference, a restated total, a chamfer, restated units.
            row("(<>)", Some(50.0), Some(Measured), None, None, &[]),
            row("3X 20 (=60)", Some(20.0), Some(Typed), Some(3), None, &[]),
            row("1x45\u{b0}", None, None, None, None, &[TextFormNotRead]),
            row("<> (1.250\")", Some(31.75), Some(Measured), None, None, &[]),
            row("<> [1.97]", Some(50.0), Some(Measured), None, None, &[]),
            // A text with its own symmetric deviation: the typed value and the
            // deviation, though the measurement repeats the value.
            row(
                "\u{2300}47.7\u{b1}0.03",
                Some(47.7),
                Some(Typed),
                None,
                Some(ToleranceRole::Symmetric { value: 0.03 }),
                &[]
            ),
            // A rounded radius: the value is the typed one (1.4), not the
            // measurement the span has (1.35) -- the drawing shows the text.
            row("R1.4", Some(1.4), Some(Typed), None, None, &[]),
            // A requirement in words has no value: said, not guessed.
            row(
                "concavity 5um~45um",
                None,
                None,
                None,
                None,
                &[TextFormNotRead]
            ),
            // Unequal deviations about a typed value, as written.
            row(
                "45+0.05/-0.02",
                Some(45.0),
                Some(Typed),
                None,
                dev(0.05, -0.02),
                &[]
            ),
        ]
    );
}

#[test]
fn a_count_is_kept_as_written_beside_the_value() {
    let v = summarize(&g25())
        .dimensions
        .iter()
        .find(|d| d.plain.as_deref() == Some("4-\u{2300}6"))
        .and_then(|d| d.value.clone())
        .unwrap();
    let count = v.count.unwrap();
    assert_eq!((count.n, count.written.as_str()), (4, "4-"));
}

/// The file stores the number the label shows, the style's factor in it: the
/// length is that number divided by the factor, times the drawing's unit.
/// (A drawing in inches whose style draws millimetres stores 3048 for points
/// 120 apart.)
#[test]
fn the_length_takes_the_factor_out_of_the_stored_number_and_the_unit_in() {
    let mut v = g5_json();
    v["header"]["insunits"] = 1.into(); // inches
    v["tables"]["dim_styles"]["ISO-25"]["length_factor"] = 25.4.into();
    entity_at(&mut v, 299)["measurement"] = 3048.0.into();
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    let value = value_of(&db, 299).unwrap();
    // The label's number is the nominal value; the drawing's length is 120 in.
    assert_eq!(value.nominal, Some(3048.0));
    assert_eq!(value.source, Some(ValueSource::Measured));
    let mm = value.length_mm.unwrap();
    assert!((mm - 3048.0).abs() < 1e-9, "{mm}");
    assert!(value.unknown.is_empty(), "{:?}", value.unknown);
}

#[test]
fn a_style_that_writes_no_factor_means_one_and_a_missing_style_means_none_is_known() {
    let mut v = g5_json();
    v["header"]["insunits"] = 4.into(); // millimetres
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    // ISO-25 states no DIMLFAC here: the format's 1.
    let value = value_of(&db, 299).unwrap();
    assert_eq!(value.length_mm, Some(120.0));
    // 302 names a style the drawing does not have: the factor is not known.
    let value = value_of(&db, 302).unwrap();
    assert_eq!(value.length_mm, None);
    assert!(value.unknown.contains(&ValueUnknown::MeasurementNotStored));
}

#[test]
fn without_a_unit_there_is_no_length_and_a_factor_of_zero_is_not_a_factor() {
    let mut v = g5_json();
    let db: CadDatabase = serde_json::from_value(v.clone()).unwrap();
    // g5 states no $INSUNITS.
    let value = value_of(&db, 299).unwrap();
    assert_eq!(value.length_mm, None);
    assert_eq!(value.unknown, [ValueUnknown::UnitUnknown]);
    // The nominal value does not need the unit.
    assert_eq!(value.nominal, Some(120.0));

    v["header"]["insunits"] = 4.into();
    v["tables"]["dim_styles"]["ISO-25"]["length_factor"] = 0.0.into();
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    let value = value_of(&db, 299).unwrap();
    assert_eq!(value.length_mm, None);
    assert_eq!(value.unknown, [ValueUnknown::FactorUnknown]);
}

#[test]
fn a_suppressed_text_shows_no_value_and_an_angle_has_none() {
    let mut v = g5_json();
    v["header"]["insunits"] = 4.into();
    entity_at(&mut v, 304)["kind"] =
        serde_json::to_value(uncad_model::model::DimensionKind::Angular2Line).unwrap();
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    // 300 draws no text.
    let suppressed = value_of(&db, 300).unwrap();
    assert_eq!(suppressed.nominal, None);
    assert!(suppressed.unknown.contains(&ValueUnknown::TextSuppressed));
    // The length is still the measurement the file stores.
    assert_eq!(suppressed.length_mm, Some(120.0));
    assert_eq!(value_of(&db, 304), None, "an angle has no length or count");
}

#[test]
fn a_style_that_shows_a_tolerance_the_text_does_not_write_is_said() {
    let mut v = g5_json();
    v["header"]["insunits"] = 4.into();
    v["tables"]["dim_styles"]["ISO-25"]["tolerances"] = true.into();
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    let value = value_of(&db, 299).unwrap();
    assert_eq!(value.tolerance, None);
    assert!(value.unknown.contains(&ValueUnknown::StyleToleranceShown));
}

fn override_of(variable: u16, value: Value) -> Value {
    serde_json::json!({ "variable": variable, "value": value })
}

/// The style a dimension sees is its own with the dimension's overrides laid
/// over it: a tolerance the override turns on is said, one it turns off is not.
#[test]
fn the_tolerance_a_style_shows_is_the_one_the_dimensions_overrides_leave() {
    let mut v = g5_json();
    v["header"]["insunits"] = 4.into();
    entity_at(&mut v, 299)["style_overrides"] = serde_json::json!([override_of(
        71,
        serde_json::json!({"type": "INTEGER", "data": 1})
    )]);
    let db: CadDatabase = serde_json::from_value(v.clone()).unwrap();
    let value = value_of(&db, 299).unwrap();
    assert!(
        value.unknown.contains(&ValueUnknown::StyleToleranceShown),
        "{:?}",
        value.unknown
    );

    // The style shows one; the override turns it off.
    v["tables"]["dim_styles"]["ISO-25"]["tolerances"] = true.into();
    entity_at(&mut v, 299)["style_overrides"] = serde_json::json!([override_of(
        71,
        serde_json::json!({"type": "INTEGER", "data": 0})
    )]);
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    let value = value_of(&db, 299).unwrap();
    assert!(
        !value.unknown.contains(&ValueUnknown::StyleToleranceShown),
        "{:?}",
        value.unknown
    );
}

/// A length factor the dimension overrides in a kind that is not a number is
/// not the format's 1: the length is not known.
#[test]
fn an_unreadable_length_factor_override_is_not_a_factor_of_one() {
    let mut v = g5_json();
    v["header"]["insunits"] = 4.into();
    entity_at(&mut v, 299)["style_overrides"] = serde_json::json!([override_of(
        144,
        serde_json::json!({"type": "TEXT", "data": "2"})
    )]);
    let db: CadDatabase = serde_json::from_value(v).unwrap();
    let value = value_of(&db, 299).unwrap();
    assert_eq!(value.length_mm, None);
    assert!(
        value.unknown.contains(&ValueUnknown::FactorUnknown),
        "{:?}",
        value.unknown
    );
}
