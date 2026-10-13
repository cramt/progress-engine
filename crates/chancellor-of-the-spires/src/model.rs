//! What an object is, what a continuous effect does, and the state the
//! layers are applied to.

use std::collections::BTreeSet;

use imbl::{OrdMap, Vector};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlayerId(pub u8);

/// CR 613.7. Each object entering, each resolved effect and each attachment
/// takes the next one, so two are never equal by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u32);

/// A static ability's identity, so an object can have it, lose it or be
/// granted it in layer 6 without comparing what it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StaticId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CardType {
    Artifact,
    Battle,
    Creature,
    Enchantment,
    Instant,
    Kindred,
    Land,
    Planeswalker,
    Sorcery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Supertype {
    Basic,
    Legendary,
    Snow,
    World,
}

/// The five land types that carry a mana ability (CR 305.6), which is what
/// setting one does more than other subtypes do (CR 305.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BasicLandType {
    Plains,
    Island,
    Swamp,
    Mountain,
    Forest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subtype {
    BasicLand(BasicLandType),
    /// Desert, Gate, Urza's and the rest (CR 205.3i).
    OtherLand(String),
    /// Aura, Equipment, Saga: what isn't a land type and matters to a filter.
    Other(String),
    /// A creature type (CR 205.3m).
    Creature(String),
}

