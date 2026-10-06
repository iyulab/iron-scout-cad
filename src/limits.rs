//! The bounds on following block references, shared by every walk in this
//! crate that expands them.
//!
//! A drawing is untrusted input: a block may reference itself through
//! another, nest a reference inside a hundred others, or place a block ten
//! times that places the next ten times, ten levels down -- a billion
//! entities from a file of a few kilobytes. Each walk reports what these
//! bounds left out rather than ending without an answer; a well-formed
//! drawing stays far inside them.

use uncad_model::model::Ref;
use uncad_model::tables::{BlockRecord, Tables};

/// How deep block references may nest before a walk stops following them:
/// a reference inside this many others is not expanded.
pub(crate) const MAX_BLOCK_REF_DEPTH: usize = 20;

/// How many entities one walk may meet inside the blocks it expands, across
/// every level.
///
/// [`MAX_BLOCK_REF_DEPTH`] bounds nesting but not breadth: ten references
/// per block, ten levels down, is a billion entities long before the depth
/// bound engages. Counting references alone does not bound the work either
/// -- a block of ten thousand entities placed a million times is ten
/// billion. So the budget is the walk's work itself: expanding a block
/// costs its entity count (an empty block costs one), and a block that
/// does not fit in what is left is not expanded at all, so a block is
/// either walked whole or not entered.
pub(crate) const EXPANSION_BUDGET: usize = 10_000_000;

/// What one walk has spent of [`EXPANSION_BUDGET`].
#[derive(Debug, Default)]
pub(crate) struct Expansion {
    spent: usize,
}

impl Expansion {
    /// Whether a block of `entities` entities may be expanded, charging the
    /// budget when it may.
    pub(crate) fn take(&mut self, entities: usize) -> bool {
        let cost = entities.max(1);
        if cost > EXPANSION_BUDGET - self.spent {
            return false;
        }
        self.spent += cost;
        true
    }
}

/// Why a walk did not enter the block a reference draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotEntered {
    /// The reference names no block.
    Absent,
    /// The reference points at nothing the drawing answers to.
    Unresolved,
    /// The name resolves, but the drawing holds no block of that name.
    Undefined,
    /// The block is already being expanded: the reference draws itself,
    /// and following it would never end.
    Cycle,
    /// The reference sits inside [`MAX_BLOCK_REF_DEPTH`] others.
    TooDeep,
    /// The reference is placed on a plane tilted out of the world's: no 2D
    /// placement puts its block exactly.
    Tilted,
    /// The block would take the walk past [`EXPANSION_BUDGET`].
    BudgetExhausted,
}

/// What one walk that follows block references has open and has spent --
/// the one judgement of whether a reference is followed, shared by every
/// walk in this crate.
#[derive(Debug, Default)]
pub(crate) struct Walk {
    /// The blocks being expanded, outermost first.
    open: Vec<String>,
    expansion: Expansion,
}

impl Walk {
    /// Enters the block `name` names, placed by `placed` (given the block):
    /// the block resolved, not already open, not too deep, placed, and
    /// within the budget -- checked in that order, the budget charged only
    /// when all the rest hold. The block stays open until [`Self::leave`].
    pub(crate) fn enter<'t, T>(
        &mut self,
        tables: &'t Tables,
        name: &Ref<String>,
        placed: impl FnOnce(&BlockRecord) -> Option<T>,
    ) -> Result<(&'t BlockRecord, T), NotEntered> {
        let name = match name {
            Ref::Resolved(name) => name,
            Ref::Absent => return Err(NotEntered::Absent),
            Ref::Unresolved(_) => return Err(NotEntered::Unresolved),
        };
        let block = tables
            .block_records
            .get(name)
            .ok_or(NotEntered::Undefined)?;
        if self.open.iter().any(|open| open == name) {
            return Err(NotEntered::Cycle);
        }
        if self.open.len() >= MAX_BLOCK_REF_DEPTH {
            return Err(NotEntered::TooDeep);
        }
        let placement = placed(block).ok_or(NotEntered::Tilted)?;
        if !self.expansion.take(block.entities.len()) {
            return Err(NotEntered::BudgetExhausted);
        }
        self.open.push(name.clone());
        Ok((block, placement))
    }

    /// Leaves the block entered last.
    pub(crate) fn leave(&mut self) {
        self.open.pop();
    }

    /// Whether the walk is at the top level -- inside no block.
    pub(crate) fn at_top(&self) -> bool {
        self.open.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_taken_whole_or_not_at_all() {
        let mut e = Expansion::default();
        assert!(e.take(EXPANSION_BUDGET - 10));
        // Eleven do not fit in the ten left; ten do; then nothing does --
        // not even an empty block, which costs one.
        assert!(!e.take(11));
        assert!(e.take(10));
        assert!(!e.take(0));
    }
}
