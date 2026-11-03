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
        "lifelink" => Keyword::Lifelink,
        "menace" => Keyword::Menace,
        "reach" => Keyword::Reach,
        "trample" => Keyword::Trample,
        "vigilance" => Keyword::Vigilance,
        _ => return None,
    }])
}

/// A static ability is one ability however many sentences it has: each
/// part is an `Op` over one filter. Parts over different objects would be
/// two effects, and nothing needs that yet.
fn static_ability(parts: &[i::Static], scope: &[Objects]) -> Result<StaticAbility, String> {
    // "This creature gets +1/+1 as long as you control a Mountain": the
    // effect, and when it exists.
    if let [i::Static::AsLongAs { cond, then, .. }] = parts {
        let condition = match cond {
            i::Condition::YouControl(o) => Condition::YouControl(you_control(o)?),
            i::Condition::ItsYourTurn => Condition::YourTurn,
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
        condition: None,
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
            // "each Locus on the battlefield": a filter over permanents
            // reads the battlefield already.
            Post::In(i::Zone::Battlefield) => continue,
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

/// One instruction of a spell or an ability, as it resolves (CR 608.2c).
/// Each names what it acts on through the ability's own [`Who`] and
/// [`Obj`], never a guess: a target by its slot, chosen as the ability
/// was put on the stack (CR 601.2c, 603.3d).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// "Search your library for a Mountain or Forest card": `who`
    /// searches their library and may find a card the filter matches (CR
    /// 701.23a–b), bound to `into`. `enters` if an instruction after puts
    /// it onto the battlefield, so only a card the engine runs is found.
    Search {
        who: Who,
        filter: CardFilter,
        into: Slot,
        enters: bool,
    },
    /// "Put it onto the battlefield (tapped)", under `who`'s control.
    ToBattlefield {
        cards: Cards,
        tapped: bool,
        who: Who,
    },
    /// "Then shuffle": a player's library (CR 701.24a).
    Shuffle(Who),
    /// "Draw two cards" (CR 121.2).
    Draw {
        who: Who,
        n: u32,
    },
    /// "Discard two cards": the player chooses which (CR 701.9a).
    Discard {
        who: Who,
        n: u32,
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
    Destroy(Obj),
    /// "Exile target creature" (CR 701.13a).
    Exile(Obj),
    /// "Return target permanent to its owner's hand".
    ToOwnersHand(Obj),
    /// "Tap target land", "untap it" (CR 701.26a).
    Tap(Obj),
    Untap(Obj),
    /// "Sacrifice this creature" (CR 701.21a): only a permanent its
    /// controller controls.
    Sacrifice(Obj),
    /// "Counter target spell" (CR 701.6a): the spell in this target slot.
    Counter(usize),
    /// "Create a 0/1 green Plant creature token": `n` tokens with this
    /// text, its controller's (CR 111.1–111.3).
    Create {
        n: u32,
        token: Box<crate::deck::CardText>,
        tapped: bool,
    },
    /// "~ deals 3 damage to any target": the source is the spell or the
    /// ability's source (CR 120.1).
    Damage {
        to: DamageTo,
        n: Amount,
    },
    /// The controller scries (CR 701.22a).
    Scry(u32),
    /// CR 122.1, 122.6.
    PutCounters {
        on: Obj,
        counter: Counter,
        n: u32,
    },
    /// "Target creature gets +3/+3 until end of turn": a continuous effect
    /// on the object as it resolves (CR 611.2c), ending in cleanup (CR
    /// 514.2).
    UntilEndOfTurn {
        on: Obj,
        ops: Vec<Op>,
        /// "+2/+0 … for each other attacking Goblin": the modification
        /// once per permanent the filter matches, read from the ability's
        /// source and counted as it resolves (CR 608.2h).
        per: Option<Filter>,
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
    /// "Reveal the top card of your library": the top `n` cards of
    /// `whose` library are revealed (CR 701.20a), and bound to `into`.
    RevealTop {
        whose: Who,
        n: u32,
        into: Slot,
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
    /// "Put that card into your hand": into `who`'s hand.
    ToHand {
        cards: Cards,
        who: Who,
    },
    /// "Put them back in any order", "the rest on the bottom of your
    /// library in any order": the controller orders them (CR 401.4).
    ToLibrary {
        cards: Cards,
        top: bool,
    },
    /// "If it's a land card, …": these instructions, only if the card
    /// bound to `card` matches.
    IfCard {
        card: Slot,
        filter: CardFilter,
        then: Vec<Effect>,
    },
}

impl Effect {
    /// The instructions this one holds, which a resolution may enter.
    pub fn inner(&self) -> Option<&[Effect]> {
        match self {
            Effect::May(_, then) | Effect::IfCard { then, .. } => Some(then),
            _ => None,
        }
    }
}

/// A card an instruction found as it resolves, bound for those after it
/// (`Effect::With` in Insight's [binders]): its place in the resolution's
/// record of what it found, one per binder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(pub usize);

/// Cards a resolution found, as an instruction names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cards {
    /// "that card", "them".
    Bound(Slot),
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
}

/// An object an instruction names: the ability's own source ("this
/// creature") or a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obj {
    Itself,
    Target(usize),
    /// The object the trigger event names: exalted's "that creature".
    Triggering,
}

/// How much: a number, or "that much", the amount of the event that
/// triggered the ability (Jackal Pup's "it deals that much damage to
/// you").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Amount {
    N(u32),
    ThatMuch,
    /// "Its mana value": of the card bound there.
    ManaValueOf(Slot),
    /// "Equal to its power": of the object, or as it last existed (CR
    /// 608.2h).
    PowerOf(Obj),
}

/// What damage is dealt to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageTo {
    /// A target, a player or a permanent as its slot allows.
    Target(usize),
    You,
    /// "the player or planeswalker it's attacking": whom the ability's
    /// source attacks as it resolves, if it still does.
    Attacked,
    /// "That player": the one the trigger event names.
    EventPlayer,
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
    /// Exalted's "Whenever a creature you control attacks alone": the only
    /// creature declared as an attacker is one the filter matches (CR
    /// 506.5, 702.83a).
    AttacksAlone(Filter),
    /// "Whenever a player casts a spell with mana value 3 or less",
    /// prowess's "Whenever you cast a noncreature spell": a player `by`
    /// names, relative to its controller, casts a spell the filter
    /// matches, by its card's characteristics (CR 601.2i, 702.108a).
    Casts { by: CastBy, spells: CardFilter },
    /// "Whenever this creature is dealt damage": a permanent the filter
    /// matches is dealt damage, the amount the event's (CR 120.3).
    DealtDamage(Filter),
    /// "Whenever this creature deals damage to a player", "… deals combat
    /// damage to a player": a permanent the filter matches deals damage to
    /// a player, once for each player dealt damage at once (CR 120.3,
    /// 510.2), only combat damage if `combat`.
    DealsDamageToPlayer { what: Filter, combat: bool },
    /// "At the beginning of your upkeep": a step begins in a turn whose
    /// active player `whose` names, relative to the ability's controller
    /// (CR 503.1a, 504.2, 513.1a).
    Begins { step: TurnStep, whose: Whose },
    /// "When this creature dies", "Whenever another creature you control
    /// dies": a creature the filter matches, as it last existed, is put
    /// into a graveyard from the battlefield (CR 700.4, 603.6c, 603.10a).
    Dies(Filter),
}

/// The steps a triggered ability can wait for the beginning of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStep {
    Upkeep,
    Draw,
    End,
}

