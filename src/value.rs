//! A dimension's value as the drawing requires it, read from what the file
//! states and from nothing else.
//!
//! The pieces are scattered: the number the measurement stores, the text that
//! may restate or replace it, a count written in front, a tolerance written
//! behind or stacked, the factor and the unit that turn it into a length.
//! [`DimensionValue`] puts what can be read in one place. It combines and
//! reads the text's words; it does not redraw the text, convert a fit into
//! deviations or decide what a value means for a part. What cannot be read
//! is named in [`DimensionValue::unknown`] and left out of the other fields:
//! a wrong nominal value is worse than none.

use crate::summary::{StyleTolerance, TextStack};
use serde::{Deserialize, Serialize};
use uncad_model::model::{DimensionEntity, DimensionKind, TextOverride};
use uncad_model::text::{tokens, Special, TextKind, Token};

/// Where [`DimensionValue::nominal`] was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ValueSource {
    /// A number the dimension's text writes.
    Typed,
    /// The measurement the file stores -- the text is the measurement
    /// (`<>`), or says nothing about the value.
    Measured,
}

/// A count written in front of the value: `4X `, `4-`, `4 x `.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DimensionCount {
    /// How many, at least 2 (a `1x45` is a chamfer, not a count).
    pub n: u32,
    /// The prefix as the text wrote it, spaces included.
    pub written: String,
}

/// What a tolerance written in the dimension's text is, read from where it
/// stands in the text. Numbers are as written, signed, and never reordered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ToleranceRole {
    /// Two limits and no nominal value: a stack that is the whole text.
    Limits { upper: f64, lower: f64 },
    /// Deviations from a nominal value: a stack after the value.
    Deviation { upper: f64, lower: f64 },
    /// `±` after the value or the placeholder.
    Symmetric { value: f64 },
}

/// Something the text or the file shows that this reading does not turn
/// into a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ValueUnknown {
    /// The text is suppressed: the drawing shows no value.
    TextSuppressed,
    /// The text has a form this reading does not take a number from (a
    /// thread, a chamfer, words).
    TextFormNotRead,
    /// A fit (`H7`, `g6`, `H7/g6`) is written; it stays in the text.
    FitNotRead,
    /// A tolerance is written whose numbers cannot be read.
    ToleranceFormNotRead,
    /// The style shows a tolerance (DIMTOL or DIMLIM) that the text does not
    /// write; the style's numbers are in `style_tolerance`.
    StyleToleranceShown,
    /// The file does not store the measurement.
    MeasurementNotStored,
    /// `length_mm` needs the drawing's unit and the file states none.
    UnitUnknown,
    /// `length_mm` needs the style's length factor and it is not known (or
    /// not above zero).
    FactorUnknown,
}

/// A linear dimension's value, as the drawing requires it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DimensionValue {
    /// The number the drawing requires, in the unit the label shows. A number
    /// typed in the text wins over the stored measurement (both are in the
    /// summary: [`crate::DimensionSummary::measurement`] is still the file's).
    /// `None` when the text has no number to read or states limits only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal: Option<f64>,
    /// Where `nominal` came from; `None` exactly when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ValueSource>,
    /// The length the dimension measures in millimetres: the stored
    /// measurement divided by the style's length factor (the file stores the
    /// number its label shows, factor included) times the drawing's unit.
    /// This is a length of the drawing, not the label's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<DimensionCount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<ToleranceRole>,
    /// What was not read, each once, in the order met.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<ValueUnknown>,
}

/// The value of `d`; `None` for an angular dimension (its measurement is an
/// angle and has no length, factor or count to combine).
pub(crate) fn read(
    d: &DimensionEntity,
    length_factor: Option<f64>,
    style_tolerance: Option<&StyleTolerance>,
    to_mm: Option<f64>,
) -> Option<DimensionValue> {
    if matches!(
        d.kind,
        Some(DimensionKind::Angular2Line | DimensionKind::Angular3Point)
    ) {
        return None;
    }
    let mut v = DimensionValue {
        nominal: None,
        source: None,
        length_mm: None,
        count: None,
        tolerance: None,
        unknown: Vec::new(),
    };
    let mut note = |v: &mut DimensionValue, u: ValueUnknown| {
        if !v.unknown.contains(&u) {
            v.unknown.push(u);
        }
    };

    match &d.text_override {
        TextOverride::Suppressed => note(&mut v, ValueUnknown::TextSuppressed),
        TextOverride::Measured => match d.measurement {
            Some(m) => {
                v.nominal = Some(m);
                v.source = Some(ValueSource::Measured);
            }
            None => note(&mut v, ValueUnknown::MeasurementNotStored),
        },
        TextOverride::Literal(raw) => read_literal(raw, d.measurement, &mut v, &mut note),
    }

    // The length: the stored number has the factor in it.
    if let Some(m) = d.measurement {
        match (length_factor.filter(|f| f.is_finite() && *f > 0.0), to_mm) {
            (Some(f), Some(k)) => v.length_mm = Some(m / f * k),
            (None, _) => note(&mut v, ValueUnknown::FactorUnknown),
            (Some(_), None) => note(&mut v, ValueUnknown::UnitUnknown),
        }
    }

    if v.tolerance.is_none()
        && style_tolerance.is_some_and(|s| s.shown == Some(true) || s.limits == Some(true))
    {
        note(&mut v, ValueUnknown::StyleToleranceShown);
    }
    Some(v)
}

