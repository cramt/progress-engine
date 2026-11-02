//! A card's Oracle text, read by Vivisurgeon's Insight, lowered into the
//! engine's own characteristics and continuous effects (ADR-0037).
//!
//! Lowering never drops what it can't read: it refuses, naming the line and
//! the node. What the layers don't interpret (triggered and activated
//! abilities) is still an ability the object has, which Humility can take
//! away, so it is kept, unread.

use std::collections::BTreeSet;
use std::fmt;

use vivisurgeons_insight::ast::{
    self as i, Adj, Characteristic, Lost, Objects, Post, Quant, TypeSet,
};
use vivisurgeons_insight::lex::{Pt as IPt, PtPart};
use vivisurgeons_insight::{read_face, Face};

use crate::model::*;

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

/// One card, printed: its characteristics, the static abilities that
/// generate its continuous effects, and the rest of its abilities.
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
                        Some(k) => {
                            self.chars.abilities.insert(Ability::Keyword(k));
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
            what.quant == Quant::SelfRef && pays_life_or_tapped(then).is_some()
        }
        // "If you control two or more other lands, this land enters
        // tapped."
        If { then, .. } => !then.is_empty() && then.iter().all(not_characteristics),
        _ => listed_not_characteristics(s),
    }
}

/// "As this land enters, you may pay 2 life. If you don't, it enters
/// tapped": the life, for a replacement effect whose choice is made before
/// the permanent enters (CR 614.1c, 614.12a).
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
                ops.push(Op::Grant(Ability::Keyword(keyword(k).ok_or_else(refuse)?)));
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
    if let Some(b) = basic_land_type(&word) {
        Subtype::BasicLand(b)
    } else if types.contains(&CardType::Land) {
        Subtype::OtherLand(word)
    } else if types.contains(&CardType::Creature) || types.contains(&CardType::Kindred) {
        Subtype::Creature(word)
    } else {
        Subtype::Other(word)
    }
}

fn capitalised(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| {
            f.to_uppercase()
                .chain(c.flat_map(|x| x.to_lowercase()))
                .collect()
        })
        .unwrap_or_default()
}

fn keyword(k: &i::Keyword) -> Option<Keyword> {
    if k.param.is_some() {
        return None;
    }
    Some(match k.name.as_str() {
        "deathtouch" => Keyword::Deathtouch,
        "flying" => Keyword::Flying,
        "haste" => Keyword::Haste,
        "lifelink" => Keyword::Lifelink,
        "trample" => Keyword::Trample,
        "vigilance" => Keyword::Vigilance,
        _ => return None,
    })
}

/// A static ability is one ability however many sentences it has: each
/// part is an `Op` over one filter. Parts over different objects would be
/// two effects, and nothing needs that yet.
fn static_ability(parts: &[i::Static], scope: &[Objects]) -> Result<StaticAbility, String> {
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
        let (what, part_ops) = static_part(part, scope)?;
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
    Ok(StaticAbility {
        filter: filter.ok_or("an empty static ability")?,
        ops,
    })
}

fn static_part<'a>(
    part: &'a i::Static,
    scope: &[Objects],
) -> Result<(&'a Objects, Vec<Op>), String> {
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
                ops.push(Op::Grant(Ability::Keyword(
                    keyword(k).ok_or_else(unlowered)?,
                )));
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
        // "each equal to its mana value": the object's own, when "its"
        // names the binder `what` is.
        i::Static::BasePtEqual {
            what,
            each_equal_to: i::Amount::ManaValueOf(of),
        } if binder(of, scope).is_some() && binder(of, scope) == binder(what, scope) => {
            (what, vec![Op::SetBasePt(PtValue::ManaValue)])
        }
        _ => return Err(unlowered()),
    })
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

/// "You control a Mountain or a Plains", "you control two or fewer other
/// lands": how many of the permanents you control match any of the
/// filters, read once, as the condition is checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YouControl {
    pub any_of: Vec<Filter>,
    pub count: Count,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    AtLeast(u32),
    AtMost(u32),
}

