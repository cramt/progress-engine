//! A card's Oracle text, read by Vivisurgeon's Insight, lowered into the
//! engine's own characteristics and continuous effects (ADR-0037).
//!
//! Lowering never drops what it can't read: it refuses, naming the line and
//! the node. What the layers don't interpret (triggered and activated
//! abilities) is still an ability the object has, which Humility can take
//! away, so it is kept, unread.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use vivisurgeons_insight::ast::{
    self as i, Adj, Characteristic, Lost, Objects, Post, Quant, TypeSet,
};
use vivisurgeons_insight::lex::{Pt as IPt, PtPart};
use vivisurgeons_insight::{read_face, Face};

use crate::model::*;

/// "If any source you control would deal damage to a permanent or player
/// this turn, it deals double that damage to that permanent or player
/// instead" (Quest for Pure Flame), as Insight binds it: the replacement
/// for the turn binds the source and the recipient of each event it
/// replaces ([replacement-binds-event]), the recipient innermost. `None`
/// when the tree isn't that shape; a refusal by name when it is but says
/// what the engine can't run.
fn damage_this_turn(then: &[i::Effect]) -> Option<Result<Effect, String>> {
    let [i::Effect::Replacement { event, instead }] = then else {
        return None;
    };
    let i::Event::DealsDamageTo {
        what: source_is,
        to,
    } = &**event
    else {
        return None;
    };
    let [i::Recipient::Objects(recipient_is)] = to.as_slice() else {
        return Some(Err(
            "damage replaced to a recipient other than any one".into()
        ));
    };
    // The event's binders in the order it says them: the source, then the
    // recipient, the innermost.
    let (source, recipient) = (1, 0);
    let you = i::Filter {
        kind: Some(i::Noun::Source),
        posts: vec![Post::Controller(i::Player::You)],
        ..i::Filter::default()
    };
    if source_is.quant != Quant::One || source_is.filter != you {
        return Some(Err(
            "damage replaced from a source other than one you control".into(),
        ));
    }
    let any = i::Filter {
        kind: Some(i::Noun::Permanent),
        or: Some(Box::new(i::Filter {
            kind: Some(i::Noun::Player),
            ..i::Filter::default()
        })),
        ..i::Filter::default()
    };
    if recipient_is.quant != Quant::One || recipient_is.filter != any {
        return Some(Err(
            "damage replaced to a recipient other than any permanent or player".into(),
        ));
    }
    let [i::Effect::ThatDamage {
        source: by,
        n,
        to: same,
    }] = instead.as_slice()
    else {
        return Some(Err(
            "a damage replacement doing more than dealing that damage".into(),
        ));
    };
    if by.quant != Quant::Bound(source) || by.filter != i::Filter::default() {
        return Some(Err("damage replaced by another source's".into()));
    }
    let same_recipient = match same.as_deref() {
        None => true,
        Some([i::Recipient::Objects(o)]) => o.quant == Quant::Bound(recipient),
        Some(_) => false,
    };
    if !same_recipient {
        return Some(Err("damage replaced to another recipient".into()));
    }
    let by = match n {
        i::Amount::Twice(a) if **a == i::Amount::ThatDamage => 2,
        i::Amount::Thrice(a) if **a == i::Amount::ThatDamage => 3,
        _ => {
            return Some(Err(
                "damage replaced by other than double or triple it".into()
            ))
        }
    };
    Some(Ok(Effect::MultiplyDamageThisTurn { by }))
}

/// A card as Scryfall gives it, the fields the layers read.
pub struct Card<'a> {
    pub name: &'a str,
    pub type_line: &'a str,
    pub oracle_text: &'a str,
    pub mana_value: u32,
    pub colors: &'a [Color],
    pub pt: Option<Pt>,
}

pub struct Lowered {
    pub chars: Chars,
    pub statics: Vec<StaticAbility>,
    /// The abilities the layers carry but don't read, in the order of
    /// `Ability::Unread`'s index, for the state machine to run.
    pub unread: Vec<i::Ability>,
    /// The Oracle line each unread ability was read from, by the same
    /// index, so a log line's ability text can say which it is.
    pub unread_text: Vec<String>,
}

#[derive(Debug)]
pub enum Refusal {
    TypeLine(String),
    /// Insight couldn't read the line.
    Unparsed {
        line: String,
        at: String,
    },
    /// Insight read it, and the engine has no lowering for this node yet.
    Unlowered {
        line: String,
        node: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::TypeLine(t) => write!(f, "can't read the type line {t:?}"),
            Refusal::Unparsed { line, at } => write!(f, "Insight breaks at {at:?} in {line:?}"),
            Refusal::Unlowered { line, node } => write!(f, "no lowering for {node} in {line:?}"),
        }
    }
}

/// A card's characteristics outside the battlefield as far as its type
/// line, mana value and colours say, without its text: what a card in a
/// hand or library is matched by when its text can't be lowered. A
/// multi-faced card is its front face (CR 712.8a); a split card's mana
/// value is its halves' together (CR 709.4).
pub fn printed(c: &crate::deck::CardText) -> Result<Chars, Refusal> {
    let front = c.type_line.split("//").next().unwrap_or("").trim();
    let (types, supertypes, subtypes) = type_line(front)?;
    Ok(Chars {
        types,
        supertypes,
        subtypes,
        colors: c.colors.iter().copied().collect(),
        abilities: BTreeSet::new(),
        pt: c.pt,
        mana_value: c.mana_value,
        controller: PlayerId(0),
    })
}

/// One card, printed: its characteristics, the static abilities that
/// generate its continuous effects, and the rest of its abilities.
/// Whether an ability of the card says it works while the card is in a
/// graveyard ("As long as this card is in your graveyard", Anger's), so
/// it functions there (CR 113.6b) and a game with the card there runs it
/// or can't go on.
pub fn works_in_graveyard(card: &Card) -> bool {
    use vivisurgeons_insight::walk::{Visitor, Walk};
    struct Finds(bool);
    impl Visitor for Finds {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn ability(&mut self, _: &mut i::Ability) -> bool {
            true
        }
        fn condition(&mut self, c: &mut i::Condition) {
            if let i::Condition::In {
                what,
                zone: i::Zone::Graveyard(_) | i::Zone::OwnersGraveyard,
            } = c
            {
                self.0 |= what.quant == Quant::SelfRef;
            }
        }
    }
    let face = Face {
        name: card.name,
        type_line: card.type_line,
        text: card.oracle_text,
    };
    let mut finds = Finds(false);
    for line in read_face(&face, card.name) {
        match line.parsed {
            Ok(mut a) => a.walk(&mut finds),
            // A line Insight can't read may still say its zone: taken as
            // one that does, rather than as none.
            Err(_) => finds.0 |= says_graveyard_zone(&line.text),
        }
    }
    finds.0
}

/// Whether a card's words say it is in a graveyard ("as long as ~ is in
/// your graveyard"), read without Insight, where Insight can't.
pub fn says_graveyard_zone(text: &str) -> bool {
    let text = text.to_lowercase();
    text.contains("in your graveyard") || text.contains("in a graveyard")
}

pub fn card(card: &Card) -> Result<Lowered, Refusal> {
    let (types, supertypes, subtypes) = type_line(card.type_line)?;
    let mut lowered = Lowered {
        chars: Chars {
            types,
            supertypes,
            subtypes,
            colors: card.colors.iter().copied().collect(),
            abilities: BTreeSet::new(),
            pt: card.pt,
            mana_value: card.mana_value,
            controller: PlayerId(0),
        },
        statics: Vec::new(),
        unread: Vec::new(),
        unread_text: Vec::new(),
    };
    let face = Face {
        name: card.name,
        type_line: card.type_line,
        text: card.oracle_text,
    };
    for line in read_face(&face, card.name) {
        let ability = line.parsed.map_err(|b| Refusal::Unparsed {
            line: line.text.clone(),
            at: b.at,
        })?;
        lowered
            .ability(ability)
            .map_err(|node| Refusal::Unlowered {
                line: line.text.clone(),
                node,
            })?;
        let read = lowered.unread.len();
        lowered.unread_text.resize(read, line.text.clone());
    }
    Ok(lowered)
}

impl Lowered {
    /// Carry an ability the layers don't read, so an effect can still
    /// remove it, and the state machine can run it.
    fn unread(&mut self, ability: i::Ability) {
        let index = self.unread.len() as u16;
        self.chars.abilities.insert(Ability::Unread(index));
        self.unread.push(ability);
    }

    /// One ability, or the name of the node it can't lower.
    fn ability(&mut self, ability: i::Ability) -> Result<(), String> {
        match ability {
            // An ability word adds no rules (CR 207.2c): what it labels is
            // the ability.
            i::Ability::Labelled { ability, .. } => self.ability(*ability)?,
            // Carried with its binders, since the state machine resolves
            // them: the targets, the trigger's objects.
            a if carried(&a) => self.unread(a),
            i::Ability::Static(parts) => self.statics.push(static_ability(&parts, &[])?),
            // A static's objects, bound so another sentence of it can say
            // "its" ([binders] in Insight's SEMANTICS.md).
            i::Ability::Let { bind, ability } => match *ability {
                i::Ability::Static(parts) => self.statics.push(static_ability(&parts, &bind)?),
                // Hideaway's binder is the card it exiles, for the line that
                // plays it (CR 702.75a).
                i::Ability::Keywords(ks) if ks.iter().all(|k| keyword(k).is_none()) => {
                    self.unread(i::Ability::Let {
                        bind,
                        ability: Box::new(i::Ability::Keywords(ks)),
                    })
                }
                other => return Err(short(&other)),
            },
            i::Ability::Keywords(keywords) => {
                for k in keywords {
                    match keyword(&k) {
                        Some(ks) => {
                            for k in ks {
                                self.chars.abilities.insert(Ability::Keyword(k));
                            }
                        }
                        // Ward, flash, reach: the layers carry what they don't
                        // grant or remove by name.
                        None => self.unread(i::Ability::Keywords(vec![k])),
                    }
                }
            }
            other => return Err(short(&other)),
        }
        Ok(())
    }
}

/// What the layers carry unread. A spell's instructions are an ability too
/// (CR 113.3a), run as it resolves; a loyalty ability is activated (CR
/// 606.1), a chapter ability triggered (CR 714.2b).
fn carried(a: &i::Ability) -> bool {
    match a {
        i::Ability::Triggered { .. }
        | i::Ability::Activated { .. }
        | i::Ability::Spell(_)
        | i::Ability::Loyalty { .. }
        | i::Ability::Chapter { .. }
        | i::Ability::DieRow { .. }
        | i::Ability::Spree { .. }
        | i::Ability::Tiered(_)
        | i::Ability::OpeningHandAction(_) => true,
        i::Ability::Static(parts) => parts.iter().all(not_characteristics),
        i::Ability::Let { ability, .. } | i::Ability::Labelled { ability, .. } => carried(ability),
        // Power-up's rules ride on the activated ability (CR 702.193a).
        i::Ability::KeywordOn {
            keyword: i::Prefix::PowerUp,
            ability,
        } => carried(ability),
        _ => false,
    }
}

/// The binder a reference names, as an index into `scope` (a `Let`'s
/// `bind`, outermost first), counting de Bruijn from the innermost.
fn binder(o: &Objects, scope: &[Objects]) -> Option<usize> {
    match o.quant {
        Quant::Bound(i) | Quant::Introduced(i) => scope.len().checked_sub(1 + i as usize),
        _ => None,
    }
}

/// The objects a reference names, or the objects themselves.
fn resolve<'a>(o: &'a Objects, scope: &'a [Objects]) -> Result<&'a Objects, String> {
    match o.quant {
        Quant::Bound(_) | Quant::Introduced(_) => binder(o, scope)
            .map(|b| &scope[b])
            .ok_or_else(|| "a reference out of scope".into()),
        _ => Ok(o),
    }
}

/// Static abilities that change no characteristic of any object: rules,
/// costs, permissions, replacement and prevention effects, and effects on
/// players (CR 613.10–11, 614, 615, 601.2f). The layers carry them unread.
/// What isn't listed may change one, so lowering it is refused rather than
/// carried, and a characteristic is never dropped. "As this enters" is
/// here only as a shock land's choice: setting power and toughness as it
/// enters is a copiable effect, layer 1 (CR 613.2a).
fn not_characteristics(s: &i::Static) -> bool {
    use i::Static::*;
    match s {
        AsEnters { what, then } => {
            what.quant == Quant::SelfRef
                && (pays_life_or_tapped(then).is_some() || chooses_creature_type(then))
        }
        // "You may have this land enter tapped as a copy of any land on
        // the battlefield": a replacement effect on its own entering (CR
        // 614.1c, 707.5), whose copy the state machine makes.
        EntersAsCopy { what, .. } => what.quant == Quant::SelfRef,
        // "If you control two or more other lands, this land enters
        // tapped."
        If { then, .. } => !then.is_empty() && then.iter().all(not_characteristics),
        // "During your turn, spells you cast cost {1} less": a rule while
        // it holds.
        AsLongAs {
            cond: i::Condition::ItsYourTurn,
            then,
            ..
        } => !then.is_empty() && then.iter().all(not_characteristics),
        _ => listed_not_characteristics(s),
    }
}

/// "As this enchantment enters, choose a creature type" (CR 614.12a).
pub fn chooses_creature_type(then: &[i::Effect]) -> bool {
    matches!(then, [i::Effect::Choose(i::Choice::CreatureType)])
}

