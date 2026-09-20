//! Partitioning a library into groups of interchangeable cards.

use crate::mana::ManaSource;
use thiserror::Error;

/// A `u64` mask carries one bit per query.
pub const MAX_QUERIES: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GroupingError {
    #[error("too many distinct queries: {0} (limit {MAX_QUERIES})")]
    TooManyQueries(usize),
}

/// Cards bucketed by exactly which queries they match.
///
/// Two cards matching the same set of queries are indistinguishable to any
/// criterion, so they collapse into one group. Cards matching nothing collapse
/// into a single group too — usually most of the deck. This is why the work
/// depends on how many *queries* you asked about rather than on deck size.
///
/// **Except that mana splits a group too.** Two lands matching the same
/// queries are still not interchangeable if one taps for white and the other
/// for blue, because the gate can tell them apart even when no query can. So a
/// group is a set of cards agreeing on their queries *and* on their
/// [`ManaSource`], and the extra splitting is exactly the cost of asking a mana
/// question: a file that asks none hands every card the same `Spell` and gets
/// the groups it always had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouping {
    queries: Vec<String>,
    group_masks: Vec<u64>,
    group_sizes: Vec<u32>,
    group_mana: Vec<ManaSource>,
}

impl Grouping {
    /// Build from per-card match results. `matches[i]` is the mask of queries
    /// card `i` satisfies; `qty[i]` is how many copies are in the library.
    ///
    /// Nothing here is a mana source. Kept beside [`Grouping::with_mana`]
    /// rather than folded into it because a caller with no card data to hand —
    /// the property tests, chiefly — is describing a library by its queries,
    /// and making it spell out that none of its cards are lands would be asking
    /// it to state something it never claimed.
    pub fn build(
        queries: Vec<String>,
        cards: impl IntoIterator<Item = (u64, u32)>,
    ) -> Result<Self, GroupingError> {
        Self::with_mana(
            queries,
            cards
                .into_iter()
                .map(|(mask, qty)| (mask, ManaSource::Spell, qty)),
        )
    }

    /// Build from per-card match results and what each card does for mana.
    pub fn with_mana(
        queries: Vec<String>,
        cards: impl IntoIterator<Item = (u64, ManaSource, u32)>,
    ) -> Result<Self, GroupingError> {
        if queries.len() > MAX_QUERIES {
            return Err(GroupingError::TooManyQueries(queries.len()));
        }
        let mut group_masks: Vec<u64> = Vec::new();
        let mut group_sizes: Vec<u32> = Vec::new();
        let mut group_mana: Vec<ManaSource> = Vec::new();
        for (mask, mana, qty) in cards {
            match group_masks
                .iter()
                .zip(&group_mana)
                .position(|(m, s)| *m == mask && *s == mana)
            {
                Some(i) => group_sizes[i] += qty,
                None => {
                    group_masks.push(mask);
                    group_sizes.push(qty);
                    group_mana.push(mana);
                }
            }
        }
        Ok(Grouping {
            queries,
            group_masks,
            group_sizes,
            group_mana,
        })
    }

    pub fn queries(&self) -> &[String] {
        &self.queries
    }

    pub fn group_sizes(&self) -> &[u32] {
        &self.group_sizes
    }

    pub fn group_masks(&self) -> &[u64] {
        &self.group_masks
    }

    /// What each group does for mana, in group order.
    pub fn group_mana(&self) -> &[ManaSource] {
        &self.group_mana
    }

    pub fn query_index(&self, query: &str) -> Option<usize> {
        self.queries.iter().position(|q| q == query)
    }

    /// Total library size.
    pub fn population(&self) -> u32 {
        self.group_sizes.iter().sum()
    }

    /// How many cards in the whole library match `query_idx`.
    pub fn matching_total(&self, query_idx: usize) -> u32 {
        self.sum_matching(self.group_sizes(), query_idx)
    }

    /// How many of `counts` (one per group) match `query_idx`.
    ///
    /// A card can be in several queries at once, which is exactly why this sums
    /// across groups rather than reading one: overlapping categories are the
    /// case that breaks naive inclusion-exclusion.
    pub fn count_matching(&self, counts: &[u32], query_idx: usize) -> u32 {
        self.sum_matching(counts, query_idx)
    }

    fn sum_matching(&self, counts: &[u32], query_idx: usize) -> u32 {
        if query_idx >= self.queries.len() {
            return 0;
        }
        let bit = 1u64 << query_idx;
        self.group_masks
            .iter()
            .zip(counts)
            .filter(|(mask, _)| *mask & bit != 0)
            .map(|(_, n)| *n)
            .sum()
    }
}
