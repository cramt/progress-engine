//! `sort-mismatch`: a reference bound to an object its words can't name.
//!
//! The resolver ([backref]) picks the nearest introducer it calls
//! compatible, and what it calls compatible comes from verb names. This
//! check reads the bound tree again with a typed notion of what each binder
//! is, and flags a reference whose words no referent can satisfy: "that
//! permanent or player" bound to a revealed card (Erratic Explosion), "the
//! nonland card" bound to any target (Explosive Revelation), "from among
//! them" over an equipped creature (Conjurer's Mantle). It also flags one
//! effect whose two sides are the same binder where the text says two
//! things: a source dealing damage to itself through "that creature"
//! (Moonlight Hunt), an Equipment attached to itself (Goldwardens' Gambit).
//!
//! A binder's sort comes from its description, or for one an effect made
//! from that effect, and grows as later effects move it: a card put onto
//! the battlefield is a permanent from then on (CR 400.7), a permanent that
//! leaves is a card too (and still the permanent it was, for "that
//! creature's power", CR 608.2h). What an effect, a cost or an event does
//! to its objects is one exhaustive `match` each, so a new variant does not
//! compile until it says. Sorts only grow: "put it onto the battlefield.
//! Otherwise, put that card into your hand" reads both branches, so a
//! reference is flagged only when no reading of the text fits it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ast::*;
use crate::binders::bindings;
use crate::walk::{Visitor, Walk};

/// The kind of object a binder is at some point in its ability.
#[derive(Clone, Debug, PartialEq)]
pub enum Sort {
    /// On the battlefield. `types` are its card types where its words say
    /// them (empty: unknown); `token` whether it is one, where known.
    Permanent {
        types: BTreeSet<String>,
        token: Option<bool>,
    },
    /// A card in a zone other than the battlefield (`None`: off the
    /// battlefield, zone unsaid).
    Card {
        zone: Option<Zone>,
        types: BTreeSet<String>,
    },
    Player,
    /// A spell on the stack: a card or a copy (CR 112.1), which becomes a
    /// permanent as it resolves if it is a permanent spell (CR 608.3).
    Spell,
    /// An activated or triggered ability on the stack (CR 113.1).
    Ability,
}

/// What a reference's words allow it to name.
#[derive(Clone, Debug, PartialEq)]
enum Want {
    /// "that creature", "that permanent": a permanent of these types.
    Permanent(BTreeSet<String>),
    /// "that token".
    Token,
    /// "that card", "the nonland card", a pool "from among them".
    Card,
    /// "that player" of "that permanent or player".
    Player,
    /// "that spell".
    Spell,
    /// "that ability".
    Ability,
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let types = |t: &BTreeSet<String>| {
            if t.is_empty() {
                String::new()
            } else {
                format!(" ({})", t.iter().cloned().collect::<Vec<_>>().join("/"))
            }
        };
        match self {
            Sort::Permanent { types: t, token } => write!(
                f,
                "{}permanent{}",
                if *token == Some(true) { "token " } else { "" },
                types(t)
            ),
            Sort::Card { zone, types: t } => match zone {
                Some(z) => write!(f, "card in {z:?}{}", types(t)),
                None => write!(f, "card off the battlefield{}", types(t)),
            },
            Sort::Player => f.write_str("player"),
            Sort::Spell => f.write_str("spell"),
            Sort::Ability => f.write_str("ability"),
        }
    }
}

/// One violation, said for a reader.
#[derive(Clone, Debug)]
pub struct Finding {
    pub line: usize,
    pub why: String,
}

const CARD_TYPES: [&str; 9] = [
    "artifact",
    "battle",
    "creature",
    "enchantment",
    "land",
    "planeswalker",
    "kindred",
    "instant",
    "sorcery",
];

const PERMANENT_TYPES: [&str; 6] = [
    "artifact",
    "battle",
    "creature",
    "enchantment",
    "land",
    "planeswalker",
];

/// The card types a filter's alternatives say, through its `or`s.
fn card_types(f: &Filter) -> BTreeSet<String> {
    let mut t = BTreeSet::new();
    let mut g = Some(f);
    while let Some(h) = g {
        for alt in &h.alts {
            for a in alt {
                if let Adj::Type(x) = a {
                    let x = x.to_lowercase();
                    if CARD_TYPES.contains(&x.as_str()) {
                        t.insert(x);
                    }
                }
            }
        }
        g = h.or.as_deref();
    }
    t
}

/// The types an alternative of a filter says (one `Filter` without its
/// `or`), which "artifact or creature" lists both of.
fn own_types(f: &Filter) -> BTreeSet<String> {
    card_types(&Filter {
        or: None,
        ..f.clone()
    })
}

fn zone_of(f: &Filter) -> Option<Zone> {
    f.posts.iter().find_map(|p| match p {
        Post::In(z) => Some(z.clone()),
        _ => None,
    })
}

fn targeted_any(q: &Quant) -> bool {
    matches!(q, Quant::AnyTarget { .. } | Quant::AnyOtherTarget)
}

