//! The summary: what a drawing contains, compactly, with nothing guessed.

use crate::extent::SpaceExtent;
use crate::select::{select, Selected, Selection};
use crate::signature::{signature, Signature};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uncad_model::model::{
    AcadTableEntity, Confidence, DimensionEntity, DimensionKind, Entity, EntityId, Ref,
    StyleOverride, TextOverride,
};
use uncad_model::tables::{ExternalReference, Tables};
use uncad_model::text::TextKind;
use uncad_model::{CadDatabase, Ocs, Point2D, Units};

/// A layer and how much of the drawing is on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LayerSummary {
    pub name: String,
    pub color_index: i16,
    /// Top-level entities whose layer resolves to this one.
    pub entity_count: usize,
}

/// A block definition and how often it is referenced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BlockSummary {
    pub name: String,
    /// Entities in the definition.
    pub entity_count: usize,
    /// Top-level INSERTs that resolve to this block.
    pub insert_count: usize,
    /// The drawing this block stands for, when the block states that it is
    /// an external reference: that drawing's content is not in this file,
    /// so [`Self::entity_count`] counts only what was bound or cached into
    /// the definition -- often nothing -- and its references draw no more.
    /// `None` for an ordinary block.
    pub external_reference: Option<ExternalReference>,
}

/// A block reference whose block the drawing does not hold: the file names
/// one it never defines, or names none. It draws nothing, and no
/// [`BlockSummary`] counts it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UnresolvedInsert {
    /// The INSERT.
    pub insert: EntityId,
    /// What it points at, as the model holds it: `Unresolved` with what the
    /// file wrote (a name, or a handle), or `Absent`.
    pub block: Ref<String>,
}

/// One attribute value attached to a block reference: what a title block's
/// fields look like in the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AttributeValue {
    /// The INSERT that carries the value.
    pub insert: EntityId,
    /// The block the INSERT refers to, as the model resolved it.
    pub block: Ref<String>,
    /// The ATTRIB entity itself.
    pub id: EntityId,
    /// The tag the value answers to (the block's ATTDEF tag).
    pub tag: String,
    /// The value as the model carries it, its codes included.
    pub value: String,
    /// The value as plain text (see [`plain_text`]).
    pub plain: String,
    /// Where the value's baseline starts (DXF 10), in world coordinates
    /// of [`Self::space`] -- the point to show a reader where it came
    /// from. Absent when the value is written on a plane tilted out of the
    /// world's, which no 2D position places exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_point: Option<Point2D>,
    /// The space the value is drawn in, as [`crate::Hit::space`] names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    pub confidence: Confidence,
}

/// A piece of text by reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TextRef {
    pub id: EntityId,
    /// The text as the model carries it, its codes included.
    pub text: String,
    /// The text as plain text (see [`plain_text`]).
    pub plain: String,
    /// Where the text's baseline starts (DXF 10), in world coordinates of
    /// [`Self::space`]. Absent for a text on a plane tilted out of the
    /// world's, which no 2D position places exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_point: Option<Point2D>,
    /// The space the text is drawn in, as [`crate::Hit::space`] names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
}

impl TextRef {
    fn of(
        id: EntityId,
        text: &str,
        start_point: Option<Point2D>,
        spaces: &BTreeMap<EntityId, &str>,
    ) -> Self {
        TextRef {
            id,
            text: text.to_string(),
            plain: plain_text(text),
            start_point,
            space: spaces.get(&id).map(|s| s.to_string()),
        }
    }
}

/// A text's start point in world coordinates, when its plane is the
/// world's or a mirror of it.
fn world_start(start_point: Point2D, extrusion: uncad_model::Point3D) -> Option<Point2D> {
    Ocs::of(extrusion)
        .and_then(|o| o.flat_map())
        .map(|plan| plan.apply(start_point))
}

/// What a TEXT or ATTRIB reads as, without the codes it is written in: the
/// characters, with `%%d`, `%%p` and `%%c` as the degree, plus-minus and
/// diameter signs they name and underline and overline switches dropped.
/// `%%nnn` stays as written -- which character it draws depends on the font.
/// This is what labels are matched by and values returned as; the text as
/// written stays beside it.
pub fn plain_text(text: &str) -> String {
    plain(text, TextKind::Line)
}

/// `text` read as `kind` into plain text, as [`plain_text`] reads a line of
/// text -- for MTEXT codes (a dimension's text), paragraph and column
/// breaks read as spaces and a stack as `top/bottom`.
fn plain(text: &str, kind: TextKind) -> String {
    use uncad_model::text::{tokens, Special, Token};
    let mut out = String::with_capacity(text.len());
    for token in tokens(text, kind) {
        match token {
            Token::Char(c) => out.push(c),
            Token::Special(Special::Degree) => out.push('\u{b0}'),
            Token::Special(Special::PlusMinus) => out.push('\u{b1}'),
            Token::Special(Special::Diameter) => out.push('\u{2300}'),
            Token::CharCode(code) => out.push_str(&format!("%%{code:03}")),
            Token::Unknown(raw) => out.push_str(raw),
            Token::Break(_) | Token::NonBreakingSpace => out.push(' '),
            Token::Stack { top, bottom, .. } => {
                out.push_str(top);
                out.push('/');
                out.push_str(bottom);
            }
            Token::StackUnsplit(body) => out.push_str(body),
            Token::Toggle(_) | Token::Property { .. } | Token::GroupStart | Token::GroupEnd => {}
        }
    }
    out
}

