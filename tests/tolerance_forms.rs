//! The ways a drawing states a tolerance (G21), as the summary reports them.
//!
//! A dimension's stacked pair is reported as written -- upper over lower, the
//! separator the drawing used, the figures spelled as typed. The summary does
//! not reorder a pair that reads upper-below-lower, does not turn `0` into
//! `+0`, and does not read a fit symbol: that is text, and reading it is a
//! layer above this one.

use iron_scout_cad::summarize;
use uncad_model::CadDatabase;

fn g21() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g21.expected.json"))
        .expect("the golden model deserializes")
}

/// The stack of each dimension, in drawing order, as `(top, bottom, separator)`.
fn stacks() -> Vec<Vec<(String, String, char)>> {
    summarize(&g21())
        .dimensions
        .iter()
        .map(|d| {
            d.text_stacks
                .iter()
                .map(|s| (s.top.clone(), s.bottom.clone(), s.separator))
                .collect()
        })
        .collect()
}

fn pair(top: &str, bottom: &str) -> Vec<(String, String, char)> {
    vec![(top.to_string(), bottom.to_string(), '^')]
}

#[test]
fn a_stacked_pair_is_reported_as_the_drawing_wrote_it() {
    assert_eq!(
        stacks(),
        [
            // A pair with a sign on one side and a bare zero on the other.
            pair("+0.1", "0"),
            pair("0", "-0.1"),
            // The upper figure below the lower, and both negative: kept as written.
            pair("+0.000", "+0.200"),
            pair("-0.1", "-0.05"),
            // A value typed over the measurement, with a pair after it.
            pair("0", "-0.3"),
            // A fit symbol is text, not a stack.
            vec![],
        ]
    );
}

#[test]
fn a_typed_value_and_the_measurement_are_both_reported() {
    let summary = summarize(&g21());
    let typed = summary
        .dimensions
        .iter()
        .find(|d| {
            d.text_stacks.len() == 1 && d.plain.as_deref().is_some_and(|p| p.starts_with("49.6"))
        })
        .expect("the dimension with a typed value");
    // The drawing states 49.6 and recorded a measurement of 50: neither replaces the other.
    assert_eq!(typed.measurement, Some(50.0));
}

#[test]
fn a_fit_symbol_is_text_the_summary_carries_unread() {
    let summary = summarize(&g21());
    let diameter = summary
        .dimensions
        .iter()
        .find(|d| d.plain.as_deref().is_some_and(|p| p.contains("H7")))
        .expect("the diameter with a fit symbol");
    // Neither a stack nor a measurement of its own is made of the symbol.
    assert!(diameter.text_stacks.is_empty());
    assert_eq!(diameter.measurement, Some(20.0));
}