/// What a description names, by its words. `None`: it could be anything
/// the check knows of, so it never fails.
fn described(o: &Objects) -> Option<Vec<Sort>> {
    let mut out = Vec::new();
    if targeted_any(&o.quant) {
        // "any target": a creature, player, planeswalker or battle (CR
        // 115.4).
        out.push(Sort::Permanent {
            types: BTreeSet::new(),
            token: None,
        });
        out.push(Sort::Player);
    }
    let mut g = Some(&o.filter);
    while let Some(f) = g {
        let types = own_types(f);
        match &f.kind {
            Some(Noun::Card | Noun::PermanentCard) => out.push(Sort::Card {
                zone: zone_of(f),
                types,
            }),
            Some(Noun::Permanent) => out.push(Sort::Permanent { types, token: None }),
            Some(Noun::Token) => out.push(Sort::Permanent {
                types,
                token: Some(true),
            }),
            Some(Noun::Player | Noun::Opponent) => out.push(Sort::Player),
            Some(Noun::Spell) => out.push(Sort::Spell),
            Some(Noun::Ability) => out.push(Sort::Ability),
            Some(Noun::SpellOrAbility) => {
                out.push(Sort::Spell);
                out.push(Sort::Ability);
            }
            // A commander may be in the command zone or on the battlefield;
            // a source may be any object; a copy a spell or a token.
            Some(Noun::Commander | Noun::Source | Noun::Copy | Noun::Pile | Noun::Counter(_)) => {
                return None
            }
            None if targeted_any(&o.quant) && f.alts.is_empty() => {}
            None => {
                if types.is_empty()
                    || zone_of(f).is_some_and(|z| z != Zone::Battlefield)
                    || !types.iter().any(|t| PERMANENT_TYPES.contains(&t.as_str()))
                {
                    return None;
                }
                out.push(Sort::Permanent {
                    types: types.clone(),
                    token: None,
                });
                // "the exiled creature", "the sacrificed artifact": a card
                // now, and still the permanent it was for its last known
                // information (CR 608.2h).
                if f.posts
                    .iter()
                    .any(|p| matches!(p, Post::Marked(m) if gone(*m)))
                {
                    out.push(Sort::Card { zone: None, types });
                }
            }
        }
        g = f.or.as_deref();
    }
    (!out.is_empty()).then_some(out)
}

/// A mark that says the object left the battlefield, or never was on it.
fn gone(m: Marked) -> bool {
    match m {
        Marked::Exiled
        | Marked::Sacrificed
        | Marked::Destroyed
        | Marked::Revealed
        | Marked::Found
        | Marked::Milled
        | Marked::Discarded
        | Marked::Returned
        | Marked::Cast
        | Marked::Cycled
        | Marked::Copied => true,
        Marked::Chosen
        | Marked::Tapped
        | Marked::Removed
        | Marked::PutOntoBattlefield
        | Marked::Manifested => false,
    }
}

/// What a reference's words allow. `None`: a bare "it", "them", "that
/// one", which may name anything.
fn wants(o: &Objects) -> Option<Vec<Want>> {
    let mut out = Vec::new();
    let mut g = Some(&o.filter);
    while let Some(f) = g {
        match &f.kind {
            Some(Noun::Card | Noun::PermanentCard) => out.push(Want::Card),
            Some(Noun::Permanent) => out.push(Want::Permanent(own_types(f))),
            Some(Noun::Token) => out.push(Want::Token),
            Some(Noun::Player | Noun::Opponent) => out.push(Want::Player),
            Some(Noun::Spell) => out.push(Want::Spell),
            Some(Noun::Ability) => out.push(Want::Ability),
            Some(Noun::SpellOrAbility) => {
                out.push(Want::Spell);
                out.push(Want::Ability);
            }
            Some(Noun::Commander | Noun::Source | Noun::Copy | Noun::Pile | Noun::Counter(_)) => {
                return None
            }
            None => {
                let t = own_types(f);
                if t.is_empty() || !t.iter().all(|x| PERMANENT_TYPES.contains(&x.as_str())) {
                    return None;
                }
                out.push(Want::Permanent(t));
            }
        }
        g = f.or.as_deref();
    }
    (!out.is_empty()).then_some(out)
}

fn satisfies(s: &Sort, w: &Want) -> bool {
    match (s, w) {
        (Sort::Permanent { types, .. }, Want::Permanent(want)) => {
            types.is_empty() || want.is_empty() || !types.is_disjoint(want)
        }
        (Sort::Permanent { token, .. }, Want::Token) => *token != Some(false),
        (Sort::Permanent { .. }, _) => false,
        (Sort::Card { .. }, Want::Card | Want::Spell) => true,
        (Sort::Card { .. }, _) => false,
        (Sort::Player, Want::Player) => true,
        (Sort::Player, _) => false,
        // A spell is a card on the stack and resolves into a permanent
        // ("whenever you cast a creature spell, that creature enters with
        // …"), or into a token if it is a copy.
        (Sort::Spell, Want::Spell | Want::Card | Want::Permanent(_) | Want::Token) => true,
        (Sort::Spell, _) => false,
        (Sort::Ability, Want::Ability) => true,
        (Sort::Ability, _) => false,
    }
}