/// A dimension, as the file states it: what it measures, the measurement
/// it recorded, the text it shows, and where that text sits -- a point to
/// hand [`crate::hit_test`] to point at it. Nothing here is recomputed from
/// the dimension's points: the recorded measurement and the text are the
/// file's, and where they disagree both are listed as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DimensionSummary {
    pub id: EntityId,
    /// The anonymous block that draws the dimension -- its lines, arrowheads
    /// and text; what it draws is searched by [`crate::hit_test`] as a block
    /// reference's is.
    pub block: Ref<String>,
    /// What the dimension measures; `None` when the file does not state it.
    pub kind: Option<DimensionKind>,
    /// The measurement the drawing recorded (DXF 42), in the drawing's
    /// units -- radians for an angular dimension; `None` when the file does
    /// not carry it.
    pub measurement: Option<f64>,
    /// The text the dimension shows, as the model folds it: the
    /// measurement, nothing, or a literal (which may stand `<>` for the
    /// measurement inside a longer string).
    pub text: TextOverride,
    /// A literal text as plain text, its codes read as the MTEXT codes a
    /// dimension's text is written in (see [`plain_text`]); absent
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plain: Option<String>,
    /// The middle of the dimension's text (DXF 11), where it is drawn.
    pub text_midpoint: Point2D,
    /// The DIMSTYLE the dimension names.
    pub style: Ref<String>,
    /// What that style states about tolerances, as it states it; absent
    /// when [`Self::style`] names no style of the drawing's table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_tolerance: Option<StyleTolerance>,
    /// The dimension's own overrides of the style's tolerance variables --
    /// DIMTOL (71), DIMLIM (72), DIMTP (47), DIMTM (48) and DIMTDEC (272) --
    /// as the file states them, in its order: `[]` when it overrides none,
    /// `None` when the reader did not read the dimension's overrides at
    /// all. Which value is shown -- the style's or the override's -- is not
    /// decided here.
    pub tolerance_overrides: Option<Vec<StyleOverride>>,
    /// The stacks a literal text writes (`\S…;`), in order, as written --
    /// `\S+0.1^-0.05;` is a tolerance written into the text itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_stacks: Vec<TextStack>,
    /// DIMLFAC in force for this dimension -- its style's (DXF 144), or its
    /// own override of it: the factor the drawing's distance is multiplied
    /// by for the measurement it shows, so that a drawing measured in one
    /// unit can be dimensioned in another. `None` when [`Self::style`]
    /// names no style of the drawing's table, when the reader did not read
    /// the dimension's overrides (whether it overrides the factor is then
    /// not known), or when neither states it -- see the model's
    /// `DimStyleRecord` for what the format means by an unwritten value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_factor: Option<f64>,
    /// DIMGAP in force for this dimension -- its style's (DXF 147), or its
    /// own override of it: the gap between the text and the dimension line,
    /// **negative when the text is drawn in a box**. `None` under the same
    /// conditions as [`Self::length_factor`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_gap: Option<f64>,
    /// How the file writes this dimension, by the two marks drafting
    /// standards put on the dimension itself; see [`DimensionRole`]. `None`
    /// when it states neither -- the ordinary dimension -- and also when it
    /// states both, or when a mark cannot be ruled out because the gap is
    /// not known: nothing is guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<DimensionRole>,
    pub confidence: Confidence,
}

/// How a dimension is written, read from what its file states and from
/// nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum DimensionRole {
    /// The text is drawn in a box: the dimension's [`DimensionSummary::text_gap`]
    /// is negative. A basic dimension -- a theoretically exact value -- is
    /// written that way (ASME Y14.5; AutoCAD's negative `DIMGAP`). Only the
    /// box is read: a style whose every dimension is boxed says that of
    /// every dimension.
    Basic,
    /// The text is the measurement in parentheses -- the whole text sits
    /// inside one pair, as in `(<>)` or `(125)` -- and the text is not
    /// boxed. A reference dimension -- given for information -- is written
    /// that way. Only a literal text is read: a measurement the drawing
    /// computes is not, even where the style's text suffix (`DIMPOST`)
    /// wraps it in parentheses, so such a dimension has no role.
    Reference,
}

/// The tolerance variables of a dimension style, each as the style states
/// it (`None` where the style's record does -- see the model's
/// `DimStyleRecord`). Nothing is combined: whether a tolerance is shown,
/// and which, follows from these together with the dimension's own
/// overrides and text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StyleTolerance {
    /// DIMTOL: whether tolerances are appended to the measurement.
    pub shown: Option<bool>,
    /// DIMLIM: whether the text is the two limits instead.
    pub limits: Option<bool>,
    /// DIMTP: the upper tolerance.
    pub upper: Option<f64>,
    /// DIMTM: the lower tolerance, as a magnitude below the measurement.
    pub lower: Option<f64>,
    /// DIMTDEC: the decimal places the tolerances are written with.
    pub decimal_places: Option<i32>,
}

/// One MTEXT stack (`\S top separator bottom ;`) of a text, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TextStack {
    pub top: String,
    pub bottom: String,
    /// `^` (stacked without a bar -- how tolerances are written), `/` (a
    /// fraction bar) or `#` (a diagonal bar).
    pub separator: char,
}

/// A feature control frame (TOLERANCE), as the file states it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToleranceFrame {
    pub id: EntityId,
    /// The frame's text as written: MTEXT codes (`{\Fgdt;j}` selects the
    /// symbol font for the letter after it), `%%v` between what reads as
    /// its cells, a line feed between its rows.
    pub text: String,
    /// The text with its MTEXT codes read (see [`DimensionSummary::plain`]):
    /// the font switch dropped, the symbol letter and `%%v` kept.
    pub plain: String,
    /// Where the frame is inserted (DXF 10) -- a point [`crate::hit_test`]
    /// finds it at.
    pub insertion_point: Point2D,
    /// The DIMSTYLE the frame names.
    pub style: Ref<String>,
    pub confidence: Confidence,
}

