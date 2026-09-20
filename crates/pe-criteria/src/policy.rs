//! Who gets the land drop, when two parts of a run both want it.
//!
//! A land drop is one resource a turn and this engine has two claimants for
//! it. The effect library spends it looking at cards — it plays the
//! deepest-looking land you hold, a rule adopted because with no mana model
//! there was nothing else to tell two drops apart. The gate spends it paying
//! for spells — it assumes you played whichever land covers the cost. Both are
//! answers to *which land did you play this turn*, they disagree exactly on the
//! hands that matter, and a run holding both was refused rather than
//! arbitrated.
//!
//! This is the arbitration, and it is not a new idea: it is the **declared
//! priority over queries** that mulligan bottoming
//! ([#7](https://github.com/cramt/progress-engine/issues/7)) and selection
//! routing ([#17](https://github.com/cramt/progress-engine/issues/17)) already
//! are, over a third resource. The file names land queries in the order it
//! would play them, and the engine plays the first one it is holding. Given
//! the composition, that is a deterministic function of counts, which is the
//! one condition the exact engine runs on.
//!
//! Two rules make it total, and both are stated in the run rather than left
//! here:
//!
//! - **A land the list does not name is played last.** The list is a
//!   preference, not a whitelist; a land nobody preferred is still a land, and
//!   a drop you decline is a drop you never get back.
//! - **A tie inside one entry goes to the deeper look, and then to the card
//!   the decklist names first.** Two lands the list cannot separate are still
//!   different cards, so something has to choose, and the deeper look is the
//!   rule the effect walk already used — which means a list that does not
//!   mention your surveil land still fires the surveil.

/// A declared priority over the one land drop a turn, resolved to grouping
/// queries.
///
/// `any_land` is not one of the preferences and is not optional: it is the
/// query that says what a land *is*, so the engine can play a land nobody
/// ranked without having to be told separately which groups those are. Holding
/// it as its own field rather than as the last element of `prefer` is what
/// makes "the list always ends in a catch-all" a fact about the type instead of
/// an invariant somebody has to maintain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandDropPolicy {
    prefer: Vec<usize>,
    any_land: usize,
}

impl LandDropPolicy {
    /// `prefer` is grouping query indices, highest priority first; `any_land`
    /// is the index of the query matching every land in the deck.
    pub fn new(prefer: Vec<usize>, any_land: usize) -> LandDropPolicy {
        LandDropPolicy { prefer, any_land }
    }

    /// Every tier in priority order, the catch-all last.
    pub fn tiers(&self) -> impl Iterator<Item = usize> + '_ {
        self.prefer
            .iter()
            .copied()
            .chain(std::iter::once(self.any_land))
    }

    /// The query that decides whether a group is a land at all.
    pub fn any_land(&self) -> usize {
        self.any_land
    }

    /// How the run says it broke a tie, so the sentence is written once and
    /// read by whatever has to print it.
    pub const TIE_BREAK: &'static str =
        "a tie inside one entry goes to the deeper look, then to the card this decklist names \
         first";
}