// ---------------------------------------------------------------- motion

/// Where an effect, cost or event takes an object it names.
#[derive(Clone, Copy, Debug, PartialEq)]
enum To {
    /// Onto the battlefield: a permanent from then on (CR 400.7).
    Battlefield,
    /// To a zone other than the battlefield: a card from then on, and
    /// still the permanent it was for what is said of it after.
    Off,
    /// Cast: a spell.
    Stack,
    /// Its types are set anew: "becomes a 3/3 creature", "transform it".
    Retyped,
}

/// What one effect does to the objects it names, and what it makes that
/// no `Objects` names.
#[derive(Default)]
struct Says<'a> {
    moves: Vec<(&'a Objects, To)>,
    /// `Some` for an effect that makes objects a `With` binds as `Made`.
    makes: Option<Vec<Sort>>,
}

fn token_sort(t: &Token) -> Vec<Sort> {
    let types = t
        .types
        .iter()
        .map(|x| x.to_lowercase())
        .filter(|x| CARD_TYPES.contains(&x.as_str()))
        .collect();
    vec![Sort::Permanent {
        types,
        token: Some(true),
    }]
}

fn cards(zone: Option<Zone>) -> Option<Vec<Sort>> {
    Some(vec![Sort::Card {
        zone,
        types: BTreeSet::new(),
    }])
}

fn tokens() -> Option<Vec<Sort>> {
    Some(vec![Sort::Permanent {
        types: BTreeSet::new(),
        token: Some(true),
    }])
}

fn moves(what: &Objects, to: To) -> Says<'_> {
    Says {
        moves: vec![(what, to)],
        makes: None,
    }
}

