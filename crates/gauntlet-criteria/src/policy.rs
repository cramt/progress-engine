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

/// A declared priority over the turn's mana, resolved to grouping queries.
///
/// One more resource on the one mechanism, beside the looked-at set and the
/// land drop above — the only one of the three that depletes as you spend it.
/// One Island and one Opt and one Preordain: which do you cast? The file says,
/// in the order it would cast them, and the walk takes the first thing it is
/// holding that the pool still covers.
///
/// **There is no catch-all, and that is the one place this differs from
/// [`LandDropPolicy`].** A land the list does not name is still played, because
/// declining a land drop is not something a preference can be read as asking
/// for and because "any other land" costs one query bit. Pricing *every* spell
/// in a deck is not one bit: it splits the library by mana cost, forty ways on
/// a Commander list, and the enumeration it produces answers nothing anybody
/// asked. So the list is the line — the spells you are asking about — and a
/// spell it does not name is one this run declines to cast. That is the same
/// reading the gate already takes of the land drop ("nobody plays their lands
/// badly"), and it is stated in every run that uses one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastingPolicy {
    prefer: Vec<usize>,
}

impl CastingPolicy {
    /// `prefer` is grouping query indices, highest priority first.
    pub fn new(prefer: Vec<usize>) -> CastingPolicy {
        CastingPolicy { prefer }
    }

    /// Every tier in priority order.
    pub fn tiers(&self) -> impl Iterator<Item = usize> + '_ {
        self.prefer.iter().copied()
    }

    /// What a run says about the spells this list is silent on.
    pub const THEN: &'static str = "a spell this list does not name is not cast at all";

    /// How a tie inside one entry is settled.
    ///
    /// The cheaper spell first, which is the same shape of rule as the land
    /// drop's deeper look: inside one entry the file has said it wants these
    /// equally, so the only sense in which one is better is that paying for it
    /// leaves more of the pool for the rest.
    pub const TIE_BREAK: &'static str =
        "a tie inside one entry goes to the cheaper cost, then to the card this decklist names \
         first";
}