/// "As this land enters, you may pay 2 life. If you don't, it enters
/// tapped": the life, for a replacement effect whose choice is made before
/// the permanent enters (CR 614.1c, 614.12a).
/// Mox Diamond's "If this artifact would enter, you may discard a land
/// card instead. If you do, put this artifact onto the battlefield. If
/// you don't, put it into its owner's graveyard.": the card discarded.
pub fn discard_to_enter(event: &i::Event, instead: &[i::Effect]) -> Option<CardFilter> {
    let i::Event::Enters {
        what,
        under: None,
        state: None,
    } = event
    else {
        return None;
    };
    let me = |o: &Objects| o.quant == Quant::SelfRef && o.filter == i::Filter::default();
    let moved = |e: &[i::Effect], zone: i::Zone| {
        matches!(e, [i::Effect::Move { what, from: None, to, tapped: false, transformed: false, under: None, order: None }]
            if me(what) && *to == zone)
    };
    match instead {
        [i::Effect::May {
            who: i::Player::You,
            then,
        }, i::Effect::IfYouDo(enters), i::Effect::IfYouDont(not)]
            if me(what)
                && moved(enters, i::Zone::Battlefield)
                && moved(not, i::Zone::OwnersGraveyard) =>
        {
            match then.as_slice() {
                [i::Effect::Discard {
                    who: i::Player::You,
                    what,
                }] => card_filter(what).ok(),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn pays_life_or_tapped(then: &[i::Effect]) -> Option<u32> {
    match then {
        [i::Effect::May {
            who: i::Player::You,
            then: pay,
        }, i::Effect::IfYouDont(otherwise)] => match (pay.as_slice(), otherwise.as_slice()) {
            (
                [i::Effect::Pay {
                    who: i::Player::You,
                    cost,
                }],
                [i::Effect::EntersTapped(o)],
            ) if o.quant == Quant::SelfRef => match cost.as_slice() {
                [i::Cost::PayLife(i::Amount::N(n))] => Some(*n),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn listed_not_characteristics(s: &i::Static) -> bool {
    use i::Static::*;
    matches!(
        s,
        X(_) | Restrict { .. }
            | EntersTapped { .. }
            | EntersTappedUnless { .. }
            | EntersUntapped { .. }
            | EntersWithCounters { .. }
            | EscapesWithCounters { .. }
            | EntersPrepared { .. }
            | CostChange { .. }
            | CostChangePer { .. }
            | AbilityCostChange { .. }
            | AbilityAdditionalCost { .. }
            | ActivatedCostChange { .. }
            | AdditionalCost { .. }
            | AdditionalCostFor { .. }
            | OptionalAdditionalCost { .. }
            | AlternativeCost { .. }
            | PaysRather { .. }
            | CastPaying { .. }
            | CastWithoutPaying
            | CastFromUsing { .. }
            | CastAsThoughFlash { .. }
            | CastAsThoughFlashFor { .. }
            | CastOnlyAsSorcery { .. }
            | ActivateAsThoughHaste { .. }
            | ActivationsPerTurn { .. }
            | AssignAsThoughUnblocked { .. }
            | ExtraLand { .. }
            | PlayerRestrict { .. }
            | PermissionRules { .. }
            | PlayFrom { .. }
            | PlayWithTopRevealed { .. }
            | LookAtTopAnyTime
            | UntapDuring { .. }
            | MaxHandSize
            | MaxHandSizeIs { .. }
            | YouHave { .. }
            | SkipStep { .. }
            | CanBeCommander
            | DeckCanHave { .. }
            | Replacement { .. }
            | AdditionalTrigger { .. }
            | EnteringDoesntTrigger { .. }
            | ProtectionDoesntRemove { .. }
            | PreventDamage { .. }
            | PreventNoncombat { .. }
            | CantCauseSearch { .. }
            | SpendAsAnyColor { .. }
            | SpendAsAnyType { .. }
            | ExertAsAttacks { .. }
            | Goaded { .. }
    )
}

/// What a resolving effect leaves on the objects it was told to affect: the
/// continuous effect, and counters put on them. Which objects is the state
/// machine's: the targets chosen, or the set an "all" matched as it
/// resolved (CR 611.2c). How long it lasts is the state machine's too.
pub struct Resolution {
    pub ops: Vec<Op>,
    pub counters: Vec<(Counter, u32)>,
}

/// One effect of a spell or an ability, as it resolves.
pub fn resolution(effect: &i::Effect) -> Result<Resolution, Refusal> {
    let refuse = || Refusal::Unlowered {
        line: format!("{effect:?}"),
        node: short(effect),
    };
    match effect {
        // CR 701.66a: "Target land you control becomes a 0/0 land creature
        // with haste in addition to its other types. Put N +1/+1 counters
        // on it." Its return-to-the-battlefield trigger is the state
        // machine's.
        i::Effect::Action {
            name,
            n: Some(i::Amount::N(n)),
            ..
        } if name == "earthbend" => Ok(Resolution {
            ops: vec![
                Op::AddTypes([CardType::Land, CardType::Creature].into()),
                Op::SetBasePt(PtValue::Fixed(pt(0, 0))),
                Op::Grant(Ability::Keyword(Keyword::Haste)),
            ],
            counters: vec![(Counter::PlusOne, *n)],
        }),
        // "Target creature gets +3/+3 until end of turn."
        i::Effect::Modify { pt: p, gains, .. } => {
            let mut ops = Vec::new();
            if let Some(p) = p {
                ops.push(Op::ModifyPt(
                    fixed(p).filter(|_| p.signed).ok_or_else(refuse)?,
                ));
            }
            for k in gains {
                for k in keyword(k).ok_or_else(refuse)? {
                    ops.push(Op::Grant(Ability::Keyword(k)));
                }
            }
            Ok(Resolution {
                ops,
                counters: vec![],
            })
        }
        // "This land becomes a 2/2 Assembly-Worker artifact creature
        // until end of turn. It's still a land.": it keeps every prior
        // card type and subtype (CR 205.1b), and a colour it is given
        // replaces its others (CR 105.3).
        i::Effect::Becomes {
            pt: p,
            types: adjs,
            gains,
            keeps: i::Keeps::OtherTypes,
            ..
        }
        | i::Effect::Becomes {
            pt: p,
            types: adjs,
            gains,
            keeps: i::Keeps::Listed(_),
            ..
        } => {
            let mut types = BTreeSet::new();
            let mut colors = BTreeSet::new();
            for adj in adjs {
                match adj {
                    Adj::Type(t) => {
                        types.insert(card_type(t).ok_or_else(refuse)?);
                    }
                    Adj::Color(c) => {
                        colors.insert(color(c).ok_or_else(refuse)?);
                    }
                    Adj::Subtype(_) => {}
                    _ => return Err(refuse()),
                }
            }
            let subtypes: BTreeSet<Subtype> = adjs
                .iter()
                .filter_map(|a| match a {
                    Adj::Subtype(t) => Some(subtype(t, &types)),
                    _ => None,
                })
                .collect();
            let mut ops = vec![Op::AddTypes(types)];
            if !subtypes.is_empty() {
                ops.push(Op::AddSubtypes(subtypes));
            }
            if !colors.is_empty() {
                ops.push(Op::SetColors(colors));
            }
            for k in gains {
                for k in keyword(k).ok_or_else(refuse)? {
                    ops.push(Op::Grant(Ability::Keyword(k)));
                }
            }
            if let Some(p) = p {
                ops.push(Op::SetBasePt(PtValue::Fixed(
                    fixed(p).filter(|_| !p.signed).ok_or_else(refuse)?,
                )));
            }
            Ok(Resolution {
                ops,
                counters: vec![],
            })
        }
        _ => Err(refuse()),
    }
}

/// The node an effect needs, for a refusal: "Effect::Draw", and through
/// `With` and `May` the first effect they hold, "Effect::May(Move)".
pub fn effect_node(e: &i::Effect) -> String {
    fn inner(e: &i::Effect) -> String {
        match e {
            i::Effect::With { then, .. } => then.first().map_or("With".into(), inner),
            i::Effect::May { then, .. } => {
                format!("May({})", then.first().map_or(String::new(), inner))
            }
            other => short(other),
        }
    }
    format!("Effect::{}", inner(e))
}

/// The node a trigger needs: "Trigger::When(Event::Enters)",
/// "Trigger::At(Step::Upkeep)".
pub fn trigger_node(t: &i::Trigger) -> String {
    match t {
        i::Trigger::When(e) => format!("Trigger::When(Event::{})", short(e)),
        i::Trigger::At(s) => format!("Trigger::At(Step::{})", short(s)),
        i::Trigger::InterveningIf(t, _) => format!("{} if", trigger_node(t)),
        i::Trigger::State(c) => format!("Trigger::State(Condition::{})", short(c)),
        i::Trigger::WhenAndAt(e, s) => {
            format!(
                "Trigger::WhenAndAt(Event::{}, Step::{})",
                short(e),
                short(s)
            )
        }
    }
}

fn short(node: &dyn fmt::Debug) -> String {
    let s = format!("{node:?}");
    let end = s.find(['(', '{', ' ']).unwrap_or(s.len());
    s[..end].to_string()
}

/// Card types, supertypes and subtypes, from the type line.
type Types = (BTreeSet<CardType>, BTreeSet<Supertype>, BTreeSet<Subtype>);

fn type_line(line: &str) -> Result<Types, Refusal> {
    let (before, after) = line.split_once(" — ").unwrap_or((line, ""));
    let mut types = BTreeSet::new();
    let mut supertypes = BTreeSet::new();
    // Scryfall writes a token's as "Token Artifact"; being a token is the
    // object's, not a type (CR 111.1).
    for word in before.split_whitespace().filter(|w| *w != "Token") {
        if let Some(t) = card_type(word) {
            types.insert(t);
        } else if let Some(t) = supertype(word) {
            supertypes.insert(t);
        } else {
            return Err(Refusal::TypeLine(line.to_string()));
        }
    }
    let subtypes = after
        .split_whitespace()
        .map(|w| subtype(w, &types))
        .collect();
    Ok((types, supertypes, subtypes))
}

fn card_type(word: &str) -> Option<CardType> {
    Some(match word.to_ascii_lowercase().as_str() {
        "artifact" => CardType::Artifact,
        "battle" => CardType::Battle,
        "creature" => CardType::Creature,
        "enchantment" => CardType::Enchantment,
        "instant" => CardType::Instant,
        "kindred" => CardType::Kindred,
        "land" => CardType::Land,
        "planeswalker" => CardType::Planeswalker,
        "sorcery" => CardType::Sorcery,
        _ => return None,
    })
}

fn supertype(word: &str) -> Option<Supertype> {
    Some(match word.to_ascii_lowercase().as_str() {
        "basic" => Supertype::Basic,
        "legendary" => Supertype::Legendary,
        "snow" => Supertype::Snow,
        "world" => Supertype::World,
        _ => return None,
    })
}

fn basic_land_type(word: &str) -> Option<BasicLandType> {
    Some(match word.to_ascii_lowercase().as_str() {
        "plains" => BasicLandType::Plains,
        "island" => BasicLandType::Island,
        "swamp" => BasicLandType::Swamp,
        "mountain" => BasicLandType::Mountain,
        "forest" => BasicLandType::Forest,
        _ => return None,
    })
}

/// A subtype is a land type on a land, a creature type on a creature or
/// kindred (CR 205.3d), and otherwise kept as written.
fn subtype(word: &str, types: &BTreeSet<CardType>) -> Subtype {
    let word = capitalised(word);
    let said_alone =
        |kind: fn(&vivisurgeons_insight::vocab::Vocab) -> &std::collections::HashSet<String>| {
            types.is_empty()
                && kind(vivisurgeons_insight::vocab::vocab()).contains(&word.to_lowercase())
        };
    if let Some(b) = basic_land_type(&word) {
        Subtype::BasicLand(b)
    } else if types.contains(&CardType::Land)
        // "each Locus": a land type said alone is a land's (CR 205.3i).
        || said_alone(|v| &v.land_types)
    {
        Subtype::OtherLand(word)
    } else if types.contains(&CardType::Creature)
        || types.contains(&CardType::Kindred)
        // "another Human you control": a creature type said alone is a
        // creature's (CR 205.3m).
        || said_alone(|v| &v.creature_types)
    {
        Subtype::Creature(word)
    } else {
        Subtype::Other(word)
    }
}

pub fn capitalised(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| {
            f.to_uppercase()
                .chain(c.flat_map(|x| x.to_lowercase()))
                .collect()
        })
        .unwrap_or_default()
}

/// The keyword abilities a keyword is, by name: one, or several for
/// "protection from black and from red" (CR 702.16g). `None` for one the
/// layers don't name, which they carry unread.
fn keyword(k: &i::Keyword) -> Option<Vec<Keyword>> {
    let name = k.name.to_lowercase();
    if name == "protection" {
        return match k.param.as_ref()? {
            i::KeywordParam::Colors(cs) => cs
                .iter()
                .map(|c| match c {
                    i::Color::Multicolored => Some(Keyword::Protection(Quality::Multicolored)),
                    c => color(c).map(|c| Keyword::Protection(Quality::Color(c))),
                })
                .collect(),
            // "Protection from everything" (CR 702.16j).
            i::KeywordParam::Objects(o)
                if o.quant == Quant::All && o.filter == i::Filter::default() =>
            {
                Some(vec![Keyword::Protection(Quality::Everything)])
            }
            _ => None,
        };
    }
    // CR 702.21a: a ward of generic mana, of a number of life or of life
    // equal to its own power; one of coloured mana or of another cost
    // isn't read.
    if name == "ward" {
        let i::KeywordParam::Cost(cost) = k.param.as_ref()? else {
            return None;
        };
        let ward = match cost.as_slice() {
            [i::Cost::Mana(m)] => {
                let cost = crate::mana::ManaCost::from_symbols(m).ok()?;
                if !cost.shards.is_empty() {
                    return None;
                }
                WardCost::Mana(cost.generic)
            }
            [i::Cost::PayLife(i::Amount::N(n))] => WardCost::Life(*n),
            [i::Cost::PayLife(i::Amount::PowerOf(o))] if itself(o) => WardCost::LifeEqualToPower,
            _ => return None,
        };
        return Some(vec![Keyword::Ward(ward)]);
    }
    if k.param.is_some() {
        return None;
    }
    Some(vec![match name.as_str() {
        "deathtouch" => Keyword::Deathtouch,
        "defender" => Keyword::Defender,
        "double strike" => Keyword::DoubleStrike,
        "first strike" => Keyword::FirstStrike,
        "flying" => Keyword::Flying,
        "haste" => Keyword::Haste,
        "hexproof" => Keyword::Hexproof,
        "indestructible" => Keyword::Indestructible,
        "lifelink" => Keyword::Lifelink,
        "menace" => Keyword::Menace,
        "reach" => Keyword::Reach,
        "shadow" => Keyword::Shadow,
        "shroud" => Keyword::Shroud,
        "trample" => Keyword::Trample,
        "vigilance" => Keyword::Vigilance,
        "plainswalk" => Keyword::Landwalk(BasicLandType::Plains),
        "islandwalk" => Keyword::Landwalk(BasicLandType::Island),
        "swampwalk" => Keyword::Landwalk(BasicLandType::Swamp),
        "mountainwalk" => Keyword::Landwalk(BasicLandType::Mountain),
        "forestwalk" => Keyword::Landwalk(BasicLandType::Forest),
        _ => return None,
    }])
}

/// A characteristic-defining ability's power and toughness (CR 604.3):
/// "~'s power is equal to the number of creatures you control", "~'s
/// power and toughness are each equal to …", "~'s power is equal to …
/// and its toughness is equal to that number plus 1". `None` if the
/// static isn't one.
fn defines_pt(parts: &[i::Static]) -> Option<Result<Op, String>> {
    let me = |o: &Objects| o.quant == Quant::SelfRef && o.filter == i::Filter::default();
    let (power, toughness) = match parts {
        [i::Static::SetPower { what, equal_to }] if me(what) => (Some(equal_to), None),
        [i::Static::SetToughness { what, equal_to }] if me(what) => (None, Some((equal_to, 0))),
        [i::Static::SetPt {
            what,
            each_equal_to,
        }] if me(what) => (Some(each_equal_to), Some((each_equal_to, 0))),
        [i::Static::SetPower { what, equal_to }, i::Static::SetToughness {
            what: same,
            equal_to: i::Amount::Plus(that, k),
        }] if me(what) && me(same) && **that == i::Amount::ThatNumber => {
            (Some(equal_to), Some((equal_to, *k as i32)))
        }
        _ => return None,
    };
    Some((|| {
        let define = |a: &i::Amount, plus: i32| defined(a).map(|count| Defined { count, plus });
        Ok(Op::DefinePt {
            power: power.map(|a| define(a, 0)).transpose()?,
            toughness: toughness.map(|(a, k)| define(a, k)).transpose()?,
        })
    })())
}

/// What a characteristic-defining ability counts.
fn defined(a: &i::Amount) -> Result<DefinedCount, String> {
    let in_all_graveyards = |o: &Objects| {
        o.filter.kind == Some(i::Noun::Card)
            && o.filter.or.is_none()
            && matches!(
                o.filter.posts.as_slice(),
                [Post::In(i::Zone::Graveyard(i::Player::Each))]
            )
    };
    match a {
        i::Amount::NumberOfCardTypes(o) if in_all_graveyards(o) && o.filter.alts.is_empty() => {
            Ok(DefinedCount::GraveyardCardTypes)
        }
        // "The number of cards in all graveyards".
        i::Amount::CountOf(o) if in_all_graveyards(o) && o.filter.alts.is_empty() => {
            Ok(DefinedCount::GraveyardCards(Filter::default()))
        }
        i::Amount::CountOf(o) if in_all_graveyards(o) => {
            let filter = objects(&Objects {
                quant: Quant::All,
                filter: i::Filter {
                    kind: None,
                    posts: Vec::new(),
                    ..o.filter.clone()
                },
            })?;
            if filter
                .0
                .iter()
                .any(|p| !matches!(p, Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_)))
            {
                return Err("counting cards in graveyards relative to a source".into());
            }
            Ok(DefinedCount::GraveyardCards(filter))
        }
        i::Amount::CountOf(o) => Ok(DefinedCount::Permanents(counted(o)?)),
        other => Err(format!("a power or toughness of {}", short(other))),
    }
}

fn cmp(op: i::Op) -> Cmp {
    match op {
        i::Op::Eq => Cmp::Eq,
        i::Op::Le => Cmp::AtMost,
        i::Op::Ge => Cmp::AtLeast,
        i::Op::Lt => Cmp::Below,
        i::Op::Gt => Cmp::Above,
    }
}

/// "Three or more": a comparison with a number as a count.
fn count_of(op: i::Op, v: u32) -> Result<Count, String> {
    Ok(match op {
        i::Op::Ge => Count::AtLeast(v),
        i::Op::Gt => Count::AtLeast(v + 1),
        i::Op::Le => Count::AtMost(v),
        i::Op::Lt if v > 0 => Count::AtMost(v - 1),
        _ => return Err("AsLongAs(AmountIs) of exactly a number".into()),
    })
}

/// A static ability is one ability however many sentences it has: each
/// part is an `Op` over one filter. Parts over different objects would be
/// two effects, and nothing needs that yet.
fn static_ability(parts: &[i::Static], scope: &[Objects]) -> Result<StaticAbility, String> {
    if let Some(op) = defines_pt(parts) {
        return Ok(StaticAbility {
            filter: Filter(vec![Pred::Itself]),
            ops: vec![op?],
            condition: None,
        });
    }
    // "This creature gets +1/+1 as long as you control a Mountain": the
    // effect, and when it exists.
    if let [i::Static::AsLongAs { cond, then, .. }] = parts {
        let condition = match cond {
            i::Condition::YouControl(o) => {
                let c = you_control(o)?;
                if c.any_of
                    .iter()
                    .flat_map(|f| &f.0)
                    .any(|p| matches!(p, Pred::Power(..)))
                {
                    return Err("a static's condition over power".into());
                }
                Condition::YouControl(c)
            }
            i::Condition::ItsYourTurn => Condition::YourTurn,
            // "As long as this creature has three or more +1/+1 counters
            // on it".
            i::Condition::AmountIs {
                n: i::Amount::CountOf(o),
                op,
                value: i::Amount::N(v),
            } if matches!(
                (&o.quant, &o.filter),
                (Quant::Plural, i::Filter { kind: Some(i::Noun::Counter(Some(_))), posts, alts, or: None })
                    if alts.is_empty() && matches!(posts.as_slice(), [Post::On(on)] if itself(on))
            ) =>
            {
                let Some(i::Noun::Counter(Some(kind))) = &o.filter.kind else {
                    unreachable!("matched above")
                };
                Condition::CountersOnItself(
                    counter(kind).ok_or("AsLongAs(AmountIs) of a counter")?,
                    count_of(*op, *v)?,
                )
            }
            // "As long as there are three or more cards exiled with this
            // creature": the cards its own abilities exiled (CR 607.2a).
            i::Condition::AmountIs {
                n: i::Amount::CountOf(o),
                op,
                value: i::Amount::N(v),
            } if o.quant == Quant::Plural
                && o.filter.kind == Some(i::Noun::Card)
                && o.filter.alts.is_empty()
                && o.filter.or.is_none()
                && matches!(o.filter.posts.as_slice(), [Post::ExiledWith(w)] if itself(w)) =>
            {
                Condition::ExiledWithItself(count_of(*op, *v)?)
            }
            // Threshold: "seven or more cards in your graveyard".
            i::Condition::AmountIs {
                n: i::Amount::CountOf(o),
                op,
                value: i::Amount::N(v),
            } => match cards_in_graveyards(o) {
                Some(Ok(Amount::CardsInGraveyards {
                    whose: Whose::Yours,
                    named: None,
                    filter,
                })) if filter == CardFilter(vec![Filter::default()]) => {
                    Condition::CardsInYourGraveyard(count_of(*op, *v)?)
                }
                Some(Err(node)) => return Err(node),
                _ => return Err("AsLongAs(AmountIs)".into()),
            },
            other => return Err(format!("AsLongAs({})", short(other))),
        };
        return Ok(StaticAbility {
            condition: Some(condition),
            ..static_ability(then, scope)?
        });
    }
    let mut filter: Option<Filter> = None;
    let mut ops = Vec::new();
    let mut carried = false;
    for part in parts {
        // Name what changes a characteristic and can't be lowered first;
        // a part that changes none only makes the ability mixed.
        if not_characteristics(part) {
            carried = true;
            continue;
        }
        let (what, part_ops) = static_part(part)?;
        let f = objects(resolve(what, scope)?)?;
        if filter.as_ref().is_some_and(|g| *g != f) {
            return Err("parts over different objects".into());
        }
        filter = Some(f);
        ops.extend(part_ops);
    }
    if carried {
        return Err("a characteristic and a rule in one ability".into());
    }
    let filter = filter.ok_or("an empty static ability")?;
    // Layer 7 changes power, so a static reading it would depend on
    // layer 7's effects, which `Layer::has_dependencies` says none does.
    if filter.0.iter().any(|p| matches!(p, Pred::Power(..))) {
        return Err("a static over power".into());
    }
    Ok(StaticAbility {
        filter,
        ops,
        condition: None,
    })
}

fn static_part(part: &i::Static) -> Result<(&Objects, Vec<Op>), String> {
    let unlowered = || short(part);
    Ok(match part {
        i::Static::Modify {
            what,
            pt: p,
            has,
            per: None,
        } => {
            let mut ops = Vec::new();
            if let Some(p) = p {
                ops.push(Op::ModifyPt(
                    fixed(p).filter(|_| p.signed).ok_or_else(unlowered)?,
                ));
            }
            for k in has {
                for k in keyword(k).ok_or_else(unlowered)? {
                    ops.push(Op::Grant(Ability::Keyword(k)));
                }
            }
            (what, ops)
        }
        i::Static::BasePt { what, pt: p } => {
            let p = fixed(p).filter(|_| !p.signed).ok_or_else(unlowered)?;
            (what, vec![Op::SetBasePt(PtValue::Fixed(p))])
        }
        i::Static::Loses { what, lost } => {
            let ops = lost
                .iter()
                .map(|l| match l {
                    Lost::All {
                        other: false,
                        of: Characteristic::Abilities,
                    } => Ok(Op::LoseAll),
                    _ => Err(unlowered()),
                })
                .collect::<Result<_, _>>()?;
            (what, ops)
        }
        i::Static::AlsoIs {
            what,
            types: TypeSet::Listed(adjs),
        } => {
            let mut types = BTreeSet::new();
            let mut subtypes = BTreeSet::new();
            for adj in adjs {
                match adj {
                    Adj::Type(t) => {
                        types.insert(card_type(t).ok_or_else(unlowered)?);
                    }
                    Adj::Subtype(s) => {
                        subtypes.insert(subtype(s, &types));
                    }
                    _ => return Err(unlowered()),
                }
            }
            let mut ops = Vec::new();
            if !types.is_empty() {
                ops.push(Op::AddTypes(types));
            }
            if !subtypes.is_empty() {
                ops.push(Op::AddSubtypes(subtypes));
            }
            (what, ops)
        }
        // "Lands you control are every basic land type in addition to
        // their other types": each has every basic land type's mana
        // ability too (CR 305.6).
        i::Static::AlsoIs {
            what,
            types: TypeSet::EveryBasicLandType,
        } => (
            what,
            vec![Op::AddSubtypes(
                [
                    BasicLandType::Plains,
                    BasicLandType::Island,
                    BasicLandType::Swamp,
                    BasicLandType::Mountain,
                    BasicLandType::Forest,
                ]
                .into_iter()
                .map(Subtype::BasicLand)
                .collect(),
            )],
        ),
        // "Nonbasic lands are Mountains" (CR 305.7).
        i::Static::Is {
            what,
            types: TypeSet::Subtypes(adjs),
        } => match adjs.as_slice() {
            [Adj::Subtype(s)] => (
                what,
                vec![Op::SetBasicLandType(
                    basic_land_type(s).ok_or_else(unlowered)?,
                )],
            ),
            _ => return Err(unlowered()),
        },
        // "is an artifact creature": unlike most effects that set card
        // types, it keeps the object's others (CR 205.1b). Other types
        // set would replace them (CR 205.1a), which nothing models yet.
        i::Static::Is {
            what,
            types: TypeSet::Listed(adjs),
        } if *adjs == [Adj::Type("artifact".into()), Adj::Type("creature".into())] => (
            what,
            vec![Op::AddTypes(
                [CardType::Artifact, CardType::Creature].into(),
            )],
        ),
        // "each equal to its mana value": the object's own, `Itself` each in
        // turn ([each-itself]).
        i::Static::BasePtEqual {
            what,
            each_equal_to: i::Amount::ManaValueOf(of),
        } if of.quant == i::Quant::Itself => (what, vec![Op::SetBasePt(PtValue::ManaValue)]),
        _ => return Err(unlowered()),
    })
}

/// A colour a filter asks about: one of the five, or multicolored or
/// colorless (CR 105.2b–c).
fn color_prop(c: &i::Color) -> Option<Prop> {
    match c {
        i::Color::Multicolored => Some(Prop::Multicolored),
        i::Color::Colorless => Some(Prop::Colorless),
        c => color(c).map(Prop::Color),
    }
}

fn color(c: &i::Color) -> Option<Color> {
    Some(match c {
        i::Color::White => Color::White,
        i::Color::Blue => Color::Blue,
        i::Color::Black => Color::Black,
        i::Color::Red => Color::Red,
        i::Color::Green => Color::Green,
        _ => return None,
    })
}

fn fixed(p: &IPt) -> Option<Pt> {
    match (p.power, p.toughness) {
        (PtPart::N(a), PtPart::N(b)) => Some(pt(a, b)),
        _ => None,
    }
}

/// "a legendary green creature", "two or more basic lands", "a Mountain
/// or a Plains" in "unless you control …". Its source is the permanent
/// the condition is about, so "other" is relative to it; "this" and
/// "enchanted" are refused.
pub fn you_control(o: &Objects) -> Result<YouControl, String> {
    let count = match o.quant {
        Quant::One => Count::AtLeast(1),
        Quant::AtLeast(i::Amount::N(n)) => Count::AtLeast(n),
        Quant::AtMost(i::Amount::N(n)) => Count::AtMost(n),
        ref q => return Err(format!("a condition on {}", short(q))),
    };
    if o.filter.kind.is_some() || o.filter.or.is_some() {
        return Err("a filter over a kind of object".into());
    }
    let any_of = o
        .filter
        .alts
        .iter()
        .map(|alt| {
            objects(&Objects {
                quant: Quant::All,
                filter: i::Filter {
                    alts: vec![alt.clone()],
                    posts: o.filter.posts.clone(),
                    ..i::Filter::default()
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if any_of
        .iter()
        .flat_map(|f| &f.0)
        .any(|p| matches!(p, Pred::Itself | Pred::AttachedTo))
    {
        return Err("a condition relative to its source".into());
    }
    Ok(YouControl { any_of, count })
}

/// "for each creature you control": the permanents counted, read from the
/// ability's source, so "other" and "you" are relative to it.
pub fn counted(o: &Objects) -> Result<Filter, String> {
    match o.quant {
        Quant::Plural | Quant::All | Quant::Each => objects(o),
        _ => Err(format!("counting {}", short(&o.quant))),
    }
}

/// Price of Progress's "equal to twice the number of nonbasic lands that
/// player controls", "that player" being each player dealt damage; a
/// number the same for each.
fn each_players_amount(k: &i::Amount) -> Result<Amount, String> {
    Ok(match k {
        i::Amount::N(n) => Amount::N(*n),
        i::Amount::Twice(k) => Amount::Times(2, Box::new(each_players_amount(k)?)),
        i::Amount::CountOf(o)
            if matches!(o.quant, Quant::Plural | Quant::All)
                && matches!(
                    o.filter.posts.as_slice(),
                    [Post::Controller(i::Player::That)]
                ) =>
        {
            Amount::ThatPlayerControls(objects(&Objects {
                quant: Quant::All,
                filter: i::Filter {
                    posts: Vec::new(),
                    ..o.filter.clone()
                },
            })?)
        }
        other => return Err(format!("damage to each player of {}", short(other))),
    })
}

/// "Cards named Accumulated Knowledge in all graveyards", "creature cards
/// in your graveyard": `None` if what is counted isn't cards in
/// graveyards.
fn cards_in_graveyards(o: &Objects) -> Option<Result<Amount, String>> {
    let f = &o.filter;
    let whose = f.posts.iter().find_map(|p| match p {
        Post::In(i::Zone::Graveyard(w)) => Some(w),
        _ => None,
    })?;
    Some((|| {
        if !matches!(o.quant, Quant::Plural | Quant::All | Quant::Each) {
            return Err(format!("counting {}", short(&o.quant)));
        }
        if f.kind != Some(i::Noun::Card) || f.or.is_some() || f.alts.len() > 1 {
            return Err("counting other than cards by their characteristics".into());
        }
        let whose = match whose {
            i::Player::You => Whose::Yours,
            i::Player::Each => Whose::Each,
            i::Player::EachOpponent => Whose::Opponents,
            other => {
                return Err(format!(
                    "cards in the graveyard of Player::{}",
                    short(other)
                ))
            }
        };
        let mut named = None;
        for post in &f.posts {
            match post {
                Post::In(i::Zone::Graveyard(_)) => {}
                Post::Named(i::PrintedName(n)) if named.is_none() => named = Some(n.clone()),
                other => return Err(format!("counting cards {}", short(other))),
            }
        }
        let filter = match f.alts.as_slice() {
            [] => Filter::default(),
            [alt] => objects(&Objects {
                quant: Quant::All,
                filter: i::Filter {
                    alts: vec![alt.clone()],
                    ..i::Filter::default()
                },
            })?,
            _ => unreachable!("one alternative at most"),
        };
        if filter.0.iter().any(|p| {
            !matches!(
                p,
                Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_) | Pred::ManaValue(..)
            )
        }) {
            return Err("a card filter relative to its source".into());
        }
        Ok(Amount::CardsInGraveyards {
            whose,
            named,
            filter: CardFilter(vec![filter]),
        })
    })())
}

/// "All creatures", "nontoken artifacts you control": a filter over the
/// battlefield, re-read as the layers go. Targets aren't a static's.
fn objects(o: &Objects) -> Result<Filter, String> {
    // "This creature gets +1/+1": the source alone.
    if o.quant == Quant::SelfRef && o.filter == i::Filter::default() {
        return Ok(Filter(vec![Pred::Itself]));
    }
    // "Enchanted creature", "equipped creature": what the source is
    // attached to, and only while it matches the rest of the filter.
    let mut preds = match o.quant {
        Quant::All | Quant::Each | Quant::Plural => Vec::new(),
        Quant::Enchanted | Quant::Equipped => vec![Pred::AttachedTo],
        _ => return Err(format!("quantity {}", short(&o.quant))),
    };
    let f = &o.filter;
    // "Artifact or enchantment": any of single types, supertypes or
    // colours.
    if f.alts.len() > 1 {
        let any: Option<Vec<Prop>> = f
            .alts
            .iter()
            .map(|alt| match alt.as_slice() {
                [Adj::Type(t)] => card_type(t).map(Prop::Type),
                [Adj::Supertype(t)] => supertype(t).map(Prop::Supertype),
                [Adj::Color(c)] => color_prop(c),
                _ => None,
            })
            .collect();
        let Some(any) = any else {
            return Err("a filter with alternatives".into());
        };
        preds.push(Pred::IsAny(any));
        let rest = Objects {
            quant: Quant::All,
            filter: i::Filter {
                alts: vec![vec![]],
                ..f.clone()
            },
        };
        preds.extend(objects(&rest)?.0);
        return Ok(Filter(preds));
    }
    let [alt] = f.alts.as_slice() else {
        return Err("a filter with alternatives".into());
    };
    if f.kind.is_some() || f.or.is_some() {
        return Err("a filter over a kind of object".into());
    }
    // A subtype's set is the card type it is said with: "non-Aura
    // enchantment", "non-Human creature".
    let types: BTreeSet<CardType> = alt
        .iter()
        .filter_map(|a| match a {
            Adj::Type(t) => card_type(t),
            _ => None,
        })
        .collect();
    for adj in alt {
        preds.push(match adj {
            Adj::Type(t) => Pred::Is(Prop::Type(card_type(t).ok_or(format!("type {t}"))?)),
            Adj::Non(t) => Pred::IsNot(match (card_type(t), supertype(t)) {
                (Some(t), _) => Prop::Type(t),
                (None, Some(s)) => Prop::Supertype(s),
                (None, None) => Prop::Subtype(subtype(t, &types)),
            }),
            Adj::Supertype(t) => Pred::Is(Prop::Supertype(
                supertype(t).ok_or(format!("supertype {t}"))?,
            )),
            Adj::Subtype(t) => Pred::Is(Prop::Subtype(subtype(t, &types))),
            Adj::Color(c) => Pred::Is(color_prop(c).ok_or(short(c))?),
            Adj::NonColor(c) => Pred::IsNot(color_prop(c).ok_or(short(c))?),
            Adj::State(i::State::Nontoken) => Pred::Nontoken,
            Adj::State(i::State::Other) => Pred::Other,
            Adj::State(i::State::Attacking) => Pred::Attacking,
            other => return Err(short(other)),
        });
    }
    for post in &f.posts {
        preds.push(match post {
            Post::Controller(i::Player::You) => Pred::YouControl,
            // "A permanent an opponent controls" is one any of them does.
            Post::Controller(i::Player::EachOpponent | i::Player::AnOpponent) => {
                Pred::OpponentsControl
            }
            Post::With(i::Compare {
                stat: stat @ (i::Stat::Power | i::Stat::ManaValue),
                op,
                value: i::Amount::N(v),
            }) => {
                let cmp = cmp(*op);
                match stat {
                    i::Stat::Power => Pred::Power(cmp, *v),
                    _ => Pred::ManaValue(cmp, *v),
                }
            }
            // "each Locus on the battlefield": a filter over permanents
            // reads the battlefield already.
            Post::In(i::Zone::Battlefield) => continue,
            Post::OfChosenType => Pred::OfChosenType,
            other => return Err(short(other)),
        });
    }
    Ok(Filter(preds))
}

/// "Any land on the battlefield": what an object entering as a copy may
/// copy, a permanent the filter matches (CR 707.5).
pub fn copy_of(o: &Objects) -> Result<Filter, String> {
    if o.quant != Quant::One {
        return Err(format!("a copy of {}", short(&o.quant)));
    }
    objects(&Objects {
        quant: Quant::All,
        filter: o.filter.clone(),
    })
}

/// A copy effect's exception (CR 707.9), as far as the engine makes one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Except {
    /// "Except it has this ability": the ability making the copy is part
    /// of the copy's copiable values (CR 707.9a).
    HasThisAbility,
    /// "Except it's an Illusion in addition to its other types": card
    /// types and subtypes, as the type line spells them, added to the
    /// copy's (CR 707.9b).
    AlsoIs(Vec<TypeWord>),
    /// "And it has "When this creature becomes the target …"": the quoted
    /// abilities, in the copying ability's own words, added to the copy's
    /// rules text (CR 707.9a).
    Has(Vec<String>),
}

/// A word a copy's type line gains.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeWord {
    /// "Artifact", a card type, before the type line's others.
    Type(String),
    /// "Illusion", a subtype, after the dash.
    Subtype(String),
}

/// A copy effect's exceptions, each one the engine makes or refused by
/// name. `line` is the copying ability's text, which a quoted ability the
/// copy gains is taken from: without it, or with its quotes not one for
/// each such ability, one is refused.
pub fn excepts(except: &[i::Exception], line: Option<&str>) -> Result<Vec<Except>, String> {
    except
        .iter()
        .map(|e| match e {
            i::Exception::HasThisAbility => Ok(Except::HasThisAbility),
            i::Exception::AlsoIs(adjs) => adjs
                .iter()
                .map(|a| match a {
                    i::Adj::Type(t) if card_type(t).is_some() => Ok(TypeWord::Type(capitalised(t))),
                    i::Adj::Subtype(t) => Ok(TypeWord::Subtype(capitalised(t))),
                    other => Err(format!("a copy that is also {}", short(other))),
                })
                .collect::<Result<_, _>>()
                .map(Except::AlsoIs),
            i::Exception::Has {
                keywords,
                abilities,
            } if keywords.is_empty() && !abilities.is_empty() => {
                let quoted: Vec<String> = line
                    .unwrap_or_default()
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect();
                if line.is_none() {
                    return Err("a copy that has abilities, outside an ability's own line".into());
                }
                if quoted.len() != abilities.len() {
                    return Err("a copy that has abilities not quoted one for one".into());
                }
                Ok(Except::Has(quoted))
            }
            other => Err(format!("a copy except {}", short(other))),
        })
        .collect()
}

/// "a Mountain or Forest card", "a basic land card": a card outside the
/// battlefield matched by its own characteristics, any one of the
/// alternatives (CR 701.23a).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CardFilter(pub Vec<Filter>);

impl CardFilter {
    pub fn matches(&self, c: &Chars) -> bool {
        self.0.iter().any(|f| f.matches_card(c))
    }
}

/// Something that happened this turn, which an instruction asks of as it
/// is followed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThisTurn {
    /// "A nonland permanent left the battlefield this turn": one the
    /// filter matches, by its characteristics as it last existed there
    /// (CR 608.2h), left for any zone.
    PermanentLeft(Filter),
}

/// One instruction of a spell or an ability, as it resolves (CR 608.2c).
/// Each names what it acts on through the ability's own [`Who`] and
/// [`Obj`], never a guess: a target by its slot, chosen as the ability
/// was put on the stack (CR 601.2c, 603.3d).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Suspend's "remove a time counter from it", of the suspended card
    /// whose ability this is (CR 702.62a).
    RemoveTimeCounter,
    /// Suspend's "you may play it without paying its mana cost": the
    /// exiled card whose ability this is is cast (CR 702.62a, 702.62d).
    CastSuspended,
    /// "You may cast the discarded card without paying its mana cost":
    /// the card bound to the slot cast as the instruction resolves, by an
    /// alternative cost of none (CR 608.2g, 118.9). Read, but not run:
    /// reaching it refuses by name.
    CastWithoutPaying(Slot),
    /// Madness's "its owner may cast it by paying [cost] rather than
    /// paying its mana cost", of the card whose trigger this is, if it is
    /// still the card madness exiled (CR 702.35a, 608.2g).
    CastMadness,
    /// Madness's "If that player doesn't, they put this card into their
    /// graveyard" (CR 702.35a).
    MadnessToGraveyard,
    /// "Search your library for a Mountain or Forest card": `who`
    /// searches their library and may find a card the filter matches (CR
    /// 701.23a–b), bound to `into`. `enters` if an instruction after puts
    /// it onto the battlefield, so only a card the engine runs is found.
    Search {
        who: Who,
        filter: CardFilter,
        into: Slot,
        enters: bool,
        /// "Reveal it", "reveal those cards": an instruction after shows
        /// the card found (CR 701.23a, 701.20a).
        reveals: bool,
    },
    /// "Search your library for up to two basic land cards": the cards
    /// each search of one found, bound together to `into`.
    Gather {
        from: Vec<Slot>,
        into: Slot,
    },
    /// "Put it onto the battlefield (tapped)", under `who`'s control.
    ToBattlefield {
        cards: Cards,
        tapped: bool,
        who: Who,
    },
    /// Reanimate's "Put target creature card from a graveyard onto the
    /// battlefield under your control": the card targeted in that slot,
    /// if it is still there (CR 608.2b), entering under the controller of
    /// the spell or ability (CR 110.2a).
    FromGraveyardToBattlefield {
        slot: usize,
        tapped: bool,
    },
    /// "Then shuffle": a player's library (CR 701.24a).
    Shuffle(Who),
    /// "Draw two cards" (CR 121.2), the number worked out as it is
    /// followed (CR 608.2h).
    Draw {
        who: Who,
        n: Amount,
    },
    /// "Exile target player's graveyard": every card in it moved to exile
    /// (CR 701.13a, 406.2).
    ExileGraveyard(Who),
    /// "Each opponent exiles a card from their hand": each of those
    /// players, in APNAP order, chooses one card of their own hand, and
    /// then the chosen cards are exiled at once (CR 101.4, 701.13a).
    ExileFromHand(Whose),
    /// "Discard two cards": the player chooses which (CR 701.9a), bound
    /// to `into` for "each land card discarded this way"; "at random",
    /// chance does (CR 701.9b).
    Discard {
        who: Who,
        n: u32,
        into: Slot,
        random: bool,
    },
    /// "Target player reveals their hand. You choose a nonland card from
    /// it. That player discards that card." (CR 701.20a, 701.9a): the
    /// controller chooses a card the filter matches, if there is one.
    RevealChooseDiscard {
        who: Who,
        filter: CardFilter,
    },
    /// CR 119.3.
    LoseLife {
        who: Who,
        n: Amount,
    },
    GainLife {
        who: Who,
        n: Amount,
    },
    /// "Destroy target creature" (CR 701.8a): to its owner's graveyard.
    Destroy(On),
    /// "Exile target creature" (CR 701.13a).
    Exile(On),
    /// "Exile the top five cards of your library": the top `n` of
    /// `whose` library go to exile, face up (CR 406.3), bound to `into`,
    /// which "cards exiled this way" reads.
    ExileTop {
        whose: Who,
        n: u32,
        into: Slot,
    },
    /// "You may play cards exiled this way until the end of your next
    /// turn": the controller may play the cards bound to `cards` while
    /// they stay in exile (CR 400.7), a land as a land play (CR 305.2a)
    /// and a spell as its type allows (CR 117.1a), until the cleanup of
    /// their next turn (CR 514.2, 611.2a).
    MayPlayExiled {
        cards: Slot,
    },
    /// "You may play an additional land this turn": the controller's
    /// lands this turn go up by `n` (CR 305.2), ending in cleanup (CR
    /// 514.2).
    ExtraLandsThisTurn(u32),
    /// "Exile target nonland permanent an opponent controls until this
    /// Aura leaves the battlefield": exiled only while the ability's
    /// source is still on the battlefield, and returned under its owner's
    /// control as the source leaves (CR 610.3, 610.3b–c).
    ExileUntilLeaves(On),
    /// "Return target permanent to its owner's hand".
    ToOwnersHand(On),
    /// Condemn's "Put target attacking creature on the bottom of its
    /// owner's library" (CR 400.3): one target permanent.
    ToOwnersLibraryBottom(On),
    /// "Return this card from your graveyard to your hand": the
    /// ability's source, if it is still in its owner's graveyard. Such an
    /// ability functions only there (CR 113.6m, `Triggered::from_graveyard`).
    SelfFromGraveyardToHand,
    /// "Tap target land", "untap it" (CR 701.26a).
    Tap(On),
    Untap(On),
    /// Provoke's "block this creature this combat if able": a requirement
    /// on `blocker` (CR 509.1c, 702.39a) for this combat.
    MustBlock {
        blocker: Obj,
        attacker: Obj,
    },
    /// "Prevent all combat damage that would be dealt to and dealt by this
    /// creature this turn" (CR 615.1): a shield around `of` until the
    /// turn ends, on the combat damage dealt to it if `to`, by it if `by`.
    PreventCombatDamage {
        of: Obj,
        to: bool,
        by: bool,
    },
    /// "Untap up to three lands": its controller chooses as it resolves
    /// up to `n` of the tapped permanents `on` matches, and untaps them
    /// (CR 608.2d, 701.26b).
    UntapUpTo {
        n: u32,
        on: On,
    },
    /// "Sacrifice this creature" (CR 701.21a): only a permanent its
    /// controller controls.
    Sacrifice(Obj),
    /// "Sacrifice that many permanents", annihilator's "defending player
    /// sacrifices a permanent of their choice" (CR 701.21a, 702.86a): `n`
    /// of the permanents `who` controls that `filter` matches, `n` counted
    /// as it is followed, chosen by that player as it resolves.
    SacrificeChosen {
        who: Who,
        n: Amount,
        filter: Filter,
    },
    /// "Counter target spell", "counter target activated or triggered
    /// ability" (CR 701.6a): the spell or ability in this target slot.
    Counter(usize),
    /// "Create a 0/1 green Plant creature token", "create X … tokens,
    /// where X is the number of creatures you control": `n` tokens with
    /// this text, its controller's, `n` counted as it is followed (CR
    /// 111.1–111.3, 608.2h).
    /// "For each opponent, create a 1/1 … token that's tapped and
    /// attacking that player": with `attacking`, `n` tokens for each
    /// opponent still in the game, each attacking that opponent (CR
    /// 508.4).
    Create {
        n: Amount,
        token: Box<crate::deck::CardText>,
        tapped: bool,
        attacking: bool,
    },
    /// "~ deals 3 damage to any target": the source is the spell or the
    /// ability's source (CR 120.1).
    Damage {
        to: DamageTo,
        n: Amount,
    },
    /// "If any source you control would deal damage to a permanent or
    /// player this turn, it deals double that damage to that permanent or
    /// player instead" (Quest for Pure Flame): a replacement effect (CR
    /// 614.1a) on all damage a source its controller controls deals, until
    /// it ends in cleanup (CR 514.2), multiplying it by `by`.
    MultiplyDamageThisTurn {
        by: u32,
    },
    /// "Damage can't be prevented this turn" (Stomp): until cleanup (CR
    /// 514.2), prevention effects prevent no damage (CR 615.12).
    DamageCantBePreventedThisTurn,
    /// The controller scries (CR 701.22a).
    Scry(u32),
    /// "Draw a card at the beginning of the next turn's upkeep": a
    /// delayed triggered ability created as this resolves (CR 603.7a),
    /// triggering once at the next such step of a later turn (CR
    /// 603.7b). Its instructions name nothing of the spell's but its
    /// controller.
    Delayed {
        step: TurnStep,
        effects: Vec<Effect>,
    },
    /// Dark Ritual's "Add {B}{B}{B}": mana in its controller's pool (CR
    /// 106.3).
    AddMana(crate::mana::ManaPool),
    /// "Add one mana of any color": the one its controller chooses of
    /// these, as it resolves (CR 106.3, 608.2d).
    AddManaOf(Vec<crate::mana::ManaPool>),
    /// CR 122.1, 122.6: Blast Zone's X counted as it resolves.
    PutCounters {
        on: On,
        counter: Counter,
        n: Amount,
    },
    /// "Target creature gets +3/+3 until end of turn": a continuous effect
    /// on the object as it resolves (CR 611.2c), ending in cleanup (CR
    /// 514.2).
    UntilEndOfTurn {
        on: On,
        ops: Vec<Op>,
        /// The modification as many times as `Per` says.
        per: Option<Per>,
    },
    /// "Target opponent gains control of it": from now on, `who` controls
    /// the permanent (CR 613.1b), a control change that removes it from
    /// combat (CR 506.4).
    GainControl {
        who: Who,
        of: Obj,
    },
    /// "You may …": the controller chooses, as the instruction is
    /// reached, whether to follow these (CR 608.2d; a triggered ability's
    /// "may" is chosen as it resolves, CR 603.5).
    May(Who, Vec<Effect>),
    /// "Put a Goblin permanent card from your hand onto the battlefield":
    /// the controller chooses one the filter matches, if they have one.
    PutFromHand {
        filter: CardFilter,
        tapped: bool,
        /// Aether Vial's "with mana value equal to the number of charge
        /// counters on this artifact": counted from its source as the
        /// instruction is followed (CR 608.2h).
        mana_value: Option<Counter>,
    },
    /// "Each opponent may put any number of artifact, creature,
    /// enchantment, and/or land cards from their hand": each player
    /// `whose` names, relative to the controller, chooses any number of
    /// the cards of their own hand that `filter` matches, zero included
    /// (CR 107.1c), or up to `up_to` ("each player may put a … card",
    /// Show and Tell), in APNAP order (CR 101.4), all bound to `into`.
    PutFromHands {
        whose: Whose,
        filter: CardFilter,
        up_to: Option<u32>,
        into: Slot,
    },
    /// "…onto the battlefield", after `PutFromHands`: each of the cards
    /// still in its owner's hand enters under that owner's control, the
    /// player who put it (CR 110.2a), all at once (CR 101.4).
    HandsToBattlefield {
        cards: Cards,
        tapped: bool,
    },
    /// "Reveal the top card of your library": the top `n` cards of
    /// `whose` library are revealed (CR 701.20a), and bound to `into`.
    /// `blind` if nothing the instruction goes on to do reads what they
    /// are, only where they go (Fact or Fiction's piles), so a hidden
    /// card among them may stay hidden when no one says what it is.
    RevealTop {
        whose: Who,
        n: u32,
        into: Slot,
        blind: bool,
    },
    /// "Look at the top four cards of your library": bound to `into`,
    /// seen by the controller alone, so to everyone else they stay
    /// hidden (CR 400.2).
    LookAtTop {
        whose: Who,
        n: u32,
        into: Slot,
    },
    /// "Put one of them into your hand": the controller chooses `n` of
    /// the cards bound to `from`, bound to `into`.
    ChooseAmong {
        from: Slot,
        n: u32,
        into: Slot,
    },
    /// "Put up to two creature cards with total mana value 6 or less from
    /// among them": the controller chooses up to `up_to` of the cards
    /// bound to `from` that `filter` matches, whose mana values add up to
    /// no more than `total_mana_value` if it is said, bound to `into`;
    /// `enters` if an instruction after puts them onto the battlefield.
    ChooseMatching {
        from: Slot,
        up_to: u32,
        filter: CardFilter,
        total_mana_value: Option<u32>,
        enters: bool,
        /// An instruction after reveals the cards chosen ("You may reveal
        /// a Dinosaur or land card from among them").
        reveals: bool,
        into: Slot,
    },
    /// "An opponent separates those cards into two piles": an opponent
    /// of the controller puts each card bound to `from` into one of two
    /// piles (CR 700.3a), the first bound to `into`, the second the rest.
    /// Either may be empty (CR 700.3d), and the cards stay where they are
    /// (CR 700.3c).
    SeparatePiles {
        from: Slot,
        into: Slot,
    },
    /// "Put one pile into your hand": the controller chooses one of the
    /// two piles `SeparatePiles` made of `from`, the first bound to
    /// `first`, and it is bound to `into`; the other pile is the rest.
    ChoosePile {
        from: Slot,
        first: Slot,
        into: Slot,
    },
    /// "Put that card into your hand": into `who`'s hand.
    ToHand {
        cards: Cards,
        who: Who,
    },
    /// "Put them back in any order", "the rest on the bottom of your
    /// library in any order": the controller orders them (CR 401.4), or
    /// chance does, "in a random order".
    ToLibrary {
        cards: Cards,
        top: bool,
        from: FoundIn,
        order: Arranged,
    },
    /// "The rest into your graveyard": each card found goes to its
    /// owner's graveyard (CR 400.3), Strategic Planning's two.
    ToGraveyard {
        cards: Cards,
        from: FoundIn,
    },
    /// "You may reveal that card": the cards bound to `cards`, looked at
    /// by their controller alone, are revealed to everyone (CR 701.20a)
    /// and bound to `into`, which a "revealed this way" reads.
    Reveal {
        cards: Slot,
        into: Slot,
    },
    /// "Transform this creature": the ability's own permanent turns over
    /// to its other face (CR 701.27a).
    Transform,
    /// Thespian's Stage's "This land becomes a copy of target land, except
    /// it has this ability": the ability's own permanent takes the copiable
    /// values of `of` for as long as it stays (CR 707.2, 707.4, 613.2a).
    BecomeCopy {
        of: Obj,
        except: Vec<Except>,
    },
    /// "If it's a land card, …": these instructions, only if the card
    /// bound to `card` matches. `otherwise` is the "Otherwise, …" right
    /// after one (Archdruid's Charm): only if that one's card didn't.
    IfCard {
        card: Slot,
        filter: CardFilter,
        otherwise: bool,
        then: Vec<Effect>,
    },
    /// Pyroblast's "Counter target spell if it's blue": these instructions
    /// only if the target in the slot still matches the filter as they
    /// are followed, a spell by its card's characteristics and a
    /// permanent by what the layers make it.
    IfTarget {
        target: usize,
        filter: Filter,
        then: Vec<Effect>,
    },
    /// Void's "~ deals 3 damage instead if a nonland permanent left the
    /// battlefield this turn": these instructions only if what happened
    /// this turn holds as they are followed, or with `holds` false only if
    /// it doesn't, as the instruction an "instead" replaces is.
    IfThisTurn {
        cond: ThisTurn,
        holds: bool,
        then: Vec<Effect>,
    },
    /// "If this spell was kicked, …" (CR 702.33d): these instructions only
    /// if the spell resolving was kicked, or with `was` false only if it
    /// wasn't, as the instruction an "instead" replaces is.
    Kicked {
        was: bool,
        then: Vec<Effect>,
    },
    /// "If there are four or more chorus counters on ~, …", "if its
    /// mana value is 2 or less": these instructions, only if `n` compares
    /// with `than` as `cmp` says, both worked out as they are followed,
    /// of an object gone as it last existed (CR 608.2h). A negative power
    /// is compared as it is (CR 107.1b), and an amount of an illegal
    /// target can't be found, so the instructions aren't followed (CR
    /// 608.2b).
    IfAmount {
        n: Amount,
        cmp: Cmp,
        than: Amount,
        then: Vec<Effect>,
    },
    /// "Then if you control four or more lands, …": these instructions,
    /// only if the controller controls as many permanents as the condition
    /// counts as it is followed.
    IfControl {
        control: YouControl,
        then: Vec<Effect>,
    },
    /// "Sacrifice it unless you pay {R}", "Counter target spell unless its
    /// controller pays {3}", "sacrifice this creature unless you discard a
    /// card": `who` may pay the cost, and if they don't, these
    /// instructions are followed (CR 118.12a).
    Unless {
        who: Who,
        cost: UnlessCost,
        otherwise: Vec<Effect>,
    },
    /// "Attach this permanent to target creature you control", "attach up
    /// to one target Equipment you control to target attacking creature":
    /// the Equipment `what` names becomes attached to `to`, unless it
    /// already is or couldn't equip it (CR 701.3a–b, 301.5c).
    Attach {
        what: Obj,
        to: Obj,
    },
    /// "Choose two cards in your hand drawn this turn": the controller
    /// chooses `n` of the cards in their hand drawn this turn, all of them
    /// if they have no more, bound to `into`.
    ChooseDrawn {
        n: u32,
        into: Slot,
    },
    /// "For each of those cards, …" said once for each card of a set of
    /// at most `k + 1`: the `k`-th card bound to `from`, if there is one,
    /// bound to `into` (Insight's `ForEach`, unrolled).
    Nth {
        from: Slot,
        k: usize,
        into: Slot,
    },
    /// "Pay 4 life or put the card on top of your library": `who` may pay
    /// `life` if they have that much (CR 119.4), and if they don't, these
    /// instructions are followed.
    UnlessLife {
        who: Who,
        life: Amount,
        otherwise: Vec<Effect>,
    },
    /// "Choose a nonland card name": the controller names a card of the
    /// Oracle card reference the filter matches (CR 201.4, 201.4a), for
    /// the instructions after to read.
    ChooseName(CardFilter),
    /// "Target player reveals their hand" (CR 701.20a): every card of it
    /// seen, so a hidden one is named.
    RevealHand(Who),
    /// "Discards all cards with that name": each card of the player's
    /// hand with the name chosen (CR 701.9a).
    DiscardNamed(Who),
}

/// What an "unless" asks of the player it names (CR 118.12a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnlessCost {
    /// "Unless you pay {R}", "unless its controller pays {3}".
    Mana(crate::mana::ManaCost),
    /// "Unless you discard a card": one card of their hand the filter
    /// matches (CR 701.9a).
    Discard(CardFilter),
    /// "Unless you sacrifice any number of creatures with total power 12
    /// or greater": any number of permanents of theirs the filter
    /// matches, their powers adding up to at least `power` (CR 701.21a).
    SacrificePower { filter: Filter, power: u32 },
    /// "Pays {1} for each card in your graveyard", cumulative upkeep's
    /// "pay [cost] for each age counter on it": the cost once for each of
    /// what the amount counts as the instruction resolves (CR 702.24a).
    ManaTimes {
        each: crate::mana::ManaCost,
        times: Amount,
    },
}

impl Effect {
    /// The instructions this one holds, which a resolution may enter.
    pub fn inner(&self) -> Option<&[Effect]> {
        match self {
            Effect::May(_, then)
            | Effect::IfCard { then, .. }
            | Effect::IfTarget { then, .. }
            | Effect::IfControl { then, .. }
            | Effect::IfAmount { then, .. }
            | Effect::Kicked { then, .. }
            | Effect::IfThisTurn { then, .. }
            | Effect::Unless {
                otherwise: then, ..
            }
            | Effect::UnlessLife {
                otherwise: then, ..
            } => Some(then),
            _ => None,
        }
    }
}

/// A card an instruction found as it resolves, bound for those after it
/// (`Effect::With` in Insight's [binders]): its place in the resolution's
/// record of what it found, one per binder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(pub usize);

/// Where the cards an instruction moves were found: only those still
/// there are its own, one moved since being a new object (CR 400.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundIn {
    /// Revealed, looked at or searched for in a library.
    Library,
    /// Chosen from a hand ("choose two cards in your hand drawn this
    /// turn").
    Hand,
}

/// Cards a resolution found, as an instruction names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cards {
    /// "that card", "them".
    Bound(Slot),
    /// "all Goblin cards revealed this way": those bound to the slot that
    /// the filter matches.
    Matching { from: Slot, filter: CardFilter },
    /// "the rest": those bound to `from` that aren't bound to `of`.
    Rest { of: Slot, from: Slot },
}

/// A player an instruction names: its controller ("you", CR 109.5) or a
/// target player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    You,
    Target(usize),
    /// "defending player": the player the ability's source is attacking.
    Defending,
    /// "Its controller": of the object, or as it last existed (CR
    /// 608.2h).
    ControllerOf(Obj),
    /// "That player": the one the trigger event names, the caster of the
    /// spell cast.
    EventPlayer,
    /// "Its controller" of the spell in this target slot.
    SpellController(usize),
}

/// An object an instruction names: the ability's own source ("this
/// creature") or a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obj {
    Itself,
    Target(usize),
    /// The object the trigger event names: exalted's "that creature".
    Triggering,
    /// The permanent a card an instruction found became: Fabled Passage's
    /// "untap that land", put onto the battlefield.
    Found(Slot),
}

/// What an instruction acts on: one object, or each permanent a filter
/// matches as it is followed, "each other Human you control", "all
/// creatures", read from the ability's source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum On {
    One(Obj),
    Each(Filter),
    /// "Each artifact and creature with mana value equal to the number of
    /// fuse counters on this artifact": each the filter matches whose mana
    /// value compares so with the amount, counted as the instruction is
    /// followed (CR 608.2h).
    EachWithManaValue {
        filter: Filter,
        cmp: Cmp,
        n: Box<Amount>,
    },
}

