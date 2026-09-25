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
//! **What a trigger is allowed to do.** A land drop is free and hard-capped at
//! one per turn, so an effect that fires on one cannot compound: by turn `T` at
//! most `T` of them have happened, whatever the deck. A cast fires as often as
//! the pool pays for it, which the budget knows — so [`Trigger::Cast`] exists,
//! and what it may do is a [`Fetch`] and not a look. A fetch is a deterministic
//! removal from a named group, which is a subtraction from the population the
//! later gaps are drawn out of; a look on a cast is a replacement draw, which
//! changes the *shape* of the enumeration rather than the population carried
//! through it, and is still refused by name.
//!
//! **Counts, never cards.** A looked-at card is routed by which group it is in,
//! and a group is a set of cards no criterion can tell apart. So given the
//! composition, routing is a deterministic function of counts — which is the
//! same restriction the rest of the engine runs on, and the reason this stays
//! enumerable instead of becoming a simulation. A fetch obeys it too: it takes
//! the first group its declared priority reaches that the library still holds,
//! which is a function of counts and branches nothing.

use crate::mana::{Constraint, Cost, Demand, Source};
use crate::{Counted, Grouping, Schedule, Zone};
use chip_stats::Path;
use thiserror::Error;

/// When an effect gets to happen.
///
/// Two variants, and the list is short for the same reason it has always been:
/// a variant here is a promise that the engine knows when the effect fires. The
/// free-non-land tier — cycling for zero — is still missing, and it is rare
/// enough that guessing at it would cost more in wrong numbers than it pays in
/// coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The turn a matching land is played. Free, and one per turn.
    LandDrop,
    /// The turn the declared line casts a matching spell.
    ///
    /// Knowable because the budget knows it: `[casting] prefer = [...]` names
    /// the line and the pool is spent on it, so *how many of these resolved by
    /// turn T* is a count the walk already keeps. What such an effect is
    /// allowed to **do** is the restriction — see [`Fetch`]. A look on a cast
    /// is a replacement draw and is still refused, because that is the one
    /// that changes the shape of the enumeration rather than the population
    /// carried through it.
    Cast,
}

impl Trigger {
    /// Every trigger an effect may name, for the message that lists them.
    pub const ACCEPTED: &'static str = "landdrop, cast";

    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::LandDrop => "landdrop",
            Trigger::Cast => "cast",
        }
    }

    pub fn parse(name: &str) -> Result<Trigger, TriggerError> {
        match name {
            "landdrop" => Ok(Trigger::LandDrop),
            "cast" => Ok(Trigger::Cast),
            _ => Err(TriggerError::Unknown {
                name: name.to_string(),
            }),
        }
    }
}

/// A card this effect goes and gets out of the library.
///
/// The cheap half of
/// [#18](https://github.com/cramt/progress-engine/issues/18): a **deterministic
/// removal from a named group**, which is a subtraction rather than a
/// distribution. Exiling three cards off the top is the other half — those are
/// a random sample, so what remains is a distribution and the path branches the
/// way a draw does — and it is not here.
///
/// `prefer` is the declared priority over grouping queries, highest first, the
/// same mechanism as the land drop and the casting line rather than a fifth
/// policy language. The search is total in the only way that matters: a tutor
/// that finds nothing fetches nothing, which is what an empty library of
/// targets means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetch {
    pub prefer: Vec<usize>,
    pub to: Fetched,
}

/// Where a fetched card is put.
///
/// Deliberately not [`crate::Zone`]. A zone is somewhere a criterion counts
/// cards; this is somewhere the engine can *put* one, and the two lists differ
/// — the library is a zone and is not a destination, and a variant here is a
/// promise that the walk really moves the card there rather than counting it
/// somewhere plausible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    /// Trinket Mage: the card goes to your hand, where the budget can then
    /// cast it out of the same turn's mana if the line named it.
    Hand,
    /// Onto the battlefield, and two cards arrive there this way.
    ///
    /// A fetchland: the land arrives **in place of** the land whose drop
    /// fetched it, because that land sacrificed itself to do it. Only
    /// reachable from [`Trigger::LandDrop`], and only for a priority that
    /// names lands — the same restriction `zone = "battlefield"` is already
    /// under, and for the same reason.
    ///
    /// Urza's Saga: a [`Delay`]ed fetch, so the card arrives **beside** the
    /// land that waited two turns for it, and it is an artifact rather than a
    /// land. What it may find is the opposite restriction, for the mirror of
    /// the same reason: a land arriving off a chapter ability would put mana in
    /// the pool on a turn nothing says whether it entered tapped.
    Battlefield,
}

