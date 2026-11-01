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
    /// The file states no dimension kind, so the stored number may be an
    /// angle (in radians): neither it nor a length made of it is given.
    KindUnknown,
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

    // The stored number is what the dimension measures only when it is a
    // finite number and the kind is known (an angle's is in radians): a
    // number that is not both is not stored as far as this reading goes.
    let kind_known = d.kind.is_some();
    let measurement = match d.measurement {
        Some(m) if !m.is_finite() => {
            note(&mut v, ValueUnknown::MeasurementNotStored);
            None
        }
        Some(_) if !kind_known => {
            note(&mut v, ValueUnknown::KindUnknown);
            None
        }
        m => m,
    };

    match &d.text_override {
        TextOverride::Suppressed => note(&mut v, ValueUnknown::TextSuppressed),
        TextOverride::Measured => match measurement {
            Some(m) => {
                v.nominal = Some(m);
                v.source = Some(ValueSource::Measured);
            }
            None if d.measurement.is_none() => note(&mut v, ValueUnknown::MeasurementNotStored),
            None => {} // said above
        },
        TextOverride::Literal(raw) => read_literal(raw, measurement, &mut v, &mut note),
    }

    // The length: the stored number has the factor in it.
    if let Some(m) = measurement {
        match (length_factor.filter(|f| f.is_finite() && *f > 0.0), to_mm) {
            (Some(f), Some(k)) => {
                let length = m / f * k;
                if length.is_finite() {
                    v.length_mm = Some(length);
                } else {
                    note(&mut v, ValueUnknown::MeasurementNotStored);
                }
            }
            (None, _) => note(&mut v, ValueUnknown::FactorUnknown),
            (Some(_), None) => note(&mut v, ValueUnknown::UnitUnknown),
        }
    }

    // A style that shows a tolerance the text does not write is said -- unless
    // the text draws nothing at all.
    if v.tolerance.is_none()
        && !matches!(d.text_override, TextOverride::Suppressed)
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
    /// What follows the first stack, in the same characters as `lead`.
    trail: String,
    /// A second stack: which one is the tolerance is not told.
    more_stacks: bool,
}

/// Stands for a character this reading cannot name (a font's own glyph, an
/// unknown code): it is in no number, so the number around it is not read.
const UNREAD: char = '\u{fffd}';

/// The character a token puts in the text this reading works on; `None` for
/// one that only changes how the text is drawn.
fn glyph(t: Token) -> Option<char> {
    match t {
        Token::Char(c) => Some(c),
        Token::Special(s) => Some(match s {
            Special::Degree => '\u{b0}',
            Special::PlusMinus => '\u{b1}',
            Special::Diameter => '\u{2300}',
        }),
        // A break or a space that does not break still separates.
        Token::Break(_) | Token::NonBreakingSpace => Some(' '),
        // What the glyph is depends on the font; a code this module does
        // not read, and a stack that has no separator, are not digits.
        Token::CharCode(_) | Token::Unknown(_) | Token::StackUnsplit(_) => Some(UNREAD),
        Token::Toggle(_)
        | Token::Property { .. }
        | Token::GroupStart
        | Token::GroupEnd
        | Token::Stack { .. } => None,
    }
}

fn parts(raw: &str) -> Parts {
    let mut p = Parts {
        lead: String::new(),
        stack: None,
        trail: String::new(),
        more_stacks: false,
    };
    for t in tokens(raw, TextKind::MText) {
        if let Token::Stack {
            top,
            bottom,
            separator,
        } = t
        {
            if p.stack.is_none() {
                p.stack = Some(TextStack {
                    top: top.to_string(),
                    bottom: bottom.to_string(),
                    separator,
                });
            } else {
                p.more_stacks = true;
            }
        } else if let Some(c) = glyph(t) {
            if p.stack.is_none() {
                p.lead.push(c);
            } else {
                p.trail.push(c);
            }
        }
    }
    p
}

