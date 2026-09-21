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

use crate::mana::{Constraint, Cost, Demand, Source};
use crate::{Counted, Grouping, Schedule, Zone};
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
    /// Refused by name rather than approximated, and **what it is refused for
    /// has changed.**
    ///
    /// Both halves of the mana model now ship. The gate says whether Opt was
    /// castable on turn two; the budget says how many Opts the turn actually
    /// paid for, which is one however many you are holding. So *how often does
    /// this fire* is no longer the missing piece.
    ///
    /// What is missing is what firing it **does**. Every card in this tier
    /// draws — Opt, Preordain, Brainstorm — and a replacement draw makes *cards
    /// seen by turn T* a fact about the path rather than about the schedule.
    /// The enumeration reveals cards one checkpoint each so that it can tell
    /// the order they came off the top, so a card the walk might or might not
    /// draw needs a checkpoint of its own; each one multiplies the enumeration
    /// by the group count; and a turn with T mana can cast T cantrips. On both
    /// decks in `decks/` that is over the ceiling by turn three for any line
    /// with a colour in it, against north stars that ask about turn five.
    #[error(
        "`on = {name:?}` is not modelled, and the reason is the draw rather than the mana. \
         How many spells a turn pays for is answered — declare `[casting] prefer = [...]` and \
         count them with `cast`.\n\
         What a cast spell then draws is not: a replacement draw makes how many cards you have \
         seen by a turn depend on the path rather than on the schedule, which costs an \
         enumeration checkpoint per turn and goes over the ceiling on every question this tool \
         exists for (https://github.com/cramt/progress-engine/issues/57).\n\
         Only `on = \"landdrop\"` is free, capped at one per turn, and draws nothing."
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

/// The spells of a run whose file declared which ones to cast.
///
/// The budget half of [#10](https://github.com/cramt/progress-engine/issues/10).
/// A gate asks whether the pool *could* have paid; this spends it, and the
/// difference is the whole of HANDS.md hand 1 — one Island and six Opt can
/// cast Opt and casts exactly one, because the first one takes the Island.
///
/// `tiers` is the priority as groups, highest first, each already sorted by
/// the tie rule. `cost` is what a group's card puts on the pool, `None` for
/// every group the priority does not name — those are never cast, which is the
/// list being a line rather than a preference over the whole deck. `cast_at`
/// is what that came to on this path, and `spent` is what the line took out of
/// each turn, so a `can_cast` clause beside it reads what is left rather than
/// what the turn started with.
struct Casting {
    tiers: Vec<Vec<usize>>,
    cost: Vec<Option<Demand>>,
    cast_at: Vec<Vec<u32>>,
    spent: Vec<Demand>,
    /// Copies cast so far on this path, per group. Scratch, reused.
    live_cast: Vec<u32>,
}

