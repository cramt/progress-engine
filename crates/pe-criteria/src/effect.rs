//! What a card does when you play it, and where the cards it looks at go.
//!
//! Nothing in any card's data says a surveil land surveils one. The amount has
//! to be declared by somebody, and the product requirement is that the somebody
//! is not the user — so a library of effects ships with the tool, keyed on
//! queries rather than on card names. This module is the engine half of that:
//! an [`Effect`] is one library entry after its queries have been resolved
//! against a [`Grouping`], and [`Board`] is what happens when the enumeration
//! walks a path with those effects live.
//!
//! Two restrictions keep this exact, and both are load-bearing rather than
//! temporary.
//!
//! **One trigger.** Only [`Trigger::LandDrop`] exists. A land drop is free and
//! hard-capped at one per turn, so an effect that fires on one cannot compound:
//! by turn `T` at most `T` of them have happened, whatever the deck. The
//! mana-gated tier — Opt, tutors, anything with a cost — has no such cap, and
//! knowing you could cast Opt on turn two means knowing you had an untapped
//! blue source, which is the mana model. So `on = "cast"` is refused by name
//! rather than guessed at.
//!
//! **Counts, never cards.** A looked-at card is routed by which group it is in,
//! and a group is a set of cards no criterion can tell apart. So given the
//! composition, routing is a deterministic function of counts — which is the
//! same restriction the rest of the engine runs on, and the reason this stays
//! enumerable instead of becoming a simulation.

use crate::{Grouping, Schedule, Zone};
use pe_stats::Path;
use thiserror::Error;

/// When an effect gets to happen.
///
/// One variant, and it is not an oversight. A variant here is a promise that
/// the engine knows when the effect fires, and the other two tiers of
/// availability are not knowable yet: the mana-gated tier needs castability,
/// and the free-non-land tier (cycling for zero) is rare enough that guessing
/// at it would cost more in wrong numbers than it pays in coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The turn a matching land is played. Free, and one per turn.
    LandDrop,
}

impl Trigger {
    /// Every trigger an effect may name, for the message that lists them.
    pub const ACCEPTED: &'static str = "landdrop";

    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::LandDrop => "landdrop",
        }
    }

    pub fn parse(name: &str) -> Result<Trigger, TriggerError> {
        match name {
            "landdrop" => Ok(Trigger::LandDrop),
            "cast" | "spell" | "mana" => Err(TriggerError::NeedsMana {
                name: name.to_string(),
            }),
            _ => Err(TriggerError::Unknown {
                name: name.to_string(),
            }),
        }
    }
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A trigger this engine will not fire.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TriggerError {
    /// Refused by name rather than approximated, exactly as
    /// [`crate::ZoneError::Battlefield`] is, and for the same reason: the
    /// tempting approximation is "held it, so it happened", and an opening hand
    /// of one Island and six Opt casts one Opt. A model that fires an effect
    /// whenever the card is in hand overstates that turn sixfold and the number
    /// looks perfectly reasonable in a report.
    #[error(
        "`on = {name:?}` is not modelled. Knowing you cast a spell on a turn means knowing you \
         could pay for it, and that needs the mana model \
         (https://github.com/cramt/progress-engine/issues/10).\n\
         Holding a card is not casting it, so firing on the holding would overstate the turn. \
         Only `on = \"landdrop\"` is free and capped at one per turn, so only that is modelled."
    )]
    NeedsMana { name: String },
    #[error(
        "`on = {name:?}` is not a trigger this tool knows. Accepted: {}.",
        Trigger::ACCEPTED
    )]
    Unknown { name: String },
}

/// Where a looked-at card goes.
///
/// A **router**, not a filter. Framing selection as keep-versus-discard is
/// wrong for the deck this was built for: binning Life from the Loam is not a
/// consolation prize for failing to keep it, it is the deck working. So each
/// looked-at card names a destination and a criterion asks about whichever
/// destination it was routed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Nothing leaves the top of the library.
    ///
    /// The default, and it is deliberately a no-op rather than a guess. A card
    /// left on top is the card you draw next turn, so a look that routes
    /// nothing changes no count anywhere — which is the honest answer when
    /// nobody has said where the cards should go. The effect library ships this
    /// way: it knows a surveil land looks at one card, and it cannot know
    /// whether you want that card in your hand or in your yard.
    Nowhere,
    /// Every looked-at card goes to the graveyard. This is mill.
    Everything,
    /// Cards matching this grouping query go to the graveyard; the rest stay on
    /// top and arrive in hand on the following turn.
    Matching(usize),
}