/// A table (ACAD_TABLE), as the file states it: where it is, the block that
/// draws it, and what its cells say.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TableSummary {
    pub id: EntityId,
    /// Where the table is inserted -- a point [`crate::hit_test`] finds it
    /// at.
    pub insertion_point: Point2D,
    /// The space the table is drawn in, as [`crate::Hit::space`] names it
    /// -- a point is only a place on a picture of that space.
    pub space: Option<String>,
    /// The block that draws the table; what it draws is searched by
    /// [`crate::hit_test`] as a block reference's is.
    pub block: Ref<String>,
    /// The table's cells. `None` when the drawing's reader did not read
    /// them: the table is there, and what it says is not known -- which is
    /// not the same as a table that says nothing.
    pub cells: Option<TableCells>,
    pub confidence: Confidence,
}

/// The size of a table and the cells that say something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TableCells {
    pub rows: usize,
    pub columns: usize,
    /// Every cell with text, row by row from the top, each row from the
    /// left. A cell not listed says nothing: it is empty, holds a block, or
    /// lies under another cell's span.
    pub texts: Vec<CellText>,
}

/// What one table cell says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CellText {
    /// The cell's row, 0 at the top.
    pub row: usize,
    /// The cell's column, 0 at the left.
    pub column: usize,
    /// The text as the model carries it, its MTEXT codes included.
    pub text: String,
    /// The text with its MTEXT codes read (see [`DimensionSummary::plain`]).
    pub plain: String,
    /// How many columns the cell spans, itself included; 1 for a cell that
    /// spans nothing.
    pub span_columns: u32,
    /// How many rows the cell spans, likewise.
    pub span_rows: u32,
}

impl TableSummary {
    fn of(t: &AcadTableEntity, spaces: &BTreeMap<EntityId, &str>) -> Self {
        TableSummary {
            id: t.common.id,
            insertion_point: Point2D {
                x: t.insertion_point.x,
                y: t.insertion_point.y,
            },
            space: spaces.get(&t.common.id).map(|s| s.to_string()),
            block: t.block_name.clone(),
            cells: t.grid.as_ref().map(|grid| TableCells {
                rows: grid.rows.len(),
                columns: grid.column_widths.len(),
                texts: grid
                    .rows
                    .iter()
                    .enumerate()
                    .flat_map(|(row, r)| {
                        r.cells.iter().enumerate().filter_map(move |(column, c)| {
                            let text = c.text.as_ref().filter(|_| !c.covered)?;
                            Some(CellText {
                                row,
                                column,
                                text: text.clone(),
                                plain: plain(text, TextKind::MText),
                                span_columns: c.span_columns,
                                span_rows: c.span_rows,
                            })
                        })
                    })
                    .collect(),
            }),
            confidence: t.common.confidence,
        }
    }
}

/// The dimension-style variables that state tolerances, by DXF group.
const TOLERANCE_VARIABLES: [u16; 5] = [71, 72, 47, 48, 272];

