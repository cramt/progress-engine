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

use crate::mana::{Constraint, Cost, Source};
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
    /// Refused by name rather than approximated.
    ///
    /// The gate half of the mana model ships — [`Board::can_cast`] will tell
    /// you whether Opt was castable on turn two — and this still does not,
    /// because firing an effect needs the other half. An opening hand of one
    /// Island and six Opt *can cast* Opt and casts exactly one of them: the
    /// first one spends the Island. A model that fires an effect whenever the
    /// card is in hand and the mana is there overstates that turn sixfold, and
    /// the number looks perfectly reasonable in a report.
    #[error(
        "`on = {name:?}` is not modelled. Knowing you cast a spell on a turn means knowing the \
         mana was still there after the last one, and that is the budget half of the mana model \
         (https://github.com/cramt/progress-engine/issues/10).\n\
         Being able to cast a card is not casting it six times, so firing on the holding would \
         overstate the turn. Only `on = \"landdrop\"` is free and capped at one per turn, so \
         only that is modelled. `can_cast` answers whether you could have."
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
    /// Which groups are lands, so the gate does not walk the whole deck.
    land_groups: Vec<usize>,
    /// `[turn][group]`, filled by [`Board::walk`].
    hand: Vec<Vec<u32>>,
    yard: Vec<Vec<u32>>,
    /// Land drops made by the end of each turn.
    ///
    /// One a turn and use-it-or-lose-it, so this is not the number of lands
    /// drawn: `drops(T) = min(lands drawn by T, drops(T-1) + 1)`. Recorded per
    /// turn rather than recomputed, because the gate reads both this turn's and
    /// last turn's on every question it answers.
    drops: Vec<u32>,
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
    /// What each land group makes, parallel to `land_groups`. Fixed for the
    /// whole run: how many are available moves with the path, what they produce
    /// does not.
    pool: Vec<Source>,
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
        let land_groups: Vec<usize> = grouping
            .group_mana()
            .iter()
            .enumerate()
            .filter(|(_, mana)| mana.is_land())
            .map(|(group, _)| group)
            .collect();
        let pool = land_groups
            .iter()
            .map(|&group| Source::of(grouping.group_mana()[group]))
            .collect();
        Board {
            grouping,
            schedule,
            group_effect,
            routed,
            land_groups,
            pool,
            drops: vec![0; turns],
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

            // One drop a turn, and it is wasted if you are holding nothing to
            // play. That is what makes this a fact about the path rather than
            // about the hand: five lands drawn by turn 3 are three lands in
            // play, because the other two drops never happened.
            let held: u32 = self.land_groups.iter().map(|&g| self.live_hand[g]).sum();
            self.drops[turn] = match turn.checked_sub(1) {
                None => 0,
                Some(previous) => held.min(self.drops[previous] + 1),
            };

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
            Zone::Battlefield => self.played_by(turn, query),
        }
    }

    /// How many lands matching `query` could have been played by `turn`.
    ///
    /// The same use-it-or-lose-it recurrence as [`Board::drops`], run over the
    /// matching lands alone: one drop a turn, and a drop you could not use is
    /// gone. Answering per query rather than reading a single played-lands
    /// figure is what lets "two Islands in play" differ from "two lands in
    /// play" on the same path.
    ///
    /// It is the *most* you could have had, which is the reading a gate wants:
    /// nobody plays their lands badly, so a question about what is in play is a
    /// question about the line that plays them well. A criterion asking for two
    /// disjoint sets of lands at once is then two questions each answered as if
    /// it were the priority — [`Board::drops`] caps their total, but nothing
    /// makes one line serve both. Asking for one land set per criterion is the
    /// honest way to write it.
    fn played_by(&self, turn: usize, query: usize) -> u32 {
        let mut played = 0;
        for t in 1..=turn {
            let drawn = self.grouping.count_matching(&self.hand[t], query);
            played = drawn.min(played + 1);
        }
        played
    }

    /// Whether `cost` could have been paid on `turn`.
    ///
    /// Only lands pay, because only a land arrives without being cast. What
    /// makes this exact rather than a guess is that the choice a pilot has —
    /// which lands to play, and in which order — is fully enumerable here:
    ///
    /// - **Which lands.** At most one card enters hand per turn after the
    ///   opening, so any set of lands you have drawn and can afford drops for
    ///   can be the set you played. No land you drew is stuck behind another.
    /// - **Which one is tapped.** A land untaps on your next turn, so the only
    ///   land that can still be tapped is the one played *this* turn. Everything
    ///   else in play makes mana whatever it did on the way in.
    ///
    /// So there are three lines to check, and the answer is whether any of them
    /// pays: play nothing relevant this turn and pay with what was already
    /// down; play a land you were already holding and tap it too, which needs it
    /// to enter untapped; or play the land that arrived this turn and tap that,
    /// which needs the same of it.
    pub fn can_cast(&self, turn: usize, cost: &Cost) -> bool {
        if cost.is_free() {
            return true;
        }
        let (Some(previous), Some(hand)) = (turn.checked_sub(1), self.hand.get(turn)) else {
            // Turn 0 is the opening hand, before any land drop. Nothing is in
            // play, so nothing but a free spell is castable.
            return false;
        };
        let drops = self.drops[turn];
        if cost.total() > drops {
            return false;
        }
        let earlier = &self.hand[previous];

        // Held is what was in hand when the turn began: every one of those has
        // had a turn on which it could have been played, so any of them can be
        // among the lands already down.
        let held = |slot: usize| earlier[self.land_groups[slot]];

        // Line one: every land paying this was already on the battlefield when
        // the turn began, so none of them can be tapped.
        if cost.total() <= self.drops[previous]
            && cost.payable(&self.pool, held, Constraint::Anything)
        {
            return true;
        }
        // Line two: one of the lands paying this is the drop made this turn,
        // taken from what was already in hand — so it has to enter untapped.
        if cost.payable(&self.pool, held, Constraint::IncludesUntapped) {
            return true;
        }
        // Line three: the drop made this turn is a land that arrived this turn,
        // which is the only turn it could be played on. Same requirement of it,
        // and however many arrived only one of them can be played.
        for (slot, &group) in self.land_groups.iter().enumerate() {
            if hand[group] == earlier[group] || self.pool[slot].tapped {
                continue;
            }
            let arrived = |i: usize| held(i) + u32::from(i == slot);
            if cost.payable(&self.pool, arrived, Constraint::Includes(slot)) {
                return true;
            }
        }
        false
    }

    pub fn turns(&self) -> usize {
        self.hand.len()
    }
}