/// The land drops of a run whose file declared which land to play.
///
/// `tiers` is the priority as groups: one list per tier, highest first, each
/// already sorted by the tie rule so the walk can take the first group it is
/// holding. `played_at` is what that came to on this path — `[turn][group]`,
/// the lands actually played by the end of each turn, which is the thing a
/// run without a declaration has no honest way to state.
struct Declared {
    tiers: Vec<Vec<usize>>,
    played_at: Vec<Vec<u32>>,
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
    /// Everything that only exists where the file declared a priority.
    ///
    /// One `Option` rather than a field each, because the ranking and the
    /// record of what it played are the same fact in two shapes: a board
    /// holding one without the other would be a policy that decided drops
    /// nothing could read, or a record of drops nothing decided.
    ///
    /// `None` is what selects every older behaviour below — the effects
    /// choosing the drop among themselves, and the gate assuming the line that
    /// pays.
    declared: Option<Declared>,
    /// Everything that only exists where the file declared a casting priority.
    ///
    /// `None` is a run that casts nothing at all, which is every run there was
    /// before the budget: no card leaves the hand, no pool is spent, and a
    /// `can_cast` clause asks what the lands could have paid rather than what
    /// they have left.
    casting: Option<Casting>,
    /// `[turn][group]`, filled by [`Board::walk`].
    hand: Vec<Vec<u32>>,
    yard: Vec<Vec<u32>>,
    /// Which group the drop of each turn went to, for the one land that can
    /// still be tapped: the one played this turn.
    drop_at: Vec<Option<usize>>,
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
    /// The same count per group, which is what a policy plays from: it ranks
    /// lands rather than effects, and a land it already played is not in hand
    /// to be played again.
    live_played: Vec<u32>,
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
        let group_effect: Vec<Option<usize>> = grouping
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
        // The tie rule, applied once here rather than on every path: a group
        // belongs to the first tier that names it, and the order inside a tier
        // is the deeper look first — the rule the effect walk already used, so
        // a list that does not mention your surveil land still fires it — then
        // the group the decklist reached first.
        let declared = schedule.land_drop().map(|policy| {
            let is_land =
                |group: usize| grouping.group_masks()[group] & (1u64 << policy.any_land()) != 0;
            let look_of = |group: usize| group_effect[group].map_or(0, |e| effects[e].look);
            let mut claimed = vec![false; groups];
            Declared {
                tiers: policy
                    .tiers()
                    .map(|query| {
                        let mut tier: Vec<usize> = (0..groups)
                            .filter(|&g| !claimed[g] && is_land(g))
                            .filter(|&g| grouping.group_masks()[g] & (1u64 << query) != 0)
                            .collect();
                        for &g in &tier {
                            claimed[g] = true;
                        }
                        tier.sort_by_key(|&g| (std::cmp::Reverse(look_of(g)), g));
                        tier
                    })
                    .collect(),
                played_at: vec![vec![0; groups]; turns],
            }
        });
        // The same tie rule applied once, over the other contested resource:
        // a group belongs to the first tier that names it, and inside a tier
        // the cheaper spell is cast first — which is the rule that gets you
        // more of what you said you wanted — and then the group the decklist
        // reached first. A group the list never names has no cost recorded and
        // is never cast.
        let casting = schedule.casting().map(|policy| {
            let cost: Vec<Option<Demand>> = grouping
                .group_mana()
                .iter()
                .map(|mana| mana.castable())
                .collect();
            let mut claimed = vec![false; groups];
            Casting {
                tiers: policy
                    .tiers()
                    .map(|query| {
                        let mut tier: Vec<usize> = (0..groups)
                            .filter(|&g| !claimed[g] && cost[g].is_some())
                            .filter(|&g| grouping.group_masks()[g] & (1u64 << query) != 0)
                            .collect();
                        for &g in &tier {
                            claimed[g] = true;
                        }
                        tier.sort_by_key(|&g| (cost[g].map_or(0, Demand::total), g));
                        tier
                    })
                    .collect(),
                cost,
                cast_at: vec![vec![0; groups]; turns],
                spent: vec![Demand::FREE; turns],
                live_cast: vec![0; groups],
            }
        });
        Board {
            grouping,
            schedule,
            group_effect,
            routed,
            land_groups,
            pool,
            casting,
            drops: vec![0; turns],
            hand: vec![vec![0; groups]; turns],
            yard: vec![vec![0; groups]; turns],
            drop_at: vec![None; turns],
            declared,
            fresh: Vec::with_capacity(schedule.gaps().iter().sum::<u32>() as usize),
            fresh_head: 0,
            kept: Vec::new(),
            played: vec![0; effects.len()],
            live_played: vec![0; groups],
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
        self.live_played.fill(0);
        self.live_hand.fill(0);
        self.live_yard.fill(0);
        self.drop_at.fill(None);
        if let Some(casting) = &mut self.casting {
            casting.live_cast.fill(0);
        }

        let effects = self.schedule.effects();
        for turn in 0usize..self.schedule.turns() {
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

            // The land drop, and there is exactly one of them a turn. Which
            // land it is has two answers, and which one this run uses is the
            // whole of issue #54: a file that declared a priority gets the land
            // it declared, and every part of the run reads that same drop.
            match turn.checked_sub(1) {
                None => self.drops[turn] = 0,
                Some(previous) if self.declared.is_some() => {
                    let chosen = self.declared_drop();
                    self.drop_at[turn] = chosen;
                    if let Some(group) = chosen {
                        self.live_played[group] += 1;
                        if let Some(effect) = self.group_effect[group] {
                            self.look(effect, effects[effect].look);
                        }
                    }
                    self.drops[turn] = self.drops[previous] + u32::from(chosen.is_some());
                }
                Some(previous) => {
                    if let Some(chosen) = self.land_drop() {
                        self.played[chosen] += 1;
                        self.look(chosen, effects[chosen].look);
                    }
                    // One drop a turn, and it is wasted if you are holding
                    // nothing to play. That is what makes this a fact about the
                    // path rather than about the hand: five lands drawn by turn
                    // 3 are three lands in play, because the other two drops
                    // never happened.
                    let held: u32 = self.land_groups.iter().map(|&g| self.live_hand[g]).sum();
                    self.drops[turn] = held.min(self.drops[previous] + 1);
                }
            }

            // Recorded **before** the spells, because the budget asks the
            // board what this turn can pay with and gets its answer from these
            // very slots. Left until after, it would read whatever the
            // previous path left behind — which is not a stale number so much
            // as another deal's board, and it cast spells off lands that hand
            // never played.
            self.hand[turn].copy_from_slice(&self.live_hand);
            self.yard[turn].copy_from_slice(&self.live_yard);
            if let Some(declared) = &mut self.declared {
                declared.played_at[turn].copy_from_slice(&self.live_played);
            }
            // The spells, after the land, because you play your land and then
            // cast off it.
            if self.casting.is_some() {
                self.cast(turn);
                // And the hand again, because a card you cast is not a card
                // you are holding. Only the hand moves: casting spends lands
                // rather than playing them, so nothing above changes under it.
                self.hand[turn].copy_from_slice(&self.live_hand);
                if let Some(casting) = &mut self.casting {
                    casting.cast_at[turn].copy_from_slice(&casting.live_cast);
                }
            }
        }
    }

