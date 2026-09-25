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
//! The verb set of this crate is these two read-only verbs.
//! `docs/principles.md` section 5 makes a new verb a proposal.

#![forbid(unsafe_code)]

mod geometry;
mod hit_test;
mod summary;

pub use hit_test::{hit_test, Hit, HitTest, NotSearched, NotSearchedReason};
pub use summary::{
    plain_text, summarize, AttributeValue, BlockSummary, DimensionSummary, LabelledText,
    LayerSummary, Lookup, Summary, TextRef,
};

pub use uncad_model::CadDatabase;
