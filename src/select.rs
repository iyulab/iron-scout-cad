//! Picking entities out of a summary: by type, layer, window, space or
//! reference ID -- the way one selects in a CAD program, so that a caller
//! who knows *what* it is looking for, but not *where*, can find it.
//!
//! A selection filters on what the file states (an entity's type and layer)
//! and on the same points the summary's extents are measured by. It never
//! compares a field's value: which of the selected circles is the one
//! wanted is the caller's to read from their records.

use crate::extent::{bounds, points_of, Bounds};
use crate::hit_test::spaces;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uncad_model::model::{Entity, EntityId, Ref};
use uncad_model::{CadDatabase, Point2D};

/// Which space a selection keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum SpaceFilter {
    /// `*Model_Space`.
    Model,
    /// Every `*Paper_Space` sheet.
    Paper,
}

/// What to select. Every filter given must hold; none given selects every
/// top-level entity.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Selection {
    entity_type: Option<String>,
    layer: Option<String>,
    within: Option<Bounds>,
    space: Option<SpaceFilter>,
    ids: Vec<EntityId>,
    limit: Option<usize>,
    detail: bool,
}

impl Selection {
    /// Selects every top-level entity.
    pub fn new() -> Self {
        Self::default()
    }

    /// Only entities of this DXF type name (`CIRCLE`), matched as CAD
    /// programs match names: without regard to case.
    pub fn of_type(mut self, name: &str) -> Self {
        self.entity_type = Some(name.to_string());
        self
    }

    /// Only entities on this layer, matched without regard to case. An
    /// entity whose layer the file points at but the drawing does not hold,
    /// or that points at none, is on no named layer and never matches.
    pub fn on_layer(mut self, name: &str) -> Self {
        self.layer = Some(name.to_string());
        self
    }

    /// Only entities whose points reach into this window -- a crossing
    /// selection: one partly inside is kept. An entity this crate takes no
    /// point from cannot be judged, so it is left out, and its type is named
    /// in [`Selected::not_measured`].
    pub fn crossing(mut self, window: Bounds) -> Self {
        self.within = Some(window);
        self
    }

    /// Only entities of this space.
    pub fn in_space(mut self, space: SpaceFilter) -> Self {
        self.space = Some(space);
        self
    }

    /// Only the entities with these reference IDs.
    pub fn with_ids(mut self, ids: impl IntoIterator<Item = EntityId>) -> Self {
        self.ids.extend(ids);
        self
    }

    /// At most this many entities in [`Selected::entities`], the first by
    /// reference ID; [`Selected::total`] still counts all of them.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Each selected entity also carries its model record, exactly as the
    /// model serializes it -- the field names an edit addresses.
    pub fn with_detail(mut self) -> Self {
        self.detail = true;
        self
    }
}

/// One selected entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SelectedEntity {
    pub id: EntityId,
    /// The DXF type name the model reports for the entity.
    pub entity_type: String,
    /// The entity's layer, as the model carries it (a reference that may
    /// not resolve).
    pub layer: Ref<String>,
    /// The space the entity is in, as the extents name it; absent when the
    /// drawing lists no space block that holds the entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    /// The box around the points the entity is measured by, as the extents
    /// measure them (see [`crate::SpaceExtent`]); `None` for an entity this
    /// crate takes no point from.
    pub bounds: Option<Bounds>,
    /// The model record, when [`Selection::with_detail`] asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<Entity>,
}

/// What a [`Selection`] picked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Selected {
    /// How many top-level entities the filters kept, before the limit.
    pub total: usize,
    /// The kept entities, by reference ID, up to the limit.
    pub entities: Vec<SelectedEntity>,
    /// Entity types this crate took no point from among those the other
    /// filters kept: without a window they are selected with no bounds,
    /// with one they are left out, since whether they reach into it cannot
    /// be told. Sorted, each once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_measured: Vec<String>,
}

fn crosses(b: &Bounds, window: &Bounds) -> bool {
    b.min.x <= window.max.x
        && b.max.x >= window.min.x
        && b.min.y <= window.max.y
        && b.max.y >= window.min.y
}

fn in_space(space: Option<&str>, filter: SpaceFilter) -> bool {
    let Some(name) = space else {
        return false;
    };
    let upper = name.to_ascii_uppercase();
    match filter {
        SpaceFilter::Model => upper == "*MODEL_SPACE",
        SpaceFilter::Paper => upper.starts_with("*PAPER_SPACE"),
    }
}

/// The top-level entities of `db` that `selection` keeps -- the same
/// entities [`crate::Summary::by_type`] counts. The input is never modified.
pub fn select(db: &CadDatabase, selection: &Selection) -> Selected {
    let space_of = spaces(db);
    let ids: BTreeSet<EntityId> = selection.ids.iter().copied().collect();
    let mut kept = Vec::new();
    let mut not_measured = BTreeSet::new();
    for e in &db.entities {
        let common = e.common();
        if !ids.is_empty() && !ids.contains(&common.id) {
            continue;
        }
        if let Some(t) = &selection.entity_type {
            if !e.type_name().eq_ignore_ascii_case(t) {
                continue;
            }
        }
        if let Some(layer) = &selection.layer {
            match &common.layer {
                Ref::Resolved(name) if name.eq_ignore_ascii_case(layer) => {}
                _ => continue,
            }
        }
        let space = space_of.get(&common.id).copied();
        if let Some(filter) = selection.space {
            if !in_space(space, filter) {
                continue;
            }
        }
        let mut points: Vec<Point2D> = Vec::new();
        let measured = points_of(e, &mut points);
        let b = if measured { bounds(&points) } else { None };
        if b.is_none() {
            not_measured.insert(e.type_name().to_string());
        }
        if let Some(window) = &selection.within {
            match &b {
                Some(b) if crosses(b, window) => {}
                _ => continue,
            }
        }
        kept.push(SelectedEntity {
            id: common.id,
            entity_type: e.type_name().to_string(),
            layer: common.layer.clone(),
            space: space.map(str::to_string),
            bounds: b,
            record: selection.detail.then(|| e.clone()),
        });
    }
    kept.sort_by_key(|s| s.id);
    let total = kept.len();
    if let Some(n) = selection.limit {
        kept.truncate(n);
    }
    Selected {
        total,
        entities: kept,
        not_measured: not_measured.into_iter().collect(),
    }
}
