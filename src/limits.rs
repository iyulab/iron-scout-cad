//! The bounds on following block references, shared by every walk in this
//! crate that expands them.
//!
//! A drawing is untrusted input: a block may reference itself through
//! another, nest a reference inside a hundred others, or place a block ten
//! times that places the next ten times, ten levels down -- a billion
//! entities from a file of a few kilobytes. Each walk reports what these
//! bounds left out rather than ending without an answer; a well-formed
//! drawing stays far inside them.

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
