//! Partitioning a library into groups of interchangeable cards.

use crate::mana::{LandDetail, ManaSource, Resolves};
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
    /// Cards of each group that start in the **command zone** rather than the
    /// library: never dealt, never drawn, and there to be cast.
    ///
    /// Not part of [`Grouping::group_sizes`], which is the library and nothing
    /// else — every draw, every hypergeometric and the sampler's deck are
    /// built from that, and a commander is in none of them. A group can hold
    /// only command-zone cards and have a library size of zero. Only a card the
    /// run can cast is ever here: a commander nobody's line names moves no
    /// number, so it is not grouped at all.
    group_command: Vec<u32>,
    /// For each query, the groups whose cards match it. Derived from the
    /// masks once, because counting a query is asked on every path and a
    /// pass over every group testing a bit was a sixth of a run.
    members: Vec<Vec<usize>>,
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
        Self::assemble(
            queries,
            cards
                .into_iter()
                .map(|(mask, mana, qty)| (mask, mana, qty, 0)),
        )
    }

    /// The same library, and beside it the cards that start in the command
    /// zone: `commanders` is `(mask, what it costs, copies)` per card.
    ///
    /// A commander joins a library group that matches the same queries and
    /// costs the same — to every question the two are the same card, except
    /// that one of them is never drawn — and otherwise gets a group of its own
    /// with nothing in the library. The library groups keep their positions,
    /// so the decklist order the tie rules read is unchanged and a commander
    /// comes after every library card in it.
    pub fn with_command_zone(
        self,
        commanders: impl IntoIterator<Item = (u64, ManaSource, u32)>,
    ) -> Grouping {
        let library = (0..self.group_sizes.len()).map(|g| {
            (
                self.group_masks[g],
                self.group_mana[g],
                self.group_sizes[g],
                self.group_command[g],
            )
        });
        let command = commanders
            .into_iter()
            .map(|(mask, mana, qty)| (mask, mana, 0, qty));
        Self::assemble(self.queries, library.chain(command).collect::<Vec<_>>())
            .expect("adding command-zone cards names no new query")
    }

    fn assemble(
        queries: Vec<String>,
        cards: impl IntoIterator<Item = (u64, ManaSource, u32, u32)>,
    ) -> Result<Self, GroupingError> {
        if queries.len() > MAX_QUERIES {
            return Err(GroupingError::TooManyQueries(queries.len()));
        }
        let mut group_masks: Vec<u64> = Vec::new();
        let mut group_sizes: Vec<u32> = Vec::new();
        let mut group_mana: Vec<ManaSource> = Vec::new();
        let mut group_command: Vec<u32> = Vec::new();
        for (mask, mana, qty, command) in cards {
            match group_masks
                .iter()
                .zip(&group_mana)
                .position(|(m, s)| *m == mask && *s == mana)
            {
                Some(i) => {
                    group_sizes[i] += qty;
                    group_command[i] += command;
                }
                // A group of nothing is not a group. Only a coarsening that
                // erased a commander's cost produces one, and a card that
                // cannot be cast from the command zone is not in the game.
                None if qty == 0 && command == 0 => {}
                None => {
                    group_masks.push(mask);
                    group_sizes.push(qty);
                    group_mana.push(mana);
                    group_command.push(command);
                }
            }
        }
        let members = (0..queries.len())
            .map(|q| {
                group_masks
                    .iter()
                    .enumerate()
                    .filter(|(_, mask)| *mask & (1u64 << q) != 0)
                    .map(|(g, _)| g)
                    .collect()
            })
            .collect();
        Ok(Grouping {
            queries,
            group_masks,
            group_sizes,
            group_mana,
            group_command,
            members,
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

    /// Cards of each group in the command zone, in group order: zero for every
    /// group in a run whose line names no commander.
    pub fn group_command(&self) -> &[u32] {
        &self.group_command
    }

    /// What each group does for mana, in group order.
    pub fn group_mana(&self) -> &[ManaSource] {
        &self.group_mana
    }

    /// The groups whose cards match `query_idx`, empty for a query this
    /// grouping does not hold.
    pub fn members(&self, query_idx: usize) -> &[usize] {
        self.members.get(query_idx).map_or(&[], Vec::as_slice)
    }

    /// Whether some card this grouping casts is put into the graveyard when it
    /// resolves: an instant or a sorcery the declared line names.
    ///
    /// The second way a card reaches the graveyard, beside an effect routing
    /// one there, and so the second half of whether the zone is reachable at
    /// all.
    pub fn casts_into_graveyard(&self) -> bool {
        self.group_mana
            .iter()
            .any(|mana| mana.resolves() == Some(Resolves::IntoGraveyard))
    }

    pub fn query_index(&self, query: &str) -> Option<usize> {
        self.queries.iter().position(|q| q == query)
    }

    /// The coarsest grouping that still tells apart everything `keep` names.
    ///
    /// Which cards are interchangeable depends on **who is asking**. A
    /// criterion counting `cat:"Ramp"` cannot tell a Plains from an Island; a
    /// `can_cast` clause can. Grouping a file once, as the join of everything
    /// any criterion needs, makes the first question pay for the second — on a
    /// real Commander manabase that is a dozen extra groups, and the
    /// enumeration widens by a factor of that per checkpoint. So the engine
    /// coarsens per class of questions instead.
    ///
    /// `keep` is a mask of the query bits one class reads, and it has to
    /// include the bits the **walk** reads as well as the bits its clauses do:
    /// which cards an effect applies to, where it routes them, and the
    /// land-drop priority. Those decide where a card ends up, so two cards that
    /// disagree about one of them are not interchangeable however little the
    /// criterion cares.
    ///
    /// `mana` is the same choice for what a land makes, and it has three
    /// useful settings rather than two. Only [`crate::Cost`] can see a
    /// palette at all, so a class with no casting clause asks for
    /// [`LandDetail::Ignored`] and the manabase collapses back to one group.
    /// A class that does ask about a cost need only keep the pips that cost
    /// demands — [`LandDetail`] is where that argument lives — and
    /// [`LandDetail::Pips`] with [`crate::Palette::ALL`] is the whole palette,
    /// for a caller that cannot prove a narrower one.
    ///
    /// Bits outside `keep` are **cleared, never renumbered**, so every index a
    /// clause already holds still means what it meant. Land groups are a
    /// different story: merging two of them renumbers the rest, and
    /// [`crate::Board`] rebuilds its own slot numbering — and the
    /// [`crate::mana::Constraint::Includes`] that indexes it — from whatever
    /// grouping it is handed, so those indices stay internally consistent. The
    /// cost of all of it is the reason this is not a general-purpose
    /// constructor: a question reading a cleared bit counts zero here, so only
    /// the class this was coarsened for may be answered against it.
    pub fn coarsened(&self, keep: u64, mana: LandDetail) -> Grouping {
        let cards = self
            .group_masks
            .iter()
            .zip(&self.group_mana)
            .zip(&self.group_sizes)
            .zip(&self.group_command)
            .map(|(((mask, source), qty), command)| {
                let seen = source.seen_as(mana);
                // A commander is only in the game to be cast, so a class that
                // erases what it costs erases it: it cannot be drawn, and
                // nothing in that class could have paid for it.
                let command = if seen.castable().is_some() {
                    *command
                } else {
                    0
                };
                (mask & keep, seen, *qty, command)
            });
        // The same query list at the same bit positions: this is a coarser
        // partition of the same library, not a different question.
        Grouping::assemble(self.queries.clone(), cards)
            .expect("coarsening names no query the original did not")
    }

    /// Where each of this grouping's groups lands in `coarse`, which has to be
    /// this grouping coarsened to `keep` and `detail` — or a coarsening of one.
    ///
    /// `None` where some group has nowhere to go, which is `coarse` not being
    /// coarser than this at all. Read by a class that decides something on
    /// one grouping and answers it on another: a mulligan strategy chooses on
    /// the openers it can tell apart, and each class plays the rest of the
    /// game on its own, and the two have to agree which of their groups are
    /// the same cards.
    ///
    /// Sound because every narrowing composes: bits are cleared, never
    /// renumbered, and a land's palette is only ever intersected, so a group
    /// coarsened twice lands where it would have landed coarsened once.
    pub fn projection(
        &self,
        coarse: &Grouping,
        keep: u64,
        detail: LandDetail,
    ) -> Option<Vec<usize>> {
        self.group_masks
            .iter()
            .zip(&self.group_mana)
            .zip(&self.group_sizes)
            .map(|((mask, mana), &size)| {
                let (mask, mana) = (mask & keep, mana.seen_as(detail));
                coarse
                    .group_masks
                    .iter()
                    .zip(&coarse.group_mana)
                    .position(|(m, s)| *m == mask && *s == mana)
                    // A group holding only command-zone cards has nothing in
                    // the library, so no deal ever puts a card of it anywhere,
                    // and a coarsening that could not cast it dropped it. Any
                    // group is as good a home for a count that is always zero.
                    .or_else(|| (size == 0 && !coarse.group_masks.is_empty()).then_some(0))
            })
            .collect()
    }

    /// How many groups a deal can put a card of: every group with something
    /// in the library.
    ///
    /// The width of an enumeration is counted in these, not in
    /// [`Grouping::group_sizes`]'s length. A group holding only command-zone
    /// cards is never dealt from, so it is a bin every composition leaves
    /// empty, and counting it would report — and refuse — a walk wider than
    /// the one that happens.
    pub fn dealt(&self) -> usize {
        self.group_sizes.iter().filter(|&&size| size > 0).count()
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
        match self.members.get(query_idx) {
            Some(members) => members.iter().map(|&g| counts[g]).sum(),
            None => 0,
        }
    }
}