    /// Spend this turn's mana on the spells the file said to cast.
    ///
    /// The whole budget, and it is short because the two hard parts are
    /// elsewhere: *which* spell is a declared priority, and *can this be paid*
    /// is one matching over the summed bill. What is left is a walk down the
    /// list taking whatever the pool still covers.
    ///
    /// Two things make it a budget rather than a gate. The bill **accumulates**
    /// across the turn — casting a second Opt asks whether `{U}{U}` is payable,
    /// not whether `{U}` is, which is the only reason one Island casts one Opt
    /// — and a spell that is cast **leaves the hand**, so the same copy cannot
    /// be cast again next turn.
    ///
    /// Greedy down the list, and that is the policy rather than a shortcut: the
    /// file said which spell it wanted first, so the engine takes it first even
    /// where skipping it would have bought two cheaper ones. A pilot who wants
    /// the two cheaper ones says so by listing them first.
    fn cast(&mut self, turn: usize) {
        // Moved out and put back rather than borrowed, because paying is a
        // question about the whole board and casting writes to part of it.
        let Some(mut casting) = self.casting.take() else {
            return;
        };
        // Turn 0 is the opening hand: no land has been played, so there is no
        // mana and nothing to spend it on.
        if turn > 0 {
            let mut spent = Demand::FREE;
            for tier in &casting.tiers {
                for &group in tier {
                    let Some(cost) = casting.cost[group] else {
                        continue;
                    };
                    while self.live_hand[group] > 0 {
                        let trial = spent.plus(cost);
                        // The count first, because it settles most turns
                        // without a matching: a bill for more sources than you
                        // have land drops cannot be paid however they are
                        // coloured, and this runs on every path.
                        if trial.total() > self.drops[turn] || !self.can_pay(turn, trial) {
                            break;
                        }
                        spent = trial;
                        self.live_hand[group] -= 1;
                        casting.live_cast[group] += 1;
                    }
                }
            }
            casting.spent[turn] = spent;
        }
        self.casting = Some(casting);
    }

    /// Which land the declared priority plays this turn, if any.
    ///
    /// The first tier holding a land this path has drawn and not yet played,
    /// and within a tier the first group — which [`Board::new`] has already
    /// sorted by the tie rule. A land nobody ranked is in the last tier rather
    /// than out of the list, because declining a drop is not something a
    /// priority list can be read as asking for.
    ///
    /// This answers with a *group*, not with an effect, which is the
    /// difference that settles the argument: the effect that fires is whatever
    /// the played land carries, and the mana the turn has is whatever the
    /// played land makes. One decision, read by both.
    fn declared_drop(&self) -> Option<usize> {
        let tiers = &self.declared.as_ref()?.tiers;
        tiers
            .iter()
            .flatten()
            .copied()
            .find(|&group| self.live_hand[group] > self.live_played[group])
    }

    /// Which effect's land gets played this turn, if any, in a run that
    /// declared no priority.
    ///
    /// One drop per turn, so holding three surveil lands on turn one plays one
    /// of them. A land already played is not in hand to be played again, which
    /// is what `played` counts.
    ///
    /// Where two effects are both available the deeper look wins, ties going to
    /// the later declaration. That is a decision the engine makes on the
    /// pilot's behalf and it is stated rather than discovered: with nothing
    /// declared there is nothing else to tell two land drops apart, so looking
    /// further is the only sense in which one is better. It is also why a mana
    /// question cannot be asked beside a live effect without a priority — this
    /// plays the land that looks deepest and the gate assumes the land that
    /// pays, and the two are not the same land. The tie rule inside a declared
    /// tier is this same one, so declaring a priority never silently switches
    /// off an effect it did not mention.
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