/// How much: a number, or "that much", the amount of the event that
/// triggered the ability (Jackal Pup's "it deals that much damage to
/// you").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Amount {
    N(u32),
    ThatMuch,
    /// "Its mana value": of the card bound there.
    ManaValueOf(Slot),
    /// "That card's mana value": of the card in a graveyard targeted in
    /// that slot, as it last existed there (CR 608.2h).
    TargetCardManaValue(usize),
    /// "Equal to its power": of the object, or as it last existed (CR
    /// 608.2h).
    PowerOf(Obj),
    /// "Equal to its toughness", the same way.
    ToughnessOf(Obj),
    /// "If its mana value is 2 or less": of the object, or as it last
    /// existed (CR 608.2h).
    ManaValueOfObj(Obj),
    /// "Gain 3 life for each land card discarded this way": `each` for
    /// each card bound to `slot` that `filter` matches.
    PerFound {
        each: u32,
        slot: Slot,
        filter: CardFilter,
    },
    /// "The number of Goblins on the battlefield": the permanents the
    /// filter matches as it is followed, read from the ability's source
    /// (CR 608.2h).
    CountOf(Filter),
    /// "The number of cards named Accumulated Knowledge in all
    /// graveyards": the cards in the graveyards of the players `whose`
    /// names, relative to the ability's controller, that have the name if
    /// one is given and that the filter matches, as it is followed.
    CardsInGraveyards {
        whose: Whose,
        named: Option<String>,
        filter: CardFilter,
    },
    /// "The number of fuse counters on this artifact": on the ability's
    /// source, or as it last existed if it is gone, sacrificed to pay for
    /// the ability (CR 608.2h).
    CountersOnItself(Counter),
    /// "Twice the number of …".
    Times(u32, Box<Amount>),
    /// "The number of nonbasic lands that player controls", of damage
    /// dealt to each player: the permanents the filter matches that the
    /// player being dealt damage controls.
    ThatPlayerControls(Filter),
    /// The spell's X, the value announced or set as it was cast, while it
    /// is on the stack (CR 107.3a).
    X,
}

/// How many times a modification until end of turn applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Per {
    /// "+2/+0 … for each other attacking Goblin": once per permanent the
    /// filter matches, read from the ability's source and counted as it
    /// resolves (CR 608.2h).
    Each(Filter),
    /// Blazing Shoal's "+X/+0": the spell's X (CR 107.3a).
    X,
}

/// What damage is dealt to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageTo {
    /// A target, a player or a permanent as its slot allows.
    Target(usize),
    You,
    /// "the player or planeswalker it's attacking": whom the ability's
    /// source attacks as it resolves, if it still does.
    Attacked,
    /// "That player": the one the trigger event names.
    EventPlayer,
    /// "To each creature": each permanent the filter matches, dealt
    /// damage at once (CR 120.3).
    Each(Filter),
    /// "To each player": every player still in the game, at once, each
    /// the amount counted for them.
    EachPlayer,
}

/// What a target slot may be (CR 115.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSpec {
    /// "target player", "target opponent".
    Player { opponent: bool },
    /// "target creature": a permanent the filter matches, "you control"
    /// read from the controller.
    Permanent(Filter),
    /// "any target": a creature, player, planeswalker or battle (CR 115.4).
    Any,
    /// "target player or planeswalker".
    PlayerOrPlaneswalker,
    /// "target spell with mana value 2": a spell on the stack the filter
    /// matches, by its card's characteristics, other than itself (CR
    /// 115.5).
    Spell(CardFilter),
    /// "target activated or triggered ability": an ability on the stack of
    /// the kinds named (CR 113.1c), never a mana ability, which doesn't go
    /// on the stack (CR 605.3b).
    Ability { activated: bool, triggered: bool },
    /// "target card from a graveyard", "target creature card in your
    /// graveyard": a card in the graveyards `whose` names, the filter
    /// matching its own characteristics (CR 115.2: an object in another
    /// zone, as it says).
    GraveyardCard { filter: CardFilter, whose: Whose },
    /// "up to one target …": the slot's own kind of target, or none (CR
    /// 115.6, 601.2c). `Scope::slot_up_to_one` makes it, never around
    /// another.
    UpToOne(Box<TargetSpec>),
}

/// A spell's or ability's instructions and the targets they name, by slot.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Program {
    pub targets: Vec<TargetSpec>,
    pub effects: Vec<Effect>,
}

/// What a triggered ability waits for (CR 603.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerEvent {
    /// "When this creature enters", "Whenever another Human you control
    /// enters": a permanent the filter matches, read from the trigger's
    /// source, enters the battlefield (CR 603.6a).
    Enters(Filter),
    /// "Whenever this creature attacks": a creature the filter matches is
    /// declared as an attacker (CR 508.3a).
    Attacks(Filter),
    /// "Whenever you attack with one or more Lizards": its controller
    /// declares at least `n` attackers the filter matches, once however
    /// many (CR 508.3a).
    AttacksWith { filter: Filter, n: u32 },
    /// Training's "Whenever this creature and at least one other creature
    /// with power greater than this creature's power attack" (CR
    /// 702.149a).
    Trains,
    /// Exalted's "Whenever a creature you control attacks alone": the only
    /// creature declared as an attacker is one the filter matches (CR
    /// 506.5, 702.83a).
    AttacksAlone(Filter),
    /// "Whenever a player casts a spell with mana value 3 or less",
    /// prowess's "Whenever you cast a noncreature spell": a player `by`
    /// names, relative to its controller, casts a spell the filter
    /// matches, by its card's characteristics (CR 601.2i, 702.108a).
    Casts { by: CastBy, spells: CardFilter },
    /// "Whenever a source you control deals damage to an opponent": a
    /// spell or permanent its controller controls deals damage to a
    /// player, an opponent if `opponent`, once for each player dealt
    /// damage at once (CR 120.3).
    YourSourceDamages { opponent: bool },
    /// "When you cycle this card": its owner cycles it (CR 702.29a), the
    /// ability triggering from wherever the card has gone.
    CyclesItself,
    /// "When you cast this spell": its controller casts it, the ability
    /// triggering from the stack (CR 113.6k, 601.2i).
    CastsItself,
    /// Madness's "When this card is exiled this way": its owner discarded
    /// it and madness exiled it instead (CR 702.35a), the ability
    /// triggering from exile. Only that discard fires it.
    ExiledByMadness,
    /// "Whenever this land becomes tapped": a permanent the filter
    /// matches becomes tapped, for mana, a cost or anything else.
    BecomesTapped(Filter),
    /// "Whenever an opponent draws a card": a player `by` names, relative
    /// to its controller, draws a card, once for each (CR 121.2), "that
    /// player" the one who drew.
    Draws { by: CastBy },
    /// "Whenever this creature is dealt damage": a permanent the filter
    /// matches is dealt damage, the amount the event's (CR 120.3).
    DealtDamage(Filter),
    /// "Whenever this creature deals damage to a player", "… deals combat
    /// damage to a player": a permanent the filter matches deals damage to
    /// a player, once for each player dealt damage at once (CR 120.3,
    /// 510.2), only combat damage if `combat`, only to an opponent of its
    /// controller if `opponent` ("… to an opponent"). That player is the
    /// event's.
    DealsDamageToPlayer {
        what: Filter,
        combat: bool,
        opponent: bool,
    },
    /// "Whenever this creature deals damage": a permanent the filter
    /// matches deals damage to anything, the amount all it dealt at once
    /// (CR 120.3, 510.2).
    DealsDamage(Filter),
    /// "When this creature is turned face up": a permanent the filter
    /// matches is turned face up (CR 702.37e).
    TurnedFaceUp(Filter),
    /// "When this creature becomes the target of a spell or ability": a
    /// permanent the filter matches becomes a target of a spell or
    /// ability anyone controls, once however many of its slots name it
    /// (CR 603.2c); `spell`, of a spell only (Bonecrusher Giant), whose
    /// controller the event names.
    BecomesTarget { filter: Filter, spell: bool },
    /// "When this creature transforms into ~": a permanent the filter
    /// matches transforms, the ability on the face it transformed into
    /// (CR 701.27e).
    TransformsInto(Filter),
    /// "At the beginning of your upkeep": a step begins in a turn whose
    /// active player `whose` names, relative to the ability's controller
    /// (CR 503.1a, 504.2, 513.1a).
    Begins { step: TurnStep, whose: Whose },
    /// "When this creature dies", "Whenever another creature you control
    /// dies": a creature the filter matches, as it last existed, is put
    /// into a graveyard from the battlefield (CR 700.4, 603.6c, 603.10a).
    Dies(Filter),
    /// "Whenever this creature blocks": a creature the filter matches is
    /// declared as a blocker, once however many it blocks (CR 509.3a).
    Blocks(Filter),
    /// "Whenever this creature becomes blocked": an attacking creature the
    /// filter matches becomes blocked as blockers are declared, once
    /// however many block it (CR 509.1h, 509.3c).
    BecomesBlocked(Filter),
    /// "When this creature enters or dies": any of these, once for an
    /// event that is more than one of them (CR 603.2c).
    Or(Vec<TriggerEvent>),
}

impl TriggerEvent {
    /// The events it triggers on, each of an "or".
    pub fn parts(&self) -> Vec<&TriggerEvent> {
        match self {
            TriggerEvent::Or(v) => v.iter().flat_map(|e| e.parts()).collect(),
            e => vec![e],
        }
    }
}

/// The steps a triggered ability can wait for the beginning of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStep {
    Upkeep,
    Draw,
    BeginningOfCombat,
    /// "At the beginning of your second main phase": the turn's second
    /// main phase (CR 505.1b). The engine makes no additional combat
    /// phase, so it is the postcombat main phase (CR 505.1a).
    SecondMain,
    End,
}

/// Whose turn a step is in, for "at the beginning of your upkeep", and
/// whose graveyards are counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    Yours,
    Each,
    Opponents,
}

/// A triggered ability, lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Triggered {
    pub event: TriggerEvent,
    pub program: Program,
    /// An "intervening if" clause (CR 603.4): checked as the event
    /// happens, the ability triggering only if it holds, and again as it
    /// resolves, doing nothing if it doesn't.
    pub condition: Option<Intervening>,
    /// "Whenever you cast a spell during an opponent's turn": whose turn the
    /// event must happen in. Part of the trigger event (CR 603.2), checked
    /// as it happens and never again, unlike an intervening "if" (CR 603.4).
    pub during: Option<During>,
}

/// Whose turn a trigger event says it happens in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum During {
    /// "during your turn".
    YourTurn,
    /// "during an opponent's turn", "while it's not your turn": the active
    /// player isn't you (CR 102.1). The engine plays no game between teams,
    /// so every other player is an opponent (CR 102.2, and 102.3 once there
    /// are teams, which would make the two different).
    NotYourTurn,
}

impl Triggered {
    /// Whether it functions only in its card's owner's graveyard, its
    /// effect moving the card out of there (CR 113.6m: Squee's "you may
    /// return this card from your graveyard to your hand"), and never on
    /// the battlefield.
    pub fn from_graveyard(&self) -> bool {
        fn moves(effects: &[Effect]) -> bool {
            effects.iter().any(|e| match e {
                Effect::SelfFromGraveyardToHand => true,
                Effect::May(_, then) => moves(then),
                _ => false,
            })
        }
        moves(&self.program.effects)
    }
}

/// An intervening "if" (CR 603.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intervening {
    /// "If you control at least five other Mountains": as many permanents
    /// its controller controls as the condition counts, read from the
    /// ability's source, and "other" than the object its event names when
    /// `excluding` (Valakut's "a Mountain … enters, if you control five
    /// other Mountains").
    Control {
        control: YouControl,
        excluding: bool,
    },
    /// Echo's "if this permanent came under your control since the
    /// beginning of your last upkeep" (CR 702.30a).
    ArrivedSinceLastUpkeep,
    /// Cumulative upkeep's "if this permanent is on the battlefield" (CR
    /// 702.24a).
    OnBattlefield,
    /// "If this creature is tapped": its source is a permanent, tapped
    /// (CR 110.5).
    Tapped,
    /// Evoke's "if its evoke cost was paid": the permanent was the spell
    /// cast for it (CR 702.74a).
    Evoked,
}

/// A spell's instructions, its paragraphs in order (CR 608.2c), with the
/// `Let` that binds its targets, if any.
pub fn spell(bind: &[Objects], effects: &[i::Effect]) -> Result<Program, String> {
    let mut scope = Scope::new(bind);
    scope.cost_x = true;
    let lowered = scope.effects(effects)?;
    scope.program.effects = lowered;
    Ok(scope.program)
}