impl DimensionSummary {
    pub(crate) fn of(d: &DimensionEntity, tables: &Tables) -> Self {
        use uncad_model::text::{tokens, Token};
        let literal = match &d.text_override {
            TextOverride::Literal(text) => Some(text.as_str()),
            _ => None,
        };
        let text_stacks = literal
            .map(|text| {
                tokens(text, TextKind::MText)
                    .filter_map(|t| match t {
                        Token::Stack {
                            top,
                            bottom,
                            separator,
                        } => Some(TextStack {
                            top: top.to_string(),
                            bottom: bottom.to_string(),
                            separator,
                        }),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let plain_text = literal.map(|text| plain(text, TextKind::MText));
        // The style as this dimension sees it: its own, with the dimension's
        // overrides laid over it. Not known when the style is not in the
        // table or the overrides were not read.
        let effective = d
            .style_name
            .resolved()
            .and_then(|name| tables.dim_styles.get(name))
            .zip(d.style_overrides.as_ref())
            .map(|(style, overrides)| style.overridden(overrides));
        let text_gap = effective.as_ref().and_then(|style| style.text_gap);
        DimensionSummary {
            id: d.common.id,
            block: d.block_name.clone(),
            kind: d.kind,
            measurement: d.measurement,
            role: dimension_role(
                text_gap,
                plain_text.as_deref(),
                matches!(d.text_override, TextOverride::Suppressed),
            ),
            text_gap,
            plain: plain_text,
            text: d.text_override.clone(),
            text_midpoint: d.text_midpoint,
            style: d.style_name.clone(),
            style_tolerance: d
                .style_name
                .resolved()
                .and_then(|name| tables.dim_styles.get(name))
                .map(|st| StyleTolerance {
                    shown: st.tolerances,
                    limits: st.limits,
                    upper: st.tolerance_upper,
                    lower: st.tolerance_lower,
                    decimal_places: st.tolerance_decimal_places,
                }),
            tolerance_overrides: d.style_overrides.as_ref().map(|all| {
                all.iter()
                    .filter(|o| TOLERANCE_VARIABLES.contains(&o.variable))
                    .cloned()
                    .collect()
            }),
            text_stacks,
            length_factor: effective.as_ref().and_then(|style| style.length_factor),
            confidence: d.common.confidence,
        }
    }
}

/// A dimension's role from its effective text gap and its literal text in
/// plain form: boxed text (a negative gap) is a basic dimension unless the
/// text is also parenthesized -- the file then states both marks and no role
/// is read; a parenthesized text is a reference dimension only when the gap
/// is known not to box it. A dimension whose text is suppressed draws none, so
/// there is no text to be boxed or parenthesized and no role is read.
fn dimension_role(
    gap: Option<f64>,
    plain_text: Option<&str>,
    suppressed: bool,
) -> Option<DimensionRole> {
    if suppressed {
        return None;
    }
    let boxed = gap.is_some_and(|g| g < 0.0);
    let unboxed = gap.is_some_and(|g| g >= 0.0);
    let parenthesized = plain_text.is_some_and(enclosed_in_parentheses);
    match (boxed, parenthesized) {
        (true, false) => Some(DimensionRole::Basic),
        (false, true) if unboxed => Some(DimensionRole::Reference),
        _ => None,
    }
}

/// Whether one pair of parentheses holds the whole text: `(125)` and
/// `((a) b)` do, `(2X) 10` and `(a)(b)` do not.
fn enclosed_in_parentheses(text: &str) -> bool {
    let chars: Vec<char> = text.trim().chars().collect();
    if chars.first() != Some(&'(') || chars.last() != Some(&')') {
        return false;
    }
    let mut depth = 0i32;
    for (i, c) in chars.iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                // The first pair closes before the end, or a `)` has no `(`.
                if depth < 0 || (depth == 0 && i + 1 != chars.len()) {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

#[cfg(test)]
mod role_tests {
    use super::*;

    #[test]
    fn one_pair_of_parentheses_must_hold_the_whole_text() {
        for yes in ["(<>)", "(125)", " (5) ", "((a) b)"] {
            assert!(enclosed_in_parentheses(yes), "{yes}");
        }
        for no in ["", "(", ")(", "(2X) 10", "(a)(b)", "125", "(125", "<>"] {
            assert!(!enclosed_in_parentheses(no), "{no}");
        }
    }
}

/// Two loose TEXT entities that read as a label and its value. Nothing but
/// position ties them, which is how a title block drawn without a block
/// carries its fields. Two placements are read, and the pair says which:
///
/// - [`LabelDirection::Right`]: on the same row, the value the nearest text
///   to the right of the label.
/// - [`LabelDirection::Below`]: the label a smaller text at the top left of
///   its field and the value the larger text beneath it, in the same column
///   (the layout title blocks are drawn in when each field carries its name
///   above its value). A value that goes on over several lines lists the
///   later lines in [`Self::continued_by`].
///
/// When several texts are nearest at the same distance (texts drawn on top
/// of each other), the label is listed once per candidate, and
/// [`Summary::labelled`] then answers with every distinct value rather than
/// picking one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LabelledText {
    pub label: TextRef,
    pub value: TextRef,
    pub confidence: Confidence,
    /// Where the value sits relative to the label.
    #[serde(default)]
    pub direction: LabelDirection,
    /// The lines that continue [`Self::value`] beneath it, in reading order.
    /// Empty (and left out of the serialized pair) for a value of one line,
    /// and always for a [`LabelDirection::Right`] pair.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continued_by: Vec<TextRef>,
}

/// Where a [`LabelledText`]'s value sits relative to its label.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelDirection {
    /// On the label's row, to its right.
    #[default]
    Right,
    /// Beneath the label, in its column.
    Below,
}

/// The answer to "what is the value of X in this drawing".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "UPPERCASE")]
pub enum Lookup<T> {
    /// Exactly one distinct value.
    Unique(T),
    /// No carrier of that name.
    Absent,
    /// Several carriers with different values, all listed, none chosen.
    Ambiguous(Vec<T>),
}

/// The unit the drawing's header states for its numbers (`$INSUNITS`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DrawingUnits {
    /// The code as the file states it.
    pub code: u16,
    /// The unit the code names in the DXF reference's table: `"mm"`, `"in"`,
    /// `"m"`, ... -- or `"du"` ("drawing units") for code 0 (unitless) and
    /// for a code the table does not define.
    pub name: String,
    /// How many millimetres one of the drawing's units is, from that same
    /// table (`1.0` for `"mm"`, `25.4` for `"in"`). `None` for `"du"`: a
    /// length the drawing states without a unit is not converted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_mm: Option<f64>,
}

/// The identifiers a drawing's header states for where it came from and
/// which saved state it is, as stated (braces included) -- to ask whether two
/// files are one drawing before comparing them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DrawingIds {
    /// `$FINGERPRINTGUID`: given when the drawing was created and kept
    /// through every later save -- also by a drawing created as a copy of it,
    /// or from the same template, so two unrelated drawings can state the
    /// same value. It says where a drawing came from, not which drawing it
    /// is: a different value is strong evidence of two drawings, the same
    /// value only weak evidence of one. `None` when the header does not
    /// state it.
    pub fingerprint: Option<String>,
    /// `$VERSIONGUID`: the drawing's state as of a save, given anew when a
    /// save changes it. `None` when the header does not state it.
    pub version: Option<String>,
}

/// What a drawing contains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Summary {
    /// The unit the drawing's coordinates and measurements are in, as its
    /// header states it; `None` when the header does not say -- a unit is
    /// never assumed. The summary's coordinates, and the lengths measured
    /// from them, are in this unit.
    #[serde(default)]
    pub units: Option<DrawingUnits>,
    /// The identifiers the header states for the drawing's origin and saved
    /// state; each `None` when not stated.
    #[serde(default)]
    pub drawing_ids: DrawingIds,
    /// Top-level entities (model and paper space), as the model lists them.
    pub entity_count: usize,
    /// Count per entity type name, by name.
    pub by_type: BTreeMap<String, usize>,
    /// Every layer of the layer table, by name.
    pub layers: Vec<LayerSummary>,
    /// Every block definition except the two space records, by name.
    pub blocks: Vec<BlockSummary>,
    /// Every top-level block reference whose block the drawing does not
    /// hold, by reference ID: counted in `by_type`, drawn as nothing,
    /// listed here with what it points at.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_inserts: Vec<UnresolvedInsert>,
    /// Every attribute value attached to a block reference, by the INSERT's
    /// reference ID, then the ATTRIB's.
    pub attributes: Vec<AttributeValue>,
    /// Loose TEXT pairs that read as label and value, by the label's
    /// reference ID.
    pub labelled_texts: Vec<LabelledText>,
    /// Loose TEXTs left out of that pairing because they are written on a
    /// plane tilted out of the world's: where they are drawn in plan is not
    /// something this crate measures, so they are neither labels nor values
    /// here -- and are listed, by reference ID, so that a label missing
    /// from [`Self::labelled_texts`] is never silently "not in the
    /// drawing". A plane facing up or down (a mirror copy's included) is
    /// measured and paired.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unplaced_texts: Vec<TextRef>,
    /// Every dimension of the drawing's own spaces, by reference ID.
    #[serde(default)]
    pub dimensions: Vec<DimensionSummary>,
    /// Every feature control frame (TOLERANCE) of the drawing's own spaces,
    /// by reference ID: its text as written and where it is. What its
    /// symbols and cells mean is not read here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tolerance_frames: Vec<ToleranceFrame>,
    /// Every table (ACAD_TABLE) of the drawing's own spaces, by reference
    /// ID: where it is and what its cells say.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<TableSummary>,
    /// Where each space of the drawing is -- model space and every
    /// paper-space sheet, by block name: a coordinate range to point into
    /// (see [`SpaceExtent`]).
    #[serde(default)]
    pub extents: Vec<SpaceExtent>,
    /// The drawing's shape signature: what its model space draws, counted
    /// into integers whose places each have a meaning (see [`Signature`]).
    /// `None` only when the summary was projected without it
    /// ([`Summary::without`]), which [`Self::omitted`] then says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
    /// The parts a projection left out ([`Summary::without`]), so that a part
    /// that is not there is never mistaken for a part that is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<Part>,
    /// The entities a [`Selection`] picked, when the summary was asked for
    /// one ([`summarize_with`]); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selected>,
    /// The lowest confidence of any entity summarized; `High` for a drawing
    /// with no entities.
    pub confidence: Confidence,
    /// The drawing's own read diagnostics, passed through unchanged.
    pub warnings: Vec<String>,
}

/// A part of a [`Summary`] a projection can leave out ([`Summary::without`]),
/// named as the summary names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Part {
    /// The shape signature: the largest part of the summary of a drawing
    /// with many views.
    Signature,
}