/// Every effect says what it does to the objects it names. Listed whole,
/// so a new effect is a compile error here until it says.
fn effect(e: &Effect) -> Says<'_> {
    match e {
        Effect::Draw { .. } => Says {
            makes: cards(None),
            ..Says::default()
        },
        Effect::Mill { .. } => Says {
            makes: cards(None),
            ..Says::default()
        },
        Effect::LookAtTop { .. } | Effect::RevealTop { .. } => Says {
            makes: cards(None),
            ..Says::default()
        },
        Effect::ExileZone(_) => Says {
            makes: cards(Some(Zone::Exile)),
            ..Says::default()
        },
        Effect::DiscardHand(_) => Says {
            makes: cards(None),
            ..Says::default()
        },
        Effect::RevealHand(_) => Says {
            makes: cards(None),
            ..Says::default()
        },
        Effect::UntilFound { .. } | Effect::DigUntilTotal { .. } => Says {
            makes: cards(None),
            ..Says::default()
        },
        Effect::Create { token, .. }
        | Effect::CreateNamed { token, .. }
        | Effect::CreateThosePlus { token, .. } => Says {
            makes: Some(token_sort(token)),
            ..Says::default()
        },
        Effect::CreateAttached { token, .. } => Says {
            makes: Some(token_sort(token)),
            ..Says::default()
        },
        Effect::CreateThose { .. } | Effect::CreateCopy { .. } | Effect::Amass { .. } => Says {
            makes: tokens(),
            ..Says::default()
        },
        Effect::Copy { .. } => Says {
            makes: Some(vec![Sort::Spell]),
            ..Says::default()
        },
        // The keyword actions that make objects: tokens, or a face-down
        // permanent (CR 701.34, 701.58).
        Effect::Action { name, .. } => Says {
            makes: match name.as_str() {
                "investigate" | "incubate" | "populate" | "amass" => tokens(),
                "manifest" | "manifest dread" | "cloak" => Some(vec![Sort::Permanent {
                    types: BTreeSet::new(),
                    token: Some(false),
                }]),
                _ => None,
            },
            ..Says::default()
        },
        Effect::Destroy { what, .. }
        | Effect::Exile(what)
        | Effect::ExileFaceDown(what)
        | Effect::ExileUntilLeaves { what, .. }
        | Effect::ExileUntil { what, .. }
        | Effect::ExileWithCounters { what, .. }
        | Effect::Sacrifice { what, .. }
        | Effect::Discard { what, .. }
        | Effect::ShuffleInto { what, .. }
        | Effect::Counter { what, .. } => moves(what, To::Off),
        Effect::ShuffleAllInto { what, .. } => Says {
            moves: what.iter().map(|o| (o, To::Off)).collect(),
            makes: None,
        },
        Effect::Move { what, to, .. } => moves(
            what,
            if *to == Zone::Battlefield {
                To::Battlefield
            } else {
                To::Off
            },
        ),
        Effect::Cast { what, .. } | Effect::CastPaying { what, .. } => moves(what, To::Stack),
        Effect::Becomes { what, .. }
        | Effect::IsNow { what, .. }
        | Effect::AlsoIs { what, .. }
        | Effect::BecomesCopy { what, .. }
        | Effect::Transform(what)
        | Effect::TurnFaceUp(what)
        | Effect::Meld { what, .. } => moves(what, To::Retyped),
        Effect::LifeTotalBecomes { .. }
        | Effect::Damage { .. }
        | Effect::PutCounters { .. }
        | Effect::RemoveCounters { .. }
        | Effect::Modify { .. }
        | Effect::GainLife { .. }
        | Effect::LoseLife { .. }
        | Effect::AddMana(_)
        | Effect::Search { .. }
        | Effect::Scry(_)
        | Effect::Surveil(_)
        | Effect::Tap(_)
        | Effect::Untap(_)
        | Effect::TapOrUntap(_)
        | Effect::Fight { .. }
        | Effect::FightEachOther(_)
        | Effect::Shuffle(_)
        | Effect::Reveal { .. }
        | Effect::Restrict { .. }
        | Effect::GainControl { .. }
        | Effect::Regenerate(_)
        | Effect::Attach { .. }
        | Effect::ExtraTurn { .. }
        | Effect::ChooseNewTargets(_)
        | Effect::ClassLevel(_)
        | Effect::May { .. }
        | Effect::IfYouDont(_)
        | Effect::Otherwise(_)
        | Effect::CountersArePut { .. }
        | Effect::Pay { .. }
        | Effect::During { .. }
        | Effect::Prevent { .. }
        | Effect::BaseBecomes { .. }
        | Effect::Designation(_)
        | Effect::Delayed { .. }
        | Effect::EntersTapped(_)
        | Effect::Choose(_)
        | Effect::GetEnergy(_)
        | Effect::Emblem(_)
        | Effect::Vote(_)
        | Effect::Simultaneously(_)
        | Effect::StartingWith { .. }
        | Effect::AnyTime { .. }
        | Effect::Where { .. }
        | Effect::IfYouDo(_)
        | Effect::With { .. }
        | Effect::If { .. }
        | Effect::PermissionRules { .. }
        | Effect::Unless { .. }
        | Effect::ForEach { .. }
        | Effect::ForEachColor { .. }
        | Effect::Modal { .. }
        | Effect::GrantAbility { .. }
        | Effect::LoseAbility { .. }
        | Effect::Performed { .. }
        | Effect::PlayerGains { .. }
        | Effect::PlayerRestrict { .. }
        | Effect::PhaseOut(_)
        | Effect::Instead { .. }
        | Effect::Or(_)
        | Effect::PutThose { .. }
        | Effect::DelayedWhen { .. }
        | Effect::DelayedNext { .. }
        | Effect::WhenYouDo(_)
        | Effect::WhenYouDoIf { .. }
        | Effect::Labelled { .. }
        | Effect::ChangeTarget(_)
        | Effect::ChangeTargetTo { .. }
        | Effect::LookAt { .. }
        | Effect::ExchangeLifeWithStat { .. }
        | Effect::ExchangeControl(_)
        | Effect::ExchangeLifeTotals { .. }
        | Effect::LookAtHand { .. }
        | Effect::CastAsThoughFlash { .. }
        | Effect::LoseKeywords { .. }
        | Effect::LoseGame(_)
        | Effect::ExtraLandPlays { .. }
        | Effect::InsteadOf { .. }
        | Effect::ActionOn { .. }
        | Effect::MoveCounters { .. }
        | Effect::PutThoseCounters { .. }
        | Effect::Double { .. }
        | Effect::WinGame(_)
        | Effect::RollDie(_)
        | Effect::RollDice { .. }
        | Effect::KeepsMana { .. }
        | Effect::Repeat { .. }
        | Effect::RepeatUntil(_)
        | Effect::Process(_)
        | Effect::ShuffleZones { .. }
        | Effect::NoMaxHandSize { .. }
        | Effect::EntersWithCounters { .. }
        | Effect::DistributeCounters { .. }
        | Effect::AdditionalPhases { .. }
        | Effect::DoesTheSame(_)
        | Effect::SeparatePiles { .. }
        | Effect::Replacement { .. }
        | Effect::DoubleCounters { .. }
        | Effect::ThatDamage { .. }
        | Effect::GetCounters { .. }
        | Effect::DelayedWhenever { .. }
        | Effect::RemoveFromCombat(_)
        | Effect::SkipTurn(_)
        | Effect::SpendAsAnyColor { .. }
        | Effect::SpendAsAnyType { .. }
        | Effect::PaysRather { .. }
        | Effect::EntersWith { .. }
        | Effect::EntersAttacking(_)
        | Effect::EntersAttached { .. }
        | Effect::DamageCantBePrevented(_)
        | Effect::Loses { .. }
        | Effect::GainsChoiceOf { .. }
        | Effect::GetEnergyAmount(_)
        | Effect::RepeatProcess
        | Effect::ControlPlayerNextTurn(_)
        | Effect::BecomesSized { .. }
        | Effect::KeywordCostIs { .. }
        | Effect::EmblemFor { .. }
        | Effect::ForEachCardType(_)
        | Effect::FlipCoin
        | Effect::CostChange { .. }
        | Effect::DamageDivided { .. }
        | Effect::EndTheTurn
        | Effect::PutCountersEach { .. }
        | Effect::WhenThisWay { .. }
        | Effect::RemoveAllCounters(_)
        | Effect::RemoveAllCountersOf { .. }
        | Effect::LosesAllCounters(_)
        | Effect::NextTime { .. }
        | Effect::PreventThatDamage
        | Effect::ProducesInstead { .. }
        | Effect::OnSpend { .. }
        | Effect::DoubleX(_)
        | Effect::BecomesPrepared(_)
        | Effect::BecomesPlotted(_)
        | Effect::BecomesSaddled(_)
        | Effect::RemoveAnyCounters { .. } => Says::default(),
    }
}