    /// How many cards matching `query` this path has put where `counted` says,
    /// by the end of `turn`.
    ///
    /// Returns 0 for a turn beyond the horizon rather than panicking: a
    /// criterion asking about turn 9 of a 5-turn run should be false, not a
    /// crash.
    pub fn count_at(&self, turn: usize, query: usize, counted: Counted) -> u32 {
        let Some(hand) = self.hand.get(turn) else {
            return 0;
        };
        let zone = match counted {
            Counted::Cast => return self.cast_by(turn, query),
            Counted::In(zone) => zone,
        };
        let in_hand = self.grouping.count_matching(hand, query);
        match zone {
            Zone::Hand => in_hand,
            Zone::Graveyard => self.grouping.count_matching(&self.yard[turn], query),
            // Cannot underflow: the hand, the yard and the spells this path
            // cast are disjoint subsets of the same groups `matching_total`
            // sums over — a card is drawn once, and casting it takes it out of
            // the hand rather than copying it.
            Zone::Library => {
                self.grouping.matching_total(query)
                    - in_hand
                    - self.grouping.count_matching(&self.yard[turn], query)
                    - self.cast_by(turn, query)
            }
            Zone::Battlefield => self.played_by(turn, query),
        }
    }

    /// How many cards matching `query` this path has cast by `turn`.
    ///
    /// Zero in a run with no declared casting priority, and that zero is a
    /// fact rather than a gap: nobody said which spells they would cast, so
    /// the run cast none. The report says so.
    fn cast_by(&self, turn: usize, query: usize) -> u32 {
        self.casting
            .as_ref()
            .and_then(|c| c.cast_at.get(turn))
            .map_or(0, |counts| self.grouping.count_matching(counts, query))
    }

    /// How many lands matching `query` were played by `turn` — or, where
    /// nobody declared which lands they would play, could have been.
    ///
    /// **With a priority declared it is a count, not an argument**: the drops
    /// are on record, one per turn, and a criterion asking about two disjoint
    /// sets of lands gets one answer per set off the same line rather than two
    /// answers each pretending to be the priority. That is the whole of the
    /// paragraph below going away, and it goes away because somebody said what
    /// they would have played.
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
        // A policy run has nothing to work out: the lands it played are the
        // lands the priority chose, turn by turn, and counting them is reading
        // them off. The recurrence below is what the question means when
        // nobody declared which land they would have played.
        if let Some(declared) = &self.declared {
            return match declared.played_at.get(turn) {
                Some(counts) => self.grouping.count_matching(counts, query),
                None => 0,
            };
        }
        let mut played = 0;
        for t in 1..=turn {
            let drawn = self.grouping.count_matching(&self.hand[t], query);
            played = drawn.min(played + 1);
        }
        played
    }

    /// Whether `cost` could have been paid on `turn`.
    ///
    /// Only lands pay, because only a land arrives without being cast.
    ///
    /// **Where a policy was declared there is nothing to work out**, and that
    /// is the point of declaring one: the lands in play are the lands the
    /// priority played, the only one that can still be tapped is this turn's,
    /// and the matching runs over exactly those. It is also the only reading
    /// that can sit beside a routing effect, because it is the drop the effect
    /// walk made rather than a second opinion about it.
    ///
    /// Everything below is the other reading: what this question means when
    /// nobody said which land they would have played. It is the most generous
    /// one, because nobody plays their lands badly, and what makes it exact
    /// rather than a guess is that the choice a pilot has — which lands to
    /// play, and in which order — is fully enumerable here:
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
        // **Beside a budget this asks what is left, not what there was.** One
        // pool, one accounting: a run that declared a casting priority has
        // already spent some of this turn on it, and answering "could you have
        // paid {U}" out of the whole turn's lands while a declared line
        // already took them would be two claimants on one resource — the thing
        // the land drop taught us not to do. So the line's bill is added to
        // this one and the pair is asked together. A file that declares no
        // casting priority spends nothing, so nothing moves.
        let spent = self
            .casting
            .as_ref()
            .and_then(|c| c.spent.get(turn))
            .copied()
            .unwrap_or(Demand::FREE);
        self.can_pay(turn, spent.plus(cost.demand()))
    }

    /// The matching behind [`Board::can_cast`], over a bill rather than a
    /// written cost — because the budget pays several spells out of one turn
    /// and that is one bill.
    fn can_pay(&self, turn: usize, cost: Demand) -> bool {
        if cost.is_free() {
            return true;
        }
        if let Some(declared) = &self.declared {
            let Some(counts) = declared.played_at.get(turn) else {
                return false;
            };
            // The land played this turn is the only one that has not untapped,
            // so it is the only one a tapped-ness assumption can take out of
            // the pool. Everything else in play makes mana whatever it did on
            // the way in.
            let tapped_now = self.drop_at[turn].and_then(|group| {
                self.land_groups
                    .iter()
                    .position(|&land| land == group)
                    .filter(|&slot| self.pool[slot].tapped)
            });
            // Cannot underflow: `drop_at[turn]` names the land this turn
            // played, so `counts` was written with that land in it. The walk
            // records the drop before anything reads it, which is the
            // invariant this subtraction rests on.
            let usable =
                |slot: usize| counts[self.land_groups[slot]] - u32::from(tapped_now == Some(slot));
            return cost.payable(&self.pool, usable, Constraint::Anything);
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
