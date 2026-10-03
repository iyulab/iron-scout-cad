//! Deterministic eyes for a 2D CAD drawing in the [`uncad_model`] entity
//! model: a compact [`Summary`] of what the drawing contains, and
//! [`hit_test`], which turns a point into the entity references at that
//! spot.
//!
//! Nothing here infers. A value that cannot be established is reported as
//! such -- an attribute that several title blocks give different values for
//! is [`Lookup::Ambiguous`] with every value, a point that two coincident
//! lines pass through is two hits, what a block reference draws is searched
//! in place and what could not be searched is listed with its reason -- and
//! a summary never carries a higher confidence than the entities it was
//! made from. The input is never
//! modified, and the same drawing gives the same summary, byte for byte.
//!
//! The verb set of this crate is these two read-only verbs. A summary can
//! also carry a [`Selection`] -- entities picked by type, layer, window,
//! space or reference ID, each with its box and, on request, its model
//! record -- a parameter of the summary, not a verb of its own.
//! `docs/principles.md` section 5 makes a new verb a proposal.

#![forbid(unsafe_code)]

mod boundary;
mod curve;
mod extent;
mod geometry;
mod hit_test;
mod limits;
mod select;
mod signature;
mod summary;

pub use extent::{BoundedBy, Bounds, SpaceExtent};
pub use hit_test::{hit_test, Hit, HitTest, NotSearched, NotSearchedReason};
pub use select::{select, Selected, SelectedEntity, Selection, SpaceFilter};
pub use signature::{signature, Signature, ToleranceCounts, SIGNATURE_VERSION};
pub use summary::{
    plain_text, summarize, summarize_with, AttributeValue, BlockSummary, DimensionSummary,
    DrawingUnits, LabelledText, LayerSummary, Lookup, StyleTolerance, Summary, TextRef, TextStack,
    ToleranceFrame, UnresolvedInsert,
};

pub use uncad_model::CadDatabase;