/// An effect that waits: it is set up by its trigger and resolves some whole
/// number of turns later.
///
/// Urza's Saga, and it is the only card this was written for. It arrives on a
/// land drop with a lore counter, gains one after each of your next two draw
/// steps, and chapter III — two turns after the drop — searches the library.
/// So the trigger is the land drop, the effect is a [`Fetch`], and the wait is
/// `turns = 2`.
///
/// **It resolves after the draw step and before the land drop**, because a lore
/// counter goes on as the precombat main phase begins and its chapter ability
/// resolves before you could play a land into it. The draw is already in hand,
/// so a card drawn that turn is not in the library for the fetch to find.
///
/// Nothing is revealed and nothing branches: a delayed fetch is the same
/// subtraction from a named group an immediate one is, taken on a later turn.
/// Which is why this stays exact, and why a delayed **look** is refused — that
/// would need a checkpoint on a turn the schedule has no way to know the
/// effect fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delay {
    /// Whole turns between the trigger and the effect. Never zero: an effect
    /// that waits no turns is an effect with no `Delay`.
    pub turns: u32,
    /// Whether the card that set this up leaves the battlefield when it
    /// resolves. A Saga is sacrificed after its last chapter.
    ///
    /// It leaves at the end of the turn it resolves on, in the sense that
    /// matters here: its mana is still that turn's, because a Saga's chapter
    /// ability waits on the stack while you tap it, and mana made in the main
    /// phase stays in the pool for the rest of it. From the next turn on it is
    /// not there to tap. It is not counted in the graveyard, which is the same
    /// stance a cracked fetchland takes: a card that left play is not counted
    /// anywhere a criterion asks about, rather than somewhere plausible.
    pub sacrifice: bool,
}