/// Whose turn a step is in, for "at the beginning of your upkeep".
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
}

/// A spell's instructions, its paragraphs in order (CR 608.2c), with the
/// `Let` that binds its targets, if any.
pub fn spell(bind: &[Objects], effects: &[i::Effect]) -> Result<Program, String> {
    let mut scope = Scope::new(bind);
    let lowered = scope.effects(effects)?;
    scope.program.effects = lowered;
    Ok(scope.program)
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
    let event = match trigger {
        i::Trigger::When(e) => match &**e {
            i::Event::Enters {
                what,
                under: None,
                state: None,
            } => TriggerEvent::Enters(entering(what)?),
            i::Event::Attacks(what) => TriggerEvent::Attacks(entering(what)?),
            i::Event::IsDealtDamage(what) => TriggerEvent::DealtDamage(entering(what)?),
            i::Event::Dies(what) => TriggerEvent::Dies(entering(what)?),
            i::Event::DealsDamageTo { what, to }
                if matches!(to.as_slice(), [i::Recipient::Player(i::Player::APlayer)]) =>
            {
                TriggerEvent::DealsDamageToPlayer {
                    what: entering(what)?,
                    combat: false,
                }
            }
            i::Event::DealsCombatDamageToPlayer(what) => TriggerEvent::DealsDamageToPlayer {
                what: entering(what)?,
                combat: true,
            },
            // A spell cast by itself triggers from the stack, where
            // nothing looks for triggers yet.
            i::Event::Casts { who, what } if what.quant != Quant::SelfRef => TriggerEvent::Casts {
                by: match who {
                    i::Player::You => CastBy::You,
                    i::Player::APlayer | i::Player::Each => CastBy::Anyone,
                    i::Player::AnOpponent | i::Player::EachOpponent => CastBy::Opponents,
                    _ => return Err(trigger_node(trigger)),
                },
                spells: spell_filter(what)?,
            },
            _ => return Err(trigger_node(trigger)),
        },
        i::Trigger::At(step) => {
            let (step, whose) = match step {
                i::Step::Upkeep(w) => (TurnStep::Upkeep, w),
                i::Step::DrawStep(w) => (TurnStep::Draw, w),
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
    let mut scope = Scope::new(bind);
    scope.amount = matches!(event, TriggerEvent::DealtDamage(_));
    scope.caster = matches!(event, TriggerEvent::Casts { .. });
    let lowered = scope.effects(effect)?;
    scope.program.effects = lowered;
    Ok(Triggered {
        event,
        program: scope.program,
    })
}

/// The keywords that are triggered abilities and that the engine runs,
/// written out as their rules define them: exalted (CR 702.83a) and
/// prowess (CR 702.108a). `None` for any other keyword.
pub fn keyword_trigger(k: &i::Keyword) -> Option<Triggered> {
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
                    on: Obj::Triggering,
                    ops: plus_one,
                    per: None,
                }],
            },
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
                    on: Obj::Itself,
                    ops: plus_one,
                    per: None,
                }],
            },
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
    /// Whether the trigger event names a player, a spell's caster, whom
    /// "that player" names.
    caster: bool,
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
    /// What a search looks for, by the slot of the card it finds, and
    /// whether that card is put onto the battlefield.
    described: BTreeMap<Slot, (Objects, bool)>,
}