/// What an event does to the objects it names: "dies" takes a creature
/// off the battlefield, "enters" puts it on.
fn event(e: &Event, out: &mut Vec<(*const Objects, To)>) {
    let mut put = |o: &Objects, to| out.push((o as *const Objects, to));
    match e {
        Event::Dies(o)
        | Event::Leaves { what: o, .. }
        | Event::Discards { what: o, .. }
        | Event::Sacrifices { what: o, .. }
        | Event::Exiles { what: o, .. }
        | Event::Exiled(o)
        | Event::ReturnedToHand(o)
        | Event::Countered(o)
        | Event::Cycles { what: Some(o), .. } => put(o, To::Off),
        Event::PutInto { what, to, .. } | Event::Puts { what, to, .. } => put(
            what,
            if *to == Zone::Battlefield {
                To::Battlefield
            } else {
                To::Off
            },
        ),
        Event::Enters { what, .. } => put(what, To::Battlefield),
        Event::Casts { what, .. }
        | Event::Copies { what, .. }
        | Event::WasCast(what)
        | Event::CastFrom { what, .. }
        | Event::CastUsing { what, .. } => put(what, To::Stack),
        Event::TurnedFaceUp(what) | Event::TransformsInto { what, .. } => put(what, To::Retyped),
        Event::Or(es) => es.iter().for_each(|e| event(e, out)),
        Event::While { event: e, .. }
        | Event::AndIf { event: e, .. }
        | Event::InterveningIf { event: e, .. } => event(e, out),
        Event::FirstTimeEachTurn(e) => event(e, out),
        Event::ManaSpent(_)
        | Event::Attacks(_)
        | Event::Blocks(_)
        | Event::AttacksOrBlocks(_)
        | Event::BecomesBlocked(_)
        | Event::BlockedWith { .. }
        | Event::DealsCombatDamageToPlayer(_)
        | Event::DealsCombatDamageTo { .. }
        | Event::DealsDamage(_)
        | Event::IsDealtDamage(_)
        | Event::GainsLife(_)
        | Event::RollsDice { .. }
        | Event::LosesLife(_)
        | Event::Draws(_)
        | Event::YouAttack
        | Event::BecomesTapped(_)
        | Event::BecomesUntapped(_)
        | Event::BecomesCrewed(_)
        | Event::Crews { .. }
        | Event::BecomesTarget { .. }
        | Event::Cycles { what: None, .. }
        | Event::CounterPutOn(_)
        | Event::PlaysLand(_)
        | Event::Creates { .. }
        | Event::CountersPut { .. }
        | Event::TappedForMana(_)
        | Event::DrawsNth { .. }
        | Event::DrawsExceptFirstInDrawStep(_)
        | Event::Searches { .. }
        | Event::TapsForMana { .. }
        | Event::DealsDamageTo { .. }
        | Event::BecomesLevel { .. }
        | Event::PlayerAttacks { .. }
        | Event::AttacksWith { .. }
        | Event::BeginsExtraTurn(_)
        | Event::NthCounterPutOn { .. }
        | Event::IsAttacked(_)
        | Event::DealsNoncombatDamageTo { .. }
        | Event::TapsFor { .. }
        | Event::AddsMana { .. }
        | Event::LeftUnderControlOf { .. }
        | Event::DealsCombatDamage(_)
        | Event::AttacksAnyOf { .. }
        | Event::Activates { .. }
        | Event::TappedForAtLeast { .. }
        | Event::DrawsOneOrMore(_)
        | Event::DrawsByCycling { .. }
        | Event::AttacksAlone(_)
        | Event::AttacksUnblocked { .. }
        | Event::Exerts { .. }
        | Event::BecomesMonarch(_)
        | Event::Performs { .. } => {}
    }
}