/// A modal spell's "Choose one —" (CR 700.2): each mode's instructions,
/// lowered apart with targets of its own (CR 700.2c), or the node one
/// can't be lowered at, so a spell can be cast with the modes the engine
/// runs. `None` if the spell isn't modal.
pub fn modes(
    bind: &[Objects],
    effects: &[i::Effect],
) -> Option<Result<Vec<Result<Program, String>>, String>> {
    // Archdruid's Charm's "Search your library for a creature or land
    // card": Insight binds what a mode finds in a `With` around the whole
    // `Modal`, the effect it is said in. A `With` does nothing itself, so
    // it is that `With` around each mode that refers to it; in the others
    // its binder is in scope bound to nothing, which no instruction reads.
    let (found, modal) = match effects {
        [i::Effect::With { bind: found, then }] => (Some(found), then.as_slice()),
        _ => (None, effects),
    };
    let [i::Effect::Modal { choose, modes }] = modal else {
        return None;
    };
    if !matches!(choose, i::Choose::One) {
        return Some(Err(format!("a modal spell choosing {}", short(choose))));
    }
    let Some(found) = found else {
        return Some(Ok(modes.iter().map(|m| spell(bind, m)).collect()));
    };
    Some(Ok(modes
        .iter()
        .map(|m| {
            let wrapped = vec![i::Effect::With {
                bind: found.clone(),
                then: m.clone(),
            }];
            let tree = i::Ability::Let {
                bind: bind.to_vec(),
                ability: Box::new(i::Ability::Spell(wrapped.clone())),
            };
            let ours = bind.len()..bind.len() + found.len();
            let reads = vivisurgeons_insight::binders::bindings(&tree)
                .refs
                .values()
                .flatten()
                .any(|b| b.is_some_and(|b| ours.contains(&b)));
            if reads {
                return spell(bind, &wrapped);
            }
            let mut scope = Scope::new(bind);
            scope.cost_x = true;
            scope.withs.extend(std::iter::repeat_n(None, found.len()));
            let lowered = scope.effects(m)?;
            scope.program.effects = lowered;
            Ok(scope.program)
        })
        .collect()))
}

/// A triggered ability (CR 603), with the `Let` that binds its targets and
/// its event's objects.
pub fn triggered(
    bind: &[Objects],
    trigger: &i::Trigger,
    effect: &[i::Effect],
    limits: &[i::Limit],
) -> Result<Triggered, String> {
    if let Some(l) = limits.first() {
        return Err(format!("a triggered ability limited by {}", short(l)));
    }
    let (trigger, condition) = match trigger {
        i::Trigger::InterveningIf(t, cond) => (&**t, Some(intervening(cond, bind)?)),
        t => (t, None),
    };
    // "Whenever a Mountain you control enters": the object a `Let` binds.
    let bound = |o: &Objects| -> Objects {
        match o.quant {
            Quant::Introduced(k) | Quant::Bound(k) if o.filter == i::Filter::default() => bind
                .len()
                .checked_sub(1 + k as usize)
                .map_or_else(|| o.clone(), |b| bind[b].clone()),
            _ => o.clone(),
        }
    };
    // "Whenever you cast a spell during an opponent's turn".
    let (trigger, during) = match trigger {
        i::Trigger::When(e) => match &**e {
            i::Event::While { event, cond } => {
                let during = match &**cond {
                    i::Condition::ItsYourTurn => During::YourTurn,
                    i::Condition::OpponentsTurn | i::Condition::NotYourTurn => During::NotYourTurn,
                    c => return Err(format!("Trigger::When(Event::While({}))", short(c))),
                };
                (i::Trigger::When(event.clone()), Some(during))
            }
            _ => (trigger.clone(), None),
        },
        t => (t.clone(), None),
    };
    let trigger = &trigger;
    let event = match trigger {
        i::Trigger::When(e) => when(e, &bound)?,
        i::Trigger::At(step) => {
            let (step, whose) = match step {
                i::Step::Upkeep(w) => (TurnStep::Upkeep, w),
                i::Step::DrawStep(w) => (TurnStep::Draw, w),
                i::Step::Combat(w) => (TurnStep::BeginningOfCombat, w),
                i::Step::SecondMain(w) => (TurnStep::SecondMain, w),
                i::Step::EndStep(w) => (TurnStep::End, w),
                _ => return Err(trigger_node(trigger)),
            };
            let whose = match whose {
                i::Whose::Yours => Whose::Yours,
                i::Whose::Each => Whose::Each,
                i::Whose::EachOpponent => Whose::Opponents,
                _ => return Err(trigger_node(trigger)),
            };
            TriggerEvent::Begins { step, whose }
        }
        _ => return Err(trigger_node(trigger)),
    };
    // A creature's own death and a card's own cycling trigger off the
    // battlefield, where nothing reads whose turn it is yet.
    if during.is_some()
        && event.parts().iter().any(|e| {
            matches!(
                e,
                TriggerEvent::Dies(_) | TriggerEvent::CyclesItself | TriggerEvent::CastsItself
            )
        })
    {
        return Err("Trigger::When(Event::While) of a trigger off the battlefield".into());
    }
    // A spell's own cast trigger is looked for apart from the
    // battlefield's, where its intervening "if" isn't read.
    if condition.is_some()
        && event
            .parts()
            .iter()
            .any(|e| matches!(e, TriggerEvent::CastsItself))
    {
        return Err("an intervening if of a trigger on casting this spell".into());
    }
    let mut scope = Scope::new(bind);
    scope.amount = matches!(
        event,
        TriggerEvent::DealtDamage(_) | TriggerEvent::DealsDamage(_)
    );
    scope.caster = matches!(
        event,
        TriggerEvent::Casts { .. }
            | TriggerEvent::Draws { .. }
            | TriggerEvent::DealsDamageToPlayer { .. }
            | TriggerEvent::BecomesTarget { spell: true, .. }
    );
    // "That spell's controller": the spell whose target it became, which
    // the event names with its controller.
    if let (TriggerEvent::BecomesTarget { spell: true, .. }, i::Trigger::When(e)) =
        (&event, trigger)
    {
        if let i::Event::BecomesTarget { of: Some(of), .. } = &**e {
            scope.event_spell = binder(of, bind);
        }
    }
    let lowered = scope.effects(effect)?;
    scope.program.effects = lowered;
    Ok(Triggered {
        event,
        program: scope.program,
        condition,
        during,
    })
}

/// "A spell", unqualified.
fn any_spell() -> Objects {
    Objects {
        quant: Quant::One,
        filter: i::Filter {
            kind: Some(i::Noun::Spell),
            ..Default::default()
        },
    }
}

/// What a "when" trigger's event lowers to, an "or" to each of its
/// events (CR 603.2).
fn when(e: &i::Event, bound: &dyn Fn(&Objects) -> Objects) -> Result<TriggerEvent, String> {
    Ok(match e {
        // "Whenever this creature blocks or becomes blocked": one ability
        // that triggers on either event, once if both happen at once.
        i::Event::Or(events) => TriggerEvent::Or(
            events
                .iter()
                .map(|e| when(e, bound))
                .collect::<Result<_, _>>()?,
        ),
        i::Event::Enters {
            what,
            under: None,
            state: None,
        } => TriggerEvent::Enters(entering(&bound(what))?),
        i::Event::Attacks(what) => TriggerEvent::Attacks(entering(what)?),
        i::Event::AttacksWith {
            who: i::Player::You,
            what,
            at: None,
        } => TriggerEvent::AttacksWith {
            filter: objects(&Objects {
                quant: Quant::All,
                filter: what.filter.clone(),
            })?,
            n: match what.quant {
                Quant::One => 1,
                Quant::AtLeast(i::Amount::N(n)) => n,
                ref q => return Err(format!("attacking with {}", short(q))),
            },
        },
        // CR 508.3d: "whenever you attack" is one or more of its
        // controller's creatures declared as attackers.
        i::Event::YouAttack => TriggerEvent::AttacksWith {
            filter: Filter(Vec::new()),
            n: 1,
        },
        i::Event::IsDealtDamage(what) => TriggerEvent::DealtDamage(entering(what)?),
        i::Event::DealsDamage(what) => TriggerEvent::DealsDamage(entering(what)?),
        i::Event::TurnedFaceUp(what) => TriggerEvent::TurnedFaceUp(entering(what)?),
        i::Event::TransformsInto { what, into }
            if into.quant == Quant::SelfRef && into.filter == i::Filter::default() =>
        {
            TriggerEvent::TransformsInto(entering(what)?)
        }
        i::Event::BecomesTapped(what) => TriggerEvent::BecomesTapped(entering(what)?),
        i::Event::BecomesTarget {
            what,
            of:
                Some(Objects {
                    quant: Quant::One,
                    filter,
                }),
        } if *filter
            == (i::Filter {
                kind: Some(i::Noun::SpellOrAbility),
                ..Default::default()
            }) =>
        {
            TriggerEvent::BecomesTarget {
                filter: entering(what)?,
                spell: false,
            }
        }
        // "Whenever this creature becomes the target of a spell": the
        // spell a `Let` binds, of any kind.
        i::Event::BecomesTarget { what, of: Some(of) } if bound(of) == any_spell() => {
            TriggerEvent::BecomesTarget {
                filter: entering(what)?,
                spell: true,
            }
        }
        i::Event::Cycles {
            who: i::Player::You,
            what: Some(o),
        } if itself(o) => TriggerEvent::CyclesItself,
        i::Event::Draws(who) => TriggerEvent::Draws {
            by: match who {
                i::Player::You => CastBy::You,
                i::Player::APlayer | i::Player::Each => CastBy::Anyone,
                i::Player::AnOpponent | i::Player::EachOpponent => CastBy::Opponents,
                _ => return Err(format!("Trigger::When(Event::{})", short(e))),
            },
        },
        i::Event::Dies(what) => TriggerEvent::Dies(entering(what)?),
        i::Event::Blocks(what) => TriggerEvent::Blocks(entering(what)?),
        i::Event::BecomesBlocked(what) => TriggerEvent::BecomesBlocked(entering(what)?),
        i::Event::DealsDamageTo { what, to }
            if matches!(
                to.as_slice(),
                [i::Recipient::Player(
                    i::Player::APlayer | i::Player::AnOpponent
                )]
            ) && what.filter.kind != Some(i::Noun::Source) =>
        {
            TriggerEvent::DealsDamageToPlayer {
                what: entering(what)?,
                combat: false,
                opponent: matches!(to.as_slice(), [i::Recipient::Player(i::Player::AnOpponent)]),
            }
        }
        i::Event::DealsDamageTo { what, to }
            if what.quant == Quant::One
                && what.filter
                    == (i::Filter {
                        kind: Some(i::Noun::Source),
                        posts: vec![Post::Controller(i::Player::You)],
                        ..i::Filter::default()
                    }) =>
        {
            TriggerEvent::YourSourceDamages {
                opponent: match to.as_slice() {
                    [i::Recipient::Player(i::Player::AnOpponent)] => true,
                    [i::Recipient::Player(i::Player::APlayer)] => false,
                    _ => return Err(format!("Trigger::When(Event::{})", short(e))),
                },
            }
        }
        i::Event::DealsCombatDamageToPlayer(what) => TriggerEvent::DealsDamageToPlayer {
            what: entering(what)?,
            combat: true,
            opponent: false,
        },
        i::Event::Casts {
            who: i::Player::You,
            what,
        } if itself(what) => TriggerEvent::CastsItself,
        i::Event::Casts { who, what } if what.quant != Quant::SelfRef => TriggerEvent::Casts {
            by: match who {
                i::Player::You => CastBy::You,
                i::Player::APlayer | i::Player::Each => CastBy::Anyone,
                i::Player::AnOpponent | i::Player::EachOpponent => CastBy::Opponents,
                _ => return Err(format!("Trigger::When(Event::{})", short(e))),
            },
            spells: spell_filter(what)?,
        },
        _ => return Err(format!("Trigger::When(Event::{})", short(e))),
    })
}

/// "Enchant land", "enchant creature you control" (CR 702.5a): what an
/// Aura spell may target and the Aura may enchant, `None` if the keyword
/// isn't enchant, and the node it can't lower if it enchants a player
/// (CR 702.5d) or a kind of object the filter can't say.
pub fn enchant(k: &i::Keyword) -> Option<Result<Filter, String>> {
    if !k.name.eq_ignore_ascii_case("enchant") {
        return None;
    }
    Some(match &k.param {
        Some(i::KeywordParam::Objects(o)) if not_a_player(o) => objects(&Objects {
            quant: Quant::All,
            filter: o.filter.clone(),
        }),
        _ => Err("enchant other than a permanent".into()),
    })
}

fn not_a_player(o: &Objects) -> bool {
    !matches!(o.filter.kind, Some(i::Noun::Player | i::Noun::Opponent))
}

/// A triggered mana ability (CR 605.1b): "Whenever enchanted land is
/// tapped for mana, its controller adds an additional {G}". It resolves
/// at once with the mana ability that triggers it (CR 605.4a), so the
/// mana is that activation's, its controller the tapped permanent's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManaTrigger {
    /// The permanent tapped for mana (CR 106.12a), read from the trigger's
    /// source.
    pub tapped: Filter,
    pub adds: crate::mana::ManaPool,
}

/// `None` if the ability doesn't trigger on a permanent tapped for mana.
pub fn mana_trigger(
    bind: &[Objects],
    trigger: &i::Trigger,
    effect: &[i::Effect],
    limits: &[i::Limit],
) -> Option<Result<ManaTrigger, String>> {
    let i::Trigger::When(e) = trigger else {
        return None;
    };
    let i::Event::TappedForMana(what) = &**e else {
        return None;
    };
    let bound = |o: &Objects| -> Option<Objects> {
        match o.quant {
            Quant::Introduced(k) | Quant::Bound(k) if o.filter == i::Filter::default() => bind
                .len()
                .checked_sub(1 + k as usize)
                .map(|b| bind[b].clone()),
            _ => None,
        }
    };
    Some((|| {
        if !limits.is_empty() {
            return Err("a triggered mana ability with a limit".to_string());
        }
        let what = bound(what).unwrap_or_else(|| what.clone());
        let tapped = match what.quant {
            Quant::Enchanted | Quant::Equipped => objects(&what)?,
            _ => entering(&what)?,
        };
        // "its controller adds": the controller of the permanent tapped,
        // the trigger's object.
        let [i::Effect::Performed {
            by: i::Player::ControllerOf(by),
            effect,
        }] = effect
        else {
            return Err("a triggered mana ability's effect".into());
        };
        if bound(by).is_none() {
            return Err("mana added to another player".into());
        }
        let i::Effect::AddMana(i::ManaAdd::Additional(add)) = &**effect else {
            return Err("a triggered mana ability adding other than additional mana".into());
        };
        match crate::mana::adds(add)? {
            crate::mana::Adds::Options(o) if o.len() == 1 => Ok(ManaTrigger { tapped, adds: o[0] }),
            _ => Err("a triggered mana ability adding a choice of mana".into()),
        }
    })())
}

/// An intervening "if" a triggered ability's trigger condition has (CR
/// 603.4), of those lowered: "if you control at least five other
/// Mountains", "other" than the object a `Let` binds being the event's.
fn intervening(cond: &i::Condition, bind: &[Objects]) -> Result<Intervening, String> {
    if let i::Condition::Is { what, filter } = cond {
        let tapped = i::Filter {
            alts: vec![vec![Adj::State(i::State::Tapped)]],
            ..i::Filter::default()
        };
        if what.quant == Quant::SelfRef && what.filter == i::Filter::default() && *filter == tapped
        {
            return Ok(Intervening::Tapped);
        }
    }
    let i::Condition::YouControl(o) = cond else {
        return Err(format!("an intervening if of {}", short(cond)));
    };
    let mut excluding = false;
    let mut o = o.clone();
    for alt in &mut o.filter.alts {
        alt.retain(|a| match a {
            Adj::OtherThan(x)
                if matches!(x.quant, Quant::Bound(k) if (k as usize) < bind.len())
                    && x.filter == i::Filter::default() =>
            {
                excluding = true;
                false
            }
            _ => true,
        });
    }
    Ok(Intervening::Control {
        control: you_control(&o)?,
        excluding,
    })
}

/// The keywords that are triggered abilities and that the engine runs,
/// written out as their rules define them: exalted (CR 702.83a), prowess
/// (CR 702.108a), training (CR 702.149a), provoke (CR 702.39a), annihilator
/// (CR 702.86a), and echo and cumulative upkeep
/// for mana (CR 702.30a, 702.24a). `None` for any other keyword.
pub fn keyword_trigger(k: &i::Keyword) -> Option<Triggered> {
    if k.name.eq_ignore_ascii_case("cumulative upkeep") {
        let Some(i::KeywordParam::Cost(cost)) = &k.param else {
            return None;
        };
        let [i::Cost::Mana(m)] = cost.as_slice() else {
            return None;
        };
        let age = Counter::named("age");
        return Some(Triggered {
            event: TriggerEvent::Begins {
                step: TurnStep::Upkeep,
                whose: Whose::Yours,
            },
            program: Program {
                targets: Vec::new(),
                effects: vec![
                    Effect::PutCounters {
                        on: On::One(Obj::Itself),
                        counter: age.clone(),
                        n: Amount::N(1),
                    },
                    Effect::Unless {
                        who: Who::You,
                        cost: UnlessCost::ManaTimes {
                            each: crate::mana::ManaCost::from_symbols(m).ok()?,
                            times: Amount::CountersOnItself(age),
                        },
                        otherwise: vec![Effect::Sacrifice(Obj::Itself)],
                    },
                ],
            },
            condition: Some(Intervening::OnBattlefield),
            during: None,
        });
    }
    if k.name.eq_ignore_ascii_case("echo") {
        let Some(i::KeywordParam::Cost(cost)) = &k.param else {
            return None;
        };
        let [i::Cost::Mana(m)] = cost.as_slice() else {
            return None;
        };
        return Some(Triggered {
            event: TriggerEvent::Begins {
                step: TurnStep::Upkeep,
                whose: Whose::Yours,
            },
            program: Program {
                targets: Vec::new(),
                effects: vec![Effect::Unless {
                    who: Who::You,
                    cost: UnlessCost::Mana(crate::mana::ManaCost::from_symbols(m).ok()?),
                    otherwise: vec![Effect::Sacrifice(Obj::Itself)],
                }],
            },
            condition: Some(Intervening::ArrivedSinceLastUpkeep),
            during: None,
        });
    }
    // CR 702.86a: "Whenever this creature attacks, defending player
    // sacrifices N permanents."
    if k.name.eq_ignore_ascii_case("annihilator") {
        let Some(i::KeywordParam::N(i::Amount::N(n))) = &k.param else {
            return None;
        };
        return Some(Triggered {
            event: TriggerEvent::Attacks(Filter(vec![Pred::Itself])),
            program: Program {
                targets: Vec::new(),
                effects: vec![Effect::SacrificeChosen {
                    who: Who::Defending,
                    n: Amount::N(*n),
                    filter: Filter(Vec::new()),
                }],
            },
            condition: None,
            during: None,
        });
    }
    if k.param.is_some() {
        return None;
    }
    let plus_one = vec![Op::ModifyPt(pt(1, 1))];
    match k.name.to_lowercase().as_str() {
        "exalted" => Some(Triggered {
            event: TriggerEvent::AttacksAlone(Filter(vec![Pred::YouControl])),
            program: Program {
                targets: Vec::new(),
                effects: vec![Effect::UntilEndOfTurn {
                    on: On::One(Obj::Triggering),
                    ops: plus_one,
                    per: None,
                }],
            },
            condition: None,
            during: None,
        }),
        // CR 702.39a: "Whenever this creature attacks, you may choose to
        // have target creature defending player controls block this
        // creature this combat if able. If you do, untap that creature."
        "provoke" => Some(Triggered {
            event: TriggerEvent::Attacks(Filter(vec![Pred::Itself])),
            program: Program {
                targets: vec![TargetSpec::Permanent(Filter(vec![
                    Pred::Is(Prop::Type(CardType::Creature)),
                    Pred::DefendingControls,
                ]))],
                effects: vec![Effect::May(
                    Who::You,
                    vec![
                        Effect::MustBlock {
                            blocker: Obj::Target(0),
                            attacker: Obj::Itself,
                        },
                        Effect::Untap(On::One(Obj::Target(0))),
                    ],
                )],
            },
            condition: None,
            during: None,
        }),
        "training" => Some(Triggered {
            event: TriggerEvent::Trains,
            program: Program {
                targets: Vec::new(),
                effects: vec![Effect::PutCounters {
                    on: On::One(Obj::Itself),
                    counter: Counter::PlusOne,
                    n: Amount::N(1),
                }],
            },
            condition: None,
            during: None,
        }),
        "prowess" => Some(Triggered {
            event: TriggerEvent::Casts {
                by: CastBy::You,
                spells: CardFilter(vec![Filter(vec![Pred::IsNot(Prop::Type(
                    CardType::Creature,
                ))])]),
            },
            program: Program {
                targets: Vec::new(),
                effects: vec![Effect::UntilEndOfTurn {
                    on: On::One(Obj::Itself),
                    ops: plus_one,
                    per: None,
                }],
            },
            condition: None,
            during: None,
        }),
        _ => None,
    }
}

/// "this creature", "another Human you control", "a creature": what an
/// enters trigger matches, read from its source.
fn entering(o: &Objects) -> Result<Filter, String> {
    let all = |o: &Objects| {
        objects(&Objects {
            quant: Quant::All,
            filter: o.filter.clone(),
        })
    };
    match o.quant {
        Quant::SelfRef if o.filter == i::Filter::default() => Ok(Filter(vec![Pred::Itself])),
        Quant::Another => {
            let mut f = all(o)?;
            f.0.insert(0, Pred::Other);
            Ok(f)
        }
        Quant::One => all(o),
        ref q => Err(format!("an enters trigger on {}", short(q))),
    }
}

/// The binders and target slots of the ability being lowered.
struct Scope<'a> {
    /// The `Let`'s binders, outermost first.
    bind: &'a [Objects],
    /// The slot each target binder became, by binder.
    bound: Vec<Option<usize>>,
    /// The one target player the ability names: "target player … that
    /// player".
    player: Option<usize>,
    /// Whether the trigger event has an amount "that much" names.
    amount: bool,
    /// Whether the instructions being lowered are a "may"'s, followed
    /// only if its player chose to.
    in_may: bool,
    /// Whether the trigger event names a player, a spell's caster, whom
    /// "that player" names.
    caster: bool,
    /// The binder of the spell whose target the trigger's permanent
    /// became, "that spell".
    event_spell: Option<usize>,
    program: Program,
    /// The `With` binders in scope, outermost first: the slot each binds,
    /// or `None` for one nothing the engine runs reads.
    withs: Vec<Option<Slot>>,
    /// Slots made so far.
    slots: usize,
    /// The slot a `Made` binder waits for its introducing instruction to
    /// fill.
    made: Option<Slot>,
    /// Whose library each slot's cards came from, and the slot a choice
    /// among cards chose from.
    whose: BTreeMap<Slot, Who>,
    among: BTreeMap<Slot, Slot>,
    /// The most cards a slot can hold, where an instruction fixes it.
    sizes: BTreeMap<Slot, u32>,
    /// The slots whose cards were chosen from a hand, not a library.
    in_hand: BTreeSet<Slot>,
    /// What a search looks for, by the slot of the card it finds, and
    /// whether that card is put onto the battlefield.
    described: BTreeMap<Slot, (Objects, bool)>,
    /// The described slots a search finds a card for.
    searched: BTreeSet<Slot>,
    /// The slots whose cards an instruction after the search reveals.
    revealing: BTreeSet<Slot>,
    /// The slots of cards looked at, which a reveal shows everyone.
    looked: BTreeSet<Slot>,
    /// The slot the last cards revealed are bound to, "revealed this
    /// way".
    revealed: Option<Slot>,
    /// Whether an instruction before has chosen a card name, "that name".
    named: bool,
    /// What X is, "where X is the number of Goblins on the battlefield".
    x: Option<Amount>,
    /// The slot the cards an instruction before discarded are bound to,
    /// "discarded this way".
    discarded: Option<Slot>,
    /// The slot the cards an instruction before exiled from the top of a
    /// library are bound to, "exiled this way".
    exiled: Option<Slot>,
    /// The cards an instruction before separated into two piles, and the
    /// slot of the first pile, which "one pile" chooses between.
    piles: Option<(Slot, Slot)>,
    /// Whether a cost sets these instructions' X (CR 107.3a): a spell's,
    /// or an activated ability's with {X} in its activation cost.
    cost_x: bool,
}