impl Route {
    /// Whether this route can put a card anywhere other than back on top.
    pub fn is_live(self) -> bool {
        !matches!(self, Route::Nowhere)
    }
}

/// One effect, with its queries resolved to positions in a [`Grouping`].
///
/// `matched_by` is a group bit meaning *this card's effect is this one* rather
/// than *this card matches this effect's query*. The difference is last-wins:
/// two entries can match one card, and the resolution of that overlap happens
/// once, against real card data, before the enumeration starts. Carrying the
/// raw match query here instead would make every path re-decide the overlap,
/// which is a second opinion about the same question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    pub matched_by: usize,
    /// How many cards off the top this examines. At least one.
    pub look: u32,
    pub trigger: Trigger,
    pub route: Route,
}

/// The state one path through the enumeration leaves the zones in.
///
/// Rebuilt in place for every path rather than allocated per path: the exact
/// engine visits up to `MAX_PATHS` of them and an allocation each would cost
/// more than the walk itself.
///
/// The walk is the whole model of the land-drop tier, and it is short on
/// purpose. Cards come off the top in the order the enumeration revealed them;
/// a card that is drawn is in hand, a card that is looked at and routed is in
/// the graveyard, and a card that is looked at and kept is left exactly where
/// it was — on top, to be drawn next turn, which is why keeping is
/// indistinguishable from never having looked.
pub struct Board<'a> {
    grouping: &'a Grouping,
    schedule: &'a Schedule,
    /// Which effect applies to each group, already resolved by last-wins.
    group_effect: Vec<Option<usize>>,
    /// `routed[effect][group]`: does a card of this group leave for the yard
    /// when this effect looks at it.
    routed: Vec<Vec<bool>>,
    /// `[turn][group]`, filled by [`Board::walk`].
    hand: Vec<Vec<u32>>,
    yard: Vec<Vec<u32>>,
    // --- scratch, reused across paths -----------------------------------
    /// Revealed and not yet consumed, in reveal order: the top of the library.
    fresh: Vec<usize>,
    fresh_head: usize,
    /// Looked at and left on top, shallower than anything in `fresh`.
    kept: Vec<usize>,
    /// How many lands of each effect have been played. A land is played once.
    played: Vec<u32>,
    live_hand: Vec<u32>,
    live_yard: Vec<u32>,
}

impl<'a> Board<'a> {
    pub fn new(grouping: &'a Grouping, schedule: &'a Schedule) -> Self {
        let groups = grouping.group_sizes().len();
        let effects = schedule.effects();
        // Last-wins, read off the mask: the bits are disjoint by construction —
        // whoever built the grouping resolved the overlap per card — so at most
        // one can be set, and `rposition` is the same answer as `position`.
        // Written as last-wins anyway, because that is the rule this is
        // implementing and a reader should not have to know the bits are
        // disjoint to believe it.
        let group_effect = grouping
            .group_masks()
            .iter()
            .map(|mask| {
                effects
                    .iter()
                    .rposition(|e| mask & (1u64 << e.matched_by) != 0)
            })
            .collect();
        let routed = effects
            .iter()
            .map(|e| {
                grouping
                    .group_masks()
                    .iter()
                    .map(|mask| match e.route {
                        Route::Nowhere => false,
                        Route::Everything => true,
                        Route::Matching(q) => mask & (1u64 << q) != 0,
                    })
                    .collect()
            })
            .collect();
        let turns = schedule.turns();
        Board {
            grouping,
            schedule,
            group_effect,
            routed,
            hand: vec![vec![0; groups]; turns],
            yard: vec![vec![0; groups]; turns],
            fresh: Vec::with_capacity(schedule.gaps().iter().sum::<u32>() as usize),
            fresh_head: 0,
            kept: Vec::new(),
            played: vec![0; effects.len()],
            live_hand: vec![0; groups],
            live_yard: vec![0; groups],
        }
    }