fn read_literal(
    raw: &str,
    measurement: Option<f64>,
    v: &mut DimensionValue,
    note: &mut impl FnMut(&mut DimensionValue, ValueUnknown),
) {
    let Parts {
        lead,
        stack,
        trail,
        more_stacks,
    } = parts(raw);
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
        // Nothing before a stack: limits. With no stack either the text has
        // no content to read.
        if stack.is_none() {
            note(v, ValueUnknown::TextFormNotRead);
        }
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
        } else if tail.contains('\u{b1}') || has_inline_tolerance(tail) {
            // A tolerance is written and its numbers are not taken: said,
            // not dropped.
            note(v, ValueUnknown::ToleranceFormNotRead);
        }
    }

    if let Some(s) = stack {
        match s.separator {
            '^' => {
                // The stack is the tolerance when the text around it leaves
                // no other reading: a unit word may follow, a second stack
                // or more text may not.
                let tail = trail.trim();
                let unit_only = tail.chars().all(|c| c.is_ascii_alphabetic() || c == '"');
                match (signed_number(&s.top), signed_number(&s.bottom)) {
                    _ if more_stacks || !unit_only => {
                        v.tolerance = None;
                        note(v, ValueUnknown::ToleranceFormNotRead);
                    }
                    // Two tolerances in one text: which counts is not told.
                    (Some(_), Some(_)) if v.tolerance.is_some() => {
                        v.tolerance = None;
                        note(v, ValueUnknown::ToleranceFormNotRead);
                    }
                    (Some(upper), Some(lower)) if v.nominal.is_some() => {
                        v.tolerance = Some(ToleranceRole::Deviation { upper, lower });
                    }
                    (Some(upper), Some(lower)) if rest.trim().is_empty() => {
                        v.tolerance = Some(ToleranceRole::Limits { upper, lower });
                    }
                    _ => note(v, ValueUnknown::ToleranceFormNotRead),
                }
            }
            // A fraction or diagonal bar: a fit, or a fraction.
            _ => {
                if is_fit_pair(&s.top, &s.bottom) {
                    note(v, ValueUnknown::FitNotRead);
                } else {
                    // The whole part is not the value the text writes
                    // (`25{\S1/2;}` is 25 and a half).
                    v.nominal = None;
                    v.source = None;
                    note(v, ValueUnknown::TextFormNotRead);
                }
            }
        }
    }
}