impl<'a> Scope<'a> {
    fn new(bind: &'a [Objects]) -> Scope<'a> {
        Scope {
            bind,
            bound: vec![None; bind.len()],
            player: None,
            amount: false,
            in_may: false,
            caster: false,
            event_spell: None,
            program: Program::default(),
            withs: Vec::new(),
            slots: 0,
            made: None,
            whose: BTreeMap::new(),
            among: BTreeMap::new(),
            sizes: BTreeMap::new(),
            in_hand: BTreeSet::new(),
            described: BTreeMap::new(),
            searched: BTreeSet::new(),
            revealing: BTreeSet::new(),
            looked: BTreeSet::new(),
            revealed: None,
            named: false,
            x: None,
            discarded: None,
            exiled: None,
            piles: None,
            cost_x: false,
        }
    }

    fn new_slot(&mut self) -> Slot {
        self.slots += 1;
        Slot(self.slots - 1)
    }

    /// The slot an introducing instruction fills: the `Made` binder
    /// waiting for it, or one nothing reads.
    fn made(&mut self) -> Slot {
        match self.made.take() {
            Some(s) => s,
            None => self.new_slot(),
        }
    }

    /// The `With` binder a reference names, if it names one.
    fn found(&self, i: u32) -> Option<Result<Slot, String>> {
        let i = i as usize;
        (i < self.withs.len()).then(|| {
            self.withs[self.withs.len() - 1 - i]
                .ok_or_else(|| "a reference to an object the engine doesn't bind".into())
        })
    }

    /// "That card", "them", "the rest": cards found as the ability
    /// resolves.
    fn cards(&self, o: &Objects) -> Result<Cards, String> {
        match o.quant {
            Quant::Bound(i) | Quant::Introduced(i) => match self.found(i.into()) {
                Some(slot) => Ok(Cards::Bound(slot?)),
                None => Err("a card bound as the ability was put on the stack".into()),
            },
            // "the rest" of cards found and not chosen among, which are
            // those still where they were found: an instruction before
            // it moved the others (Goblin Ringleader's).
            Quant::Rest { of } => {
                let of = self.found(of.into()).ok_or("the rest of nothing found")??;
                match self.among.get(&of) {
                    Some(from) => Ok(Cards::Rest { of, from: *from }),
                    None => Ok(Cards::Bound(of)),
                }
            }
            // "all Goblin cards revealed this way": of the innermost cards
            // found.
            Quant::All
                if matches!(
                    o.filter.posts.as_slice(),
                    [Post::Marked(i::Marked::Revealed)]
                ) =>
            {
                let from = self
                    .withs
                    .last()
                    .copied()
                    .flatten()
                    .ok_or("cards revealed by nothing the engine binds")?;
                let filter = card_filter(&Objects {
                    quant: Quant::One,
                    filter: i::Filter {
                        posts: Vec::new(),
                        ..o.filter.clone()
                    },
                })?;
                Ok(Cards::Matching { from, filter })
            }
            ref q => Err(format!("cards {}", short(q))),
        }
    }

    /// The player whose library a revealing or looking instruction reads.
    fn library_of(&mut self, who: &i::Player, whose: &i::Player) -> Result<Who, String> {
        match (who, whose) {
            (i::Player::You, i::Player::You) => Ok(Who::You),
            (i::Player::You, p @ i::Player::Target) => self.who(p),
            (i::Player::DefendingPlayer, i::Player::That) => Ok(Who::Defending),
            _ => Err(format!(
                "a library of {} looked at by {}",
                short(whose),
                short(who)
            )),
        }
    }

    fn slot(&mut self, spec: TargetSpec) -> usize {
        self.program.targets.push(spec);
        self.program.targets.len() - 1
    }

    /// A target binder's slot, made the first time it is named. The
    /// is how many `With` binders are in scope inside the `Let`'s.
    fn binder(&mut self, i: u32) -> Result<usize, String> {
        let i = (i as usize)
            .checked_sub(self.withs.len())
            .ok_or("a reference to an object found as it resolves")?;
        let b = self
            .bind
            .len()
            .checked_sub(1 + i)
            .ok_or("a reference out of scope")?;
        if let Some(s) = self.bound[b] {
            if matches!(self.program.targets[s], TargetSpec::GraveyardCard { .. }) {
                return Err("a target card in a graveyard read as a permanent".into());
            }
            return Ok(s);
        }
        let o = &self.bind[b];
        let spec = match o.quant {
            Quant::Target {
                n: i::Amount::N(1),
                up_to: false,
            } if o.filter.kind == Some(i::Noun::Spell) => TargetSpec::Spell(spell_filter(o)?),
            Quant::Target {
                n: i::Amount::N(1),
                up_to: false,
            } => TargetSpec::Permanent(target_filter(o)?),
            ref q => return Err(format!("a binder of {}", short(q))),
        };
        let s = self.slot(spec);
        self.bound[b] = Some(s);
        Ok(s)
    }

    /// The slot of a target binder that is a card in a graveyard
    /// ("target creature card from a graveyard"), made the first time it
    /// is named; `None` if the reference names something else. Only the
    /// instructions that read a card there use it: any other refuses it
    /// in `binder`.
    fn graveyard_binder(&mut self, o: &Objects) -> Option<Result<usize, String>> {
        let (Quant::Introduced(i) | Quant::Bound(i)) = o.quant else {
            return None;
        };
        if o.filter.alts.iter().any(|a| !a.is_empty())
            || !o.filter.posts.is_empty()
            || self.found(i.into()).is_some()
        {
            return None;
        }
        let i = (i as usize).checked_sub(self.withs.len())?;
        let b = self.bind.len().checked_sub(1 + i)?;
        if let Some(s) = self.bound[b] {
            return matches!(self.program.targets[s], TargetSpec::GraveyardCard { .. })
                .then_some(Ok(s));
        }
        let bound = &self.bind[b];
        if bound.quant
            != (Quant::Target {
                n: i::Amount::N(1),
                up_to: false,
            })
        {
            return None;
        }
        let spec = match graveyard_card(bound)? {
            Ok(spec) => spec,
            Err(e) => return Some(Err(e)),
        };
        let s = self.slot(spec);
        self.bound[b] = Some(s);
        Some(Ok(s))
    }

    fn obj(&mut self, o: &Objects) -> Result<Obj, String> {
        match o.quant {
            Quant::SelfRef if o.filter == i::Filter::default() => Ok(Obj::Itself),
            Quant::Introduced(i) | Quant::Bound(i) if self.found(i.into()).is_some() => {
                Ok(Obj::Found(self.found(i.into()).expect("found")?))
            }
            Quant::Introduced(i) | Quant::Bound(i) => Ok(Obj::Target(self.binder(i.into())?)),
            Quant::Target {
                n: i::Amount::N(1),
                up_to,
            } => {
                let spec = match graveyard_card(o) {
                    Some(spec) => spec?,
                    None => TargetSpec::Permanent(target_filter(o)?),
                };
                Ok(Obj::Target(self.slot(if up_to {
                    TargetSpec::UpToOne(Box::new(spec))
                } else {
                    spec
                })))
            }
            ref q => Err(format!("an object {}", short(q))),
        }
    }

    /// One object, or "each other Human you control", "all creatures":
    /// each permanent a filter matches, read from the ability's source.
    fn on(&mut self, o: &Objects) -> Result<On, String> {
        // "With mana value equal to the number of fuse counters on this
        // artifact": a mana value compared with an amount counted as the
        // instruction is followed, not a number.
        let counted = o.filter.posts.iter().position(|p| {
            matches!(p, Post::With(i::Compare { stat: i::Stat::ManaValue, value, .. })
                if !matches!(value, i::Amount::N(_)))
        });
        match (&o.quant, counted) {
            (Quant::Each | Quant::All, Some(k)) => {
                let Post::With(i::Compare { op, value, .. }) = &o.filter.posts[k] else {
                    unreachable!("found above")
                };
                let mut rest = o.clone();
                rest.filter.posts.remove(k);
                Ok(On::EachWithManaValue {
                    filter: objects(&rest)?,
                    cmp: cmp(*op),
                    n: Box::new(self.resolved_amount(value)?),
                })
            }
            (Quant::Each | Quant::All, None) => Ok(On::Each(objects(o)?)),
            _ => Ok(On::One(self.obj(o)?)),
        }
    }

    fn who(&mut self, p: &i::Player) -> Result<Who, String> {
        let opponent = match p {
            i::Player::You => return Ok(Who::You),
            i::Player::Target => false,
            i::Player::TargetOpponent => true,
            // "That player": the target player, when the ability names
            // exactly one player beside its controller.
            i::Player::That if self.caster && self.player.is_none() => return Ok(Who::EventPlayer),
            i::Player::That => {
                return self
                    .player
                    .map(Who::Target)
                    .ok_or_else(|| "Player::That".into())
            }
            i::Player::ControllerOf(o)
                if self.event_spell.is_some() && binder(o, self.bind) == self.event_spell =>
            {
                return Ok(Who::EventPlayer)
            }
            i::Player::ControllerOf(o) => return Ok(Who::ControllerOf(self.obj(o)?)),
            other => return Err(format!("Player::{}", short(other))),
        };
        match self.player {
            Some(s) if self.program.targets[s] == TargetSpec::Player { opponent } => {
                Ok(Who::Target(s))
            }
            Some(_) => Err("two kinds of target player".into()),
            None => {
                let s = self.slot(TargetSpec::Player { opponent });
                self.player = Some(s);
                Ok(Who::Target(s))
            }
        }
    }

    fn damage_to(&mut self, to: &[i::Recipient]) -> Result<DamageTo, String> {
        let one_target = |o: &Objects, types: &[&str]| {
            o.filter.kind.is_none()
                && o.filter.posts.is_empty()
                && o.filter.or.is_none()
                && o.filter.alts.len() == types.len()
                && o.filter
                    .alts
                    .iter()
                    .zip(types)
                    .all(|(a, t)| matches!(a.as_slice(), [Adj::Type(x)] if x == t))
        };
        match to {
            [i::Recipient::Objects(o)]
                if o.quant == (Quant::AnyTarget { n: i::Amount::N(1) })
                    && o.filter == i::Filter::default() =>
            {
                Ok(DamageTo::Target(self.slot(TargetSpec::Any)))
            }
            [i::Recipient::Player(i::Player::You)] => Ok(DamageTo::You),
            [i::Recipient::AttackedBy(o)]
                if o.quant == Quant::SelfRef && o.filter == i::Filter::default() =>
            {
                Ok(DamageTo::Attacked)
            }
            [i::Recipient::Player(p)] => match self.who(p)? {
                Who::Target(s) => Ok(DamageTo::Target(s)),
                Who::You => Ok(DamageTo::You),
                Who::EventPlayer => Ok(DamageTo::EventPlayer),
                Who::Defending | Who::ControllerOf(_) | Who::SpellController(_) => {
                    Err("damage to the defending player or a controller".into())
                }
            },
            [i::Recipient::Or(alts)] => match alts.as_slice() {
                [i::Recipient::Player(i::Player::Target), i::Recipient::Objects(o)]
                    if o.quant
                        == (Quant::Target {
                            n: i::Amount::N(1),
                            up_to: false,
                        })
                        && one_target(o, &["planeswalker"]) =>
                {
                    Ok(DamageTo::Target(
                        self.slot(TargetSpec::PlayerOrPlaneswalker),
                    ))
                }
                _ => Err("damage to one of several kinds of target".into()),
            },
            [i::Recipient::Objects(o)] if matches!(o.quant, Quant::Each | Quant::All) => {
                Ok(DamageTo::Each(objects(o)?))
            }
            [i::Recipient::Objects(o)] => match self.obj(o)? {
                Obj::Target(s) => Ok(DamageTo::Target(s)),
                Obj::Itself | Obj::Triggering | Obj::Found(_) => {
                    Err("damage to its own source or a card found".into())
                }
            },
            _ => Err("damage to several recipients".into()),
        }
    }

    /// One effect, or the node it can't lower.
    fn effect(&mut self, e: &i::Effect) -> Result<Vec<Effect>, String> {
        let n = |a: &i::Amount| match a {
            i::Amount::N(n) => Ok(*n),
            // "Draw two additional cards": two, beside the draw the step
            // already made (CR 504.1).
            i::Amount::Additional(k) => match **k {
                i::Amount::N(n) => Ok(n),
                ref other => Err(format!("an amount of {}", short(other))),
            },
            other => Err(format!("an amount of {}", short(other))),
        };
        let node = || effect_node(e);
        Ok(vec![match e {
            i::Effect::Draw { who, n: k } => Effect::Draw {
                who: self.who(who)?,
                n: match k {
                    i::Amount::CountOf(_) | i::Amount::X => self.resolved_amount(k)?,
                    _ => Amount::N(n(k)?),
                },
            },
            i::Effect::Discard { who, what }
                if what.filter.alts.is_empty()
                    && what.filter.kind == Some(i::Noun::Card)
                    && what.filter.or.is_none()
                    && matches!(what.filter.posts.as_slice(), [] | [i::Post::AtRandom]) =>
            {
                let random = !what.filter.posts.is_empty();
                let k = match &what.quant {
                    Quant::One => 1,
                    Quant::N(a) => n(a)?,
                    q => return Err(format!("discarding {}", short(q))),
                };
                let into = self.new_slot();
                self.discarded = Some(into);
                Effect::Discard {
                    who: self.who(who)?,
                    n: k,
                    into,
                    random,
                }
            }
            i::Effect::ExileZone(i::Zone::Graveyard(who)) => Effect::ExileGraveyard(self.who(who)?),
            i::Effect::LoseLife { who, n: k } => Effect::LoseLife {
                who: self.who(who)?,
                n: match k {
                    i::Amount::ManaValueOf(o) if self.graveyard_binder(o).is_some() => {
                        Amount::TargetCardManaValue(
                            self.graveyard_binder(o).expect("a graveyard binder")?,
                        )
                    }
                    i::Amount::ManaValueOf(o) => match self.cards(o)? {
                        Cards::Bound(slot) => Amount::ManaValueOf(slot),
                        _ => return Err("the mana value of several cards".into()),
                    },
                    // "lose X life, where X is the number of Zombies you
                    // control" (Graveborn Muse).
                    i::Amount::X => self.resolved_amount(k)?,
                    k => Amount::N(n(k)?),
                },
            },
            i::Effect::GainLife { who, n: k } => Effect::GainLife {
                who: self.who(who)?,
                n: match k {
                    i::Amount::PowerOf(o) => Amount::PowerOf(self.obj(o)?),
                    i::Amount::ToughnessOf(o) => Amount::ToughnessOf(self.obj(o)?),
                    // "You gain that much life": the event's amount.
                    i::Amount::ThatMany if self.amount => Amount::ThatMuch,
                    k => Amount::N(n(k)?),
                },
            },
            i::Effect::Create {
                who: i::Player::You,
                n: k,
                token,
            } => Effect::Create {
                n: self.resolved_amount(k)?,
                token: Box::new(token_text(token)?),
                tapped: token.tapped,
                attacking: false,
            },
            i::Effect::Destroy { what, .. } => Effect::Destroy(self.on(what)?),
            // "Attach up to one target Equipment you control to target
            // attacking creature": a target Equipment, the only kind of
            // object the game attaches; an Aura's rules differ (CR 303.4).
            i::Effect::Attach { what, to }
                if matches!(what.quant, Quant::Target { .. })
                    && !what.filter.alts.is_empty()
                    && what.filter.alts.iter().all(|alt| {
                        alt.iter().any(
                            |a| matches!(a, Adj::Subtype(s) if s.eq_ignore_ascii_case("equipment")),
                        )
                    }) =>
            {
                Effect::Attach {
                    what: self.obj(what)?,
                    to: self.obj(to)?,
                }
            }
            i::Effect::Tap(what) => Effect::Tap(self.on(what)?),
            // "Prevent all combat damage that would be dealt to and dealt
            // by this creature this turn": one object, both ways (as
            // Insight renders it), or one way. Damage to one object by
            // another, a number of damage, other damage or durations, and
            // players are refused.
            i::Effect::Prevent {
                combat_only: true,
                n: None,
                to,
                by,
                dur: i::Duration::ThisTurn,
            } => {
                let to = match to.as_deref() {
                    None => None,
                    Some([i::Recipient::Objects(o)]) => Some(o),
                    Some(_) => return Err("combat damage prevented to other than an object".into()),
                };
                let of = match (to, by) {
                    (Some(a), Some(b)) if a != b => {
                        return Err("combat damage prevented to one object by another".into())
                    }
                    (Some(o), _) | (None, Some(o)) => o,
                    (None, None) => return Err("all combat damage prevented".into()),
                };
                let On::One(of) = self.on(of)? else {
                    return Err("combat damage prevented to or by each of a kind".into());
                };
                Effect::PreventCombatDamage {
                    of,
                    to: to.is_some(),
                    by: by.is_some(),
                }
            }
            i::Effect::Untap(what) => match what.quant {
                Quant::UpTo(i::Amount::N(n)) => {
                    let all = Objects {
                        quant: Quant::All,
                        ..what.clone()
                    };
                    Effect::UntapUpTo {
                        n,
                        on: On::Each(objects(&all)?),
                    }
                }
                _ => Effect::Untap(self.on(what)?),
            },
            i::Effect::Exile(Objects {
                quant: Quant::N(k),
                filter,
            }) if *filter
                == (i::Filter {
                    kind: Some(i::Noun::Card),
                    posts: vec![Post::In(i::Zone::TopOfLibrary(i::Player::You))],
                    ..i::Filter::default()
                }) =>
            {
                let into = self.new_slot();
                self.exiled = Some(into);
                Effect::ExileTop {
                    whose: Who::You,
                    n: n(k)?,
                    into,
                }
            }
            i::Effect::Exile(what) => Effect::Exile(self.on(what)?),
            i::Effect::DamageCantBePrevented(i::Duration::ThisTurn) => {
                Effect::DamageCantBePreventedThisTurn
            }
            i::Effect::ExileUntilLeaves { what, until } if itself(until) => {
                Effect::ExileUntilLeaves(self.on(what)?)
            }
            i::Effect::Sacrifice {
                who: i::Player::You,
                what,
            } if what.quant == Quant::SelfRef => Effect::Sacrifice(self.obj(what)?),
            // A "may"'s sacrifice is the price of what follows "if you
            // do", which fewer permanents than it says don't pay; as many
            // as there are is right only for one nothing hangs on.
            i::Effect::Sacrifice { .. } if self.in_may => {
                return Err("sacrificing permanents as a \"may\"".into())
            }
            i::Effect::Sacrifice { who, what } => {
                let n = match &what.quant {
                    Quant::One => Amount::N(1),
                    Quant::N(k) => self.resolved_amount(k)?,
                    q => return Err(format!("sacrificing {}", short(q))),
                };
                let who = match who {
                    i::Player::DefendingPlayer => Who::Defending,
                    w => self.who(w)?,
                };
                // "A creature of their choice": the player who sacrifices
                // chooses, as they would unless it said otherwise.
                let mut what = what.clone();
                what.filter
                    .posts
                    .retain(|p| !matches!(p, Post::ChosenBy(i::Player::That)));
                Effect::SacrificeChosen {
                    who,
                    n,
                    filter: sacrificed(&what)?,
                }
            }
            i::Effect::Move {
                what,
                from: None,
                to: i::Zone::Hand(i::Player::You),
                tapped: false,
                transformed: false,
                under: None,
                order: None,
            } if what.quant == Quant::SelfRef
                && what.filter
                    == (i::Filter {
                        posts: vec![Post::In(i::Zone::Graveyard(i::Player::You))],
                        ..i::Filter::default()
                    }) =>
            {
                Effect::SelfFromGraveyardToHand
            }
            i::Effect::Move {
                what,
                from: None,
                to: i::Zone::Battlefield,
                tapped,
                transformed: false,
                under: Some(i::Player::You),
                order: None,
            } if self.graveyard_binder(what).is_some() => Effect::FromGraveyardToBattlefield {
                slot: self.graveyard_binder(what).expect("a graveyard binder")?,
                tapped: *tapped,
            },
            i::Effect::Move {
                what,
                from: None,
                to: i::Zone::OwnersLibraryBottom,
                tapped: false,
                transformed: false,
                under: None,
                order: None,
            } if !matches!(what.quant, Quant::Rest { .. })
                && matches!(what.filter.kind, None | Some(i::Noun::Permanent)) =>
            {
                // One target permanent. Several at once are ordered by
                // their owner (CR 401.4), which isn't asked, and the
                // source itself or a card found may not be a permanent
                // by then.
                match self.on(what)? {
                    on @ On::One(Obj::Target(_)) => Effect::ToOwnersLibraryBottom(on),
                    _ => {
                        return Err(
                            "putting other than one target on the bottom of a library".into()
                        )
                    }
                }
            }
            i::Effect::Move {
                what,
                from: None,
                to: i::Zone::OwnersHand,
                tapped: false,
                transformed: false,
                under: None,
                order: None,
            } if !matches!(what.quant, Quant::Rest { .. })
                && matches!(what.filter.kind, None | Some(i::Noun::Permanent)) =>
            {
                Effect::ToOwnersHand(self.on(what)?)
            }
            i::Effect::Counter { what, unless: None }
                if what.quant
                    == (Quant::Target {
                        n: i::Amount::N(1),
                        up_to: false,
                    })
                    && what.filter.kind == Some(i::Noun::Spell) =>
            {
                Effect::Counter(self.slot(TargetSpec::Spell(spell_filter(what)?)))
            }
            // Stifle's "counter target activated or triggered ability"
            // (CR 701.6a).
            i::Effect::Counter { what, unless: None }
                if what.quant
                    == (Quant::Target {
                        n: i::Amount::N(1),
                        up_to: false,
                    })
                    && what.filter.kind == Some(i::Noun::Ability) =>
            {
                Effect::Counter(self.slot(ability_spec(what)?))
            }
            // Pyroblast's "counter target spell" inside its "if", the spell
            // a `Let` binds.
            i::Effect::Counter { what, unless: None }
                if matches!(what.quant, Quant::Introduced(_) | Quant::Bound(_)) =>
            {
                let Obj::Target(slot) = self.obj(what)? else {
                    return Err(node());
                };
                if !matches!(self.program.targets[slot], TargetSpec::Spell(_)) {
                    return Err(format!("{} of a permanent", node()));
                }
                Effect::Counter(slot)
            }
            // "Counter target spell unless its controller pays {3}" (CR
            // 118.12a), the spell a `Let` binds.
            i::Effect::Counter {
                what,
                unless: Some((who, cost)),
            } if matches!(what.quant, Quant::Introduced(_) | Quant::Bound(_)) => {
                let Obj::Target(slot) = self.obj(what)? else {
                    return Err(node());
                };
                if !matches!(self.program.targets[slot], TargetSpec::Spell(_)) {
                    return Err(format!("{} of a permanent", node()));
                }
                let i::Player::ControllerOf(of) = who else {
                    return Err(format!("{} unless {} pays", node(), short(who)));
                };
                if self.obj(of)? != Obj::Target(slot) {
                    return Err(format!("{} unless another's controller pays", node()));
                }
                let cost = match cost.as_slice() {
                    [i::Cost::Mana(m)] => UnlessCost::Mana(crate::mana::ManaCost::from_symbols(m)?),
                    // Circular Logic's "{1} for each card in your
                    // graveyard", counted as it resolves (CR 608.2h).
                    [i::Cost::ManaPer { mana: m, per }] => {
                        let Some(times) = cards_in_graveyards(per) else {
                            return Err(format!(
                                "{} unless mana per {} is paid",
                                node(),
                                short(per)
                            ));
                        };
                        UnlessCost::ManaTimes {
                            each: crate::mana::ManaCost::from_symbols(m)?,
                            times: times?,
                        }
                    }
                    _ => return Err(format!("{} unless a cost other than mana is paid", node())),
                };
                Effect::Unless {
                    who: Who::SpellController(slot),
                    cost,
                    otherwise: vec![Effect::Counter(slot)],
                }
            }
            // "Sacrifice this creature unless you discard a card" (CR
            // 118.12a): what the player named may pay, read as they are.
            i::Effect::Unless { effect, who, cost } => {
                let who = self.who(who)?;
                let cost = match cost.as_slice() {
                    [i::Cost::Mana(m)] => UnlessCost::Mana(crate::mana::ManaCost::from_symbols(m)?),
                    [i::Cost::Discard(o)] => UnlessCost::Discard(discarded(o)?),
                    [i::Cost::Sacrifice(o)] => sacrifice_power(o)?,
                    [one] => return Err(format!("{} of a cost {}", node(), short(one))),
                    _ => return Err(format!("{} of a cost of several parts", node())),
                };
                Effect::Unless {
                    who,
                    cost,
                    otherwise: self.effect(effect)?,
                }
            }
            i::Effect::Damage {
                source: Some(source),
                n: k,
                to,
            } if source.quant == Quant::SelfRef && source.filter == i::Filter::default() => {
                if let [i::Recipient::Player(i::Player::Each)] = to.as_slice() {
                    Effect::Damage {
                        to: DamageTo::EachPlayer,
                        n: each_players_amount(k)?,
                    }
                } else {
                    Effect::Damage {
                        to: self.damage_to(to)?,
                        n: self.resolved_amount(k)?,
                    }
                }
            }
            i::Effect::Scry(k) => Effect::Scry(n(k)?),
            // "can't be regenerated this turn": the engine lowers no
            // regeneration (`Effect::Regenerate` is refused), so no shield
            // exists for this to stop (CR 701.19a–c). When regeneration
            // lowers, this must become the restriction it is.
            i::Effect::Restrict {
                rule: i::Restriction::CantBeRegenerated,
                ..
            } => return Ok(vec![]),
            i::Effect::Delayed {
                step: i::Step::Upkeep(i::Whose::NextTurns),
                then,
            } => {
                let mut inner = Scope::new(&[]);
                let effects = inner.effects(then)?;
                if !inner.program.targets.is_empty() {
                    return Err(format!("{} with a target", node()));
                }
                Effect::Delayed {
                    step: TurnStep::Upkeep,
                    effects,
                }
            }
            i::Effect::AddMana(add) => match crate::mana::adds(add)? {
                crate::mana::Adds::Options(o) if o.len() == 1 => Effect::AddMana(o[0]),
                crate::mana::Adds::Options(o) => Effect::AddManaOf(o),
                crate::mana::Adds::AnyColor => Effect::AddManaOf(
                    [
                        Color::White,
                        Color::Blue,
                        Color::Black,
                        Color::Red,
                        Color::Green,
                    ]
                    .map(|c| crate::mana::ManaPool::of([crate::mana::Mana::of_color(c)]))
                    .to_vec(),
                ),
                _ => return Err("adding a choice of mana".into()),
            },
            i::Effect::PutCounters { n: k, kind, on } => Effect::PutCounters {
                on: self.on(on)?,
                counter: counter(kind).ok_or_else(node)?,
                n: match k {
                    i::Amount::X => self.resolved_amount(k)?,
                    k => Amount::N(n(k)?),
                },
            },
            // "Put a +1/+1 counter and a double strike counter on ~": each
            // kind put on in turn.
            i::Effect::PutCountersEach { counters, on } if itself(on) => {
                let on = self.on(on)?;
                let mut each = Vec::new();
                for (k, kind) in counters {
                    each.push(Effect::PutCounters {
                        on: on.clone(),
                        counter: counter(kind).ok_or_else(node)?,
                        n: Amount::N(n(k)?),
                    });
                }
                return Ok(each);
            }
            // "Target creature gets +X/+0 until end of turn": +1/+0 as
            // many times as the spell's X; "-X/-X", -1/-1.
            i::Effect::Modify {
                what,
                pt: Some(p),
                gains,
                dur: i::Duration::UntilEndOfTurn,
            } if self.cost_x
                && self.x.is_none()
                && p.signed
                && [p.power, p.toughness]
                    .iter()
                    .any(|q| matches!(q, PtPart::X | PtPart::MinusX))
                && [p.power, p.toughness]
                    .iter()
                    .all(|q| matches!(q, PtPart::X | PtPart::MinusX | PtPart::N(0))) =>
            {
                let unit = |q: PtPart| match q {
                    PtPart::X => PtPart::N(1),
                    PtPart::MinusX => PtPart::N(-1),
                    q => q,
                };
                let once = i::Effect::Modify {
                    what: what.clone(),
                    pt: Some(IPt {
                        power: unit(p.power),
                        toughness: unit(p.toughness),
                        signed: true,
                    }),
                    gains: Vec::new(),
                    dur: i::Duration::UntilEndOfTurn,
                };
                if !gains.is_empty() {
                    return Err(format!("{} of ±X and abilities", node()));
                }
                let r = resolution(&once).map_err(|_| node())?;
                Effect::UntilEndOfTurn {
                    on: self.on(what)?,
                    ops: r.ops,
                    per: Some(Per::X),
                }
            }
            i::Effect::Modify {
                what,
                dur: i::Duration::UntilEndOfTurn,
                ..
            }
            | i::Effect::Becomes {
                what,
                dur: i::Duration::UntilEndOfTurn,
                ..
            } => {
                let on = self.on(what)?;
                let r = resolution(e).map_err(|_| node())?;
                if !r.counters.is_empty() {
                    return Err(node());
                }
                Effect::UntilEndOfTurn {
                    on,
                    ops: r.ops,
                    per: None,
                }
            }
            // "It gets +2/+0 until end of turn for each other attacking
            // Goblin": one modification, its size counted (CR 608.2h).
            // "For each land card discarded this way, gain 3 life."
            i::Effect::ForEach { each, then, .. }
                if matches!(
                    each.filter.posts.as_slice(),
                    [Post::Marked(i::Marked::Discarded)]
                ) =>
            {
                let slot = self
                    .discarded
                    .ok_or("cards discarded by nothing the engine binds")?;
                let filter = card_filter(&Objects {
                    quant: Quant::One,
                    filter: i::Filter {
                        posts: Vec::new(),
                        ..each.filter.clone()
                    },
                })?;
                match then.as_slice() {
                    [i::Effect::GainLife {
                        who: i::Player::You,
                        n: i::Amount::N(k),
                    }] => Effect::GainLife {
                        who: Who::You,
                        n: Amount::PerFound {
                            each: *k,
                            slot,
                            filter,
                        },
                    },
                    _ => return Err(format!("{} discarded this way", node())),
                }
            }
            // "For each of those cards, …": the set a binder holds, at
            // most as many as the instruction that bound it chose, the
            // instructions once for each.
            i::Effect::ForEach { each, then, .. } if matches!(self.cards(each), Ok(Cards::Bound(s)) if self.sizes.contains_key(&s)) =>
            {
                let Ok(Cards::Bound(from)) = self.cards(each) else {
                    unreachable!("matched")
                };
                let mut out = Vec::new();
                for k in 0..self.sizes[&from] as usize {
                    let into = self.new_slot();
                    if self.in_hand.contains(&from) {
                        self.in_hand.insert(into);
                    }
                    out.push(Effect::Nth { from, k, into });
                    self.withs.push(Some(into));
                    let lowered = self.effects(then);
                    self.withs.pop();
                    out.extend(lowered?);
                }
                return Ok(out);
            }
            // "Pay 4 life or put the card on top of your library".
            i::Effect::Or(options)
                if matches!(options.as_slice(), [i::Effect::Pay { cost, .. }, _]
                    if matches!(cost.as_slice(), [i::Cost::PayLife(i::Amount::N(_))])) =>
            {
                let [i::Effect::Pay { who, cost }, otherwise] = options.as_slice() else {
                    unreachable!("matched")
                };
                let [i::Cost::PayLife(i::Amount::N(life))] = cost.as_slice() else {
                    unreachable!("matched")
                };
                Effect::UnlessLife {
                    who: self.who(who)?,
                    life: Amount::N(*life),
                    otherwise: self.effect(otherwise)?,
                }
            }
            // "For each opponent, create a token that's tapped and
            // attacking that player or a planeswalker they control"
            // (Adeline). A planeswalker spell is refused as it is cast,
            // so the player is all there is to attack.
            i::Effect::ForEach { each, then, .. }
                if each.filter.kind == Some(i::Noun::Opponent)
                    && matches!(then.as_slice(), [i::Effect::Create { who: i::Player::You, token, .. }]
                        if token.attacking.as_ref().is_some_and(attacks_that_player)) =>
            {
                let [i::Effect::Create { n: k, token, .. }] = then.as_slice() else {
                    unreachable!("matched")
                };
                Effect::Create {
                    n: self.resolved_amount(k)?,
                    token: Box::new(token_text(&i::Token {
                        attacking: None,
                        ..token.clone()
                    })?),
                    tapped: token.tapped,
                    attacking: true,
                }
            }
            i::Effect::ForEach { each, then, .. } => match then.as_slice() {
                [m @ i::Effect::Modify {
                    what,
                    dur: i::Duration::UntilEndOfTurn,
                    ..
                }] => {
                    let on = On::One(self.obj(what)?);
                    let r = resolution(m).map_err(|_| node())?;
                    if !r.counters.is_empty() || !r.ops.iter().all(|o| matches!(o, Op::ModifyPt(_)))
                    {
                        return Err(format!("{} of more than power and toughness", node()));
                    }
                    Effect::UntilEndOfTurn {
                        on,
                        ops: r.ops,
                        per: Some(Per::Each(counted(each)?)),
                    }
                }
                // Glimmerpost's "you gain 1 life for each Locus on the
                // battlefield": as many as are counted as it resolves (CR
                // 608.2h).
                [i::Effect::GainLife {
                    who,
                    n: i::Amount::N(1),
                }] => Effect::GainLife {
                    who: self.who(who)?,
                    n: Amount::CountOf(counted(each)?),
                },
                _ => return Err(node()),
            },
            i::Effect::RevealHand(p) => Effect::RevealHand(self.who(p)?),
            // CR 201.4a.
            i::Effect::Choose(c @ (i::Choice::NonlandCardName | i::Choice::CardName)) => {
                self.named = true;
                let nonland = matches!(c, i::Choice::NonlandCardName);
                Effect::ChooseName(CardFilter(vec![Filter(if nonland {
                    vec![Pred::IsNot(Prop::Type(CardType::Land))]
                } else {
                    Vec::new()
                })]))
            }
            i::Effect::Discard { who, what }
                if self.named
                    && what.quant == Quant::All
                    && what.filter
                        == (i::Filter {
                            kind: Some(i::Noun::Card),
                            posts: vec![Post::WithThatName],
                            ..i::Filter::default()
                        }) =>
            {
                Effect::DiscardNamed(self.who(who)?)
            }
            i::Effect::Cast {
                verb: i::CastVerb::Cast,
                what,
                payment: i::Payment::Free,
                dur: i::Duration::Permanent,
            } if matches!(
                what.filter.posts.as_slice(),
                [Post::Marked(i::Marked::Discarded)]
            ) && what.filter.alts.is_empty()
                && what.filter.or.is_none()
                && what.filter.kind == Some(i::Noun::Card) =>
            {
                let slot = self
                    .discarded
                    .ok_or("a card discarded by nothing the engine binds")?;
                match self.cards(&Objects {
                    quant: what.quant.clone(),
                    filter: i::Filter::default(),
                })? {
                    Cards::Bound(s) if s == slot => Effect::CastWithoutPaying(slot),
                    _ => return Err("casting a card other than the one discarded".into()),
                }
            }
            i::Effect::With { bind, then } => return self.with(bind, then),
            i::Effect::During {
                dur: i::Duration::ThisTurn,
                then,
            } if damage_this_turn(then).is_some() => {
                damage_this_turn(then).expect("matched in the guard")?
            }
            // "…, where X is the number of Goblins on the battlefield".
            i::Effect::Where { then, x } => {
                let amount = match x {
                    i::Amount::CountOf(o) => Amount::CountOf(counted(o)?),
                    other => return Err(format!("X as {}", short(other))),
                };
                if self.x.replace(amount).is_some() {
                    return Err("an X within an X".into());
                }
                let lowered = self.effects(then);
                self.x = None;
                return lowered;
            }
            i::Effect::Search {
                who,
                zones,
                what,
                and,
            } if and.is_empty() => {
                let Cards::Bound(into) = self.cards(what)? else {
                    return Err(node());
                };
                let (described, enters) = self
                    .described
                    .get(&into)
                    .cloned()
                    .ok_or("a search for what no binder describes")?;
                let who = self.who(who)?;
                // Their own library: "your library", or "their library"
                // of the player searching.
                match (zones.as_slice(), who) {
                    ([i::Zone::Library(i::Player::You)], Who::You)
                    | ([i::Zone::Library(i::Player::That)], Who::ControllerOf(_)) => {}
                    _ => return Err(format!("{} of another library", node())),
                }
                self.whose.insert(into, who);
                self.searched.insert(into);
                Effect::Search {
                    who,
                    filter: card_filter(&described)?,
                    into,
                    enters,
                    reveals: self.revealing.contains(&into),
                }
            }
            // A card looked at is hidden to all but its controller until
            // revealed (CR 400.2, 701.20a).
            i::Effect::Reveal { what, .. } if matches!(self.cards(what), Ok(Cards::Bound(s)) if self.looked.contains(&s)) =>
            {
                let Ok(Cards::Bound(cards)) = self.cards(what) else {
                    unreachable!("matched")
                };
                let into = self.new_slot();
                self.revealed = Some(into);
                Effect::Reveal { cards, into }
            }
            // A card found is named as it is found, so revealing it shows
            // no one anything the engine doesn't know.
            i::Effect::Reveal { what, .. } if self.cards(what).is_ok() => return Ok(vec![]),
            i::Effect::Shuffle(p) => Effect::Shuffle(match p {
                i::Player::ControllerOf(o) => match self.cards(o) {
                    Ok(Cards::Bound(slot)) => *self.whose.get(&slot).ok_or_else(node)?,
                    _ => self.who(p)?,
                },
                p => self.who(p)?,
            }),
            // "Each opponent may put any number of artifact, creature,
            // enchantment, and/or land cards from their hand onto the
            // battlefield" (Tempting Wurm): no card is a choice "any
            // number" offers (CR 107.1c), so the "may" is in the choice;
            // "each player may put an artifact, creature, enchantment, or
            // land card" (Show and Tell) is up to one, the "may" its none.
            i::Effect::May {
                who: by @ (i::Player::EachOpponent | i::Player::Each),
                then,
            } => match then.as_slice() {
                [i::Effect::Move {
                    what,
                    from: None,
                    to: i::Zone::Battlefield,
                    tapped,
                    transformed: false,
                    under: None,
                    order: None,
                }] if matches!(what.quant, Quant::AnyNumber | Quant::One)
                    && what.filter.posts == [Post::In(i::Zone::Hand(i::Player::That))] =>
                {
                    let filter = hand_card(&Objects {
                        quant: Quant::One,
                        filter: i::Filter {
                            posts: Vec::new(),
                            ..what.filter.clone()
                        },
                    })?;
                    let into = self.new_slot();
                    return Ok(vec![
                        Effect::PutFromHands {
                            whose: match by {
                                i::Player::Each => Whose::Each,
                                _ => Whose::Opponents,
                            },
                            filter,
                            up_to: (what.quant == Quant::One).then_some(1),
                            into,
                        },
                        Effect::HandsToBattlefield {
                            cards: Cards::Bound(into),
                            tapped: *tapped,
                        },
                    ]);
                }
                _ => return Err(format!("a \"may\" of Player::{}", short(by))),
            },
            // "You may play cards exiled this way until the end of your
            // next turn": a permission given as it resolves, the "may"
            // answered each time a card is played, not now.
            i::Effect::May {
                who: i::Player::You,
                then,
            } if matches!(
                then.as_slice(),
                [i::Effect::Cast {
                    verb: i::CastVerb::Play,
                    payment: i::Payment::ManaCost,
                    dur: i::Duration::UntilEndOfYourNextTurn,
                    what,
                }] if what.quant == Quant::Plural
                    && what.filter
                        == i::Filter {
                            kind: Some(i::Noun::Card),
                            posts: vec![Post::Marked(i::Marked::Exiled)],
                            ..i::Filter::default()
                        }
            ) =>
            {
                let cards = self
                    .exiled
                    .ok_or("cards exiled by nothing the engine binds")?;
                Effect::MayPlayExiled { cards }
            }
            // "You may play an additional land this turn": a permission,
            // used or not as lands are played (CR 305.2).
            i::Effect::May {
                who: i::Player::You,
                then,
            } if matches!(
                then.as_slice(),
                [i::Effect::ExtraLandPlays {
                    dur: i::Duration::ThisTurn,
                    ..
                }]
            ) =>
            {
                let [i::Effect::ExtraLandPlays { n: k, .. }] = then.as_slice() else {
                    unreachable!("matched in the guard")
                };
                Effect::ExtraLandsThisTurn(n(k)?)
            }
            i::Effect::May { who, then } => {
                let who = self.who(who)?;
                let outer = std::mem::replace(&mut self.in_may, true);
                let then = self.effects(then);
                self.in_may = outer;
                Effect::May(who, then?)
            }
            i::Effect::RevealTop { who, whose, n: k } => {
                let whose = self.library_of(who, whose)?;
                let into = self.made();
                self.whose.insert(into, whose);
                Effect::RevealTop {
                    whose,
                    n: n(k)?,
                    into,
                    blind: false,
                }
            }
            // CR 700.3: "An opponent separates those cards into two
            // piles". With several opponents, which of them does is the
            // controller's choice, which the game asks only of a game
            // with one.
            i::Effect::SeparatePiles {
                who: i::Player::AnOpponent,
                what,
                n: 2,
            } => {
                let Cards::Bound(from) = self.cards(what)? else {
                    return Err(node());
                };
                let into = self.new_slot();
                self.piles = Some((from, into));
                Effect::SeparatePiles { from, into }
            }
            i::Effect::LookAtTop { who, whose, n: k } => {
                let whose = self.library_of(who, whose)?;
                let into = self.made();
                self.whose.insert(into, whose);
                self.looked.insert(into);
                Effect::LookAtTop {
                    whose,
                    n: n(k)?,
                    into,
                }
            }
            // "that player puts it into their hand": the instruction is
            // theirs to follow, and no choice is in it.
            i::Effect::Performed { by, effect } => match &**effect {
                // "Each opponent exiles a card from their hand": a card of
                // their own choosing, each in turn, then all at once (CR
                // 101.4).
                i::Effect::Exile(o)
                    if o.quant == Quant::One
                        && o.filter
                            == (i::Filter {
                                kind: Some(i::Noun::Card),
                                posts: vec![Post::In(i::Zone::Hand(i::Player::That))],
                                ..i::Filter::default()
                            }) =>
                {
                    Effect::ExileFromHand(match by {
                        i::Player::Each => Whose::Each,
                        i::Player::EachOpponent => Whose::Opponents,
                        other => {
                            return Err(format!(
                                "a card exiled from the hand of Player::{}",
                                short(other)
                            ))
                        }
                    })
                }
                i::Effect::GainControl {
                    what,
                    dur: i::Duration::Permanent,
                } => Effect::GainControl {
                    who: self.who(by)?,
                    of: self.obj(what)?,
                },
                _ => return self.effect(effect),
            },
            i::Effect::Move {
                what,
                from: None,
                to,
                tapped,
                transformed: false,
                under: None,
                order,
            } if self.cards(what).is_ok() => {
                let cards = self.cards(what)?;
                let from = match &cards {
                    Cards::Bound(slot) if self.in_hand.contains(slot) => FoundIn::Hand,
                    _ => FoundIn::Library,
                };
                if *tapped && *to != i::Zone::Battlefield {
                    return Err(node());
                }
                match (to, order) {
                    (i::Zone::Hand(p), None) => Effect::ToHand {
                        who: match (p, &cards) {
                            (i::Player::You, _) => Who::You,
                            (i::Player::That, Cards::Bound(slot)) => {
                                *self.whose.get(slot).ok_or_else(node)?
                            }
                            _ => return Err(node()),
                        },
                        cards,
                    },
                    (i::Zone::OwnersLibraryTop, Some(i::Order::Any) | None) => Effect::ToLibrary {
                        cards,
                        top: true,
                        from,
                        order: Arranged::Chosen,
                    },
                    // "Put the card on top of your library": its owner's,
                    // where a card bound for another's library goes (CR
                    // 400.3).
                    (i::Zone::TopOfLibrary(i::Player::You), None) => Effect::ToLibrary {
                        cards,
                        top: true,
                        from,
                        order: Arranged::Chosen,
                    },
                    (i::Zone::Battlefield, None) => Effect::ToBattlefield {
                        who: match &cards {
                            Cards::Bound(slot) => self.whose.get(slot).copied().unwrap_or(Who::You),
                            _ => Who::You,
                        },
                        cards,
                        tapped: *tapped,
                    },
                    // "The other on the bottom of your library": with no
                    // order said, its owner arranges them, as for "in any
                    // order" (CR 401.4).
                    (i::Zone::BottomOfLibrary(i::Player::You), Some(i::Order::Any) | None) => {
                        Effect::ToLibrary {
                            cards,
                            top: false,
                            from,
                            order: Arranged::Chosen,
                        }
                    }
                    // "The rest on the bottom of your library in a random
                    // order": chance orders them, not their owner.
                    (i::Zone::BottomOfLibrary(i::Player::You), Some(i::Order::Random)) => {
                        Effect::ToLibrary {
                            cards,
                            top: false,
                            from,
                            order: Arranged::Random,
                        }
                    }
                    (i::Zone::Graveyard(i::Player::You), None) => {
                        Effect::ToGraveyard { cards, from }
                    }
                    _ => return Err(format!("{} of found cards", node())),
                }
            }
            i::Effect::If {
                cond: i::Condition::WasKicked,
                then,
                ..
            } => Effect::Kicked {
                was: true,
                then: self.effects(then)?,
            },
            // "If there are four or more chorus counters on ~", "if its
            // toughness is less than or equal to the number of cards in
            // your graveyard": two amounts compared as it is followed.
            i::Effect::If {
                cond: i::Condition::AmountIs { n: k, op, value },
                then,
                ..
            } => Effect::IfAmount {
                n: self.compared_amount(k)?,
                cmp: cmp(*op),
                than: self.compared_amount(value)?,
                then: self.effects(then)?,
            },
            i::Effect::If {
                cond: i::Condition::YouControl(o),
                then,
                ..
            } => Effect::IfControl {
                control: you_control(o)?,
                then: self.effects(then)?,
            },
            // "If an instant or sorcery card is revealed this way": the
            // card the reveal before bound, if it matches.
            i::Effect::If {
                cond:
                    i::Condition::DoneThisWay {
                        what,
                        how: i::Marked::Revealed,
                    },
                then,
                ..
            } if self.revealed.is_some() => Effect::IfCard {
                card: self.revealed.expect("checked"),
                filter: card_filter(what)?,
                otherwise: false,
                then: self.effects(then)?,
            },
            i::Effect::Transform(o) if itself(o) => Effect::Transform,
            i::Effect::BecomesCopy {
                what,
                of,
                dur: i::Duration::Permanent,
                except,
            } if itself(what) => Effect::BecomeCopy {
                of: self.obj(of)?,
                except: excepts(except, None)?,
            },
            // "If it's blue", "it" a target the spell's `Let` binds.
            i::Effect::If {
                cond: i::Condition::Is { what, filter },
                then,
                ..
            } if matches!(what.quant, Quant::Bound(k) | Quant::Introduced(k)
                    if self.found(k.into()).is_none())
                && what.filter == i::Filter::default() =>
            {
                let Obj::Target(target) = self.obj(what)? else {
                    return Err(node());
                };
                let filter = objects(&Objects {
                    quant: Quant::All,
                    filter: filter.clone(),
                })?;
                if filter
                    .0
                    .iter()
                    .any(|p| !matches!(p, Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_)))
                {
                    return Err(format!("{} of a target relative to its source", node()));
                }
                Effect::IfTarget {
                    target,
                    filter,
                    then: self.effects(then)?,
                }
            }
            i::Effect::If {
                cond: i::Condition::Is { what, filter },
                then,
                ..
            } => {
                let Cards::Bound(card) = self.cards(what)? else {
                    return Err(node());
                };
                Effect::IfCard {
                    card,
                    filter: card_filter(&Objects {
                        quant: Quant::One,
                        filter: filter.clone(),
                    })?,
                    otherwise: false,
                    then: self.effects(then)?,
                }
            }
            i::Effect::Move {
                what,
                from: None,
                to: i::Zone::Battlefield,
                tapped,
                transformed: false,
                under: None,
                order: None,
            } if what.filter.posts.last() == Some(&Post::In(i::Zone::Hand(i::Player::You))) => {
                let (_, posts) = what.filter.posts.split_last().expect("a hand");
                let mana_value = match posts {
                    [] => None,
                    [Post::With(i::Compare {
                        stat: i::Stat::ManaValue,
                        op: i::Op::Eq,
                        value: i::Amount::CountOf(counted),
                    })] => match (&counted.quant, &counted.filter) {
                        (
                            Quant::Plural,
                            i::Filter {
                                kind: Some(i::Noun::Counter(Some(kind))),
                                posts: on,
                                ..
                            },
                        ) if matches!(on.as_slice(), [Post::On(o)] if itself(o)) => {
                            Some(counter(kind).ok_or_else(node)?)
                        }
                        _ => return Err(format!("{} with a mana value counted", node())),
                    },
                    _ => return Err(format!("{} with a mana value", node())),
                };
                Effect::PutFromHand {
                    filter: hand_card(&Objects {
                        quant: what.quant.clone(),
                        filter: i::Filter {
                            posts: Vec::new(),
                            ..what.filter.clone()
                        },
                    })?,
                    tapped: *tapped,
                    mana_value,
                }
            }
            _ => return Err(node()),
        }])
    }