/// What a cost does to the objects it names. A cost holding others
/// (`OneOf`) says nothing itself: the walk visits each of them.
fn cost(c: &Cost) -> Option<(&Objects, To)> {
    match c {
        Cost::Sacrifice(o)
        | Cost::Discard(o)
        | Cost::Exile(o)
        | Cost::ReturnToHand(o)
        | Cost::DiscardAtRandom(o) => Some((o, To::Off)),
        Cost::Mana(_)
        | Cost::AmountOfMana(_)
        | Cost::Tap
        | Cost::Untap
        | Cost::PayLife(_)
        | Cost::RemoveCounters { .. }
        | Cost::PutCounters { .. }
        | Cost::TapUntapped(_)
        | Cost::Energy(_)
        | Cost::Reveal(_)
        | Cost::Mill(_)
        | Cost::Exert(_)
        | Cost::ManaCostOf(_)
        | Cost::OneOf(_)
        | Cost::DiscardHand
        | Cost::PayEnergy(_)
        | Cost::ManaPer { .. }
        | Cost::ManaTimes { .. }
        | Cost::Action { .. }
        | Cost::ActionMana { .. }
        | Cost::RemoveCountersAmong { .. }
        | Cost::ActionAlone(_)
        | Cost::PlayerGainsLife { .. }
        | Cost::RemoveAnyCounters { .. }
        | Cost::ActionOn { .. } => None,
    }
}

/// Two sides of one effect the text can't mean as one object.
#[derive(Clone, Copy, Debug)]
enum Pair {
    /// A source dealing damage to "that creature", "it": never itself,
    /// which Oracle says "itself" (CR 120.3).
    Damage,
    /// An Equipment or Aura attached to "it": never itself (CR 301.5c,
    /// 303.4d).
    Attach,
}

// ---------------------------------------------------------------- the walk

/// What one line's bound tree says, by object in walk order (the order
/// `binders::bindings` counts in).
#[derive(Default)]
struct Facts {
    /// Where an effect, cost or event takes the object said here.
    motion: BTreeMap<usize, To>,
    /// The sort of a `Made` binder's bind entry, from the effect after it.
    made: BTreeMap<usize, Option<Vec<Sort>>>,
    /// A reference said as the pool of "a card from among them".
    pools: BTreeSet<usize>,
    /// Another line's binder: (reference, line, binder, words).
    linked: Vec<(usize, usize, usize, Objects)>,
    /// The two sides of a `Pair`: (kind, one side, the other).
    pairs: Vec<(Pair, usize, usize)>,
}

struct Reader {
    decl: usize,
    facts: Facts,
    motion: BTreeMap<*const Objects, To>,
    made: BTreeMap<*const Objects, Option<Vec<Sort>>>,
    pools: BTreeSet<*const Objects>,
    pairs: Vec<(Pair, *const Objects, *const Objects)>,
    seen: BTreeMap<*const Objects, usize>,
}

impl Visitor for Reader {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let d = self.decl;
        self.decl += 1;
        let p = o as *const Objects;
        self.seen.insert(p, d);
        if let Some(to) = self.motion.remove(&p) {
            self.facts.motion.insert(d, to);
        }
        if let Some(s) = self.made.remove(&p) {
            self.facts.made.insert(d, s);
        }
        if self.pools.remove(&p) {
            self.facts.pools.insert(d);
        }
        if let Quant::LinkedBound { line, binder } = o.quant {
            self.facts
                .linked
                .push((d, line.into(), binder.into(), o.clone()));
        }
        // "a creature card from among them": the pool is cards.
        let mut g = Some(&o.filter);
        while let Some(f) = g {
            if matches!(f.kind, Some(Noun::Card | Noun::PermanentCard)) {
                for post in &f.posts {
                    if let Post::FromAmong(pool) = post {
                        self.pools.insert(&**pool as *const Objects);
                    }
                }
            }
            g = f.or.as_deref();
        }
        true
    }

    fn ability(&mut self, a: &mut Ability) -> bool {
        // A `Let`'s `Made` is hideaway's card or what an effect deeper in
        // made, which no effect beside it says: `sort_at` reads it as
        // anything.
        if let Ability::Triggered { trigger, .. } = a {
            self.trigger(trigger);
        }
        true
    }

    fn effect(&mut self, e: &mut Effect) {
        let says = effect(e);
        for (o, to) in says.moves {
            self.motion.insert(o, to);
        }
        match e {
            Effect::With { bind, then } => {
                let makes = then.first().and_then(|f| effect(f).makes);
                for o in bind.iter() {
                    if o.quant == Quant::Made {
                        self.made.insert(o, makes.clone());
                    }
                }
            }
            Effect::DelayedWhen { event: ev, .. }
            | Effect::DelayedWhenever { event: ev, .. }
            | Effect::DelayedNext { event: ev, .. }
            | Effect::WhenThisWay { event: ev, .. }
            | Effect::ExileUntil { until: ev, .. } => self.event(ev),
            Effect::NextTime { event: ev, .. } => self.event(ev),
            // "If a nontoken creature would die, exile that card instead".
            Effect::Replacement { event: ev, .. } => self.event(ev),
            Effect::Damage {
                source: Some(s),
                to,
                ..
            } => {
                let mut rs: Vec<&Recipient> = to.iter().collect();
                while let Some(r) = rs.pop() {
                    match r {
                        Recipient::Objects(t) => self.pairs.push((Pair::Damage, s, t)),
                        Recipient::Or(more) => rs.extend(more.iter()),
                        _ => {}
                    }
                }
            }
            Effect::Attach { what, to } => self.pairs.push((Pair::Attach, what, to)),
            _ => {}
        }
    }

    fn cost(&mut self, c: &mut Cost) {
        if let Some((o, to)) = cost(c) {
            self.motion.insert(o, to);
        }
    }

    fn statik(&mut self, s: &mut Static) {
        if let Static::Replacement { event: ev, .. } = s {
            self.event(ev);
        }
    }
}