/// The text pieces the rules work on: what comes before the first stack
/// (the signs turned into their characters), the stack, and what follows.
struct Parts {
    lead: String,
    stack: Option<TextStack>,
}

fn parts(raw: &str) -> Parts {
    let mut lead = String::new();
    let mut stack = None;
    for t in tokens(raw, TextKind::MText) {
        match t {
            Token::Char(c) if stack.is_none() => lead.push(c),
            Token::Special(s) if stack.is_none() => lead.push(match s {
                Special::Degree => '\u{b0}',
                Special::PlusMinus => '\u{b1}',
                Special::Diameter => '\u{2300}',
            }),
            Token::Stack {
                top,
                bottom,
                separator,
            } if stack.is_none() => {
                stack = Some(TextStack {
                    top: top.to_string(),
                    bottom: bottom.to_string(),
                    separator,
                })
            }
            _ => {}
        }
    }
    Parts { lead, stack }
}

fn read_literal(
    raw: &str,
    measurement: Option<f64>,
    v: &mut DimensionValue,
    note: &mut impl FnMut(&mut DimensionValue, ValueUnknown),
) {
    let Parts { lead, stack } = parts(raw);
    // A text that is wholly in parentheses is a reference dimension's; the
    // value inside is read the same way (the role is `DimensionRole`'s).
    let trimmed = lead.trim();
    let lead_ref: &str = match trimmed.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        Some(inner) => inner,
        None => &lead,
    };

    // A count in front: digits, then `x`, `X`, `×` or `-`, with a value after.
    let (count, rest) = split_count(lead_ref);
    v.count = count;
    // A sign that says what the number is: the diameter sign, or an R.
    let rest = strip_symbol(rest);

    let mut after_value: Option<&str> = None;
    if let Some(after) = rest.strip_prefix("<>") {
        match measurement {
            Some(m) => {
                v.nominal = Some(m);
                v.source = Some(ValueSource::Measured);
            }
            None => note(v, ValueUnknown::MeasurementNotStored),
        }
        after_value = Some(after);
    } else if rest.trim().is_empty() {
        // Nothing before a stack: limits (or nothing at all).
    } else if let Some((number, tail)) = leading_number(rest) {
        if value_tail_ok(tail) {
            v.nominal = Some(number);
            v.source = Some(ValueSource::Typed);
            after_value = Some(tail);
        } else {
            note(v, ValueUnknown::TextFormNotRead);
        }
    } else {
        note(v, ValueUnknown::TextFormNotRead);
    }

    if let Some(tail) = after_value {
        if has_fit(tail) {
            note(v, ValueUnknown::FitNotRead);
        }
        if let Some(sym) = symmetric(tail) {
            v.tolerance = Some(ToleranceRole::Symmetric { value: sym });
        } else if tail.contains('\u{b1}') {
            note(v, ValueUnknown::ToleranceFormNotRead);
        }
    }

    if let Some(s) = stack {
        match s.separator {
            '^' => match (signed_number(&s.top), signed_number(&s.bottom)) {
                (Some(upper), Some(lower)) if v.nominal.is_some() => {
                    v.tolerance = Some(ToleranceRole::Deviation { upper, lower });
                }
                (Some(upper), Some(lower)) if rest.trim().is_empty() => {
                    v.tolerance = Some(ToleranceRole::Limits { upper, lower });
                }
                _ => note(v, ValueUnknown::ToleranceFormNotRead),
            },
            // A fraction or diagonal bar: a fit, or a fraction.
            _ => {
                if is_fit_pair(&s.top, &s.bottom) {
                    note(v, ValueUnknown::FitNotRead);
                } else {
                    note(v, ValueUnknown::TextFormNotRead);
                }
            }
        }
    }
}