/// A sign and a number written after the value in running text -- `+0.1`,
/// `-0.05`, as in `50+0.1-0.05` -- is a tolerance this reading does not take.
fn has_inline_tolerance(tail: &str) -> bool {
    let b = tail.as_bytes();
    b.windows(2)
        .any(|w| matches!(w[0], b'+' | b'-') && (w[1].is_ascii_digit() || w[1] == b'.'))
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
    // A value must follow, and read as one: `4X \u{2300}6`, `4-R5`, `3X 20 (=60)`.
    // After a minus, or after a mark set apart from the digits, it has to be a
    // value that names itself (a sign or `<>`) -- `50-0.1` and `25 - 30` are a
    // tolerance and a range, `50 x 80` a size, and `2x45\u{b0}` a chamfer: none
    // is a count of anything.
    if !value_follows(rest) {
        return (None, lead);
    }
    let attached = after_digits.starts_with(mark);
    if (mark == '-' || !attached) && !rest.starts_with(['\u{2300}', 'R', '<']) {
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

/// Whether `s` reads as a value (after its sign, if any): the placeholder, or
/// a number a value may end on.
fn value_follows(s: &str) -> bool {
    let s = strip_symbol(s);
    s.starts_with("<>") || leading_number(s).is_some_and(|(_, tail)| value_tail_ok(tail))
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
/// or a digit joined to it (`1x45`, `8x1.25`), which is another form. After a
/// space, not another number or a multiplication or minus either (`1 250`,
/// `50 x 80`, `25 - 30`): the first number alone is not what is written.
fn value_tail_ok(tail: &str) -> bool {
    match tail.chars().next() {
        None => true,
        Some(c) if c.is_whitespace() => !tail.trim_start().starts_with(|n: char| {
            n.is_ascii_digit() || matches!(n, '.' | ',' | 'x' | 'X' | '\u{d7}' | '-')
        }),
        Some(c) => matches!(c, '\u{b1}' | '+' | '(' | '[' | '{'),
    }
}

/// `±` and a number, directly after the value or the placeholder: the number
/// is the whole of what follows (`0,05` and `5%` are not `0` and `5`).
fn symmetric(tail: &str) -> Option<f64> {
    let t = tail.trim_start().strip_prefix('\u{b1}')?;
    let (n, rest) = leading_number(t)?;
    let rest_ok = rest.is_empty()
        || rest.starts_with(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '}'));
    rest_ok.then_some(n)
}

/// A plain decimal number with at most one sign: no exponent, no comma, no
/// second sign, nothing after it.
fn signed_number(s: &str) -> Option<f64> {
    let t = s.trim();
    let (negative, digits) = match t.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (n, rest) = leading_number(digits)?;
    if !rest.is_empty() || digits.starts_with(char::is_whitespace) {
        return None;
    }
    Some(if negative { -n } else { n })
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

    /// The value a literal text reads to, with a stored measurement of 50.
    fn lit(raw: &str) -> DimensionValue {
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
        read_literal(raw, Some(50.0), &mut v, &mut note);
        v
    }

    /// Nothing is read from the text's number: no nominal value, no count, no
    /// tolerance -- and the reading says so.
    fn assert_not_read(raw: &str) {
        let v = lit(raw);
        assert_eq!(v.nominal, None, "{raw:?} read a nominal value: {v:?}");
        assert_eq!(v.source, None, "{raw:?}: {v:?}");
        assert!(v.count.is_none(), "{raw:?} read a count: {v:?}");
        assert!(v.tolerance.is_none(), "{raw:?} read a tolerance: {v:?}");
        assert!(
            v.unknown.contains(&ValueUnknown::TextFormNotRead),
            "{raw:?} did not say it was not read: {v:?}"
        );
    }

    #[test]
    fn a_number_pair_is_not_a_count_and_a_value() {
        // A chamfer, a range and a typed minus tolerance each have a digit
        // run, a mark and a digit run -- none is a count of a value.
        for raw in [
            "2x45%%d", "50-0.1", "25 - 30", "10-20", "2-0.1", "3X45%%d", "4-6",
        ] {
            assert_not_read(raw);
        }
        // A count is a count when a value follows it as a value.
        let v = lit("4-%%c6");
        assert_eq!((v.count.map(|c| c.n), v.nominal), (Some(4), Some(6.0)));
        let v = lit("4X 6.5 THRU");
        assert_eq!((v.count.map(|c| c.n), v.nominal), (Some(4), Some(6.5)));
    }

    #[test]
    fn a_number_followed_by_another_is_not_the_first_number_alone() {
        for raw in [
            "1 250",
            "10 000",
            "50 x 80",
            "50 X 30%%d",
            "50 .5",
            "50 , 5",
        ] {
            assert_not_read(raw);
        }
        // What may follow a value after a space is still read.
        let v = lit("20 (=60)");
        assert_eq!(v.nominal, Some(20.0));
        let v = lit("6 THRU");
        assert_eq!(v.nominal, Some(6.0));
    }

    #[test]
    fn a_symmetric_tolerance_is_read_whole_or_not_at_all() {
        let v = lit("50 %%p0.05");
        assert_eq!(v.tolerance, Some(ToleranceRole::Symmetric { value: 0.05 }));
        for raw in ["50 %%p0,05", "50 %%p5%", "50 %%p0.1.5", "<>%%p0,1"] {
            let v = lit(raw);
            assert!(v.tolerance.is_none(), "{raw:?}: {v:?}");
            assert!(
                v.unknown.contains(&ValueUnknown::ToleranceFormNotRead),
                "{raw:?}: {v:?}"
            );
            assert!(v.nominal.is_some(), "{raw:?}: the value is read: {v:?}");
        }
    }

    #[test]
    fn a_character_code_the_font_defines_is_not_a_digit_run_break() {
        // `%%128` is a glyph that depends on the font: the number text
        // around it is not one number.
        for raw in ["50%%1280.1", "45%%127", "%%13025", "5\\Z0", "12\\P5"] {
            assert_not_read(raw);
        }
    }

    #[test]
    fn a_fraction_is_not_its_whole_part() {
        assert_not_read("25{\\S1/2;}");
        // A fit stack leaves the value: the fit is an annotation of it.
        let v = lit("%%c25{\\SH7/g6;}");
        assert_eq!(v.nominal, Some(25.0));
        assert!(v.unknown.contains(&ValueUnknown::FitNotRead), "{v:?}");
    }

    #[test]
    fn a_tolerance_written_in_running_text_is_said_not_dropped() {
        for raw in ["50+0.1-0.05", "50 +0.1/-0.2", "50 +0.1 -0.05"] {
            let v = lit(raw);
            assert_eq!(v.nominal, Some(50.0), "{raw:?}: {v:?}");
            assert!(v.tolerance.is_none(), "{raw:?}: {v:?}");
            assert!(
                v.unknown.contains(&ValueUnknown::ToleranceFormNotRead),
                "{raw:?} dropped a tolerance without saying: {v:?}"
            );
        }
    }

    #[test]
    fn a_stack_is_the_tolerance_only_when_nothing_else_competes() {
        let limits = Some(ToleranceRole::Limits {
            upper: 25.021,
            lower: 24.98,
        });
        // A unit word after the stack is not a second value.
        assert_eq!(lit("{\\S25.021^24.980;}mm").tolerance, limits);
        for raw in [
            // Text after the stack that is not a unit.
            "{\\S25.021^24.980;}50",
            // A second stack.
            "50{\\S+0.1^0;}{\\S1/2;}",
            "50{\\S+0.1^0;}{\\S+0.2^0;}",
            // A tolerance twice.
            "50 %%p0.1{\\S+0.1^0;}",
        ] {
            let v = lit(raw);
            assert!(v.tolerance.is_none(), "{raw:?}: {v:?}");
            assert!(
                v.unknown.contains(&ValueUnknown::ToleranceFormNotRead),
                "{raw:?}: {v:?}"
            );
        }
    }

    #[test]
    fn a_text_with_no_content_says_so() {
        for raw in ["", "  ", "%%u", "\\P"] {
            assert_not_read(raw);
        }
    }

    #[test]
    fn a_signed_number_is_plain_decimal_or_none() {
        assert_eq!(signed_number("+0.021"), Some(0.021));
        assert_eq!(signed_number(" -0.05 "), Some(-0.05));
        assert_eq!(signed_number("0"), Some(0.0));
        for raw in ["+-0.1", "1e3", "--1", ".", "", "0,5", "1.2.3", "inf", "NaN"] {
            assert_eq!(signed_number(raw), None, "{raw:?}");
        }
    }

    fn g5_dimension() -> DimensionEntity {
        let db: uncad_model::CadDatabase =
            serde_json::from_str(include_str!("../tests/golden/g5.expected.json")).unwrap();
        db.entities
            .iter()
            .find_map(|e| match e {
                uncad_model::model::Entity::Dimension(d) if d.common.id.value() == 299 => {
                    Some(d.clone())
                }
                _ => None,
            })
            .expect("g5 has dimension 299")
    }

    #[test]
    fn a_measurement_that_is_not_a_finite_number_is_not_stored() {
        for m in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut d = g5_dimension();
            d.measurement = Some(m);
            let v = read(&d, Some(1.0), None, Some(1.0)).unwrap();
            assert_eq!((v.nominal, v.length_mm), (None, None), "{m}: {v:?}");
            assert!(v.unknown.contains(&ValueUnknown::MeasurementNotStored));
        }
        // A factor that overflows the quotient gives no length.
        let mut d = g5_dimension();
        d.measurement = Some(f64::MAX);
        let v = read(&d, Some(1e-300), None, Some(1.0)).unwrap();
        assert_eq!(v.length_mm, None, "{v:?}");
    }

    #[test]
    fn a_dimension_of_no_stated_kind_may_be_an_angle() {
        let mut d = g5_dimension();
        d.kind = None;
        let v = read(&d, Some(1.0), None, Some(1.0)).unwrap();
        // The stored number would be radians if it is an angle: neither it
        // nor a length made of it is given.
        assert_eq!((v.nominal, v.length_mm), (None, None), "{v:?}");
        assert!(v.unknown.contains(&ValueUnknown::KindUnknown), "{v:?}");
    }

    #[test]
    fn a_style_that_shows_a_tolerance_is_not_said_when_the_text_draws_none() {
        let mut d = g5_dimension();
        d.text_override = TextOverride::Suppressed;
        let style = StyleTolerance {
            shown: Some(true),
            limits: None,
            upper: None,
            lower: None,
            decimal_places: None,
        };
        let v = read(&d, Some(1.0), Some(&style), Some(1.0)).unwrap();
        assert!(
            !v.unknown.contains(&ValueUnknown::StyleToleranceShown),
            "{v:?}"
        );
    }
}