impl Reader {
    fn event(&mut self, e: &Event) {
        let mut out = Vec::new();
        event(e, &mut out);
        self.motion.extend(out);
    }

    fn trigger(&mut self, t: &Trigger) {
        match t {
            Trigger::When(e) | Trigger::WhenAndAt(e, _) => self.event(e),
            Trigger::InterveningIf(t, _) => self.trigger(t),
            Trigger::At(_) => {}
        }
    }
}

fn read(a: &Ability) -> Facts {
    let mut a = a.clone();
    let mut r = Reader {
        decl: 0,
        facts: Facts::default(),
        motion: BTreeMap::new(),
        made: BTreeMap::new(),
        pools: BTreeSet::new(),
        pairs: Vec::new(),
        seen: BTreeMap::new(),
    };
    a.walk(&mut r);
    let pairs = std::mem::take(&mut r.pairs);
    for (k, x, y) in pairs {
        if let (Some(&x), Some(&y)) = (r.seen.get(&x), r.seen.get(&y)) {
            r.facts.pairs.push((k, x, y));
        }
    }
    r.facts
}

// ---------------------------------------------------------------- sorts

fn grow(sorts: &mut Vec<Sort>, to: To) {
    let add = |s: &mut Vec<Sort>, x: Sort| {
        if !s.contains(&x) {
            s.push(x);
        }
    };
    let now = sorts.clone();
    for s in now {
        match (to, s) {
            (To::Battlefield, Sort::Card { types, .. }) => add(
                sorts,
                Sort::Permanent {
                    types,
                    token: Some(false),
                },
            ),
            (To::Battlefield, Sort::Spell) => add(
                sorts,
                Sort::Permanent {
                    types: BTreeSet::new(),
                    token: None,
                },
            ),
            (To::Off, Sort::Permanent { types, token }) if token != Some(true) => {
                add(sorts, Sort::Card { zone: None, types })
            }
            (To::Off, Sort::Spell) => add(
                sorts,
                Sort::Card {
                    zone: None,
                    types: BTreeSet::new(),
                },
            ),
            (To::Stack, Sort::Card { .. }) => add(sorts, Sort::Spell),
            (To::Retyped, Sort::Permanent { token, .. }) => add(
                sorts,
                Sort::Permanent {
                    types: BTreeSet::new(),
                    token,
                },
            ),
            _ => {}
        }
    }
}

/// One line's binders and references, read.
struct Line {
    facts: Facts,
    binders: Vec<crate::binders::BinderInfo>,
    /// Reference (walk order) to the binder it names, with its words.
    refs: BTreeMap<usize, (usize, Objects)>,
    /// Every mention of each binder, in walk order.
    mentions: BTreeMap<usize, Vec<usize>>,
}

impl Line {
    fn new(a: &Ability) -> Line {
        let b = bindings(a);
        let mut refs = BTreeMap::new();
        let mut mentions: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (d, v) in &b.refs {
            if let [Some(binder)] = v[..] {
                let words = b.words[d].clone();
                if matches!(words.quant, Quant::Bound(_) | Quant::Introduced(_)) {
                    refs.insert(*d, (binder, words));
                }
                mentions.entry(binder).or_default().push(*d);
            }
        }
        Line {
            facts: read(a),
            binders: b.binders,
            refs,
            mentions,
        }
    }

    /// What binder `b` may be just before the object `at` (walk order) is
    /// read: its description's sorts, grown by every motion said of it
    /// before. `None`: anything.
    fn sort_at(&self, b: usize, at: usize, depth: u8) -> Option<Vec<Sort>> {
        let info = self.binders.get(b)?;
        let mut s = if info.what.quant == Quant::Made {
            self.facts.made.get(&info.decl).cloned().flatten()?
        } else if let Some((inner, _)) = self.refs.get(&info.decl) {
            // A loop over objects said before ("for each of those tokens").
            if depth > 8 {
                return None;
            }
            self.sort_at(*inner, info.decl, depth + 1)?
        } else {
            described(&info.what)?
        };
        for m in self.mentions.get(&b).into_iter().flatten() {
            if *m >= at {
                break;
            }
            if let Some(&to) = self.facts.motion.get(m) {
                grow(&mut s, to);
            }
        }
        Some(s)
    }
}

fn show(s: &[Sort]) -> String {
    s.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(" or ")
}

fn fits(sorts: &[Sort], wants: &[Want]) -> bool {
    sorts.iter().any(|s| wants.iter().any(|w| satisfies(s, w)))
}