impl Summary {
    /// This summary without the `parts`, each named in [`Self::omitted`]
    /// (once, however often it is asked for). The rest is unchanged, and
    /// the same parts left out of a summary that already lacks them change
    /// nothing.
    #[must_use]
    pub fn without(mut self, parts: &[Part]) -> Summary {
        for part in parts {
            match part {
                Part::Signature => self.signature = None,
            }
            if !self.omitted.contains(part) {
                self.omitted.push(*part);
            }
        }
        self
    }

    /// The value of the attribute `tag` across the drawing's block
    /// references, as plain text: one distinct value, none, or several (all
    /// listed).
    pub fn attribute(&self, tag: &str) -> Lookup<&str> {
        lookup(
            self.attributes
                .iter()
                .filter(|a| a.tag == tag)
                .map(|a| a.plain.as_str()),
        )
    }

    /// The value next to the loose text that reads `label`, likewise: the
    /// label is matched, and the value returned, as plain text.
    ///
    /// A value that goes on over several lines (see
    /// [`LabelledText::continued_by`]) is never `Unique`: its first line alone
    /// would pass for the whole value, so it comes back listed in `Ambiguous`.
    /// Read those from [`Self::labelled_texts`].
    pub fn labelled(&self, label: &str) -> Lookup<&str> {
        let carriers = || {
            self.labelled_texts
                .iter()
                .filter(|l| l.label.plain == label)
        };
        match lookup(carriers().map(|l| l.value.plain.as_str())) {
            // The one value is only the first line of several: listed, but
            // not given as the answer.
            Lookup::Unique(first) if carriers().any(|l| !l.continued_by.is_empty()) => {
                Lookup::Ambiguous(vec![first])
            }
            other => other,
        }
    }
}

fn lookup<'a>(values: impl Iterator<Item = &'a str>) -> Lookup<&'a str> {
    let mut distinct: Vec<&str> = values.collect();
    distinct.sort_unstable();
    distinct.dedup();
    match distinct.as_slice() {
        [] => Lookup::Absent,
        [one] => Lookup::Unique(one),
        _ => Lookup::Ambiguous(distinct),
    }
}

/// The summary of `db`, with the entities `selection` picks (see
/// [`crate::select()`]). The input is never modified.
pub fn summarize_with(db: &CadDatabase, selection: &Selection) -> Summary {
    Summary {
        selection: Some(select(db, selection)),
        ..summarize(db)
    }
}