    /// `With`: the cards an instruction makes or finds, bound for the
    /// rest of the list (`then`), "what the first instruction makes"
    /// (`Made`) or "one of them" (`N(1)` from among a binder's cards).
    fn with(&mut self, bind: &[Objects], then: &[i::Effect]) -> Result<Vec<Effect>, String> {
        let mut out = Vec::new();
        let slot = match bind {
            [o] if o.quant == Quant::Made && o.filter == i::Filter::default() => {
                let s = self.new_slot();
                self.made = Some(s);
                s
            }
            // "Draw a card, then discard a card. … cast the discarded
            // card": the card discarded, which the player chooses (CR
            // 701.9b), bound for what follows.
            [o] if o.quant == Quant::One
                && o.filter
                    == (i::Filter {
                        kind: Some(i::Noun::Card),
                        ..i::Filter::default()
                    })
                && matches!(then.first(), Some(i::Effect::Discard { what, .. })
                    if what.quant == Quant::Introduced(0) && what.filter == i::Filter::default()) =>
            {
                let Some(i::Effect::Discard { who, .. }) = then.first() else {
                    unreachable!("matched")
                };
                let into = self.new_slot();
                self.discarded = Some(into);
                out.push(Effect::Discard {
                    who: self.who(who)?,
                    n: 1,
                    into,
                    random: false,
                });
                self.withs.push(Some(into));
                let lowered = self.effects(&then[1..]);
                self.withs.pop();
                out.extend(lowered?);
                return Ok(out);
            }
            // "Search your library for a basic land card": the card the
            // search finds, which it describes.
            [o] if o.quant == Quant::One
                && o.filter.kind == Some(i::Noun::Card)
                && !o
                    .filter
                    .posts
                    .iter()
                    .any(|p| matches!(p, Post::FromAmong(_))) =>
            {
                let s = self.new_slot();
                self.described
                    .insert(s, (o.clone(), enters_battlefield(then)));
                if reveals_found(then) {
                    self.revealing.insert(s);
                }
                s
            }
            // "Search your library for up to two basic land cards": a
            // search for one, as many times, none finding what one before
            // found (CR 701.23a, 701.23d), the cards bound together.
            [o] if matches!(o.quant, Quant::UpTo(i::Amount::N(_)))
                && o.filter.kind == Some(i::Noun::Card)
                && matches!(then.first(), Some(i::Effect::Search { what, .. })
                    if what.quant == Quant::Introduced(0) && what.filter == i::Filter::default()) =>
            {
                let Quant::UpTo(i::Amount::N(n)) = o.quant else {
                    unreachable!("matched")
                };
                let enters = moves_onto_battlefield(then);
                let reveals = reveals_found(then);
                let one = Objects {
                    quant: Quant::One,
                    filter: o.filter.clone(),
                };
                let mut each = Vec::new();
                for _ in 0..n {
                    let s = self.new_slot();
                    self.described.insert(s, (one.clone(), enters));
                    if reveals {
                        self.revealing.insert(s);
                    }
                    self.withs.push(Some(s));
                    let search = self.effect(&then[0]);
                    self.withs.pop();
                    out.extend(search?);
                    each.push(s);
                }
                let into = self.new_slot();
                self.sizes.insert(into, n);
                if let Some(w) = each.first().and_then(|s| self.whose.get(s)).copied() {
                    self.whose.insert(into, w);
                }
                out.push(Effect::Gather { from: each, into });
                self.withs.push(Some(into));
                let lowered = self.effects(&then[1..]);
                self.withs.pop();
                out.extend(lowered?);
                return Ok(out);
            }
            // "Put one pile into your hand": the controller chooses one
            // of the piles separated before, and the rest is the other.
            [o] if o.quant == Quant::N(i::Amount::N(1))
                && o.filter
                    == (i::Filter {
                        kind: Some(i::Noun::Pile),
                        ..i::Filter::default()
                    }) =>
            {
                let (from, first) = self.piles.ok_or("one of piles no instruction made")?;
                let into = self.new_slot();
                self.among.insert(into, from);
                out.push(Effect::ChoosePile { from, first, into });
                into
            }
            [o] if o.quant == Quant::N(i::Amount::N(1)) => {
                let from = match o.filter.posts.as_slice() {
                    [Post::FromAmong(among)] if o.filter.alts.is_empty() => {
                        match self.cards(among)? {
                            Cards::Bound(s) => s,
                            _ => return Err("one from among several kinds of cards".into()),
                        }
                    }
                    _ => return Err("With one of some cards".into()),
                };
                let into = self.new_slot();
                self.among.insert(into, from);
                out.push(Effect::ChooseAmong { from, n: 1, into });
                into
            }
            // "Put up to two creature cards with total mana value 6 or
            // less from among them onto the battlefield": up to that many
            // of the cards found that the filter matches, their mana
            // values adding up to no more than the total (CR 202.3).
            // "You may reveal a Dinosaur or land card from among them and
            // put it into your hand": one, whose instructions are its
            // chooser's "may", is up to one, none chosen the "may"
            // declined, or none matching (CR 609.3).
            [o] if (matches!(o.quant, Quant::UpTo(i::Amount::N(_)))
                || o.quant == Quant::One
                    && matches!(
                        then.first(),
                        Some(i::Effect::May {
                            who: i::Player::You,
                            ..
                        })
                    ))
                && o.filter.kind == Some(i::Noun::Card)
                && o.filter
                    .posts
                    .iter()
                    .any(|p| matches!(p, Post::FromAmong(_))) =>
            {
                let up_to = match o.quant {
                    Quant::UpTo(i::Amount::N(n)) => n,
                    Quant::One => 1,
                    _ => unreachable!("matched"),
                };
                let mut from = None;
                let mut total_mana_value = None;
                for p in &o.filter.posts {
                    match p {
                        Post::FromAmong(among) if from.is_none() => {
                            from = Some(match self.cards(among)? {
                                Cards::Bound(s) => s,
                                _ => {
                                    return Err(
                                        "up to some from among several kinds of cards".into()
                                    )
                                }
                            })
                        }
                        Post::Total(i::Compare {
                            stat: i::Stat::ManaValue,
                            op: i::Op::Le,
                            value: i::Amount::N(k),
                        }) if total_mana_value.is_none() => total_mana_value = Some(*k),
                        _ => return Err(format!("up to some cards {}", short(p))),
                    }
                }
                let from = from.expect("matched");
                // "Up to two cards": any card, which a filter of no
                // alternatives isn't.
                let filter = if o.filter.alts.is_empty() {
                    CardFilter(vec![Filter::default()])
                } else {
                    card_filter(&Objects {
                        quant: Quant::One,
                        filter: i::Filter {
                            posts: Vec::new(),
                            ..o.filter.clone()
                        },
                    })?
                };
                let into = self.new_slot();
                self.among.insert(into, from);
                out.push(Effect::ChooseMatching {
                    from,
                    up_to,
                    filter,
                    total_mana_value,
                    enters: moves_onto_battlefield(then),
                    reveals: reveals_chosen(then),
                    into,
                });
                // Its "may" first ("You may reveal it and put it into
                // your hand"): the card chosen is the "may" taken, none
                // declined, so one chosen and then kept back, left among
                // "the rest", can't be.
                if let (Quant::One, Some(i::Effect::May { then: taken, .. })) =
                    (&o.quant, then.first())
                {
                    self.withs.push(Some(into));
                    // Only its own instructions are under the "may".
                    let outer = std::mem::replace(&mut self.in_may, true);
                    let taken = self.effects(taken);
                    self.in_may = outer;
                    let lowered = taken.and_then(|mut t| {
                        t.extend(self.effects(&then[1..])?);
                        Ok(t)
                    });
                    self.withs.pop();
                    out.extend(lowered?);
                    mark_blind(&mut out, into);
                    return Ok(out);
                }
                into
            }
            // "Choose two cards in your hand drawn this turn".
            [o] if o.filter.kind == Some(i::Noun::Card)
                && o.filter.alts.is_empty()
                && o.filter.posts
                    == [Post::In(i::Zone::Hand(i::Player::You)), Post::DrawnThisTurn]
                && matches!(o.quant, Quant::N(i::Amount::N(_)))
                && matches!(then.first(), Some(i::Effect::Choose(i::Choice::Objects(c)))
                    if c.quant == Quant::Introduced(0) && c.filter == i::Filter::default()) =>
            {
                let Quant::N(i::Amount::N(n)) = o.quant else {
                    unreachable!("matched")
                };
                let into = self.new_slot();
                self.sizes.insert(into, n);
                self.in_hand.insert(into);
                out.push(Effect::ChooseDrawn { n, into });
                self.withs.push(Some(into));
                let lowered = self.effects(&then[1..]);
                self.withs.pop();
                out.extend(lowered?);
                return Ok(out);
            }
            _ => return Err("With".into()),
        };
        self.withs.push(Some(slot));
        let lowered = self.effects(then);
        self.withs.pop();
        out.extend(lowered?);
        if self.made == Some(slot) {
            return Err("With a card no instruction makes".into());
        }
        // Living Wish's "a creature or land card you own from outside the
        // game", Commune with Dinosaurs's "a Dinosaur or land card from
        // among them": a card no search finds, which would otherwise be
        // none.
        if self.described.contains_key(&slot) && !self.searched.contains(&slot) {
            return Err("With a card no instruction finds".into());
        }
        mark_blind(&mut out, slot);
        Ok(out)
    }