/// `4X `, `4-`, `4 x` in front of the value (at least 2, and something
/// after it).
fn split_count(lead: &str) -> (Option<DimensionCount>, &str) {
    let trimmed = lead.trim_start();
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return (None, lead);
    }
    let after_digits = &trimmed[digits..];
    let spaced = after_digits.trim_start();
    let Some(mark) = spaced.chars().next() else {
        return (None, lead);
    };
    if !matches!(mark, 'x' | 'X' | '\u{d7}' | '-') {
        return (None, lead);
    }
    let after_mark = &spaced[mark.len_utf8()..];
    let rest = after_mark.trim_start();
    // Something that can be a value must follow: a sign, `<>` or a digit.
    let next = rest.chars().next();
    if !next
        .is_some_and(|c| c == '\u{2300}' || c == 'R' || c == '<' || c.is_ascii_digit() || c == '.')
    {
        return (None, lead);
    }
    let Ok(n) = trimmed[..digits].parse::<u32>() else {
        return (None, lead);
    };
    if n < 2 {
        return (None, lead);
    }
    let written_len = lead.len() - rest.len();
    (
        Some(DimensionCount {
            n,
            written: lead[..written_len].to_string(),
        }),
        rest,
    )
}

fn strip_symbol(s: &str) -> &str {
    let s = s.trim_start();
    if let Some(r) = s.strip_prefix('\u{2300}') {
        return r.trim_start();
    }
    if let Some(r) = s.strip_prefix('R') {
        if r.chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == '.')
        {
            return r;
        }
    }
    s
}

/// A non-negative decimal number at the start of `s`, and the rest.
fn leading_number(s: &str) -> Option<(f64, &str)> {
    let s = s.trim_start();
    let mut end = 0;
    let mut dot = false;
    let mut digits = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() {
            digits += 1;
            end = i + 1;
        } else if c == '.' && !dot {
            dot = true;
            end = i + 1;
        } else {
            break;
        }
    }
    if digits == 0 {
        return None;
    }
    let number: f64 = s[..end].parse().ok()?;
    number.is_finite().then_some((number, &s[end..]))
}

/// After a typed number: nothing, a space, a sign, a bracket -- not a letter
/// or a digit joined to it (`1x45`, `8x1.25`), which is another form.
fn value_tail_ok(tail: &str) -> bool {
    match tail.chars().next() {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, '\u{b1}' | '+' | '(' | '[' | '{'),
    }
}

/// `±` and a number, directly after the value or the placeholder.
fn symmetric(tail: &str) -> Option<f64> {
    let t = tail.trim_start().strip_prefix('\u{b1}')?;
    leading_number(t).map(|(n, _)| n)
}

fn signed_number(s: &str) -> Option<f64> {
    let t = s.trim();
    let n: f64 = t.strip_prefix('+').unwrap_or(t).parse().ok()?;
    n.is_finite().then_some(n)
}

/// A fit pair like `H7` / `g6`: letters then digits on each side.
fn is_fit_symbol(s: &str) -> bool {
    let s = s.trim();
    let letters = s.chars().take_while(char::is_ascii_alphabetic).count();
    let digits = s[letters..]
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    (1..=2).contains(&letters) && (1..=2).contains(&digits) && letters + digits == s.len()
}

fn is_fit_pair(top: &str, bottom: &str) -> bool {
    is_fit_symbol(top) && is_fit_symbol(bottom)
}

/// A fit written in the running text: ` H7`, ` g6`, ` H7/g6`.
fn has_fit(tail: &str) -> bool {
    let word = tail.split_whitespace().next().unwrap_or("");
    if word.contains('\u{b0}') {
        return false;
    }
    match word.split_once('/') {
        Some((a, b)) => is_fit_symbol(a) && is_fit_symbol(b),
        None => is_fit_symbol(word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_needs_two_or_more_and_a_value_after() {
        assert_eq!(
            split_count("4X \u{2300}6").0.map(|c| (c.n, c.written)),
            Some((4, "4X ".to_string()))
        );
        assert_eq!(split_count("4-\u{2300}6").0.map(|c| c.n), Some(4));
        assert_eq!(split_count("3X 20 (=60)").0.map(|c| c.n), Some(3));
        assert!(split_count("1x45\u{b0}").0.is_none());
        assert!(split_count("4X").0.is_none());
        assert!(split_count("25 H7").0.is_none());
    }

    #[test]
    fn a_number_joined_to_a_letter_is_another_form() {
        assert!(value_tail_ok(""));
        assert!(value_tail_ok(" \u{b1}0.05"));
        assert!(!value_tail_ok("x45\u{b0}"));
        assert!(!value_tail_ok("H7"));
        assert!(!value_tail_ok(".5"));
    }

    #[test]
    fn fits_are_recognised_but_not_read() {
        assert!(has_fit(" H7/g6"));
        assert!(has_fit(" g6"));
        assert!(!has_fit(" THRU"));
        assert!(!has_fit(" (=60)"));
        assert!(is_fit_pair("H7", "g6"));
        assert!(!is_fit_pair("1", "2"));
    }
}
