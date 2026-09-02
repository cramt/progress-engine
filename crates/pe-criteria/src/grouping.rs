//! Partitioning a library into groups of interchangeable cards.

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grouping {
    queries: Vec<String>,
    group_masks: Vec<u64>,
    group_sizes: Vec<u32>,
}

impl Grouping {
    /// Build from per-card match results. `matches[i]` is the mask of queries
    /// card `i` satisfies; `qty[i]` is how many copies are in the library.
    pub fn build(
        queries: Vec<String>,
        cards: impl IntoIterator<Item = (u64, u32)>,
    ) -> Result<Self, GroupingError> {
        if queries.len() > MAX_QUERIES {
            return Err(GroupingError::TooManyQueries(queries.len()));
        }
        let mut group_masks: Vec<u64> = Vec::new();
        let mut group_sizes: Vec<u32> = Vec::new();
        for (mask, qty) in cards {
            match group_masks.iter().position(|m| *m == mask) {
                Some(i) => group_sizes[i] += qty,
                None => {
                    group_masks.push(mask);
                    group_sizes.push(qty);
                }
            }
        }
        Ok(Grouping {
            queries,
            group_masks,
            group_sizes,
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