impl<'a> Scope<'a> {
    fn new(bind: &'a [Objects]) -> Scope<'a> {
        Scope {
            bind,
            bound: vec![None; bind.len()],
            player: None,
            amount: false,
            caster: false,
            program: Program::default(),
            withs: Vec::new(),
            slots: 0,
            made: None,
            whose: BTreeMap::new(),
            among: BTreeMap::new(),
            described: BTreeMap::new(),
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
            Quant::Rest { of } => {
                let of = self.found(of.into()).ok_or("the rest of nothing found")??;
                let from = *self.among.get(&of).ok_or("the rest of no choice")?;
                Ok(Cards::Rest { of, from })
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
            return Ok(s);
        }
        let o = &self.bind[b];
        let spec = match o.quant {
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

    fn obj(&mut self, o: &Objects) -> Result<Obj, String> {
        match o.quant {
            Quant::SelfRef if o.filter == i::Filter::default() => Ok(Obj::Itself),
            Quant::Introduced(i) | Quant::Bound(i) => Ok(Obj::Target(self.binder(i.into())?)),
            Quant::Target {
                n: i::Amount::N(1),
                up_to: false,
            } => Ok(Obj::Target(
                self.slot(TargetSpec::Permanent(target_filter(o)?)),
            )),
            ref q => Err(format!("an object {}", short(q))),
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
                Who::Defending | Who::ControllerOf(_) => {
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
            [i::Recipient::Objects(o)] => match self.obj(o)? {
                Obj::Target(s) => Ok(DamageTo::Target(s)),
                Obj::Itself | Obj::Triggering => Err("damage to its own source".into()),
            },
            _ => Err("damage to several recipients".into()),
        }
    }

    /// One effect, or the node it can't lower.
    fn effect(&mut self, e: &i::Effect) -> Result<Vec<Effect>, String> {
        let n = |a: &i::Amount| match a {
            i::Amount::N(n) => Ok(*n),
            other => Err(format!("an amount of {}", short(other))),
        };
        let node = || effect_node(e);
        Ok(vec![match e {
            i::Effect::Draw { who, n: k } => Effect::Draw {
                who: self.who(who)?,
                n: n(k)?,
            },
            i::Effect::Discard {
                who,
                what,
                random: false,
            } if what.filter
                == (i::Filter {
                    kind: Some(i::Noun::Card),
                    ..i::Filter::default()
                }) =>
            {
                let k = match &what.quant {
                    Quant::One => 1,
                    Quant::N(a) => n(a)?,
                    q => return Err(format!("discarding {}", short(q))),
                };
                Effect::Discard {
                    who: self.who(who)?,
                    n: k,
                }
            }
            i::Effect::LoseLife { who, n: k } => Effect::LoseLife {
                who: self.who(who)?,
                n: match k {
                    i::Amount::ManaValueOf(o) => match self.cards(o)? {
                        Cards::Bound(slot) => Amount::ManaValueOf(slot),
                        Cards::Rest { .. } => return Err("the mana value of the rest".into()),
                    },
                    k => Amount::N(n(k)?),
                },
            },
            i::Effect::GainLife { who, n: k } => Effect::GainLife {
                who: self.who(who)?,
                n: match k {
                    i::Amount::PowerOf(o) => Amount::PowerOf(self.obj(o)?),
                    k => Amount::N(n(k)?),
                },
            },
            i::Effect::Create {
                who: i::Player::You,
                n: k,
                token,
            } => Effect::Create {
                n: n(k)?,
                token: Box::new(token_text(token)?),
                tapped: token.tapped,
            },
            i::Effect::Destroy { what, .. } => Effect::Destroy(self.obj(what)?),
            i::Effect::Tap(what) => Effect::Tap(self.obj(what)?),
            i::Effect::Untap(what) => Effect::Untap(self.obj(what)?),
            i::Effect::Exile(what) => Effect::Exile(self.obj(what)?),
            i::Effect::Sacrifice {
                who: i::Player::You,
                what,
            } if what.quant == Quant::SelfRef => Effect::Sacrifice(self.obj(what)?),
            i::Effect::Move {
                what,
                from: None,
                to: i::Zone::OwnersHand,
                tapped: false,
                transformed: false,
                under: None,
                order: None,
            } if !matches!(what.quant, Quant::Rest { .. }) && what.filter.kind.is_none() => {
                Effect::ToOwnersHand(self.obj(what)?)
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
            i::Effect::Damage {
                source: Some(source),
                n: k,
                to,
            } if source.quant == Quant::SelfRef && source.filter == i::Filter::default() => {
                Effect::Damage {
                    to: self.damage_to(to)?,
                    n: match k {
                        i::Amount::ThatMany if self.amount => Amount::ThatMuch,
                        k => Amount::N(n(k)?),
                    },
                }
            }
            i::Effect::Scry(k) => Effect::Scry(n(k)?),
            i::Effect::PutCounters { n: k, kind, on } => Effect::PutCounters {
                on: self.obj(on)?,
                counter: counter(kind).ok_or_else(node)?,
                n: n(k)?,
            },
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
                let on = self.obj(what)?;
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
            i::Effect::ForEach { each, then, .. } => match then.as_slice() {
                [m @ i::Effect::Modify {
                    what,
                    dur: i::Duration::UntilEndOfTurn,
                    ..
                }] => {
                    let on = self.obj(what)?;
                    let r = resolution(m).map_err(|_| node())?;
                    if !r.counters.is_empty() || !r.ops.iter().all(|o| matches!(o, Op::ModifyPt(_)))
                    {
                        return Err(format!("{} of more than power and toughness", node()));
                    }
                    Effect::UntilEndOfTurn {
                        on,
                        ops: r.ops,
                        per: Some(counted(each)?),
                    }
                }
                _ => return Err(node()),
            },
            i::Effect::RevealHand(_) => return Err(format!("{} alone", node())),
            i::Effect::With { bind, then } => return self.with(bind, then),
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
                Effect::Search {
                    who,
                    filter: card_filter(&described)?,
                    into,
                    enters,
                }
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
            i::Effect::May { who, then } => {
                let who = self.who(who)?;
                Effect::May(who, self.effects(then)?)
            }
            i::Effect::RevealTop { who, whose, n: k } => {
                let whose = self.library_of(who, whose)?;
                let into = self.made();
                self.whose.insert(into, whose);
                Effect::RevealTop {
                    whose,
                    n: n(k)?,
                    into,
                }
            }
            i::Effect::LookAtTop { who, whose, n: k } => {
                let whose = self.library_of(who, whose)?;
                let into = self.made();
                self.whose.insert(into, whose);
                Effect::LookAtTop {
                    whose,
                    n: n(k)?,
                    into,
                }
            }
            // "that player puts it into their hand": the instruction is
            // theirs to follow, and no choice is in it.
            i::Effect::Performed { effect, .. } => return self.effect(effect),
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
                if *tapped && *to != i::Zone::Battlefield {
                    return Err(node());
                }
                match (to, order) {
                    (i::Zone::Hand(p), None) => Effect::ToHand {
                        cards,
                        who: match (p, cards) {
                            (i::Player::You, _) => Who::You,
                            (i::Player::That, Cards::Bound(slot)) => {
                                *self.whose.get(&slot).ok_or_else(node)?
                            }
                            _ => return Err(node()),
                        },
                    },
                    (i::Zone::OwnersLibraryTop, Some(i::Order::Any) | None) => {
                        Effect::ToLibrary { cards, top: true }
                    }
                    (i::Zone::Battlefield, None) => Effect::ToBattlefield {
                        cards,
                        tapped: *tapped,
                        who: match cards {
                            Cards::Bound(slot) => {
                                self.whose.get(&slot).copied().unwrap_or(Who::You)
                            }
                            Cards::Rest { .. } => Who::You,
                        },
                    },
                    (i::Zone::BottomOfLibrary(i::Player::You), Some(i::Order::Any)) => {
                        Effect::ToLibrary { cards, top: false }
                    }
                    _ => return Err(format!("{} of found cards", node())),
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
            // "Search your library for a basic land card": the card the
            // search finds, which it describes.
            [o] if o.quant == Quant::One && o.filter.kind == Some(i::Noun::Card) => {
                let s = self.new_slot();
                self.described
                    .insert(s, (o.clone(), enters_battlefield(then)));
                s
            }
            [o] if o.quant == Quant::N(i::Amount::N(1)) => {
                let from = match o.filter.posts.as_slice() {
                    [Post::FromAmong(among)] if o.filter.alts.is_empty() => {
                        match self.cards(among)? {
                            Cards::Bound(s) => s,
                            Cards::Rest { .. } => return Err("one from among the rest".into()),
                        }
                    }
                    _ => return Err("With one of some cards".into()),
                };
                let into = self.new_slot();
                self.among.insert(into, from);
                out.push(Effect::ChooseAmong { from, n: 1, into });
                into
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
                        random: false,
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
            out.extend(self.effect(&effects[k])?);
            k += 1;
        }
        Ok(out)
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
                    let cmp = match op {
                        i::Op::Eq => Cmp::Eq,
                        i::Op::Le => Cmp::AtMost,
                        i::Op::Ge => Cmp::AtLeast,
                        i::Op::Lt => Cmp::Below,
                        i::Op::Gt => Cmp::Above,
                    };
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

/// "target creature": a permanent by its characteristics, and "you
/// control"; anything relative to a source is refused, since a spell's
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
    if f.0
        .iter()
        .any(|p| !matches!(p, Pred::Is(_) | Pred::IsNot(_) | Pred::YouControl))
    {
        return Err("a target relative to its source".into());
    }
    Ok(f)
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
fn spell_filter(o: &Objects) -> Result<CardFilter, String> {
    let mut mana_value = None;
    for post in &o.filter.posts {
        match post {
            Post::With(i::Compare {
                stat: i::Stat::ManaValue,
                op,
                value: i::Amount::N(v),
            }) => {
                let cmp = match op {
                    i::Op::Eq => Cmp::Eq,
                    i::Op::Le => Cmp::AtMost,
                    i::Op::Ge => Cmp::AtLeast,
                    i::Op::Lt => Cmp::Below,
                    i::Op::Gt => Cmp::Above,
                };
                mana_value = Some(Pred::ManaValue(cmp, *v));
            }
            other => return Err(format!("a target spell {}", short(other))),
        }
    }
    let mut f = if o.filter.alts.is_empty() {
        CardFilter(vec![Filter::default()])
    } else {
        card_filter(&Objects {
            quant: Quant::One,
            filter: i::Filter {
                kind: Some(i::Noun::Card),
                posts: Vec::new(),
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

/// "+1/+1", "-1/-1" and named counters.
fn counter(kind: &i::CounterKind) -> Option<Counter> {
    match kind {
        i::CounterKind::Pt(p) if p.signed => match (p.power, p.toughness) {
            (PtPart::N(1), PtPart::N(1)) => Some(Counter::PlusOne),
            (PtPart::N(-1), PtPart::N(-1)) => Some(Counter::MinusOne),
            _ => None,
        },
        i::CounterKind::Named(n) => Some(Counter::Named(n.clone())),
        _ => None,
    }
}

/// An activated ability's cost, of the kinds the engine pays (CR 602.2b,
/// 601.2h): tapping its source, life (CR 119.4), and sacrificing the
/// source (CR 701.21a).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivationCost {
    pub tap: bool,
    pub life: u32,
    pub sacrifice: bool,
    /// "{1}, {T}: …": mana paid as the ability is activated (CR 602.2b,
    /// 601.2g–h).
    pub mana: Option<crate::mana::ManaCost>,
}

/// An activated ability that isn't a mana ability (CR 602), lowered: its
/// cost, and its instructions with the targets they name, chosen as it is
/// activated (CR 602.2b, 601.2c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activated {
    pub cost: ActivationCost,
    pub program: Program,
}

/// An activated ability's cost and effect, with the `Let` that binds its
/// targets, or the node with no lowering yet.
pub fn activated(
    bind: &[Objects],
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
            i::Cost::Mana(m) if paid.mana.is_none() => {
                paid.mana = Some(crate::mana::ManaCost::from_symbols(m)?)
            }
            other => return Err(format!("an activation cost of {}", short(other))),
        }
    }
    let mut scope = Scope::new(bind);
    let effects = scope.effects(effect)?;
    scope.program.effects = effects;
    Ok(Activated {
        cost: paid,
        program: scope.program,
    })
}

fn itself(o: &Objects) -> bool {
    o.quant == Quant::SelfRef && o.filter == i::Filter::default()
}

/// Whether an instruction in these, or a "may" in them, puts the card the
/// innermost binder names onto the battlefield.
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
        .any(|p| !matches!(p, Pred::Is(_) | Pred::IsNot(_) | Pred::ManaValue(..)))
    {
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastBy {
    Anyone,
    You,
    Opponents,
}

/// A static that is one cost change, lowered, or `None` if it isn't one.
pub fn cost_change(parts: &[i::Static]) -> Option<Result<CostChange, String>> {
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
        if per.is_some() {
            return Err("a cost change for each".into());
        }
        if what.filter.kind != Some(i::Noun::Spell) || what.filter.or.is_some() {
            return Err("a cost change of other than spells".into());
        }
        let mut cast_by = CastBy::Anyone;
        let mut posts = Vec::new();
        for post in &what.filter.posts {
            match post {
                Post::Cast(i::Player::You) => cast_by = CastBy::You,
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
        })
    })())
}
