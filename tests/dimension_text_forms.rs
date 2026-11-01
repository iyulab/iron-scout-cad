//! The texts a dimension carries beyond its measurement (G25), as the
//! summary reports them.
//!
//! Fits, limits, deviations, counts, threads and restated units are text the
//! summary passes on as the drawing wrote it: the sign escapes (`%%C`, `%%P`,
//! `%%D`) are turned into their characters and a stacked pair is split into its
//! two figures, and nothing else is read. The measurement stays beside the
//! text, `<>` stays a placeholder, and a count, a fit or a limit is not
//! interpreted. Reading them is a layer above this one.

use iron_scout_cad::summarize;
use uncad_model::CadDatabase;

fn g25() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g25.expected.json"))
        .expect("the golden model deserializes")
}

/// Each dimension in drawing order: its plain text, its stacked pairs
/// (top, bottom, separator) and its measurement.
type Row = (String, Vec<(String, String, char)>, Option<f64>);

fn rows() -> Vec<Row> {
    summarize(&g25())
        .dimensions
        .iter()
        .map(|d| {
            (
                d.plain.clone().expect("every dimension here states a text"),
                d.text_stacks
                    .iter()
                    .map(|s| (s.top.clone(), s.bottom.clone(), s.separator))
                    .collect(),
                d.measurement,
            )
        })
        .collect()
}

fn row(plain: &str, stacks: &[(&str, &str, char)], measurement: f64) -> Row {
    (
        plain.to_string(),
        stacks
            .iter()
            .map(|(t, b, c)| (t.to_string(), b.to_string(), *c))
            .collect(),
        Some(measurement),
    )
}

#[test]
fn every_text_is_reported_as_the_drawing_wrote_it_beside_its_measurement() {
    assert_eq!(
        rows(),
        [
            // Fits: a symbol pair, symbol and deviations, a shaft symbol alone,
            // and a pair stacked as a fraction (the one stack that is a fit).
            row("\u{2300}25 H7/g6", &[], 25.0),
            row("\u{2300}25 H7+0.021/0", &[("+0.021", "0", '^')], 25.0),
            row("\u{2300}25 g6", &[], 25.0),
            row("\u{2300}25H7/g6", &[("H7", "g6", '/')], 25.0),
            // Counts and a thread: text in front of or in place of the value.
            row("4X \u{2300}6", &[], 6.0),
            row("4-\u{2300}6", &[], 6.0),
            row("4X \u{2300}6 THRU", &[], 6.0),
            row("M8x1.25-6H", &[], 8.0),
            // A symmetric deviation: on a typed value, and after the placeholder.
            row("50 \u{b1}0.05", &[], 50.0),
            row("<>\u{b1}0.1", &[], 50.0),
            // A limit dimension is a stacked pair of figures, one the upper limit.
            row("25.021/24.980", &[("25.021", "24.980", '^')], 25.0),
            // A reference, a count of equal spaces, a chamfer, restated units.
            row("(<>)", &[], 50.0),
            row("3X 20 (=60)", &[], 20.0),
            row("1x45\u{b0}", &[], 1.0),
            row("<> (1.250\")", &[], 31.75),
            row("<> [1.97]", &[], 50.0),
        ]
    );
}

#[test]
fn a_count_or_a_fit_does_not_change_what_was_measured() {
    // `4X %%C6` states a count in its text; the dimension still measures one hole.
    let summary = summarize(&g25());
    let counted: Vec<_> = summary
        .dimensions
        .iter()
        .filter(|d| d.plain.as_deref().is_some_and(|p| p.starts_with("4")))
        .map(|d| d.measurement)
        .collect();
    assert_eq!(counted, [Some(6.0), Some(6.0), Some(6.0)]);
}