/// The summary of `db`. The input is never modified; the same drawing gives
/// the same summary, in the same order.
pub fn summarize(db: &CadDatabase) -> Summary {
    let mut by_type = BTreeMap::new();
    let mut per_layer: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_block: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unresolved_inserts = Vec::new();
    let mut confidence = Confidence::High;
    let mut attributes = Vec::new();
    let mut dimensions = Vec::new();
    let mut tolerance_frames = Vec::new();
    let mut tables = Vec::new();
    let spaces = crate::hit_test::spaces(db);

    for e in &db.entities {
        *by_type.entry(e.type_name().to_string()).or_insert(0) += 1;
        if let Ref::Resolved(layer) = &e.common().layer {
            *per_layer.entry(layer.as_str()).or_insert(0) += 1;
        }
        confidence = confidence.min(e.common().confidence);
        if let Entity::Dimension(d) = e {
            dimensions.push(DimensionSummary::of(d, &db.tables));
        }
        if let Entity::Tolerance(t) = e {
            tolerance_frames.push(ToleranceFrame {
                id: t.common.id,
                text: t.text_value.clone(),
                plain: plain(&t.text_value, TextKind::MText),
                insertion_point: Point2D {
                    x: t.insertion_point.x,
                    y: t.insertion_point.y,
                },
                style: t.style_name.clone(),
                confidence: t.common.confidence,
            });
        }
        if let Entity::AcadTable(t) = e {
            tables.push(TableSummary::of(t, &spaces));
        }
        if let Entity::Insert(insert) = e {
            match &insert.block_name {
                Ref::Resolved(block) => *per_block.entry(block.as_str()).or_insert(0) += 1,
                other => unresolved_inserts.push(UnresolvedInsert {
                    insert: insert.common.id,
                    block: other.clone(),
                }),
            }
            for a in &insert.attribs {
                attributes.push(AttributeValue {
                    insert: insert.common.id,
                    block: insert.block_name.clone(),
                    id: a.common.id,
                    tag: a.tag.clone(),
                    value: a.text.clone(),
                    plain: plain_text(&a.text),
                    start_point: world_start(a.start_point, a.extrusion),
                    space: spaces.get(&a.common.id).map(|s| s.to_string()),
                    confidence: a.common.confidence.min(insert.common.confidence),
                });
            }
        }
    }
    attributes.sort_by(|a, b| a.insert.cmp(&b.insert).then(a.id.cmp(&b.id)));
    dimensions.sort_by_key(|d| d.id);
    tolerance_frames.sort_by_key(|t| t.id);
    tables.sort_by_key(|t| t.id);

    let layers = db
        .tables
        .layers
        .values()
        .map(|l| LayerSummary {
            name: l.name.clone(),
            color_index: l.color_index,
            entity_count: per_layer.get(l.name.as_str()).copied().unwrap_or(0),
        })
        .collect();
    let blocks = db
        .tables
        .block_records
        .values()
        .filter(|b| !is_space(&b.name))
        .map(|b| BlockSummary {
            name: b.name.clone(),
            entity_count: b.entities.len(),
            insert_count: per_block.get(b.name.as_str()).copied().unwrap_or(0),
            external_reference: b.external_reference.clone(),
        })
        .collect();

    let (labelled_texts, unplaced_texts) = labelled_texts(db, &spaces);
    Summary {
        units: db.header.insunits.map(|code| {
            let unit = Units::from_insunits(code);
            DrawingUnits {
                code,
                name: unit.name,
                to_mm: unit.to_mm,
            }
        }),
        drawing_ids: DrawingIds {
            fingerprint: db.header.fingerprintguid.clone(),
            version: db.header.versionguid.clone(),
        },
        entity_count: db.entities.len(),
        by_type,
        layers,
        blocks,
        unresolved_inserts,
        attributes,
        labelled_texts,
        unplaced_texts,
        dimensions,
        tolerance_frames,
        tables,
        extents: crate::extent::space_extents(db),
        signature: Some(signature(db)),
        omitted: Vec::new(),
        selection: None,
        confidence,
        warnings: db.read_diagnostics.warnings.clone(),
    }
}

fn is_space(name: &str) -> bool {
    name.eq_ignore_ascii_case("*Model_Space") || name.eq_ignore_ascii_case("*Paper_Space")
}

