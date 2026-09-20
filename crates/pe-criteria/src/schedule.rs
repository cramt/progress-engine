//! When this run sees cards, and what it gets to do about it.
//!
//! Before effects existed, a run was a list of gaps — how many more cards each
//! turn draws — and a turn was a checkpoint. That is still true of a run with
//! no live effect, and [`Schedule::plain`] builds exactly that.
//!
//! An effect that looks at the top of the library breaks the one-checkpoint-
//! per-turn equality, because the order of the cards within a turn starts to
//! matter. Drawing a surveil land and then surveilling is not the same turn as
//! surveilling and then drawing it, so the card the draw takes and the card the
//! look examines cannot arrive as one unordered pair. Each gets a checkpoint of
//! its own, the enumeration tells them apart, and a turn becomes a span of
//! checkpoints rather than one.
//!
//! The extra checkpoints are what the feature costs. Each one multiplies the
//! enumeration by roughly the number of groups, so a question that was
//! comfortably enumerable without effects can become [`crate::RunError::TooWide`]
//! with them. That is the honest price of an exact answer: the alternative is
//! to sample the look, and a sampled surveil inside an exact engine is a
//! percentage nobody can attribute.

use crate::effect::Effect;
use crate::policy::LandDropPolicy;

/// The opening hand, before anybody has drawn for turn.
const OPENING_HAND: u32 = 7;

/// A run's turn structure: which checkpoints belong to which turn, and what
/// happens on each of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    gaps: Vec<u32>,
    /// First and last checkpoint of each turn, indexed by turn. The first is
    /// always the draw; anything after it is a look slot.
    spans: Vec<(usize, usize)>,
    effects: Vec<Effect>,
    /// Who gets the land drop, where the file said. `None` is a run that never
    /// declared one, and it is the state every run was in before
    /// [`LandDropPolicy`] existed: the effects choose among themselves by look
    /// depth and the gate assumes whichever line pays.
    land_drop: Option<LandDropPolicy>,
}

impl Schedule {
    /// One checkpoint per turn and no effects: what every run did before the
    /// effect library existed, and what a run with nothing live still does.
    ///
    /// Kept as a constructor of its own rather than as `build` with an empty
    /// effect list, because callers that hand over raw gaps — the property
    /// tests, chiefly — are describing a draw schedule directly rather than
    /// asking for one to be derived from a horizon.
    pub fn plain(gaps: &[u32]) -> Schedule {
        Schedule {
            gaps: gaps.to_vec(),
            spans: (0..gaps.len()).map(|t| (t, t)).collect(),
            effects: Vec::new(),
            land_drop: None,
        }
    }

    /// [`Schedule::plain`] with a land-drop priority declared.
    ///
    /// For a hand written as raw gaps — a seven-card library played out over
    /// turns that draw nothing — which is how HANDS.md's hands are asserted:
    /// one deal, every path is that deal, and a probability is a yes or a no.
    pub fn plain_under(gaps: &[u32], land_drop: LandDropPolicy) -> Schedule {
        Schedule {
            land_drop: Some(land_drop),
            ..Schedule::plain(gaps)
        }
    }

    /// The schedule for `horizon` turns with `effects` live.
    ///
    /// Every effect handed here is expected to be one that can actually move a
    /// number: it matches cards in this deck and it routes somewhere. An effect
    /// that routes nowhere leaves every card it looks at on top, which is where
    /// the card already was, so paying a checkpoint a turn for it would widen
    /// the enumeration to compute a value it cannot change.
    /// `land_drop` is the priority the file declared over the one drop a turn,
    /// and it is a parameter rather than something bolted on afterwards
    /// because a run that forgot to pass it would silently answer the mana
    /// question against a different land than the one the effects played.
    pub fn build(
        horizon: u32,
        on_the_draw: bool,
        effects: Vec<Effect>,
        land_drop: Option<LandDropPolicy>,
    ) -> Schedule {
        // One land drop a turn, so one effect fires a turn, so the deepest
        // single look is the most cards a turn can examine.
        let look_slots = effects.iter().map(|e| e.look).max().unwrap_or(0) as usize;
        let mut gaps = vec![OPENING_HAND];
        let mut spans = vec![(0usize, 0usize)];
        for turn in 1..=horizon {
            let first = gaps.len();
            gaps.push(draw_for(turn, on_the_draw));
            // One checkpoint per looked-at card, so the enumeration can tell
            // the order they came off the top. A single gap of `look_slots`
            // would hand the walk an unordered pair, and drawing a surveil land
            // and then surveilling is not the same turn as the reverse.
            gaps.extend(std::iter::repeat_n(1, look_slots));
            spans.push((first, gaps.len() - 1));
        }
        Schedule {
            gaps,
            spans,
            effects,
            land_drop,
        }
    }

    pub fn gaps(&self) -> &[u32] {
        &self.gaps
    }

    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// The declared priority over the land drop, if this run has one.
    pub fn land_drop(&self) -> Option<&LandDropPolicy> {
        self.land_drop.as_ref()
    }

    /// Turns this run covers, counting turn 0, the opening hand.
    pub fn turns(&self) -> usize {
        self.spans.len()
    }

    /// The first and last checkpoint of `turn`, inclusive.
    pub fn checkpoints_of(&self, turn: usize) -> (usize, usize) {
        self.spans[turn]
    }

    /// How many cards `turn` draws into hand before anything else happens.
    pub fn draw_at(&self, turn: usize) -> u32 {
        self.gaps[self.spans[turn].0]
    }

    /// Whether any live effect can put a card in the graveyard.
    ///
    /// Read by the report, because a graveyard count of zero means two
    /// different things and only this tells them apart.
    pub fn routes_to_graveyard(&self) -> bool {
        self.effects.iter().any(|e| e.route.is_live())
    }
}

/// Cards drawn on `turn`.
///
/// `t(0)` is the opening seven. On the play turn 1 draws nothing, so turns 0
/// and 1 see the same seven cards — which is the honest answer, not an
/// off-by-one.
fn draw_for(turn: u32, on_the_draw: bool) -> u32 {
    u32::from(turn > 0 && (on_the_draw || turn > 1))
}
