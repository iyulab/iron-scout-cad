//! The summary: what a drawing contains, compactly, with nothing guessed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uncad_model::model::{Confidence, Entity, EntityId, Ref};
use uncad_model::{CadDatabase, Ocs, Point2D};

/// A layer and how much of the drawing is on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerSummary {
    pub name: String,
    pub color_index: i16,
    /// Top-level entities whose layer resolves to this one.
    pub entity_count: usize,
}

/// A block definition and how often it is referenced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockSummary {
    pub name: String,
    /// Entities in the definition.
    pub entity_count: usize,
    /// Top-level INSERTs that resolve to this block.
    pub insert_count: usize,
}

/// One attribute value attached to a block reference: what a title block's
/// fields look like in the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub confidence: Confidence,
}

/// A piece of text by reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRef {
    pub id: EntityId,
    /// The text as the model carries it, its codes included.
    pub text: String,
    /// The text as plain text (see [`plain_text`]).
    pub plain: String,
}

impl TextRef {
    fn of(id: EntityId, text: &str) -> Self {
        TextRef {
            id,
            text: text.to_string(),
            plain: plain_text(text),
        }
    }
}

/// What a TEXT or ATTRIB reads as, without the codes it is written in: the
/// characters, with `%%d`, `%%p` and `%%c` as the degree, plus-minus and
/// diameter signs they name and underline and overline switches dropped.
/// `%%nnn` stays as written -- which character it draws depends on the font.
/// This is what labels are matched by and values returned as; the text as
/// written stays beside it.
pub fn plain_text(text: &str) -> String {
    use uncad_model::text::{tokens, Special, TextKind, Token};
    let mut out = String::with_capacity(text.len());
    for token in tokens(text, TextKind::Line) {
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

/// Two loose TEXT entities that read as a label and its value: on the same
/// row, the value the nearest text to the right of the label. Nothing but
/// position ties them, which is how a title block drawn without a block
/// carries its fields. When several texts are nearest at the same distance
/// (texts drawn on top of each other), the label is listed once per
/// candidate, and [`Summary::labelled`] then answers with every distinct
/// value rather than picking one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelledText {
    pub label: TextRef,
    pub value: TextRef,
    pub confidence: Confidence,
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

/// What a drawing contains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// Top-level entities (model and paper space), as the model lists them.
    pub entity_count: usize,
    /// Count per entity type name, by name.
    pub by_type: BTreeMap<String, usize>,
    /// Every layer of the layer table, by name.
    pub layers: Vec<LayerSummary>,
    /// Every block definition except the two space records, by name.
    pub blocks: Vec<BlockSummary>,
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
    /// The lowest confidence of any entity summarized; `High` for a drawing
    /// with no entities.
    pub confidence: Confidence,
    /// The drawing's own read diagnostics, passed through unchanged.
    pub warnings: Vec<String>,
}

impl Summary {
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
    pub fn labelled(&self, label: &str) -> Lookup<&str> {
        lookup(
            self.labelled_texts
                .iter()
                .filter(|l| l.label.plain == label)
                .map(|l| l.value.plain.as_str()),
        )
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

/// The summary of `db`. The input is never modified; the same drawing gives
/// the same summary, in the same order.
pub fn summarize(db: &CadDatabase) -> Summary {
    let mut by_type = BTreeMap::new();
    let mut per_layer: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_block: BTreeMap<&str, usize> = BTreeMap::new();
    let mut confidence = Confidence::High;
    let mut attributes = Vec::new();

    for e in &db.entities {
        *by_type.entry(e.type_name().to_string()).or_insert(0) += 1;
        if let Ref::Resolved(layer) = &e.common().layer {
            *per_layer.entry(layer.as_str()).or_insert(0) += 1;
        }
        confidence = confidence.min(e.common().confidence);
        if let Entity::Insert(insert) = e {
            if let Ref::Resolved(block) = &insert.block_name {
                *per_block.entry(block.as_str()).or_insert(0) += 1;
            }
            for a in &insert.attribs {
                attributes.push(AttributeValue {
                    insert: insert.common.id,
                    block: insert.block_name.clone(),
                    id: a.common.id,
                    tag: a.tag.clone(),
                    value: a.text.clone(),
                    plain: plain_text(&a.text),
                    confidence: a.common.confidence.min(insert.common.confidence),
                });
            }
        }
    }
    attributes.sort_by(|a, b| a.insert.cmp(&b.insert).then(a.id.cmp(&b.id)));

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
        })
        .collect();

    let (labelled_texts, unplaced_texts) = labelled_texts(db);
    Summary {
        entity_count: db.entities.len(),
        by_type,
        layers,
        blocks,
        attributes,
        labelled_texts,
        unplaced_texts,
        confidence,
        warnings: db.read_diagnostics.warnings.clone(),
    }
}

fn is_space(name: &str) -> bool {
    name.eq_ignore_ascii_case("*Model_Space") || name.eq_ignore_ascii_case("*Paper_Space")
}

/// Loose TEXT entities paired by position: for each text, the nearest text
/// to its right on the same row (within half a text height vertically) is
/// its value. A text with nothing to its right is a label of nothing and is
/// not listed; a text may be the value of one label and the label of the
/// next, which is what a row of three reads as. Several texts at the same
/// nearest distance are all listed (by ID) -- a tie is not broken by
/// picking one. Positions are the world's: a text written in a mirror
/// copy's plane is where it is drawn, and one on a plane tilted out of the
/// world's is not paired, since no 2D position places it exactly.
/// The label-value pairs among the loose texts, and the loose texts that
/// could not take part because their plane is tilted.
fn labelled_texts(db: &CadDatabase) -> (Vec<LabelledText>, Vec<TextRef>) {
    let mut texts = Vec::new();
    let mut unplaced = Vec::new();
    for e in &db.entities {
        let Entity::Text(t) = e else { continue };
        match Ocs::of(t.extrusion).and_then(|o| o.flat_map()) {
            Some(plan) => texts.push((t, plan.apply(t.start_point))),
            None => unplaced.push(TextRef::of(t.common.id, &t.text)),
        }
    }
    unplaced.sort_by_key(|t| t.id);
    let mut pairs = Vec::new();
    for &(label, here) in &texts {
        let same_row = |(t, at): &&(&uncad_model::model::TextEntity, Point2D)| {
            (at.y - here.y).abs() <= label.text_height.max(t.text_height) / 2.0 && at.x > here.x
        };
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
            .map(|(t, _)| *t)
            .collect();
        values.sort_by_key(|t| t.common.id);
        for value in values {
            pairs.push(LabelledText {
                label: TextRef::of(label.common.id, &label.text),
                value: TextRef::of(value.common.id, &value.text),
                confidence: label.common.confidence.min(value.common.confidence),
            });
        }
    }
    pairs.sort_by_key(|l| (l.label.id, l.value.id));
    (pairs, unplaced)
}