/// Loose TEXT entities paired by position, in two placements.
///
/// Beneath: a text is a label when a larger text sits right under it in the
/// same column (see [`stacked_pairs`]). Right: for each remaining text, the
/// nearest text to its right on the same row (within half a text height
/// vertically) is its value. A text with nothing to its right is a label of
/// nothing and is not listed; a text may be the value of one label and the
/// label of the next, which is what a row of three reads as. Several texts
/// at the same nearest distance are all listed (by ID) -- a tie is not broken
/// by picking one. A text that took part in a pair beneath, as label, value
/// or later line of the value, takes no part in the row rule. Positions are
/// the world's: a text written in a mirror copy's plane is where it is
/// drawn, and one on a plane tilted out of the world's is not paired, since
/// no 2D position places it exactly.
/// The label-value pairs among the loose texts, and the loose texts that
/// could not take part because their plane is tilted.
fn labelled_texts(
    db: &CadDatabase,
    spaces: &BTreeMap<EntityId, &str>,
) -> (Vec<LabelledText>, Vec<TextRef>) {
    let mut texts = Vec::new();
    let mut unplaced = Vec::new();
    for e in &db.entities {
        let Entity::Text(t) = e else { continue };
        match world_start(t.start_point, t.extrusion) {
            Some(at) => texts.push((t, at)),
            None => unplaced.push(TextRef::of(t.common.id, &t.text, None, spaces)),
        }
    }
    unplaced.sort_by_key(|t| t.id);
    let (mut pairs, stacked) = stacked_pairs(&texts, spaces);
    for &(label, here) in texts
        .iter()
        .filter(|(t, _)| !stacked.contains(&t.common.id))
    {
        let same_row = |(t, at): &&PlacedText<'_>| {
            (at.y - here.y).abs() <= label.text_height.max(t.text_height) / 2.0 && at.x > here.x
        };
        // Every placed text stands in the way, one taken by a pair beneath
        // included: the nearest text to the right is what it is.
        let candidates: Vec<_> = texts
            .iter()
            .filter(|c| c.0.common.id != label.common.id && same_row(c))
            .collect();
        let Some(nearest) = candidates
            .iter()
            .map(|(_, at)| at.x - here.x)
            .min_by(f64::total_cmp)
        else {
            continue;
        };
        let mut values: Vec<_> = candidates
            .into_iter()
            .filter(|(_, at)| at.x - here.x == nearest)
            .map(|&(t, at)| (t, at))
            .collect();
        // What stands nearest to the right belongs to a pair beneath: the
        // label has no value on its row that position can name.
        if values.iter().any(|(t, _)| stacked.contains(&t.common.id)) {
            continue;
        }
        values.sort_by_key(|(t, _)| t.common.id);
        for (value, there) in values {
            pairs.push(LabelledText {
                label: TextRef::of(label.common.id, &label.text, Some(here), spaces),
                value: TextRef::of(value.common.id, &value.text, Some(there), spaces),
                confidence: label.common.confidence.min(value.common.confidence),
                direction: LabelDirection::Right,
                continued_by: Vec::new(),
            });
        }
    }
    pairs.sort_by_key(|l| (l.label.id, l.value.id));
    (pairs, unplaced)
}

/// A loose text and where its baseline starts, in world coordinates.
type PlacedText<'a> = (&'a uncad_model::model::TextEntity, Point2D);

/// How much smaller a label is than its value, at most, for the two to read
/// as a label above a value: the label's height is at most this share of the
/// value's. Texts of one height are never paired this way -- nothing but a
/// size difference tells a stacked label from the value beneath it.
const STACK_LABEL_SHARE: f64 = 0.8;
/// How far beneath its label the value starts, at most, in value heights
/// (baseline to baseline).
const STACK_REACH: f64 = 1.8;
/// How far off its label's left edge the value may start, in value heights.
const STACK_COLUMN: f64 = 0.15;
/// How close beneath the line above a continuing line may start, at least,
/// in value heights: anything nearer is a text drawn over another (a faked
/// bold), not the next line.
const STACK_MIN_PITCH: f64 = 0.5;
/// The most lines a value may continue over. A column that goes on longer is
/// a list, not a field, and is left to the row rule.
const STACK_MAX_CONTINUED: usize = 3;
/// Two distances are the same distance when they differ by at most this share
/// of the larger one.
const STACK_TIE: f64 = 1e-9;

/// A text the stacked rule can place: left-justified on its baseline, not
/// turned, on the world's own plane. Anything else is where its alignment
/// point or its rotation puts it, which a column test cannot read.
fn stackable(t: &uncad_model::model::TextEntity) -> bool {
    use uncad_model::model::{HorizontalJustification as H, VerticalJustification as V};
    t.horizontal_justification == H::Left
        && t.vertical_justification == V::Baseline
        && t.rotation == 0.0
        && t.text_height.is_finite()
        && t.text_height > 0.0
        && Ocs::of(t.extrusion).is_some_and(|o| o.is_flat() && o.z_axis().z > 0.0)
}

/// Texts ordered by where they start along x, so the texts of one column are
/// found without looking at the rest.
struct ByX<'a> {
    sorted: Vec<&'a PlacedText<'a>>,
}

impl<'a> ByX<'a> {
    fn new(mut texts: Vec<&'a PlacedText<'a>>) -> Self {
        texts.sort_by(|a, b| {
            a.1.x
                .total_cmp(&b.1.x)
                .then(a.0.common.id.cmp(&b.0.common.id))
        });
        ByX { sorted: texts }
    }

    /// The texts starting within `reach` of `x` along x.
    fn near(&self, x: f64, reach: f64) -> impl Iterator<Item = &'a PlacedText<'a>> + '_ {
        let from = self.sorted.partition_point(|t| t.1.x < x - reach);
        self.sorted[from..]
            .iter()
            .take_while(move |t| t.1.x <= x + reach)
            .copied()
    }
}

/// Whether `a` and `b` are the same distance.
fn tied(a: f64, b: f64) -> bool {
    (a - b).abs() <= STACK_TIE * a.abs().max(b.abs())
}