    /// The effects of a spell or ability in order, where "Target player
    /// reveals their hand. You choose a nonland card from it. That player
    /// discards that card." is one instruction: the `With` binds the card
    /// chosen, and the discard names it.
    fn effects(&mut self, effects: &[i::Effect]) -> Result<Vec<Effect>, String> {
        let mut out = Vec::new();
        let mut k = 0;
        while k < effects.len() {
            if let (i::Effect::RevealHand(p), Some(i::Effect::With { bind, then })) =
                (&effects[k], effects.get(k + 1))
            {
                let who = self.who(p)?;
                let (
                    [card],
                    [i::Effect::Choose(i::Choice::Objects(chosen)), i::Effect::Discard {
                        who: discarder,
                        what,
                    }, rest @ ..],
                ) = (bind.as_slice(), then.as_slice())
                else {
                    return Err("Effect::With after RevealHand".into());
                };
                let blank = |o: &Objects, q: Quant| o.quant == q && o.filter.alts.is_empty();
                if !blank(chosen, Quant::Introduced(0)) || !blank(what, Quant::Bound(0)) {
                    return Err("a hand revealed and chosen from otherwise".into());
                }
                // The card comes from the revealed hand and the one who
                // discards is the one who revealed it.
                if self.who(discarder)? != who {
                    return Err("a card discarded by another player".into());
                }
                out.push(Effect::RevealChooseDiscard {
                    who,
                    filter: self.card_in_hand(card, who)?,
                });
                // The card chosen or found is bound for the rest, and
                // nothing the engine runs refers to it.
                self.withs.push(None);
                let rest = rest
                    .iter()
                    .map(|r| self.effect(r))
                    .collect::<Result<Vec<_>, _>>();
                self.withs.pop();
                out.extend(rest?.into_iter().flatten());
                k += 2;
                continue;
            }
            // "You may draw two additional cards. If you do, choose two
            // cards …": what follows "if you do" is the may's, followed
            // only if it was, its binders bound there.
            if let (i::Effect::May { who, then }, Some(next)) = (&effects[k], effects.get(k + 1)) {
                let more = match next {
                    i::Effect::IfYouDo(more) => Some(more.clone()),
                    i::Effect::With { bind, then: inner } => match inner.as_slice() {
                        [i::Effect::IfYouDo(more)] => Some(vec![i::Effect::With {
                            bind: bind.clone(),
                            then: more.clone(),
                        }]),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(more) = more {
                    let all: Vec<i::Effect> = then.iter().cloned().chain(more).collect();
                    out.extend(self.effect(&i::Effect::May {
                        who: who.clone(),
                        then: all,
                    })?);
                    k += 2;
                    continue;
                }
            }
            // "Put it onto the battlefield tapped if it's a land card.
            // Otherwise, put it into your hand": the second only if the
            // card didn't match (CR 608.2c, the rules of English).
            if let Some(i::Effect::Otherwise(otherwise)) = effects.get(k + 1) {
                let lowered = self.effect(&effects[k])?;
                let [Effect::IfCard {
                    card,
                    filter,
                    otherwise: false,
                    ..
                }] = lowered.as_slice()
                else {
                    return Err("an \"Otherwise\" after an \"if\" not of a card found".into());
                };
                let (card, filter) = (*card, filter.clone());
                out.extend(lowered);
                out.push(Effect::IfCard {
                    card,
                    filter,
                    otherwise: true,
                    then: self.effects(otherwise)?,
                });
                k += 2;
                continue;
            }
            // "~ deals 2 damage to any target. If this spell was kicked, it
            // deals 4 damage instead": the first only if it wasn't kicked,
            // the second only if it was (CR 702.33d).
            if let Some(i::Effect::Instead {
                cond: i::Condition::WasKicked,
                then,
            }) = effects.get(k + 1)
            {
                let unkicked = self.effect(&effects[k])?;
                let kicked = self.instead(&effects[k], &unkicked, then)?;
                out.push(Effect::Kicked {
                    was: false,
                    then: unkicked,
                });
                out.push(Effect::Kicked {
                    was: true,
                    then: kicked,
                });
                k += 2;
                continue;
            }
            // Void's "~ deals 3 damage instead if a nonland permanent left
            // the battlefield this turn": the first only if it didn't, the
            // second only if it did. Asked twice, once for each, which is
            // the same answer only because damage moves no permanent until
            // the state-based actions after the spell resolves (CR 704.3),
            // so only damage is lowered so.
            if let Some(i::Effect::Instead { cond, then }) = effects.get(k + 1) {
                if let Some(cond) = this_turn(cond) {
                    let cond = cond?;
                    let replaced = self.effect(&effects[k])?;
                    let instead = self.instead(&effects[k], &replaced, then)?;
                    let damage =
                        |e: &[Effect]| e.iter().all(|e| matches!(e, Effect::Damage { .. }));
                    if !damage(&replaced) || !damage(&instead) {
                        return Err(
                            "an instead, for what happened this turn, of other than damage".into(),
                        );
                    }
                    out.push(Effect::IfThisTurn {
                        cond: cond.clone(),
                        holds: false,
                        then: replaced,
                    });
                    out.push(Effect::IfThisTurn {
                        cond,
                        holds: true,
                        then: instead,
                    });
                    k += 2;
                    continue;
                }
            }
            out.extend(self.effect(&effects[k])?);
            k += 1;
        }
        Ok(out)
    }

    /// What an "instead" says in place of `replaced`, lowered to `lowered`:
    /// its own instructions, "it deals 4 damage instead" dealt to what
    /// the damage it replaces was, the same targets (Insight's
    /// `Recipient::Same`).
    fn instead(
        &mut self,
        replaced: &i::Effect,
        lowered: &[Effect],
        then: &[i::Effect],
    ) -> Result<Vec<Effect>, String> {
        match (replaced, lowered, then) {
            (
                i::Effect::Damage {
                    source: Some(was), ..
                },
                [Effect::Damage { to, .. }],
                [i::Effect::Damage {
                    source: Some(source),
                    n,
                    to: same,
                }],
            ) if same.as_slice() == [i::Recipient::Same] && source == was => {
                Ok(vec![Effect::Damage {
                    to: to.clone(),
                    n: self.resolved_amount(n)?,
                }])
            }
            // Anything else said again in full; a `Same` left in it is
            // refused where it is lowered.
            _ => self.effects(then),
        }
    }

    /// An instruction's amount, worked out as it is followed: a number,
    /// "that much" of the event, or the X a "where X is" counts.
    fn resolved_amount(&self, k: &i::Amount) -> Result<Amount, String> {
        Ok(match k {
            i::Amount::ThatMany if self.amount => Amount::ThatMuch,
            i::Amount::X if self.x.is_some() => self.x.clone().expect("an X"),
            // The X a cost set (CR 107.3a): a spell's, Blazing Shoal's by
            // the card its cost exiled, or the one announced for an {X}
            // in an activation cost. One with none set refuses as it
            // reads it.
            i::Amount::X if self.cost_x => Amount::X,
            i::Amount::N(n) => Amount::N(*n),
            i::Amount::CountOf(o) => match (counters_on_itself(o), cards_in_graveyards(o)) {
                (Some(kind), _) => Amount::CountersOnItself(kind?),
                (None, Some(counted)) => counted?,
                (None, None) => Amount::CountOf(counted(o)?),
            },
            other => return Err(format!("an amount of {}", short(other))),
        })
    }

    /// An amount an "if" compares: one `resolved_amount` reads, or the
    /// power, toughness or mana value of one permanent: the ability's
    /// source or a target permanent. A target spell or a card found where
    /// it isn't one is refused here, since the game reads only
    /// permanents' characteristics.
    fn compared_amount(&mut self, k: &i::Amount) -> Result<Amount, String> {
        let mut permanent = |o: &Objects| match self.obj(o)? {
            obj @ Obj::Itself => Ok(obj),
            obj @ Obj::Target(s) if matches!(self.program.targets[s], TargetSpec::Permanent(_)) => {
                Ok(obj)
            }
            _ => Err(format!(
                "an \"if\" comparing an amount of {}",
                short(&o.quant)
            )),
        };
        Ok(match k {
            i::Amount::PowerOf(o) => Amount::PowerOf(permanent(o)?),
            i::Amount::ToughnessOf(o) => Amount::ToughnessOf(permanent(o)?),
            i::Amount::ManaValueOf(o) => Amount::ManaValueOfObj(permanent(o)?),
            k => self.resolved_amount(k)?,
        })
    }

    /// "a nonland card from it with mana value 3 or less", from `who`'s
    /// hand.
    fn card_in_hand(&mut self, o: &Objects, who: Who) -> Result<CardFilter, String> {
        let mut rest = Vec::new();
        let mut mana_value = None;
        for post in &o.filter.posts {
            match post {
                Post::FromHand(p) if self.who(p)? == who => {}
                Post::With(i::Compare {
                    stat: i::Stat::ManaValue,
                    op,
                    value: i::Amount::N(v),
                }) => {
                    let cmp = cmp(*op);
                    mana_value = Some(Pred::ManaValue(cmp, *v));
                }
                other => rest.push(other.clone()),
            }
        }
        if !rest.is_empty() {
            return Err(format!("a card from a hand {}", short(&rest[0])));
        }
        let mut f = card_filter(&Objects {
            quant: o.quant.clone(),
            filter: i::Filter {
                posts: Vec::new(),
                ..o.filter.clone()
            },
        })?;
        if let Some(p) = mana_value {
            for alt in &mut f.0 {
                alt.0.push(p.clone());
            }
        }
        Ok(f)
    }
}

/// "target creature", "target creature with mana value 3 or less": a
/// permanent by its characteristics, and "you control" or "an opponent
/// controls"; anything relative to a source is refused, since a spell's
/// has none on the battlefield.
fn target_filter(o: &Objects) -> Result<Filter, String> {
    // "target permanent": any of them, by whatever else the filter says.
    let filter = match o.filter.kind {
        None => o.filter.clone(),
        Some(i::Noun::Permanent) => i::Filter {
            kind: None,
            alts: if o.filter.alts.is_empty() {
                vec![vec![]]
            } else {
                o.filter.alts.clone()
            },
            ..o.filter.clone()
        },
        _ => return Err("a target of a kind of object".into()),
    };
    let f = objects(&Objects {
        quant: Quant::All,
        filter,
    })?;
    if f.0.iter().any(|p| {
        !matches!(
            p,
            Pred::Is(_)
                | Pred::IsNot(_)
                | Pred::IsAny(_)
                | Pred::YouControl
                | Pred::OpponentsControl
                | Pred::ManaValue(..)
                // Read from combat, not from the source (CR 508.1k).
                | Pred::Attacking
        )
    }) {
        return Err("a target relative to its source".into());
    }
    Ok(f)
}

/// "Attacking that player or a planeswalker they control": the player a
/// "for each opponent" names.
fn attacks_that_player(r: &i::Recipient) -> bool {
    let that = |r: &i::Recipient| *r == i::Recipient::Player(i::Player::That);
    match r {
        i::Recipient::Or(rs) => {
            rs.first().is_some_and(that)
                && rs[1..].iter().all(|r| {
                    matches!(r, i::Recipient::Objects(o)
                if o.filter.alts == [vec![i::Adj::Type("planeswalker".into())]]
                    && o.filter.posts == [Post::Controller(i::Player::That)])
                })
        }
        r => that(r),
    }
}

/// A token's text, as a card's (CR 111.3): its name its subtypes and
/// "Token" (CR 111.4), its keywords its rules text. A predefined token, or
/// one with abilities other than keywords, isn't read yet.
fn token_text(t: &i::Token) -> Result<crate::deck::CardText, String> {
    if t.named.is_some() || !t.abilities.is_empty() || t.attacking.is_some() {
        return Err("a token with abilities, a name or attacking".into());
    }
    let mut types = Vec::new();
    let mut supertypes = Vec::new();
    let mut subtypes = Vec::new();
    for w in &t.types {
        if card_type(w).is_some() {
            types.push(capitalised(w));
        } else if supertype(w).is_some() {
            supertypes.push(capitalised(w));
        } else {
            subtypes.push(capitalised(w));
        }
    }
    let mut keywords = Vec::new();
    for k in &t.keywords {
        if k.param.is_some() {
            return Err(format!("a token with keyword {}", k.name));
        }
        keywords.push(k.name.clone());
    }
    let rules = keywords.join(", ");
    let mut c = rules.chars();
    let rules = c
        .next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default();
    let pt = match &t.pt {
        None => None,
        Some(p) => Some(fixed(p).filter(|_| !p.signed).ok_or("a token's power")?),
    };
    let colors = t
        .colors
        .iter()
        .map(|c| color(c).ok_or_else(|| short(c)))
        .collect::<Result<Vec<_>, _>>()?;
    let front: Vec<String> = supertypes.into_iter().chain(types).collect();
    Ok(crate::deck::CardText {
        name: format!("{} Token", subtypes.join(" ")),
        type_line: if subtypes.is_empty() {
            front.join(" ")
        } else {
            format!("{} — {}", front.join(" "), subtypes.join(" "))
        },
        mana_cost: String::new(),
        oracle: crate::deck::Oracle::OneFace(rules),
        mana_value: 0,
        identity: colors.clone(),
        colors,
        pt,
    })
}

/// "target spell with mana value 2", "target noncreature spell": by the
/// card's types and mana value.
/// "activated or triggered ability", and no more said of it.
fn ability_spec(o: &Objects) -> Result<TargetSpec, String> {
    if let Some(post) = o.filter.posts.first() {
        return Err(format!("a target ability {}", short(post)));
    }
    if o.filter.or.is_some() {
        return Err("a target ability or another kind of object".into());
    }
    let (mut activated, mut triggered) = (false, false);
    for alt in &o.filter.alts {
        match alt.as_slice() {
            [i::Adj::Ability(i::AbilityKind::Activated)] => activated = true,
            [i::Adj::Ability(i::AbilityKind::Triggered)] => triggered = true,
            other => return Err(format!("a target ability {other:?}")),
        }
    }
    if o.filter.alts.is_empty() {
        (activated, triggered) = (true, true);
    }
    Ok(TargetSpec::Ability {
        activated,
        triggered,
    })
}

/// "Target card from a graveyard", "target creature card in your
/// graveyard": `None` if the target isn't a card in a graveyard.
fn graveyard_card(o: &Objects) -> Option<Result<TargetSpec, String>> {
    let f = &o.filter;
    let whose = f.posts.iter().find_map(|p| match p {
        Post::In(i::Zone::AGraveyard) => Some(Ok(Whose::Each)),
        Post::In(i::Zone::Graveyard(i::Player::You)) => Some(Ok(Whose::Yours)),
        Post::In(i::Zone::Graveyard(i::Player::EachOpponent)) => Some(Ok(Whose::Opponents)),
        Post::In(i::Zone::Graveyard(w)) => Some(Err(format!(
            "a target card in the graveyard of Player::{}",
            short(w)
        ))),
        _ => None,
    })?;
    Some((|| {
        let whose = whose?;
        if f.posts.len() != 1 {
            return Err("a target card in a graveyard with more than where it is".into());
        }
        let filter = card_filter(&Objects {
            quant: Quant::One,
            filter: i::Filter {
                posts: Vec::new(),
                ..f.clone()
            },
        })?;
        let filter = if filter.0.is_empty() {
            CardFilter(vec![Filter::default()])
        } else {
            filter
        };
        Ok(TargetSpec::GraveyardCard { filter, whose })
    })())
}

fn spell_filter(o: &Objects) -> Result<CardFilter, String> {
    let mut mana_value = None;
    let mut alts = o.filter.alts.clone();
    for post in &o.filter.posts {
        match post {
            // Quirion Dryad's "a spell that's white, blue, black, or red":
            // the same alternatives said after the noun.
            Post::ThatIs(that)
                if alts.is_empty()
                    && !that.alts.is_empty()
                    && that.kind.is_none()
                    && that.posts.is_empty()
                    && that.or.is_none() =>
            {
                alts = that.alts.clone();
            }
            Post::With(i::Compare {
                stat: i::Stat::ManaValue,
                op,
                value: i::Amount::N(v),
            }) => {
                let cmp = cmp(*op);
                mana_value = Some(Pred::ManaValue(cmp, *v));
            }
            other => return Err(format!("a target spell {}", short(other))),
        }
    }
    let mut f = if alts.is_empty() {
        CardFilter(vec![Filter::default()])
    } else {
        card_filter(&Objects {
            quant: Quant::One,
            filter: i::Filter {
                kind: Some(i::Noun::Card),
                posts: Vec::new(),
                alts,
                ..o.filter.clone()
            },
        })?
    };
    if let Some(p) = mana_value {
        for alt in &mut f.0 {
            alt.0.push(p.clone());
        }
    }
    Ok(f)
}

/// "+1/+1", "-1/-1", "-0/-1" and named counters.
pub fn counter(kind: &i::CounterKind) -> Option<Counter> {
    match kind {
        i::CounterKind::Pt(p) if p.signed => match (p.power, p.toughness) {
            (PtPart::N(1), PtPart::N(1)) => Some(Counter::PlusOne),
            (PtPart::N(-1), PtPart::N(-1)) => Some(Counter::MinusOne),
            (PtPart::N(a), PtPart::N(b)) => Some(Counter::Pt(a, b)),
            _ => None,
        },
        i::CounterKind::Named(n) => Some(Counter::named(n)),
        _ => None,
    }
}

/// An activated ability's cost, of the kinds the engine pays (CR 602.2b,
/// 601.2h): tapping its source, life (CR 119.4), sacrificing the source
/// (CR 701.21a), and cards of its activator's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivationCost {
    pub tap: bool,
    pub life: u32,
    pub sacrifice: bool,
    /// Siege-Gang Commander's "Sacrifice a Goblin": that many permanents
    /// the filter matches, of the activator's, the source among them if it
    /// matches (CR 602.2b, 601.2h, 701.21a).
    pub sacrifice_others: Option<(u32, Filter)>,
    /// "{1}, {T}: …": mana paid as the ability is activated (CR 602.2b,
    /// 601.2g–h).
    pub mana: Option<crate::mana::ManaCost>,
    /// "Discard a creature card", "exile two cards from your graveyard".
    pub cards: Option<CardsPaid>,
    /// Triskelion's "Remove a +1/+1 counter from this creature": counters
    /// taken off its source as it is activated, so one without as many
    /// can't be (CR 602.2b, 118.3).
    pub remove_counters: Option<(Counter, u32)>,
    /// How many {X} its mana cost has, each paid with the X announced as
    /// it is activated, the same X for each (CR 107.3a, 107.3i). Pernicious
    /// Deed's "{X}" is 1, Blast Zone's "{X}{X}" 2.
    pub x: u32,
}

/// Cards an activated ability's cost moves out of a zone of its
/// activator's: `count` of those `filter` matches, chosen as it is paid
/// (CR 601.2h).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardsPaid {
    pub how: PaidWith,
    pub count: u32,
    pub filter: CardFilter,
    /// Blazing Shoal's "a red card with mana value X": the card's mana
    /// value is the spell's X, the X its controller announced and the
    /// card they exile then had to have (CR 107.3a, 601.2b).
    x_is_mana_value: bool,
}

impl CardsPaid {
    pub fn x_is_mana_value(&self) -> bool {
        self.x_is_mana_value
    }
}

/// Where the cards a cost names come from, and where they go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaidWith {
    /// From the hand to the graveyard (CR 701.9a): Survival of the
    /// Fittest.
    Discard,
    /// From the graveyard to exile: Grim Lavamancer.
    ExileFromGraveyard,
    /// From the hand to exile: Blazing Shoal's alternative cost.
    ExileFromHand,
}

/// An activated ability that isn't a mana ability (CR 602), lowered: its
/// cost, and its instructions with the targets they name, chosen as it is
/// activated (CR 602.2b, 601.2c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activated {
    pub cost: ActivationCost,
    pub program: Program,
    pub limits: ActivationLimits,
}

/// When an activated ability may be activated, beyond any time its
/// controller has priority (CR 602.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivationLimits {
    /// "Activate only once each turn" (CR 602.5b).
    pub once_per_turn: bool,
    /// "Activate only if …": each condition must hold as it would begin
    /// to be activated, or it is prohibited (CR 602.5).
    pub only_if: Vec<OnlyIf>,
    /// "Activate only as a sorcery": in its controller's main phase with
    /// the stack empty (CR 602.5d, 307.1).
    pub sorcery_speed: bool,
    /// Power-up's "Activate this ability only once", of this object (CR
    /// 702.193a), and its cost reduced by the permanent's mana cost if it
    /// entered this turn.
    pub power_up: bool,
}

/// An activated ability's cost and effect, with the `Let` that binds its
/// targets, or the node with no lowering yet.
pub fn activated(
    bind: &[Objects],
    cost: &[i::Cost],
    effect: &[i::Effect],
    limits: &[i::Limit],
) -> Result<Activated, String> {
    let mut limited = ActivationLimits::default();
    for l in limits {
        match l {
            i::Limit::OncePerTurn => limited.once_per_turn = true,
            i::Limit::OnlyIf(c) => limited.only_if.push(only_if(c)?),
            other => return Err(format!("an activated ability limited by {}", short(other))),
        }
    }
    let cost = activation_cost(cost)?;
    let mut scope = Scope::new(bind);
    scope.cost_x = cost.x > 0;
    let effects = scope.effects(effect)?;
    scope.program.effects = effects;
    Ok(Activated {
        cost,
        program: scope.program,
        limits: limited,
    })
}

/// What an "Activate only if" asks of the game, read from the ability's
/// controller (CR 602.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlyIf {
    /// "an opponent lost life this turn".
    OpponentLostLife,
    /// Threshold's "there are seven or more cards in your graveyard".
    CardsInYourGraveyard(Count),
    /// "an opponent controls more lands than you": some opponent controls
    /// more permanents the filter matches than its controller does.
    OpponentControlsMore(Filter),
}

fn only_if(c: &i::Condition) -> Result<OnlyIf, String> {
    Ok(match c {
        i::Condition::DidThisTurn(e)
            if matches!(&**e, i::Event::LosesLife(i::Player::AnOpponent)) =>
        {
            OnlyIf::OpponentLostLife
        }
        i::Condition::AmountIs {
            n: i::Amount::CountOf(o),
            op,
            value: i::Amount::N(v),
        } => match cards_in_graveyards(o) {
            Some(Ok(Amount::CardsInGraveyards {
                whose: Whose::Yours,
                named: None,
                filter,
            })) if filter == CardFilter(vec![Filter::default()]) => {
                OnlyIf::CardsInYourGraveyard(count_of(*op, *v)?)
            }
            Some(Err(node)) => return Err(node),
            _ => return Err("an activated ability limited by OnlyIf(AmountIs)".into()),
        },
        i::Condition::ControlsMore {
            who: i::Player::AnOpponent,
            what,
            than: i::Player::You,
        } => {
            let f = objects(&Objects {
                quant: Quant::All,
                filter: what.clone(),
            })?;
            // Counted per player, so a filter that says whose, or reads
            // its source, would count something else.
            if f.0
                .iter()
                .any(|p| !matches!(p, Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_)))
            {
                return Err(
                    "an activated ability limited by OnlyIf(ControlsMore) of a relative filter"
                        .into(),
                );
            }
            OnlyIf::OpponentControlsMore(f)
        }
        other => {
            return Err(format!(
                "an activated ability limited by OnlyIf({})",
                short(other)
            ))
        }
    })
}

fn activation_cost(cost: &[i::Cost]) -> Result<ActivationCost, String> {
    let mut paid = ActivationCost::default();
    for c in cost {
        match c {
            i::Cost::Tap => paid.tap = true,
            i::Cost::PayLife(i::Amount::N(n)) => paid.life += n,
            i::Cost::Sacrifice(o) if itself(o) => paid.sacrifice = true,
            i::Cost::Sacrifice(o) if paid.sacrifice_others.is_none() => {
                paid.sacrifice_others = Some(permanents_paid(o)?)
            }
            i::Cost::Mana(m) if paid.mana.is_none() => {
                // The {X}s are counted apart, since the X is announced as
                // it is activated; any other variable symbol refuses in
                // `from_symbols`.
                let (xs, rest): (Vec<_>, Vec<_>) = m.0.iter().cloned().partition(|s| s == "X");
                paid.x = xs.len() as u32;
                paid.mana = Some(crate::mana::ManaCost::from_symbols(&i::Mana(rest))?)
            }
            i::Cost::Discard(o) if paid.cards.is_none() && o.filter.posts.is_empty() => {
                paid.cards = Some(cards_paid(PaidWith::Discard, o, &o.filter.posts)?)
            }
            i::Cost::Exile(o)
                if paid.cards.is_none()
                    && o.filter.posts == [Post::In(i::Zone::Graveyard(i::Player::You))] =>
            {
                paid.cards = Some(cards_paid(PaidWith::ExileFromGraveyard, o, &[])?)
            }
            i::Cost::RemoveCounters {
                n: i::Amount::N(n),
                kind,
                from,
            } if paid.remove_counters.is_none() && itself(from) => {
                let kind = counter(kind).ok_or("an activation cost of a counter")?;
                paid.remove_counters = Some((kind, *n));
            }
            other => return Err(format!("an activation cost of {}", short(other))),
        }
    }
    // The source, sacrificed as it is activated, would be counted again
    // among the others.
    if paid.sacrifice && paid.sacrifice_others.is_some() {
        return Err("an activation cost sacrificing itself and another".into());
    }
    Ok(paid)
}

/// "A creature card", "two cards from your graveyard": how many, and
/// which by their characteristics, less the zone `rest` leaves of its
/// posts.
fn cards_paid(how: PaidWith, o: &Objects, rest: &[Post]) -> Result<CardsPaid, String> {
    let count = match &o.quant {
        Quant::One => 1,
        Quant::N(i::Amount::N(n)) => *n,
        q => return Err(format!("a cost of {} cards", short(q))),
    };
    // "Two cards": any card, which a filter of no alternatives isn't.
    let filter = if o.filter.alts.is_empty() && o.filter.kind == Some(i::Noun::Card) {
        CardFilter(vec![Filter::default()])
    } else {
        card_filter(&Objects {
            quant: Quant::One,
            filter: i::Filter {
                posts: rest.to_vec(),
                ..o.filter.clone()
            },
        })?
    };
    Ok(CardsPaid {
        how,
        count,
        filter,
        x_is_mana_value: false,
    })
}

/// "You may reveal this card from your opening hand. If you do, at the
/// beginning of the first upkeep, create a 1/1 red Phyrexian Goblin
/// creature token with haste" (CR 103.6b): the delayed trigger's
/// instructions.
pub fn opening_reveal(effects: &[i::Effect]) -> Result<Program, String> {
    let [i::Effect::Delayed {
        step: i::Step::Upkeep(i::Whose::First),
        then,
    }] = effects
    else {
        return Err("a reveal from the opening hand other than for the first upkeep".into());
    };
    let mut scope = Scope::new(&[]);
    let effects = scope.effects(then)?;
    if !scope.program.targets.is_empty() {
        return Err("a reveal from the opening hand with a target".into());
    }
    Ok(Program {
        targets: Vec::new(),
        effects,
    })
}

/// Equip's "[Cost]: Attach this permanent to target creature you control.
/// Activate only as a sorcery" (CR 702.6a), `None` if the keyword isn't
/// equip, and the node it can't lower if it equips only some creatures
/// ("Equip legendary creature {3}").
pub fn equip(k: &i::Keyword) -> Option<Result<Activated, String>> {
    if !k.name.eq_ignore_ascii_case("equip") {
        return None;
    }
    Some(match &k.param {
        Some(i::KeywordParam::Cost(cost)) => activation_cost(cost).map(|cost| Activated {
            cost,
            program: Program {
                targets: vec![TargetSpec::Permanent(Filter(vec![
                    Pred::Is(Prop::Type(CardType::Creature)),
                    Pred::YouControl,
                ]))],
                effects: vec![Effect::Attach {
                    what: Obj::Itself,
                    to: Obj::Target(0),
                }],
            },
            limits: ActivationLimits {
                sorcery_speed: true,
                ..ActivationLimits::default()
            },
        }),
        _ => Err("equip to only some creatures".into()),
    })
}

/// "The number of fuse counters on this artifact": the counter's kind,
/// or `None` for any other count.
fn counters_on_itself(o: &Objects) -> Option<Result<Counter, String>> {
    match (&o.quant, &o.filter) {
        (
            Quant::Plural,
            i::Filter {
                kind: Some(i::Noun::Counter(Some(kind))),
                posts,
                alts,
                or: None,
            },
        ) if alts.is_empty() && matches!(posts.as_slice(), [Post::On(on)] if itself(on)) => {
            Some(counter(kind).ok_or_else(|| "a count of an unknown counter".to_string()))
        }
        _ => None,
    }
}

fn itself(o: &Objects) -> bool {
    o.quant == Quant::SelfRef && o.filter == i::Filter::default()
}

/// What a spell's total cost asks beyond mana (CR 601.2f), paid with it
/// as the spell is cast (CR 601.2h).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OtherCosts {
    /// CR 119.4.
    pub life: u32,
    /// "Sacrifice a creature", "sacrifice two Mountains": that many
    /// permanents the filter matches, of the caster's (CR 701.21a).
    pub sacrifice: Option<(u32, Filter)>,
    /// What the cost returns to its owner's hand.
    pub returned: Option<Returned>,
    /// The cards it exiles from the hand (CR 601.2h).
    pub cards: Option<CardsPaid>,
}

/// Permanents a spell's cost returns to their owner's hand (CR 601.2h).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Returned {
    /// Gush's "return two Islands you control to their owner's hand":
    /// that many permanents the filter matches, of the caster's.
    Permanents(u32, Filter),
    /// Sneak's "returning an unblocked creature you control to its
    /// owner's hand" (CR 702.190a): an attacking creature of the
    /// caster's that no creature was declared blocking (CR 509.1h).
    UnblockedCreature,
}

/// When a card may be cast for an alternative cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastWhen {
    /// With the timing its type has (CR 117.1a), or flash's.
    AsItsType,
    /// Sneak's "any time you could cast an instant during your declare
    /// blockers step" (CR 702.190a).
    YourDeclareBlockers,
}

/// Who orders cards put into a library together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arranged {
    /// Their owner, as they like (CR 401.4).
    Chosen,
    /// Chance: "in a random order".
    Random,
}

/// Where a card is cast from by an alternative cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastFrom {
    Hand,
    Graveyard,
    /// Madness's: from exile, only as its own trigger resolves (CR
    /// 702.35a, 608.2g), never offered with priority.
    Madness,
}

/// A cost a card may be cast for rather than its mana cost (CR 118.9),
/// from the zone it says: Fireblast's "You may sacrifice two Mountains
/// rather than pay this spell's mana cost" from a hand, flashback from a
/// graveyard (CR 702.34a).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alternative {
    pub from: CastFrom,
    /// Its mana, `{0}` when it has none, to which cost changes still apply
    /// (CR 118.9d, 601.2f).
    pub mana: crate::mana::ManaCost,
    pub other: OtherCosts,
    /// Flashback's "If the flashback cost was paid, exile this card
    /// instead of putting it anywhere else any time it would leave the
    /// stack" (CR 702.34a).
    pub exiled: bool,
    /// Flashback's "if the resulting spell is an instant or sorcery
    /// spell" (CR 702.34a).
    pub instant_or_sorcery: bool,
    pub when: CastWhen,
    /// Evoke's: the permanent the spell becomes is sacrificed as it enters
    /// (CR 702.74a, `evoke`'s trigger).
    pub evoked: bool,
}

/// A cost written as Insight reads it, of the kinds a spell's cost can be
/// paid with here: mana, life and sacrificing permanents.
fn spell_cost(cost: &[i::Cost]) -> Result<(crate::mana::ManaCost, OtherCosts), String> {
    let mut mana = None;
    let mut other = OtherCosts::default();
    for c in cost {
        match c {
            i::Cost::Mana(m) if mana.is_none() => {
                mana = Some(crate::mana::ManaCost::from_symbols(m)?)
            }
            i::Cost::PayLife(i::Amount::N(n)) => other.life += n,
            i::Cost::Sacrifice(o) if other.sacrifice.is_none() => {
                other.sacrifice = Some(permanents_paid(o)?);
            }
            i::Cost::ReturnToHand(o) if other.returned.is_none() => {
                let (n, filter) = permanents_paid(o)?;
                other.returned = Some(Returned::Permanents(n, filter));
            }
            // "Exile a red card with mana value X from your hand".
            i::Cost::Exile(o)
                if other.cards.is_none()
                    && o.filter.posts.last() == Some(&Post::In(i::Zone::Hand(i::Player::You))) =>
            {
                let (_, rest) = o.filter.posts.split_last().expect("a hand");
                let x = Post::With(i::Compare {
                    stat: i::Stat::ManaValue,
                    op: i::Op::Eq,
                    value: i::Amount::X,
                });
                let (rest, x_is_mana_value) = match rest {
                    [r @ .., last] if *last == x => (r, true),
                    r => (r, false),
                };
                if o.quant != Quant::One && x_is_mana_value {
                    return Err("X the mana value of several cards".into());
                }
                let mut paid = cards_paid(PaidWith::ExileFromHand, o, rest)?;
                paid.x_is_mana_value = x_is_mana_value;
                other.cards = Some(paid);
            }
            other => return Err(format!("a spell's cost of {}", short(other))),
        }
    }
    Ok((mana.unwrap_or_default(), other))
}