impl Count {
    pub fn holds(self, n: u32) -> bool {
        match self {
            Count::AtLeast(k) => n >= k,
            Count::AtMost(k) => n <= k,
        }
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

/// "All creatures", "nontoken artifacts you control": a filter over the
/// battlefield, re-read as the layers go. Targets aren't a static's.
fn objects(o: &Objects) -> Result<Filter, String> {
    // "Enchanted creature", "equipped creature": what the source is
    // attached to, and only while it matches the rest of the filter.
    let mut preds = match o.quant {
        Quant::All | Quant::Each | Quant::Plural => Vec::new(),
        Quant::Enchanted | Quant::Equipped => vec![Pred::AttachedTo],
        _ => return Err(format!("quantity {}", short(&o.quant))),
    };
    let f = &o.filter;
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
            Adj::Color(c) => Pred::Is(Prop::Color(color(c).ok_or(short(c))?)),
            Adj::NonColor(c) => Pred::IsNot(Prop::Color(color(c).ok_or(short(c))?)),
            Adj::State(i::State::Nontoken) => Pred::Nontoken,
            Adj::State(i::State::Other) => Pred::Other,
            other => return Err(short(other)),
        });
    }
    for post in &f.posts {
        preds.push(match post {
            Post::Controller(i::Player::You) => Pred::YouControl,
            other => return Err(short(other)),
        });
    }
    Ok(Filter(preds))
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

/// One instruction of an activated ability other than a mana ability, as
/// it resolves (CR 608.2c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// "Search your library for a Mountain or Forest card, put it onto the
    /// battlefield (tapped)": the controller searches their own library
    /// and may find a card the filter matches (CR 701.23a–b).
    SearchToBattlefield { filter: CardFilter, tapped: bool },
    /// "then shuffle": the controller's library (CR 701.24a).
    Shuffle,
}

/// An activated ability's cost, of the kinds the engine pays (CR 602.2b,
/// 601.2h): tapping its source, life (CR 119.4), and sacrificing the
/// source (CR 701.21a).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivationCost {
    pub tap: bool,
    pub life: u32,
    pub sacrifice: bool,
}

/// An activated ability that isn't a mana ability (CR 602), lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activated {
    pub cost: ActivationCost,
    pub effects: Vec<Effect>,
}

/// An activated ability's cost and effect, or the node with no lowering
/// yet. A target binds it under a `Let`, which never gets here.
pub fn activated(
    cost: &[i::Cost],
    effect: &[i::Effect],
    limits: &[i::Limit],
) -> Result<Activated, String> {
    if let Some(l) = limits.first() {
        return Err(format!("an activated ability limited by {}", short(l)));
    }
    let mut paid = ActivationCost::default();
    for c in cost {
        match c {
            i::Cost::Tap => paid.tap = true,
            i::Cost::PayLife(i::Amount::N(n)) => paid.life += n,
            i::Cost::Sacrifice(o) if itself(o) => paid.sacrifice = true,
            other => return Err(format!("an activation cost of {}", short(other))),
        }
    }
    let mut effects = Vec::new();
    for e in effect {
        effects.extend(resolving(e)?);
    }
    Ok(Activated {
        cost: paid,
        effects,
    })
}

fn itself(o: &Objects) -> bool {
    o.quant == Quant::SelfRef && o.filter == i::Filter::default()
}

/// One of an ability's effects.
fn resolving(e: &i::Effect) -> Result<Vec<Effect>, String> {
    let blank = |o: &Objects, q: Quant| o.quant == q && o.filter == i::Filter::default();
    match e {
        // "Search your library for a Mountain or Forest card, put it onto
        // the battlefield, then shuffle": the card found is introduced by
        // the `With` and put onto the battlefield by the reference to it.
        i::Effect::With { bind, then } => match (bind.as_slice(), then.as_slice()) {
            (
                [card],
                [i::Effect::Search {
                    who: i::Player::You,
                    zones,
                    what,
                    and,
                }, i::Effect::Move {
                    what: moved,
                    from: None,
                    to: i::Zone::Battlefield,
                    tapped,
                    transformed: false,
                    under: None,
                    order: None,
                }, rest @ ..],
            ) if matches!(zones.as_slice(), [i::Zone::Library(i::Player::You)])
                && and.is_empty()
                && blank(what, Quant::Introduced(0))
                && blank(moved, Quant::Bound(0)) =>
            {
                let mut out = vec![Effect::SearchToBattlefield {
                    filter: card_filter(card)?,
                    tapped: *tapped,
                }];
                for r in rest {
                    out.extend(resolving(r)?);
                }
                Ok(out)
            }
            _ => Err("With".into()),
        },
        i::Effect::Shuffle(i::Player::You) => Ok(vec![Effect::Shuffle]),
        other => Err(short(other)),
    }
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
    if alts
        .iter()
        .flat_map(|f| &f.0)
        .any(|p| !matches!(p, Pred::Is(_) | Pred::IsNot(_)))
    {
        return Err("a card filter relative to its source".into());
    }
    Ok(CardFilter(alts))
}