/// How a value's later lines came out.
enum Continued<'a> {
    /// The lines that continue it (none for a one-line value).
    Lines(Vec<PlacedText<'a>>),
    /// Two lines were equally near the next one: where the value goes on is
    /// not told by position.
    Unclear(Vec<PlacedText<'a>>),
    /// More lines than a field holds.
    TooLong,
}

/// The label-above-value pairs among the loose texts, and the IDs of every
/// text that took part (label, value, or a later line of a value).
///
/// A text L is the label of a text V when V starts beneath L (within
/// [`STACK_REACH`] value heights) in L's column (within [`STACK_COLUMN`]),
/// L is smaller than V ([`STACK_LABEL_SHARE`]), and no other loose text sits
/// in that column between them. V is the nearest such text; texts equally
/// near are all listed. Lines of the same height directly beneath V, each
/// with no text between it and the line above, continue the value
/// ([`continuation`]). A value whose later lines cannot be told is not
/// listed -- its first line alone would pass for all of it -- but its texts
/// still keep out of the row rule.
fn stacked_pairs(
    texts: &[PlacedText<'_>],
    spaces: &BTreeMap<EntityId, &str>,
) -> (Vec<LabelledText>, BTreeSet<EntityId>) {
    let eligible: Vec<&PlacedText<'_>> = texts.iter().filter(|(t, _)| stackable(t)).collect();
    let tallest = eligible
        .iter()
        .map(|(t, _)| t.text_height)
        .fold(0.0, f64::max);
    let columns = ByX::new(eligible.clone());
    let everything = ByX::new(texts.iter().collect());
    let blocked = |top: Point2D, bottom: Point2D, height: f64, skip: &[EntityId]| {
        everything
            .near(top.x, STACK_COLUMN * height)
            .any(|(t, at)| !skip.contains(&t.common.id) && at.y < top.y && at.y > bottom.y)
    };
    let mut pairs = Vec::new();
    let mut taken = BTreeSet::new();
    for &&(label, here) in &eligible {
        let candidates: Vec<_> = columns
            .near(here.x, STACK_COLUMN * tallest)
            .filter(|(v, at)| {
                let drop = here.y - at.y;
                v.common.id != label.common.id
                    && drop > 0.0
                    && drop <= STACK_REACH * v.text_height
                    && (at.x - here.x).abs() <= STACK_COLUMN * v.text_height
                    && label.text_height <= STACK_LABEL_SHARE * v.text_height
            })
            .collect();
        let Some(nearest) = candidates
            .iter()
            .map(|(_, at)| here.y - at.y)
            .min_by(f64::total_cmp)
        else {
            continue;
        };
        let mut values: Vec<_> = candidates
            .into_iter()
            .filter(|(_, at)| tied(here.y - at.y, nearest))
            .map(|&(t, at)| (t, at))
            .filter(|(v, at)| !blocked(here, *at, v.text_height, &[label.common.id, v.common.id]))
            .collect();
        values.sort_by_key(|(t, _)| t.common.id);
        for (value, there) in values {
            let (continued, listed) = match continuation(&columns, &blocked, value, there) {
                Continued::Lines(lines) => (lines, true),
                Continued::Unclear(lines) => (lines, false),
                // A list, not a field: left to the row rule as it was.
                Continued::TooLong => continue,
            };
            taken.insert(label.common.id);
            taken.insert(value.common.id);
            taken.extend(continued.iter().map(|(t, _)| t.common.id));
            if !listed {
                continue;
            }
            pairs.push(LabelledText {
                label: TextRef::of(label.common.id, &label.text, Some(here), spaces),
                value: TextRef::of(value.common.id, &value.text, Some(there), spaces),
                confidence: label.common.confidence.min(value.common.confidence),
                direction: LabelDirection::Below,
                continued_by: continued
                    .iter()
                    .map(|&(t, at)| TextRef::of(t.common.id, &t.text, Some(at), spaces))
                    .collect(),
            });
        }
    }
    (pairs, taken)
}

/// The lines that continue a value beneath its first line, top to bottom: of
/// the value's height, in its column, at least [`STACK_MIN_PITCH`] and at most
/// [`STACK_REACH`] heights beneath the line above, with no text between. A
/// line that is itself the label of a larger text beneath it starts the next
/// field and ends the value.
fn continuation<'a>(
    columns: &ByX<'a>,
    blocked: &impl Fn(Point2D, Point2D, f64, &[EntityId]) -> bool,
    value: &'a uncad_model::model::TextEntity,
    first: Point2D,
) -> Continued<'a> {
    let height = value.text_height;
    let is_label = |c: &uncad_model::model::TextEntity, at: Point2D| {
        columns
            .near(at.x, STACK_COLUMN * height * 2.0)
            .any(|(d, there)| {
                let drop = at.y - there.y;
                d.common.id != c.common.id
                    && drop > 0.0
                    && drop <= STACK_REACH * d.text_height
                    && (there.x - at.x).abs() <= STACK_COLUMN * d.text_height
                    && c.text_height <= STACK_LABEL_SHARE * d.text_height
            })
    };
    let mut lines: Vec<PlacedText<'a>> = Vec::new();
    let mut above = first;
    loop {
        let candidates: Vec<_> = columns
            .near(first.x, STACK_COLUMN * height)
            .filter(|(t, at)| {
                let drop = above.y - at.y;
                t.common.id != value.common.id
                    && !lines.iter().any(|(l, _)| l.common.id == t.common.id)
                    && (t.text_height - height).abs() <= height * 1e-6
                    && drop >= STACK_MIN_PITCH * height
                    && drop <= STACK_REACH * height
            })
            .collect();
        let Some(nearest) = candidates
            .iter()
            .map(|(_, at)| above.y - at.y)
            .min_by(f64::total_cmp)
        else {
            break;
        };
        let next: Vec<_> = candidates
            .into_iter()
            .filter(|(_, at)| tied(above.y - at.y, nearest))
            .collect();
        if next.len() != 1 {
            return Continued::Unclear(lines);
        }
        let &(t, at) = next[0];
        let previous = lines.last().map_or(value.common.id, |(l, _)| l.common.id);
        if blocked(above, at, height, &[previous, t.common.id]) || is_label(t, at) {
            break;
        }
        lines.push((t, at));
        if lines.len() > STACK_MAX_CONTINUED {
            return Continued::TooLong;
        }
        above = at;
    }
    Continued::Lines(lines)
}