/// "Two Mountains", "two Islands you control": how many permanents a cost
/// moves, and which, of the caster's.
fn permanents_paid(o: &Objects) -> Result<(u32, Filter), String> {
    let n = match &o.quant {
        Quant::One => 1,
        Quant::N(i::Amount::N(n)) => *n,
        q => return Err(format!("a cost of {} permanents", short(q))),
    };
    let mut f = objects(&Objects {
        quant: Quant::All,
        filter: o.filter.clone(),
    })?;
    // Read against each permanent as if it were the source, so anything
    // relative to the source, or to a player other than its controller,
    // would match the wrong permanents or none.
    if let Some(p) = f.0.iter().find(|p| {
        !matches!(
            p,
            Pred::Is(_)
                | Pred::IsNot(_)
                | Pred::IsAny(_)
                | Pred::YouControl
                | Pred::Nontoken
                | Pred::ManaValue(..)
                | Pred::Power(..)
                | Pred::Attacking
        )
    }) {
        return Err(format!("a cost of permanents {p:?}"));
    }
    if !f.0.contains(&Pred::YouControl) {
        f.0.push(Pred::YouControl);
    }
    Ok((n, f))
}

/// What an instruction has a player sacrifice: the permanents of theirs
/// a filter matches, read without a source, so only what a permanent is
/// on its own (CR 701.21a: they can sacrifice only their own).
fn sacrificed(o: &Objects) -> Result<Filter, String> {
    // "Permanents": any of them, by whatever else the filter says.
    let filter = match o.filter.kind {
        None => o.filter.clone(),
        Some(i::Noun::Permanent) => i::Filter {
            kind: None,
            alts: if o.filter.alts.is_empty() {
                vec![vec![]]
            } else {
                o.filter.alts.clone()
            },
            ..o.filter.clone()
        },
        _ => return Err("sacrificing a kind of object".into()),
    };
    let f = objects(&Objects {
        quant: Quant::All,
        filter,
    })?;
    if let Some(p) = f.0.iter().find(|p| {
        !matches!(
            p,
            Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_) | Pred::Nontoken | Pred::ManaValue(..)
        )
    }) {
        return Err(format!("sacrificing permanents {p:?}"));
    }
    Ok(f)
}

/// "Flashback {1}{U}, Pay 3 life", "Flashback—Sacrifice a creature": `None`
/// if the keyword isn't flashback with a cost, or the node it can't
/// lower.
pub fn flashback(k: &i::Keyword) -> Option<Result<Alternative, String>> {
    if !k.name.eq_ignore_ascii_case("flashback") {
        return None;
    }
    let Some(i::KeywordParam::Cost(cost)) = &k.param else {
        return Some(Err("flashback without a cost".into()));
    };
    Some(spell_cost(cost).map(|(mana, other)| Alternative {
        from: CastFrom::Graveyard,
        mana,
        other,
        exiled: true,
        instant_or_sorcery: true,
        when: CastWhen::AsItsType,
        evoked: false,
    }))
}

/// "Evoke {2}{U}": "You may cast this card by paying [cost] rather than
/// paying its mana cost" and "When this permanent enters, if its evoke
/// cost was paid, its controller sacrifices it" (CR 702.74a). `None` if
/// the keyword isn't evoke; the node it can't lower if its cost is one a
/// spell's cost can't be paid with.
pub fn evoke(k: &i::Keyword) -> Option<Result<(Alternative, Triggered), String>> {
    if !k.name.eq_ignore_ascii_case("evoke") {
        return None;
    }
    let Some(i::KeywordParam::Cost(cost)) = &k.param else {
        return Some(Err("evoke without a cost".into()));
    };
    Some(spell_cost(cost).map(|(mana, other)| {
        (
            Alternative {
                from: CastFrom::Hand,
                mana,
                other,
                exiled: false,
                instant_or_sorcery: false,
                when: CastWhen::AsItsType,
                evoked: true,
            },
            Triggered {
                event: TriggerEvent::Enters(Filter(vec![Pred::Itself])),
                program: Program {
                    targets: Vec::new(),
                    effects: vec![Effect::Sacrifice(Obj::Itself)],
                },
                condition: Some(Intervening::Evoked),
                during: None,
            },
        )
    }))
}

/// "Sneak {3}{G}": "Any time you could cast an instant during your
/// declare blockers step, you may cast this spell by paying [cost] and
/// returning an unblocked creature you control to its owner's hand
/// rather than paying this spell's mana cost" (CR 702.190a). `None` if
/// the keyword isn't sneak; the node it can't lower if its cost is more
/// than mana.
pub fn sneak(k: &i::Keyword) -> Option<Result<Alternative, String>> {
    if !k.name.eq_ignore_ascii_case("sneak") {
        return None;
    }
    let Some(i::KeywordParam::Cost(cost)) = &k.param else {
        return Some(Err("sneak without a cost".into()));
    };
    let mana = match cost.as_slice() {
        [i::Cost::Mana(m)] => crate::mana::ManaCost::from_symbols(m),
        _ => Err("sneak with a cost other than mana".into()),
    };
    Some(mana.map(|mana| Alternative {
        from: CastFrom::Hand,
        mana,
        other: OtherCosts {
            returned: Some(Returned::UnblockedCreature),
            ..OtherCosts::default()
        },
        exiled: false,
        instant_or_sorcery: false,
        when: CastWhen::YourDeclareBlockers,
        evoked: false,
    }))
}

/// Suspend's number of time counters and its cost (CR 702.62a), and the
/// instructions of its two triggered abilities: at its owner's upkeep,
/// "remove a time counter", and when the last is removed, "you may cast
/// it without paying its mana cost".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suspend {
    pub counters: u32,
    pub cost: crate::mana::ManaCost,
    pub upkeep: Program,
    pub last: Program,
}

/// "Suspend 4—{U}": `None` if the keyword isn't suspend, the node it can't
/// lower if its number or cost is more than a number and mana.
pub fn suspend(k: &i::Keyword) -> Option<Result<Suspend, String>> {
    if !k.name.eq_ignore_ascii_case("suspend") {
        return None;
    }
    let Some(i::KeywordParam::NCost { n, cost }) = &k.param else {
        return Some(Err("suspend without a number and a cost".into()));
    };
    let i::Amount::N(counters) = n else {
        return Some(Err(format!("suspend {}", short(n))));
    };
    let cost = match cost.as_slice() {
        [i::Cost::Mana(m)] => crate::mana::ManaCost::from_symbols(m),
        _ => Err("suspend with a cost other than mana".into()),
    };
    Some(cost.map(|cost| Suspend {
        counters: *counters,
        cost,
        upkeep: Program {
            targets: Vec::new(),
            effects: vec![Effect::RemoveTimeCounter],
        },
        last: Program {
            targets: Vec::new(),
            effects: vec![Effect::May(Who::You, vec![Effect::CastSuspended])],
        },
    }))
}

/// "Madness {2}{G}": "If a player would discard this card, that player
/// discards it, but exiles it instead of putting it into their graveyard"
/// and "When this card is exiled this way, its owner may cast it by paying
/// [cost] rather than paying its mana cost. If that player doesn't, they
/// put this card into their graveyard" (CR 702.35a). Its cost follows
/// the rules for alternative costs (CR 702.35b). `None` if the keyword
/// isn't madness; the node it can't lower if its cost is one a spell's
/// cost can't be paid with.
pub fn madness(k: &i::Keyword) -> Option<Result<(Alternative, Triggered), String>> {
    if !k.name.eq_ignore_ascii_case("madness") {
        return None;
    }
    let Some(i::KeywordParam::Cost(cost)) = &k.param else {
        return Some(Err("madness without a cost".into()));
    };
    Some(spell_cost(cost).map(|(mana, other)| {
        (
            Alternative {
                from: CastFrom::Madness,
                mana,
                other,
                exiled: false,
                instant_or_sorcery: false,
                when: CastWhen::AsItsType,
                evoked: false,
            },
            Triggered {
                event: TriggerEvent::ExiledByMadness,
                program: Program {
                    targets: Vec::new(),
                    effects: vec![
                        Effect::May(Who::You, vec![Effect::CastMadness]),
                        Effect::MadnessToGraveyard,
                    ],
                },
                condition: None,
                during: None,
            },
        )
    }))
}

/// "Kicker {4}": the optional additional cost a spell may be cast with
/// (CR 702.33a, 601.2b). `None` if the keyword isn't kicker; the node it
/// can't lower if its cost is more than mana.
pub fn kicker(k: &i::Keyword) -> Option<Result<crate::mana::ManaCost, String>> {
    if !k.name.eq_ignore_ascii_case("kicker") {
        return None;
    }
    let Some(i::KeywordParam::Cost(cost)) = &k.param else {
        return Some(Err("kicker without a cost".into()));
    };
    Some(match cost.as_slice() {
        [i::Cost::Mana(m)] => crate::mana::ManaCost::from_symbols(m),
        _ => Err("kicker with a cost other than mana".into()),
    })
}

/// "Plot {2}{G}": the cost of exiling the card from hand as a plotted card
/// (CR 702.170a). `None` if the keyword isn't plot; the node it can't
/// lower if its cost is more than mana.
pub fn plot(k: &i::Keyword) -> Option<Result<crate::mana::ManaCost, String>> {
    if !k.name.eq_ignore_ascii_case("plot") {
        return None;
    }
    let Some(i::KeywordParam::Cost(cost)) = &k.param else {
        return Some(Err("plot without a cost".into()));
    };
    Some(match cost.as_slice() {
        [i::Cost::Mana(m)] => crate::mana::ManaCost::from_symbols(m),
        _ => Err("plot with a cost other than mana".into()),
    })
}

/// "You may sacrifice two Mountains rather than pay this spell's mana
/// cost": `None` if the static isn't one alternative cost.
pub fn alternative_cost(parts: &[i::Static]) -> Option<Result<Alternative, String>> {
    let [i::Static::AlternativeCost(cost)] = parts else {
        return None;
    };
    Some(spell_cost(cost).map(|(mana, other)| Alternative {
        from: CastFrom::Hand,
        mana,
        other,
        exiled: false,
        instant_or_sorcery: false,
        when: CastWhen::AsItsType,
        evoked: false,
    }))
}

/// Marks the reveal that binds `slot` blind when every other instruction
/// with it only moves the cards or groups them, never reading what they
/// are: Fact or Fiction's "An opponent separates those cards into two
/// piles. Put one pile into your hand and the other into your graveyard."
/// Anything else, or a filter over them, leaves it seeing.
fn mark_blind(effects: &mut [Effect], slot: Slot) {
    let moves_only = effects.iter().all(|e| match e {
        Effect::RevealTop { into, .. } => *into == slot,
        Effect::SeparatePiles { .. } | Effect::ChoosePile { .. } | Effect::Shuffle(_) => true,
        Effect::ToHand { cards, .. }
        | Effect::ToGraveyard { cards, .. }
        | Effect::ToLibrary { cards, .. } => !matches!(cards, Cards::Matching { .. }),
        _ => false,
    });
    if !moves_only {
        return;
    }
    for e in effects {
        if let Effect::RevealTop { into, blind, .. } = e {
            if *into == slot {
                *blind = true;
            }
        }
    }
}

/// A condition on what happened this turn, if it is one: void's "if a
/// nonland permanent left the battlefield this turn or a spell was warped
/// this turn". The warp half never holds in a game the engine runs, since
/// it refuses every card with warp (CR 702.185c: cast for its warp cost),
/// and so says nothing; `a_warp_card_is_refused` holds that up.
fn this_turn(cond: &i::Condition) -> Option<Result<ThisTurn, String>> {
    let left = |e: &i::Event| match e {
        i::Event::Leaves {
            what,
            from: i::Zone::Battlefield,
        } if what.quant == Quant::One && what.filter.kind == Some(i::Noun::Permanent) => {
            // What was on the battlefield was a permanent, so the kind
            // says nothing more.
            Some(
                objects(&Objects {
                    quant: Quant::All,
                    filter: i::Filter {
                        kind: None,
                        ..what.filter.clone()
                    },
                })
                .and_then(|f| {
                    if f.0.iter().all(|p| {
                        matches!(
                            p,
                            Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_) | Pred::ManaValue(..)
                        )
                    }) {
                        Ok(ThisTurn::PermanentLeft(f))
                    } else {
                        Err("a permanent that left, by more than its characteristics".into())
                    }
                }),
            )
        }
        _ => None,
    };
    let warped = |e: &i::Event| {
        matches!(e, i::Event::CastUsing { what, keyword }
            if keyword == "warp" && what.quant == Quant::One
                && what.filter == i::Filter { kind: Some(i::Noun::Spell), ..i::Filter::default() })
    };
    match cond {
        i::Condition::DidThisTurn(e) => left(e),
        i::Condition::Or(cs) => match cs.as_slice() {
            [i::Condition::DidThisTurn(a), i::Condition::DidThisTurn(b)] if warped(b) => left(a),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the instructions reveal the card their binder chose.
fn reveals_chosen(effects: &[i::Effect]) -> bool {
    effects.iter().any(|e| match e {
        i::Effect::Reveal { what, .. } => {
            matches!(what.quant, Quant::Bound(0) | Quant::Introduced(0))
        }
        i::Effect::May { then, .. } => reveals_chosen(then),
        _ => false,
    })
}

/// Whether an instruction reveals the cards a binder holds ("reveal it",
/// "reveal those cards"), the log's reveal line naming each.
fn reveals_found(effects: &[i::Effect]) -> bool {
    effects.iter().any(|e| match e {
        i::Effect::Reveal { what, .. } => what.quant == Quant::Bound(0),
        i::Effect::May { then, .. } => reveals_found(then),
        _ => false,
    })
}

/// Whether any instruction, however deep, puts a card onto the battlefield.
fn moves_onto_battlefield(effects: &[i::Effect]) -> bool {
    effects.iter().any(|e| match e {
        i::Effect::Move {
            to: i::Zone::Battlefield,
            ..
        } => true,
        i::Effect::May { then, .. } | i::Effect::With { then, .. } => moves_onto_battlefield(then),
        _ => false,
    })
}

fn enters_battlefield(effects: &[i::Effect]) -> bool {
    effects.iter().any(|e| match e {
        i::Effect::Move {
            what,
            to: i::Zone::Battlefield,
            ..
        } => what.quant == Quant::Bound(0),
        i::Effect::May { then, .. } => enters_battlefield(then),
        _ => false,
    })
}

/// "a Goblin permanent card from your hand": one card of your hand, by its
/// types and colours, a permanent card one of a permanent's types (CR
/// 110.4b).
fn hand_card(o: &Objects) -> Result<CardFilter, String> {
    let kind = o.filter.kind.clone();
    let card = card_filter(&Objects {
        quant: o.quant.clone(),
        filter: i::Filter {
            kind: Some(i::Noun::Card),
            posts: Vec::new(),
            ..o.filter.clone()
        },
    })?;
    match kind {
        Some(i::Noun::Card) => Ok(card),
        Some(i::Noun::PermanentCard) => Ok(CardFilter(
            card.0
                .iter()
                .flat_map(|alt| {
                    PERMANENT_TYPES.iter().map(move |t| {
                        let mut f = alt.clone();
                        f.0.push(Pred::Is(Prop::Type(*t)));
                        f
                    })
                })
                .collect(),
        )),
        other => Err(format!("a card from a hand of kind {other:?}")),
    }
}

/// CR 110.4b.
pub const PERMANENT_TYPES: [CardType; 6] = [
    CardType::Artifact,
    CardType::Battle,
    CardType::Creature,
    CardType::Enchantment,
    CardType::Land,
    CardType::Planeswalker,
];

/// "A card", "a land card": one card of a hand, by its types and colours
/// alone, any card when nothing narrows it.
fn discarded(o: &Objects) -> Result<CardFilter, String> {
    if o.quant == Quant::One
        && o.filter.alts.is_empty()
        && o.filter.kind == Some(i::Noun::Card)
        && o.filter.or.is_none()
        && o.filter.posts.is_empty()
    {
        return Ok(CardFilter(vec![Filter(Vec::new())]));
    }
    hand_card(o)
}

/// "Any number of creatures with total power 12 or greater": the
/// permanents of the payer's the filter matches, and the least their
/// powers may add up to.
fn sacrifice_power(o: &Objects) -> Result<UnlessCost, String> {
    let total = |p: &i::Post| match p {
        i::Post::Total(i::Compare {
            stat: i::Stat::Power,
            op: i::Op::Ge,
            value: i::Amount::N(n),
        }) => Some(*n),
        _ => None,
    };
    let [post] = o.filter.posts.as_slice() else {
        return Err("a sacrifice unless other than by total power".into());
    };
    let (Quant::AnyNumber, Some(power)) = (&o.quant, total(post)) else {
        return Err("a sacrifice unless other than by total power".into());
    };
    let mut filter = objects(&Objects {
        quant: Quant::All,
        filter: i::Filter {
            posts: Vec::new(),
            ..o.filter.clone()
        },
    })?;
    if filter
        .0
        .iter()
        .any(|p| matches!(p, Pred::Itself | Pred::Other))
    {
        return Err("a cost relative to its source".into());
    }
    if !filter.0.contains(&Pred::YouControl) {
        filter.0.push(Pred::YouControl);
    }
    Ok(UnlessCost::SacrificePower { filter, power })
}

/// "a Mountain or Forest card": one card, by its types and colours alone.
fn card_filter(o: &Objects) -> Result<CardFilter, String> {
    if o.quant != Quant::One {
        return Err(format!("searching for {}", short(&o.quant)));
    }
    let f = &o.filter;
    if f.kind != Some(i::Noun::Card) || f.or.is_some() || !f.posts.is_empty() {
        return Err("searching for other than a card by its characteristics".into());
    }
    let alts = f
        .alts
        .iter()
        .map(|alt| {
            objects(&Objects {
                quant: Quant::All,
                filter: i::Filter {
                    alts: vec![alt.clone()],
                    ..i::Filter::default()
                },
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if alts.iter().flat_map(|f| &f.0).any(|p| {
        !matches!(
            p,
            Pred::Is(_) | Pred::IsNot(_) | Pred::IsAny(_) | Pred::ManaValue(..)
        )
    }) {
        return Err("a card filter relative to its source".into());
    }
    Ok(CardFilter(alts))
}

/// "Goblin spells you cast cost {1} less to cast", "Noncreature spells
/// cost {1} more to cast": a static that changes the total cost of the
/// spells the filter matches (CR 601.2f), by their card's
/// characteristics, cast by whom `cast_by` says, relative to the static's
/// controller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CostChange {
    pub spells: CardFilter,
    pub cast_by: CastBy,
    /// Generic mana: a reduction takes only from the generic component
    /// (CR 118.7a).
    pub generic: u32,
    pub less: bool,
    /// "{1} less for each creature you control with power 4 or greater":
    /// the change once for each permanent the filter matches, read from
    /// the static's permanent (CR 601.2f).
    pub per: Option<Filter>,
    /// "During your turn": only in its controller's turn.
    pub your_turn: bool,
    /// "Spells with flash": only a spell whose card has flash (CR
    /// 702.8a), which its characteristics don't carry.
    pub with_flash: bool,
    /// "… cost {1} less to cast and can't be countered": the same spells
    /// can't be countered either (CR 101.2, 701.6a).
    pub uncounterable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastBy {
    Anyone,
    You,
    Opponents,
}

/// A static that is one cost change, lowered, or `None` if it isn't one.
pub fn cost_change(parts: &[i::Static]) -> Option<Result<CostChange, String>> {
    // "During your turn, …": only in its controller's turn.
    let (parts, your_turn) = match parts {
        [i::Static::AsLongAs {
            cond: i::Condition::ItsYourTurn,
            then,
            ..
        }] => (then.as_slice(), true),
        _ => (parts, false),
    };
    // "Spells with flash you cast cost {1} less to cast and can't be
    // countered": the second part over the same spells.
    let (parts, uncounterable) = match parts {
        [change @ i::Static::CostChange { what, .. }, i::Static::Restrict {
            what: same,
            rule: i::Restriction::CantBeCountered,
        }] if same == what => (std::slice::from_ref(change), true),
        _ => (parts, false),
    };
    let [i::Static::CostChange {
        what,
        by,
        less,
        per,
    }] = parts
    else {
        return None;
    };
    Some((|| {
        // "For each creature you control with power 4 or greater": counted
        // among the battlefield's permanents, read from the static's.
        let per = per.as_ref().map(counted).transpose()?;
        if what.filter.kind != Some(i::Noun::Spell) || what.filter.or.is_some() {
            return Err("a cost change of other than spells".into());
        }
        let mut cast_by = CastBy::Anyone;
        let mut with_flash = false;
        let mut posts = Vec::new();
        for post in &what.filter.posts {
            match post {
                Post::Cast(i::Player::You) => cast_by = CastBy::You,
                Post::WithKeyword(k) if k == "flash" => with_flash = true,
                Post::Cast(i::Player::EachOpponent | i::Player::AnOpponent) => {
                    cast_by = CastBy::Opponents
                }
                other => posts.push(other.clone()),
            }
        }
        let spells = spell_filter(&Objects {
            quant: Quant::One,
            filter: i::Filter {
                posts,
                ..what.filter.clone()
            },
        })?;
        let generic =
            by.0.iter()
                .map(|s| s.parse::<u32>().ok())
                .sum::<Option<u32>>()
                .ok_or("a cost change of coloured mana")?;
        Ok(CostChange {
            spells,
            cast_by,
            generic,
            less: *less,
            per,
            your_turn,
            with_flash,
            uncounterable,
        })
    })())
}

/// "Black creatures can't attack you", "Nonblack creatures can't attack
/// you unless their controller pays {2} for each creature they control
/// that's attacking you": a restriction on attacking the static's
/// controller (CR 508.1c), or a cost to (CR 508.1h), for each creature the
/// filter matches, read from the static's permanent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackRule {
    pub attackers: Filter,
    /// `None` for "can't attack you"; the mana each attacker it matches
    /// costs its controller to attack.
    pub tax: Option<crate::mana::ManaCost>,
}

/// "This creature can't attack or block unless you control seven or
/// more lands": what its own permanent can't do, while the condition, if
/// any, doesn't hold (CR 508.1c, 509.1b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombatRule {
    pub attack: bool,
    pub block: bool,
    /// The condition under which it can, `None` for a plain "can't attack",
    /// "can't block" or "can't attack or block".
    pub unless: Option<YouControl>,
}

/// A static restricting its own permanent's attacking or blocking, as
/// `CombatRule`, or `None` for any other static.
pub fn combat_rule(parts: &[i::Static]) -> Option<Result<CombatRule, String>> {
    let [i::Static::Restrict { what, rule }] = parts else {
        return None;
    };
    let (rule, unless) = match rule {
        i::Restriction::Unless(rule, cond) => (&**rule, Some(cond)),
        rule => (rule, None),
    };
    let (attack, block) = match rule {
        i::Restriction::CantAttack => (true, false),
        i::Restriction::CantBlock => (false, true),
        i::Restriction::CantAttackOrBlock => (true, true),
        _ => return None,
    };
    Some((|| {
        if !(what.quant == Quant::SelfRef && what.filter == i::Filter::default()) {
            return Err("a combat restriction on other than its own permanent".into());
        }
        let unless = match unless {
            None => None,
            Some(i::Condition::YouControl(o)) => Some(you_control(o)?),
            Some(other) => return Err(format!("a combat restriction unless {}", short(other))),
        };
        Ok(CombatRule {
            attack,
            block,
            unless,
        })
    })())
}

/// A static that restricts attacking its controller, as `AttackRule`, or
/// `None` for any other static.
pub fn attack_rule(bind: &[Objects], parts: &[i::Static]) -> Option<Result<AttackRule, String>> {
    let [i::Static::Restrict { what, rule }] = parts else {
        return None;
    };
    let you = |r: &i::Restriction| *r == i::Restriction::CantAttackPlayer(i::Player::You);
    let tax = match rule {
        r if you(r) => None,
        i::Restriction::UnlessPays {
            rule,
            who: i::Player::ControllerOf(_),
            cost,
            per,
        } if you(rule) => Some((cost, per)),
        _ => return None,
    };
    Some((|| {
        let attackers = objects(resolve(what, bind)?)?;
        let tax = match tax {
            None => None,
            Some((cost, per)) => {
                let [i::Cost::Mana(m)] = cost.as_slice() else {
                    return Err("an attack cost other than mana".into());
                };
                // "For each creature they control that's attacking you":
                // the cost once for each attacker, which the restriction
                // already applies to one by one, so the total is the
                // attackers' count (CR 508.1h).
                let each_attacker = per.as_ref().is_some_and(|o| {
                    o.filter.alts == [vec![Adj::Type("creature".into())]]
                        && o.filter.kind.is_none()
                        && o.filter.or.is_none()
                        && o.filter.posts
                            == [
                                Post::Controller(i::Player::That),
                                Post::AttackingPlayer(i::Player::You),
                            ]
                });
                if !each_attacker {
                    return Err("an attack cost counted other than by its attackers".into());
                }
                Some(crate::mana::ManaCost::from_symbols(m)?)
            }
        };
        Ok(AttackRule { attackers, tax })
    })())
}

/// Typecycling (CR 702.29e): "[Type]cycling [cost]" means "[Cost],
/// Discard this card: Search your library for a [type] card, reveal it,
/// and put it into your hand. Then shuffle your library." Its cost and
/// its instructions, or `None` for a keyword that isn't typecycling.
pub fn typecycling(k: &i::Keyword) -> Option<Result<(crate::mana::ManaCost, Program), String>> {
    let name = k.name.to_lowercase();
    let kind = name.strip_suffix("cycling")?.trim().to_string();
    if kind.is_empty() {
        return None;
    }
    Some((|| {
        let Some(i::KeywordParam::Cost(cost)) = &k.param else {
            return Err(format!("{kind}cycling with no cost"));
        };
        let [i::Cost::Mana(m)] = cost.as_slice() else {
            return Err(format!("a {kind}cycling cost other than mana"));
        };
        // "Usually a subtype (as in "mountaincycling") but can be any card
        // type, subtype, supertype, or combination thereof (as in "basic
        // landcycling")".
        let adjs = match kind.as_str() {
            "basic land" => vec![Adj::Supertype("basic".into()), Adj::Type("land".into())],
            t if card_type(t).is_some() => vec![Adj::Type(t.into())],
            t if supertype(t).is_some() => vec![Adj::Supertype(t.into())],
            t if !t.contains(' ') => vec![Adj::Subtype(t.into())],
            _ => return Err(format!("{kind}cycling")),
        };
        let filter = card_filter(&Objects {
            quant: Quant::One,
            filter: i::Filter {
                alts: vec![adjs],
                kind: Some(i::Noun::Card),
                ..i::Filter::default()
            },
        })?;
        let found = Slot(0);
        Ok((
            crate::mana::ManaCost::from_symbols(m)?,
            Program {
                targets: Vec::new(),
                effects: vec![
                    Effect::Search {
                        who: Who::You,
                        filter,
                        into: found,
                        enters: false,
                        reveals: true,
                    },
                    Effect::ToHand {
                        cards: Cards::Bound(found),
                        who: Who::You,
                    },
                    Effect::Shuffle(Who::You),
                ],
            },
        ))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CardType, Chars, PlayerId, Supertype};

    fn chars(types: &[CardType], supertypes: &[Supertype], subtypes: &[&str]) -> Chars {
        Chars {
            types: types.iter().copied().collect(),
            supertypes: supertypes.iter().copied().collect(),
            subtypes: subtypes
                .iter()
                .map(|s| subtype(s, &types.iter().copied().collect()))
                .collect(),
            colors: Default::default(),
            abilities: Default::default(),
            pt: None,
            mana_value: 0,
            controller: PlayerId(0),
        }
    }

    fn searched(kind: &str) -> CardFilter {
        let k = i::Keyword {
            name: format!("{kind}cycling"),
            param: Some(i::KeywordParam::Cost(vec![i::Cost::Mana(i::Mana(vec![
                "2".into(),
            ]))])),
        };
        let (_, program) = typecycling(&k).unwrap().unwrap();
        match &program.effects[0] {
            Effect::Search { filter, .. } => filter.clone(),
            e => panic!("{e:?}"),
        }
    }

    /// CR 702.29e: the type is "usually a subtype … but can be any card
    /// type, subtype, supertype": artifactcycling finds an artifact, not a
    /// card with the subtype "artifact", and wizardcycling a Wizard.
    #[test]
    fn typecycling_searches_for_a_card_type_supertype_or_subtype() {
        let artifact = chars(&[CardType::Artifact], &[], &[]);
        let wizard = chars(&[CardType::Creature], &[], &["Wizard"]);
        let snow = chars(&[CardType::Land], &[Supertype::Snow], &[]);
        assert!(searched("artifact").matches(&artifact));
        assert!(!searched("artifact").matches(&wizard));
        assert!(searched("wizard").matches(&wizard));
        assert!(searched("snow").matches(&snow));
        assert!(!searched("snow").matches(&artifact));
    }
}