    /// Play one path out, turn by turn, filling the per-turn zone counts.
    pub fn walk(&mut self, history: Path<'_>) {
        self.fresh.clear();
        self.fresh_head = 0;
        self.kept.clear();
        self.played.fill(0);
        self.live_hand.fill(0);
        self.live_yard.fill(0);

        let effects = self.schedule.effects();
        for turn in 0..self.schedule.turns() {
            let (first, last) = self.schedule.checkpoints_of(turn);
            // Everything this turn reveals, in the order the checkpoints
            // revealed it. Appended before the draw resolves, which is safe
            // because `fresh` is a queue: the draw still takes the topmost
            // unconsumed card, and this turn's look slots sit behind it.
            for c in first..=last {
                let Some(counts) = history.get(c) else { break };
                let previous = c.checked_sub(1).and_then(|p| history.get(p));
                for (group, &total) in counts.iter().enumerate() {
                    let before = previous.map_or(0, |p| p[group]);
                    for _ in 0..(total - before) {
                        self.fresh.push(group);
                    }
                }
            }

            // The draw step. A card kept on top by an earlier look is the card
            // this draw takes, which is the whole reason keeping costs nothing.
            for _ in 0..self.schedule.draw_at(turn) {
                let group = if self.kept.is_empty() {
                    match self.fresh.get(self.fresh_head) {
                        Some(&g) => {
                            self.fresh_head += 1;
                            g
                        }
                        None => break,
                    }
                } else {
                    self.kept.remove(0)
                };
                self.live_hand[group] += 1;
            }

            // The land drop, and there is exactly one of them a turn.
            if turn > 0 {
                if let Some(chosen) = self.land_drop() {
                    self.played[chosen] += 1;
                    self.look(chosen, effects[chosen].look);
                }
            }

            self.hand[turn].copy_from_slice(&self.live_hand);
            self.yard[turn].copy_from_slice(&self.live_yard);
        }
    }

    /// Which effect's land gets played this turn, if any.
    ///
    /// One drop per turn, so holding three surveil lands on turn one plays one
    /// of them. A land already played is not in hand to be played again, which
    /// is what `played` counts.
    ///
    /// Where two effects are both available the deeper look wins, ties going to
    /// the later declaration. That is a decision the engine makes on the
    /// pilot's behalf and it is stated rather than discovered: with no mana
    /// model there is nothing else to tell two land drops apart, so looking
    /// further is the only sense in which one is better.
    fn land_drop(&self) -> Option<usize> {
        let effects = self.schedule.effects();
        let mut best: Option<usize> = None;
        for (i, effect) in effects.iter().enumerate() {
            let in_hand: u32 = self
                .group_effect
                .iter()
                .enumerate()
                .filter(|(_, owner)| **owner == Some(i))
                .map(|(group, _)| self.live_hand[group])
                .sum();
            if in_hand <= self.played[i] {
                continue;
            }
            if best.is_none_or(|b| effect.look >= effects[b].look) {
                best = Some(i);
            }
        }
        best
    }

    /// Examine the top `look` cards and send each one where it is declared to
    /// go.
    ///
    /// The cards already looked at and kept are still the top of the library,
    /// so they are what this sees first — a surveil does not skip past them.
    /// Re-examining a kept card is normally a no-op, and is not assumed to be:
    /// a different effect can carry a different route, and then the card that
    /// was kept last turn leaves this turn.
    fn look(&mut self, effect: usize, look: u32) {
        let mut examined = 0;
        let mut i = 0;
        while examined < look {
            if i < self.kept.len() {
                let group = self.kept[i];
                if self.routed[effect][group] {
                    self.live_yard[group] += 1;
                    self.kept.remove(i);
                } else {
                    i += 1;
                }
            } else if let Some(&group) = self.fresh.get(self.fresh_head) {
                self.fresh_head += 1;
                if self.routed[effect][group] {
                    self.live_yard[group] += 1;
                } else {
                    self.kept.push(group);
                }
            } else {
                // The library ran out. The schedule reveals a slot per look per
                // turn, so this is the deck being genuinely empty rather than
                // the schedule being short.
                break;
            }
            examined += 1;
        }
    }

    /// How many cards matching `query` are in `zone` at the end of `turn`.
    ///
    /// Returns 0 for a turn beyond the horizon rather than panicking: a
    /// criterion asking about turn 9 of a 5-turn run should be false, not a
    /// crash.
    pub fn count_in(&self, turn: usize, query: usize, zone: Zone) -> u32 {
        let Some(hand) = self.hand.get(turn) else {
            return 0;
        };
        let in_hand = self.grouping.count_matching(hand, query);
        match zone {
            Zone::Hand => in_hand,
            Zone::Graveyard => self.grouping.count_matching(&self.yard[turn], query),
            // Cannot underflow: hand and yard are disjoint subsets of the same
            // groups `matching_total` sums over.
            Zone::Library => {
                self.grouping.matching_total(query)
                    - in_hand
                    - self.grouping.count_matching(&self.yard[turn], query)
            }
        }
    }

    pub fn turns(&self) -> usize {
        self.hand.len()
    }
}