impl std::fmt::Display for Trigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A trigger this engine will not fire, or will not fire for what was asked of
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TriggerError {
    /// `on = "cast"` fires — the budget knows which spells a turn paid for —
    /// and what it is allowed to do when it does is the restriction.
    ///
    /// A **fetch** is a deterministic removal from a named group. Nothing
    /// about the enumeration changes shape: the population the later gaps are
    /// drawn from is one card smaller, which is a subtraction.
    ///
    /// A **look** on a cast is a replacement draw, and that is the one that
    /// changes shape. The enumeration reveals cards one checkpoint each so it
    /// can tell the order they came off the top, so a card the walk might or
    /// might not draw needs a checkpoint of its own; each one multiplies the
    /// enumeration by the group count; and a turn with T mana can cast T
    /// cantrips. On both decks in `decks/` that is over the ceiling by turn
    /// three for any line with a colour in it, against north stars that ask
    /// about turn five.
    #[error(
        "`on = \"cast\"` fires — the budget knows which spells a turn paid for — but a `look` \
         on a cast is a replacement draw, and that is not modelled.\n\
         A replacement draw makes how many cards you have seen by a turn depend on the path \
         rather than on the schedule, which costs an enumeration checkpoint per turn and goes \
         over the ceiling on every question this tool exists for \
         (https://github.com/cramt/progress-engine/issues/57).\n\
         What a cast spell may do here is `fetch`, which removes a named card from the library \
         rather than turning over an unknown one."
    )]
    LooksOnCast,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub matched_by: usize,
    /// How many cards off the top this examines. Zero for an effect that only
    /// fetches, which turns over nothing at all.
    pub look: u32,
    pub trigger: Trigger,
    pub route: Route,
    /// What this goes and gets out of the library, if anything.
    pub fetch: Option<Fetch>,
    /// How long it waits after its trigger, if it waits at all.
    pub delay: Option<Delay>,
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
    /// `fetch_tiers[effect]` is that effect's declared priority as groups,
    /// highest tier first, each tier in decklist order. Empty for an effect
    /// that fetches nothing. Resolved once here rather than on every path, for
    /// the same reason last-wins is: a priority re-read per path is a second
    /// opinion about the same list.
    fetch_tiers: Vec<Vec<Vec<usize>>>,
    /// Whether anything in this run removes a card from the library without
    /// drawing it. Read by the enumeration, which only pays for the shrinking
    /// population where there is one.
    fetches: bool,
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
    /// Cards of each group this path has turned over, drawn or merely looked
    /// at. Not the same as *drawn*: a card kept on top has been revealed and is
    /// still in the library, and a tutor must not be able to find it twice.
    revealed: Vec<u32>,
    /// Cards of each group a fetch took out of the **unrevealed** library.
    ///
    /// Reported to the enumeration, which subtracts it from what the later
    /// gaps are dealt out of. A fetch that takes a card off the top instead
    /// does not appear here, because that card was already accounted for by
    /// the checkpoint that revealed it.
    removed: Vec<u32>,
    /// Lands on the battlefield, per group.
    ///
    /// Separate from `live_played`, which counts what left your hand: a
    /// fetchland is played and then is not in play, and those are the same
    /// number in every run that fetches nothing.
    live_field: Vec<u32>,
    /// Cards of each group a fetch put straight onto the battlefield, per turn.
    ///
    /// The library count needs it and no other zone does: such a card left the
    /// library without ever being in your hand, so subtracting the hand, the
    /// yard and what was cast leaves it counted in a library it is not in.
    landed: Vec<Vec<u32>>,
    live_landed: Vec<u32>,
    /// How many lands of each effect have been played. A land is played once.
    played: Vec<u32>,
    /// The same count per group, which is what a policy plays from: it ranks
    /// lands rather than effects, and a land it already played is not in hand
    /// to be played again.
    live_played: Vec<u32>,
    live_hand: Vec<u32>,
    live_yard: Vec<u32>,
    /// Delayed effects this path has set up and not yet resolved: the turn each
    /// one fires on, the effect, and the group of the card that set it up.
    /// Scratch, reused.
    pending: Vec<(usize, usize, usize)>,
    /// `[turn][group]`: lands a delayed effect sacrificed on that turn. Off the
    /// battlefield by the end of it, and still tapped for mana during it, so
    /// the pool reads this back in and no zone count does.
    sacrificed: Vec<Vec<u32>>,
    /// What each land group makes, parallel to `land_groups`. Fixed for the
    /// whole run: how many are available moves with the path, what they produce
    /// does not.
    pool: Vec<Source>,
    /// The declared mulligan's bottoming priority as groups, one list per
    /// tier and the catch-all last — every group no entry names. Empty where
    /// the run declared no mulligan. Resolved once here, as every other
    /// priority is, so no path re-reads the list.
    bottom_tiers: Vec<Vec<usize>>,
    /// Cards of each group the opener put back, set by whoever dealt it
    /// before [`Board::walk`] plays the path out. Zero on a run that kept its
    /// seven, which is every run without a mulligan.
    bottomed: Vec<u32>,
    /// What is still on the bottom of the library on this path: `bottomed`,
    /// less anything a tutor went and found there.
    live_bottomed: Vec<u32>,
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
        // The same tie rule a third time, over the one resource a tutor
        // contests: which of the cards it could find it actually takes. A
        // group belongs to the first tier that names it, and inside a tier the
        // group the decklist reached first — which is group order, because
        // that is the order the groups were built in.
        let fetch_tiers: Vec<Vec<Vec<usize>>> = effects
            .iter()
            .map(|effect| match &effect.fetch {
                None => Vec::new(),
                Some(fetch) => {
                    let mut claimed = vec![false; groups];
                    fetch
                        .prefer
                        .iter()
                        .map(|&query| {
                            let tier: Vec<usize> = (0..groups)
                                .filter(|&g| !claimed[g])
                                .filter(|&g| grouping.group_masks()[g] & (1u64 << query) != 0)
                                .collect();
                            for &g in &tier {
                                claimed[g] = true;
                            }
                            tier
                        })
                        .collect()
                }
            })
            .collect();
        // The mulligan's bottoming list, resolved by the same rule as the other
        // three: a group belongs to the first tier that names it. What no entry
        // names is one more tier at the end, because a hand that has to put
        // back three cards has to put back three cards whatever the list says.
        // No order inside a tier: a tie there is priced rather than broken,
        // for the reason `MulliganPolicy::TIE_BREAK` gives.
        let bottom_tiers = schedule.mulligan().map_or_else(Vec::new, |policy| {
            let mut claimed = vec![false; groups];
            let mut tiers: Vec<Vec<usize>> = policy
                .tiers()
                .map(|query| {
                    let tier: Vec<usize> = (0..groups)
                        .filter(|&g| !claimed[g])
                        .filter(|&g| grouping.group_masks()[g] & (1u64 << query) != 0)
                        .collect();
                    for &g in &tier {
                        claimed[g] = true;
                    }
                    tier
                })
                .collect();
            tiers.push((0..groups).filter(|&g| !claimed[g]).collect());
            tiers
        });
        Board {
            grouping,
            schedule,
            group_effect,
            routed,
            land_groups,
            pool,
            casting,
            fetches: effects.iter().any(|e| e.fetch.is_some()),
            fetch_tiers,
            drops: vec![0; turns],
            hand: vec![vec![0; groups]; turns],
            yard: vec![vec![0; groups]; turns],
            drop_at: vec![None; turns],
            declared,
            fresh: Vec::with_capacity(schedule.gaps().iter().sum::<u32>() as usize),
            fresh_head: 0,
            kept: Vec::new(),
            revealed: vec![0; groups],
            removed: vec![0; groups],
            live_field: vec![0; groups],
            landed: vec![vec![0; groups]; turns],
            live_landed: vec![0; groups],
            played: vec![0; effects.len()],
            live_played: vec![0; groups],
            live_hand: vec![0; groups],
            live_yard: vec![0; groups],
            pending: Vec::new(),
            sacrificed: vec![vec![0; groups]; turns],
            bottom_tiers,
            bottomed: vec![0; groups],
            live_bottomed: vec![0; groups],
        }
    }

    /// Put `bottomed` back from the opener of every path walked from here on.
    ///
    /// The caller deals the opener and decides what goes back, because that
    /// is where the two engines differ: the enumeration walks every way a tie
    /// could fall, and the sampler tosses the coin. Everything after that —
    /// what the hand holds on turn 0, and what a tutor can still find — is
    /// this board's, and is the same for both.
    pub fn bottom(&mut self, bottomed: &[u32]) {
        self.bottomed.copy_from_slice(bottomed);
    }

    /// Every way `depth` cards could go back from `opener`, with the chance of
    /// each.
    ///
    /// The declared list is walked tier by tier and each tier gives up
    /// everything it holds until what is left to put back is less than that.
    /// That tier gives up the remainder uniformly among the cards it holds —
    /// the tie, priced — which is one multivariate hypergeometric over its
    /// groups. Every other tier is all or nothing, so only one of them can
    /// branch, and a hand with no tie in it has exactly one way to bottom.
    pub fn bottomings(&self, opener: &[u32], depth: u32, mut f: impl FnMut(&[u32], f64)) {
        let mut bottomed = vec![0u32; opener.len()];
        let mut left = depth;
        for tier in &self.bottom_tiers {
            if left == 0 {
                break;
            }
            let held: u32 = tier.iter().map(|&g| opener[g]).sum();
            if held <= left {
                for &g in tier {
                    bottomed[g] = opener[g];
                }
                left -= held;
                continue;
            }
            let sizes: Vec<u32> = tier.iter().map(|&g| opener[g]).collect();
            chip_stats::for_each_composition(&sizes, left, |take, q| {
                for (&g, &t) in tier.iter().zip(take) {
                    bottomed[g] = t;
                }
                f(&bottomed, q);
            });
            return;
        }
        // A policy's depth never exceeds its opener, and the catch-all tier
        // holds every group the list did not, so the loop above ran out of
        // cards to put back rather than out of tiers.
        debug_assert_eq!(left, 0, "put back fewer cards than the depth asked for");
        f(&bottomed, 1.0);
    }

    /// What `depth` cards go back from an opener dealt in `dealt` order,
    /// written into `out`.
    ///
    /// The sampler's half of [`Board::bottomings`], and deliberately a
    /// different algorithm. A tier that gives up only part of what it holds
    /// gives up the cards of it that were dealt first, and since the deal is a
    /// uniform shuffle, that is a uniform choice among them: the same coin,
    /// tossed rather than priced. The two engines agreeing on it is the test.
    pub fn bottom_in_order(&self, dealt: &[usize], depth: u32, out: &mut [u32]) {
        out.fill(0);
        let mut left = depth;
        for tier in &self.bottom_tiers {
            for &group in dealt {
                if left == 0 {
                    return;
                }
                if tier.contains(&group) {
                    out[group] += 1;
                    left -= 1;
                }
            }
        }
    }

    /// Whether a hand holding `hand` (one count per group) is one the declared
    /// mulligan keeps. True where the run declared none, because then every
    /// seven is kept.
    pub fn keeps(&self, hand: &[u32]) -> bool {
        self.schedule.mulligan().is_none_or(|policy| {
            policy
                .keep()
                .iter()
                .all(|k| k.holds(self.grouping.count_matching(hand, k.query)))
        })
    }

    /// Whether anything in this run takes a card out of the library without
    /// drawing it.
    ///
    /// The enumeration asks, because a walk that fetches nothing is the walk
    /// that could not: the shrinking population costs a prefix replay per
    /// checkpoint, and a run with no tutor in it should not pay that to learn
    /// that nothing moved.
    pub fn fetches(&self) -> bool {
        self.fetches
    }

    /// How many cards of each group this path has taken out of the unrevealed
    /// library, after the checkpoints the last [`Board::walk`] was given.
    pub fn removed(&self) -> &[u32] {
        &self.removed
    }

    /// Play one path out, turn by turn, filling the per-turn zone counts.
    ///
    /// `history` may be a **prefix** of a path rather than a whole one, and
    /// that is what makes the shrinking population work: the enumeration asks
    /// what a path has removed before it deals the next gap, and the only
    /// honest answer is the one this same walk gives. A turn whose checkpoints
    /// the prefix does not reach is not played at all — playing half of one
    /// would land a drop off cards nobody has seen, and the removal it decided
    /// would not survive the next checkpoint.
    pub fn walk(&mut self, history: Path<'_>) {
        self.fresh.clear();
        self.fresh_head = 0;
        self.kept.clear();
        self.played.fill(0);
        self.live_played.fill(0);
        self.live_hand.fill(0);
        self.live_yard.fill(0);
        self.live_field.fill(0);
        self.revealed.fill(0);
        self.removed.fill(0);
        self.live_landed.fill(0);
        self.drop_at.fill(None);
        self.pending.clear();
        self.live_bottomed.copy_from_slice(&self.bottomed);
        if let Some(casting) = &mut self.casting {
            casting.live_cast.fill(0);
        }

        let effects = self.schedule.effects();
        for turn in 0usize..self.schedule.turns() {
            let (first, last) = self.schedule.checkpoints_of(turn);
            if last >= history.len() {
                break;
            }
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
                        self.revealed[group] += 1;
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
            // What the mulligan put back leaves the opener before anything
            // else reads it, so turn 0 is the hand that was kept: five cards
            // after a mulligan to five, not the seven it was dealt from. The
            // cards are on the bottom of the library, which is where every
            // count of the library already finds them — they are not in the
            // hand, the yard or play.
            if turn == 0 {
                for (held, back) in self.live_hand.iter_mut().zip(&self.bottomed) {
                    debug_assert!(*back <= *held, "put back a card the opener did not hold");
                    *held -= back;
                }
            }

            // The land drop, and there is exactly one of them a turn. Which
            // land it is has two answers, and which one this run uses is the
            // whole of issue #54: a file that declared a priority gets the land
            // it declared, and every part of the run reads that same drop.
            match turn.checked_sub(1) {
                None => self.drops[turn] = 0,
                Some(previous) if self.declared.is_some() => {
                    // Whatever an earlier drop set up for this turn resolves
                    // first: a chapter ability goes on the stack as the main
                    // phase begins, before the land it could be played beside.
                    self.sacrificed[turn].fill(0);
                    self.resolve_pending(turn);
                    let chosen = self.declared_drop();
                    // What is standing there when the turn is over, which is
                    // the land you played unless it went and got another one
                    // in exchange for itself.
                    let mut landed = chosen;
                    if let Some(group) = chosen {
                        self.live_played[group] += 1;
                        if let Some(effect) = self.group_effect[group] {
                            if let (Trigger::LandDrop, Some(delay)) =
                                (effects[effect].trigger, effects[effect].delay)
                            {
                                self.pending
                                    .push((turn + delay.turns as usize, effect, group));
                            } else if effects[effect].trigger == Trigger::LandDrop {
                                self.look(effect, effects[effect].look);
                                if let Some((got, to)) = self.fetch(effect) {
                                    match to {
                                        Fetched::Hand => self.live_hand[got] += 1,
                                        Fetched::Battlefield => {
                                            landed = Some(got);
                                            self.live_landed[got] += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    self.drop_at[turn] = landed;
                    if let Some(group) = landed {
                        self.live_field[group] += 1;
                    }
                    self.drops[turn] = self.drops[previous] + u32::from(chosen.is_some());
                }
                Some(previous) => {
                    if let Some(chosen) = self.land_drop() {
                        self.played[chosen] += 1;
                        self.look(chosen, effects[chosen].look);
                    }
                    // Nothing fetches here. A run with no declared priority
                    // cannot say which land it played, so a land-drop fetch is
                    // refused at the boundary rather than guessed at, and
                    // `live_field` stays the empty thing nothing in this
                    // branch reads.
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
            self.landed[turn].copy_from_slice(&self.live_landed);
            if let Some(declared) = &mut self.declared {
                // What is standing there, not what left your hand. A fetchland
                // is both played and not in play, and every run that fetches
                // no land has the two identical.
                declared.played_at[turn].copy_from_slice(&self.live_field);
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
                        // The tutor resolves before the line moves on, which
                        // is the order the pilot plays it in and the only
                        // order that lets four mana cast Trinket Mage and
                        // then the Lantern it just fetched.
                        self.fetch_on_cast(group);
                    }
                }
            }
            casting.spent[turn] = spent;
        }
        self.casting = Some(casting);
    }

    /// Resolve every delayed effect set up to fire on `turn`, in the order the
    /// drops that set them up were made.
    ///
    /// A delayed fetch puts its card **beside** the land that waited for it
    /// rather than in its place: the Saga was on the battlefield for two turns
    /// making mana, which a fetchland never is. Whether that land then leaves
    /// is its [`Delay::sacrifice`].
    fn resolve_pending(&mut self, turn: usize) {
        let mut i = 0;
        while i < self.pending.len() {
            let (fires, effect, source) = self.pending[i];
            if fires != turn {
                i += 1;
                continue;
            }
            self.pending.remove(i);
            if let Some((got, to)) = self.fetch(effect) {
                match to {
                    Fetched::Hand => self.live_hand[got] += 1,
                    Fetched::Battlefield => {
                        self.live_field[got] += 1;
                        self.live_landed[got] += 1;
                    }
                }
            }
            if self.schedule.effects()[effect]
                .delay
                .is_some_and(|d| d.sacrifice)
            {
                self.live_field[source] -= 1;
                self.sacrificed[turn][source] += 1;
            }
        }
    }

    /// Resolve the tutor on a spell that has just been cast, if it carries
    /// one.
    ///
    /// Once per casting: two Trinket Mages fetch twice, and one fetching twice
    /// would be a card appearing from nowhere.
    fn fetch_on_cast(&mut self, group: usize) {
        let Some(effect) = self.group_effect[group] else {
            return;
        };
        if self.schedule.effects()[effect].trigger != Trigger::Cast {
            return;
        }
        if let Some((got, to)) = self.fetch(effect) {
            match to {
                Fetched::Hand => self.live_hand[got] += 1,
                Fetched::Battlefield => {
                    self.live_field[got] += 1;
                    self.live_landed[got] += 1;
                }
            }
        }
    }

    /// Go and get a card, by the priority this effect declared.
    ///
    /// The first tier holding a card this path has not already taken, and
    /// inside a tier the group the decklist named first — the same shape of
    /// rule as the land drop's and the casting line's, over the third
    /// contested resource. A tier that finds nothing falls through to the
    /// next; a priority that finds nothing at all fetches nothing, which is
    /// what a tutor does when the card is already gone.
    ///
    /// **The unrevealed library first.** A card left on top by an earlier look
    /// is one you were about to draw anyway, so taking that copy is the worse
    /// line and this takes it only when it is the only one left — the same
    /// reading of a decision the pilot gets to make that the gate already
    /// takes of the land drop.
    fn fetch(&mut self, effect: usize) -> Option<(usize, Fetched)> {
        let to = self.schedule.effects()[effect].fetch.as_ref()?.to;
        for tier in 0..self.fetch_tiers[effect].len() {
            let deep = self.fetch_tiers[effect]
                .get(tier)
                .and_then(|groups| groups.iter().copied().find(|&g| self.unrevealed(g) > 0));
            if let Some(group) = deep {
                self.removed[group] += 1;
                return Some((group, to));
            }
            let on_top = self.kept.iter().position(|kept| {
                self.fetch_tiers[effect]
                    .get(tier)
                    .is_some_and(|groups| groups.contains(kept))
            });
            if let Some(i) = on_top {
                return Some((self.kept.remove(i), to));
            }
            // And a card the mulligan put on the bottom is still in the
            // library, so a search still finds it — last, because it is the one
            // copy no draw would ever have reached.
            let underneath = self.fetch_tiers[effect]
                .get(tier)
                .and_then(|groups| groups.iter().copied().find(|&g| self.live_bottomed[g] > 0));
            if let Some(group) = underneath {
                self.live_bottomed[group] -= 1;
                return Some((group, to));
            }
        }
        None
    }

    /// Cards of `group` this path has neither turned over nor fetched.
    ///
    /// The pool a tutor searches and the pool the next gap is dealt from are
    /// the same pool, which is the invariant that keeps the two engines
    /// agreeing about a fetch.
    fn unrevealed(&self, group: usize) -> u32 {
        self.grouping.group_sizes()[group] - self.revealed[group] - self.removed[group]
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
            // Cannot underflow: the hand, the yard, the spells this path cast
            // and the cards a fetch put straight onto the battlefield are
            // disjoint subsets of the same groups `matching_total` sums over —
            // a card is drawn once, casting it takes it out of the hand rather
            // than copying it, and a fetch takes its card out of a library
            // nothing has drawn from yet.
            Zone::Library => {
                self.grouping.matching_total(query)
                    - in_hand
                    - self.grouping.count_matching(&self.yard[turn], query)
                    - self.cast_by(turn, query)
                    - self.grouping.count_matching(&self.landed[turn], query)
            }
            // What is standing there: lands played and cards put there, plus
            // what the line cast. A cast spell is counted here because it is a
            // permanent — the caller refuses a battlefield question about any
            // card that is not, since an instant resolves and goes nowhere
            // this engine models. For a land question the second term is
            // zero, because a land is played rather than cast.
            Zone::Battlefield => self.played_by(turn, query) + self.cast_by(turn, query),
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
            //
            // A land sacrificed this turn is added back: it is gone by the end
            // of the turn, which is what `counts` records, and it was tapped
            // before it went, which is what the pool is.
            let sacrificed = &self.sacrificed[turn];
            let usable = |slot: usize| {
                let group = self.land_groups[slot];
                counts[group] + sacrificed[group] - u32::from(tapped_now == Some(slot))
            };
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
