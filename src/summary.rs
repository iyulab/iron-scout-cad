//! The summary: what a drawing contains, compactly, with nothing guessed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uncad_model::model::{Confidence, Entity, EntityId, Ref};
use uncad_model::CadDatabase;

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
    pub value: String,
    pub confidence: Confidence,
}

/// A piece of text by reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextRef {
    pub id: EntityId,
    pub text: String,
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
    /// The lowest confidence of any entity summarized; `High` for a drawing
    /// with no entities.
    pub confidence: Confidence,
    /// The drawing's own read diagnostics, passed through unchanged.
    pub warnings: Vec<String>,
}

impl Summary {
    /// The value of the attribute `tag` across the drawing's block
    /// references: one distinct value, none, or several (all listed).
    pub fn attribute(&self, tag: &str) -> Lookup<&str> {
        lookup(
            self.attributes
                .iter()
                .filter(|a| a.tag == tag)
                .map(|a| a.value.as_str()),
        )
    }

    /// The value next to the loose text `label`, likewise.
    pub fn labelled(&self, label: &str) -> Lookup<&str> {
        lookup(
            self.labelled_texts
                .iter()
                .filter(|l| l.label.text == label)
                .map(|l| l.value.text.as_str()),
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

    Summary {
        entity_count: db.entities.len(),
        by_type,
        layers,
        blocks,
        attributes,
        labelled_texts: labelled_texts(db),
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
/// picking one.
fn labelled_texts(db: &CadDatabase) -> Vec<LabelledText> {
    let texts: Vec<_> = db
        .entities
        .iter()
        .filter_map(|e| match e {
            Entity::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    let mut pairs = Vec::new();
    for label in &texts {
        let same_row = |t: &&uncad_model::model::TextEntity| {
            (t.start_point.y - label.start_point.y).abs()
                <= label.text_height.max(t.text_height) / 2.0
                && t.start_point.x > label.start_point.x
        };
        let candidates: Vec<_> = texts
            .iter()
            .filter(|t| t.common.id != label.common.id && same_row(t))
            .collect();
        let Some(nearest) = candidates
            .iter()
            .map(|t| t.start_point.x - label.start_point.x)
            .min_by(f64::total_cmp)
        else {
            continue;
        };
        let mut values: Vec<_> = candidates
            .into_iter()
            .filter(|t| t.start_point.x - label.start_point.x == nearest)
            .collect();
        values.sort_by_key(|t| t.common.id);
        for value in values {
            pairs.push(LabelledText {
                label: TextRef {
                    id: label.common.id,
                    text: label.text.clone(),
                },
                value: TextRef {
                    id: value.common.id,
                    text: value.text.clone(),
                },
                confidence: label.common.confidence.min(value.common.confidence),
            });
        }
    }
    pairs.sort_by_key(|l| (l.label.id, l.value.id));
    pairs
}