impl Subtype {
    pub fn is_land_type(&self) -> bool {
        matches!(self, Subtype::BasicLand(_) | Subtype::OtherLand(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Keyword {
    Deathtouch,
    Flying,
    Haste,
    Lifelink,
    Trample,
    Vigilance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ability {
    Keyword(Keyword),
    Static(StaticId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pt {
    pub power: i32,
    pub toughness: i32,
}

pub const fn pt(power: i32, toughness: i32) -> Pt {
    Pt { power, toughness }
}

/// An object's characteristics (CR 109.3) as far as the layers here touch
/// them. Printed, these are the card; after the layers, what it is now.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chars {
    pub types: BTreeSet<CardType>,
    pub supertypes: BTreeSet<Supertype>,
    pub subtypes: BTreeSet<Subtype>,
    pub colors: BTreeSet<Color>,
    pub abilities: BTreeSet<Ability>,
    /// None for an object with no power and toughness: a card printed
    /// without them, or one that has them only while it is a creature.
    pub pt: Option<Pt>,
    pub mana_value: u32,
    /// Layer 2, which nothing here changes yet.
    pub controller: PlayerId,
}

impl Chars {
    pub fn is(&self, t: CardType) -> bool {
        self.types.contains(&t)
    }
}

/// A characteristic a filter can ask about.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Prop {
    Type(CardType),
    Supertype(Supertype),
    Subtype(Subtype),
    Color(Color),
}

impl Prop {
    fn of(&self, c: &Chars) -> bool {
        match self {
            Prop::Type(t) => c.types.contains(t),
            Prop::Supertype(t) => c.supertypes.contains(t),
            Prop::Subtype(t) => c.subtypes.contains(t),
            Prop::Color(t) => c.colors.contains(t),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pred {
    Is(Prop),
    IsNot(Prop),
    /// "other": not the ability's own object.
    Other,
    /// "this creature": only the ability's own object.
    Itself,
    YouControl,
    /// "enchanted creature", "equipped creature": what the source is attached to.
    AttachedTo,
}

/// What a static ability applies to, re-read whenever it applies: every
/// predicate holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Filter(pub Vec<Pred>);

/// What a filter is matched against: the object as it is at that point in
/// the layers, and the ability's source as it is then.
pub struct Seen<'a> {
    pub id: ObjectId,
    pub chars: &'a Chars,
    pub source: ObjectId,
    pub source_chars: &'a Chars,
    pub source_attached_to: Option<ObjectId>,
}

impl Filter {
    pub fn matches(&self, s: &Seen) -> bool {
        self.0.iter().all(|p| match p {
            Pred::Is(prop) => prop.of(s.chars),
            Pred::IsNot(prop) => !prop.of(s.chars),
            Pred::Other => s.id != s.source,
            Pred::Itself => s.id == s.source,
            Pred::YouControl => s.chars.controller == s.source_chars.controller,
            Pred::AttachedTo => s.source_attached_to == Some(s.id),
        })
    }
}

/// CR 613.1, 613.4. Layers 1–3 and 7a aren't modelled yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Type,
    Color,
    Ability,
    SetPt,
    ModifyPt,
    SwitchPt,
}

impl Layer {
    /// Whether applying an effect in this layer can change what a filter
    /// reads or which abilities an object has, so whether one effect here
    /// can depend on another (CR 613.8a). Layer 7 only moves power and
    /// toughness, and no `Pred` reads them: one that does makes this true
    /// for layer 7 too.
    pub fn has_dependencies(self) -> bool {
        matches!(self, Layer::Type | Layer::Color | Layer::Ability)
    }

    pub const ALL: [Layer; 6] = [
        Layer::Type,
        Layer::Color,
        Layer::Ability,
        Layer::SetPt,
        Layer::ModifyPt,
        Layer::SwitchPt,
    ];
}

/// What one part of a continuous effect does. The variant is the layer, so
/// a part can't be filed in the wrong one (ADR-0037).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// Layer 4: "is a creature in addition to its other types".
    AddTypes(BTreeSet<CardType>),
    /// Layer 4: "is a Swamp in addition to its other land types".
    AddSubtypes(BTreeSet<Subtype>),
    /// Layer 4: "are Mountains". The land loses its other land types and
    /// the abilities its rules text gives it (CR 305.7).
    SetBasicLandType(BasicLandType),
    /// Layer 5.
    SetColors(BTreeSet<Color>),
    /// Layer 6.
    Grant(Ability),
    Lose(Ability),
    LoseAll,
    /// Layer 7b: "has base power and toughness 1/1", or each equal to its
    /// mana value.
    SetBasePt(PtValue),
    /// Layer 7c.
    ModifyPt(Pt),
    /// Layer 7d.
    SwitchPt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PtValue {
    Fixed(Pt),
    ManaValue,
}

impl Op {
    pub fn layer(&self) -> Layer {
        match self {
            Op::AddTypes(_) | Op::AddSubtypes(_) | Op::SetBasicLandType(_) => Layer::Type,
            Op::SetColors(_) => Layer::Color,
            Op::Grant(_) | Op::Lose(_) | Op::LoseAll => Layer::Ability,
            Op::SetBasePt(_) => Layer::SetPt,
            Op::ModifyPt(_) => Layer::ModifyPt,
            Op::SwitchPt => Layer::SwitchPt,
        }
    }

    /// `printed` is the object as printed, for what its rules text gives it.
    pub fn apply(&self, c: &mut Chars, printed: &Chars) {
        match self {
            Op::AddTypes(t) => c.types.extend(t.iter().copied()),
            Op::AddSubtypes(t) => c.subtypes.extend(t.iter().cloned()),
            Op::SetBasicLandType(t) => {
                c.subtypes.retain(|s| !s.is_land_type());
                c.subtypes.insert(Subtype::BasicLand(*t));
                c.abilities.retain(|a| !printed.abilities.contains(a));
            }
            Op::SetColors(colors) => c.colors = colors.clone(),
            Op::Grant(a) => {
                c.abilities.insert(*a);
            }
            Op::Lose(a) => {
                c.abilities.remove(a);
            }
            Op::LoseAll => c.abilities.clear(),
            Op::SetBasePt(v) => {
                c.pt = Some(match v {
                    PtValue::Fixed(p) => *p,
                    PtValue::ManaValue => pt(c.mana_value as i32, c.mana_value as i32),
                })
            }
            Op::ModifyPt(m) => {
                if let Some(p) = c.pt.as_mut() {
                    p.power += m.power;
                    p.toughness += m.toughness;
                }
            }
            Op::SwitchPt => {
                if let Some(p) = c.pt.as_mut() {
                    *p = pt(p.toughness, p.power);
                }
            }
        }
    }
}

/// A static ability that generates a continuous effect (CR 604.2, 611.3):
/// what it applies to is re-read, and its parts are the effect's parts in
/// each layer (CR 613.6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StaticAbility {
    pub filter: Filter,
    pub ops: Vec<Op>,
}

/// A continuous effect from a resolved spell or ability. The set it affects
/// was fixed as it began (CR 611.2c).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Resolved {
    pub timestamp: Timestamp,
    pub affects: BTreeSet<ObjectId>,
    pub ops: Vec<Op>,
}

/// The counters the layers read. Putting them on and taking them off is the
/// state machine's (ADR-0037).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Counter {
    PlusOne,
    MinusOne,
    Named(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub printed: Chars,
    pub timestamp: Timestamp,
    pub counters: OrdMap<Counter, u32>,
    pub attached_to: Option<ObjectId>,
}

/// The battlefield, as the layers need it. Persistent, so search can fork it.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub objects: OrdMap<ObjectId, Object>,
    pub statics: OrdMap<StaticId, StaticAbility>,
    pub resolved: Vector<Resolved>,
    next: u32,
}

impl State {
    fn tick(&mut self) -> u32 {
        self.next += 1;
        self.next
    }

    /// An object enters the battlefield with its static abilities, and gets
    /// a timestamp (CR 613.7d).
    pub fn enter(&mut self, mut printed: Chars, statics: Vec<StaticAbility>) -> ObjectId {
        for s in statics {
            let id = StaticId(self.tick());
            self.statics.insert(id, s);
            printed.abilities.insert(Ability::Static(id));
        }
        let id = ObjectId(self.tick());
        let timestamp = Timestamp(self.tick());
        self.objects.insert(
            id,
            Object {
                printed,
                timestamp,
                counters: OrdMap::new(),
                attached_to: None,
            },
        );
        id
    }

    /// A spell or ability resolves into a continuous effect on these
    /// objects (CR 611.2c, 613.7b).
    pub fn resolve(&mut self, affects: impl IntoIterator<Item = ObjectId>, ops: Vec<Op>) {
        let timestamp = Timestamp(self.tick());
        self.resolved.push_back(Resolved {
            timestamp,
            affects: affects.into_iter().collect(),
            ops,
        });
    }

    pub fn put_counters(&mut self, on: ObjectId, kind: Counter, n: u32) {
        if let Some(o) = self.objects.get_mut(&on) {
            *o.counters.entry(kind).or_insert(0) += n;
        }
    }

    /// An Aura or Equipment becomes attached, with a new timestamp (CR 613.7e).
    pub fn attach(&mut self, what: ObjectId, to: ObjectId) {
        let timestamp = Timestamp(self.tick());
        if let Some(o) = self.objects.get_mut(&what) {
            o.attached_to = Some(to);
            o.timestamp = timestamp;
        }
    }
}