/// Every `sort-mismatch` on one face's lines, by line.
pub fn face(lines: &[crate::Line]) -> Vec<Finding> {
    let read: Vec<Option<Line>> = lines
        .iter()
        .map(|l| l.parsed.as_ref().ok().map(Line::new))
        .collect();
    let mut out = Vec::new();
    for (i, l) in read.iter().enumerate() {
        let Some(l) = l else { continue };
        let mut check = |d: usize, sorts: Option<Vec<Sort>>, words: &Objects| {
            let w = if l.facts.pools.contains(&d) {
                Some(vec![Want::Card])
            } else {
                wants(words)
            };
            if let (Some(s), Some(w)) = (sorts, w) {
                if !fits(&s, &w) {
                    out.push(Finding {
                        line: i,
                        why: format!(
                            "{:?} names {w:?} but its binder is a {}",
                            words.quant,
                            show(&s)
                        ),
                    });
                }
            }
        };
        for (&d, (b, words)) in &l.refs {
            check(d, l.sort_at(*b, d, 0), words);
        }
        for (d, line, binder, words) in &l.facts.linked {
            let sorts = read
                .get(*line)
                .and_then(Option::as_ref)
                .and_then(|o| o.sort_at(*binder, usize::MAX, 0));
            check(*d, sorts, words);
        }
        for &(k, x, y) in &l.facts.pairs {
            let (Some((bx, _)), Some((by, wy))) = (l.refs.get(&x), l.refs.get(&y)) else {
                continue;
            };
            if bx == by && matches!(wy.quant, Quant::Bound(_)) {
                out.push(Finding {
                    line: i,
                    why: format!("{k:?}: both sides are one binder"),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(quant: Quant, alts: Vec<Vec<Adj>>, kind: Option<Noun>) -> Objects {
        Objects {
            quant,
            filter: Filter {
                alts,
                kind,
                posts: vec![],
                or: None,
            },
        }
    }

    fn found(a: Ability) -> Vec<String> {
        let line = crate::Line {
            text: String::new(),
            parsed: Ok(a),
            continues: false,
            said: None,
        };
        face(&[line]).into_iter().map(|f| f.why).collect()
    }

    fn to_hand(what: Objects) -> Effect {
        Effect::Move {
            what,
            from: None,
            to: Zone::Hand(Player::You),
            tapped: false,
            transformed: false,
            under: None,
            order: None,
        }
    }

    /// Explosive Revelation as 278e7cbf read it: "Choose any target. …
    /// Put the nonland card into your hand" with the card bound to the
    /// target.
    #[test]
    fn a_card_bound_to_any_target_is_flagged() {
        let nonland = || vec![vec![Adj::Non("land".into())]];
        let tree = |card: u8| Ability::Let {
            bind: vec![obj(Quant::AnyTarget { n: Amount::N(1) }, vec![], None)],
            ability: Box::new(Ability::Spell(vec![
                Effect::Choose(Choice::Objects(obj(Quant::Introduced(0), vec![], None))),
                Effect::With {
                    bind: vec![obj(Quant::One, nonland(), Some(Noun::Card))],
                    then: vec![
                        Effect::UntilFound {
                            who: Player::You,
                            verb: DigVerb::Reveal,
                            zone: Zone::TopOfLibrary(Player::You),
                            until: obj(Quant::Introduced(0), vec![], None),
                        },
                        to_hand(obj(Quant::Bound(card), nonland(), Some(Noun::Card))),
                    ],
                },
            ])),
        };
        assert_eq!(found(tree(1)).len(), 1);
        assert_eq!(found(tree(0)), Vec::<String>::new());
    }

    /// "Put a creature card from among them onto the battlefield. Tap that
    /// creature": a card put onto the battlefield is a permanent after.
    #[test]
    fn a_card_put_onto_the_battlefield_is_a_permanent_after() {
        let creature = || vec![vec![Adj::Type("creature".into())]];
        let tree = Ability::Spell(vec![Effect::With {
            bind: vec![obj(Quant::One, creature(), Some(Noun::Card))],
            then: vec![
                Effect::Move {
                    what: obj(Quant::Introduced(0), vec![], None),
                    from: None,
                    to: Zone::Battlefield,
                    tapped: false,
                    transformed: false,
                    under: None,
                    order: None,
                },
                Effect::Tap(obj(Quant::Bound(0), creature(), None)),
            ],
        }]);
        assert_eq!(found(tree), Vec::<String>::new());
    }

    /// Goldwardens' Gambit: "you may attach an Equipment you control to
    /// it", with "it" bound to the Equipment.
    #[test]
    fn an_equipment_attached_to_itself_is_flagged() {
        let tree = Ability::Spell(vec![Effect::With {
            bind: vec![obj(
                Quant::One,
                vec![vec![Adj::Subtype("equipment".into())]],
                None,
            )],
            then: vec![Effect::Attach {
                what: obj(Quant::Introduced(0), vec![], None),
                to: obj(Quant::Bound(0), vec![], None),
            }],
        }]);
        assert_eq!(found(tree).len(), 1);
    }
}
