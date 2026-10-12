//! Checks a tree can fail without a reader: a reference back with nothing
//! to refer to, a comparison of a thing with itself, a filter no object can
//! pass, an X nothing defines. Each is a complete parse that the round trip
//! cannot catch, because its words all come back, under the wrong node.
//! `insight lint` runs them over the corpus; `tests/lint.rs` holds the
//! pinned trees to none and the rest to a ratchet.

use std::fmt;

use crate::ast::*;
use crate::golden::Entry;
use crate::walk::{Visitor, Walk};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// "it", "that creature" with no object said before it in the ability.
    DanglingAntecedent,
    /// "that player" with no player said before it in the ability.
    DanglingThatPlayer,
    /// Both sides of a comparison are the same expression.
    SelfComparison,
    /// A spell in a zone other than the stack: a spell is a card on the
    /// stack (CR 112.1), so where it was is `CastFrom`.
    SpellInZone,
    /// A spell or card that is tapped, attacking, enchanted or equipped:
    /// only permanents have those states (CR 110.5, 506.4, 303.4, 301.5).
    PermanentStateOffBattlefield,
    /// A permanent's state tested of a triggering spell.
    StateOfTriggeringSpell,
    /// An equipped Equipment: an Equipment can't equip an Equipment
    /// (CR 301.5c).
    EquippedEquipment,
    /// "other" with no kind of object after it.
    BareOther,
    /// An X no cost, "where X is" or triggering spell defines (CR 107.3).
    UnboundX,
    /// A fight between one object and itself (CR 701.14b).
    FightOneObject,
    /// A type-setting effect whose types are no card type: "is a black
    /// Dragon" sets the color and subtype only (CR 205.1).
    TypesWithoutCardType,
}

impl Kind {
    pub const ALL: [Kind; 11] = [
        Kind::DanglingAntecedent,
        Kind::DanglingThatPlayer,
        Kind::SelfComparison,
        Kind::SpellInZone,
        Kind::PermanentStateOffBattlefield,
        Kind::StateOfTriggeringSpell,
        Kind::EquippedEquipment,
        Kind::BareOther,
        Kind::UnboundX,
        Kind::FightOneObject,
        Kind::TypesWithoutCardType,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::DanglingAntecedent => "dangling-antecedent",
            Kind::DanglingThatPlayer => "dangling-that-player",
            Kind::SelfComparison => "self-comparison",
            Kind::SpellInZone => "spell-in-zone",
            Kind::PermanentStateOffBattlefield => "permanent-state-off-battlefield",
            Kind::StateOfTriggeringSpell => "state-of-triggering-spell",
            Kind::EquippedEquipment => "equipped-equipment",
            Kind::BareOther => "bare-other",
            Kind::UnboundX => "unbound-x",
            Kind::FightOneObject => "fight-one-object",
            Kind::TypesWithoutCardType => "types-without-card-type",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == s)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What the rest of the face says that one ability's words lean on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    /// The face's mana cost, an additional cost or a keyword cost on it
    /// has an X (CR 107.3a), so every X in its text is that one.
    pub x_defined: bool,
    /// The line continues a spell whose paragraphs before it named
    /// something, which its references back may reach (CR 608.2c).
    pub prior: bool,
}

/// Each line's context on one face, in order: `lines` are the face's lines
/// as `read_face` gives them, `mana_cost` as Scryfall prints it.
pub fn face_context(lines: &[crate::Line], mana_cost: &str) -> Vec<Context> {
    let x_defined = mana_cost.contains("{X}")
        || lines.iter().any(|l| match &l.parsed {
            Ok(a) => defines_x(a),
            // An unread line may be the one that defines it: a d20 table's
            // "1—9 | X is one", an additional cost the grammar can't read.
            Err(_) => l.text.contains('X'),
        });
    let mut named = false;
    lines
        .iter()
        .map(|l| {
            if !l.continues {
                named = false;
            }
            let c = Context {
                x_defined,
                prior: named,
            };
            // An unread paragraph may have named anything: a d20 table's
            // rows lean on the roll above them.
            named |= l.parsed.as_ref().map_or(true, names_anything);
            c
        })
        .collect()
}

/// An X said in an additional cost or a keyword's cost is chosen as the
/// spell is cast or the cost paid (CR 107.3a, 107.3d), for the whole face.
fn defines_x(a: &Ability) -> bool {
    let x = |s: String| s.contains("\"X\"") || s.contains("(X)") || s.contains(": X");
    match a {
        Ability::Keywords(k) => x(format!("{k:?}")),
        Ability::Static(s) => s.iter().any(|s| {
            matches!(
                s,
                Static::AdditionalCost(_) | Static::OptionalAdditionalCost(_)
            ) && x(format!("{s:?}"))
        }),
        _ => false,
    }
}

/// Whether an ability names an object, or a player other than "you".
fn names_anything(a: &Ability) -> bool {
    struct Any(bool);
    impl Visitor for Any {
        fn enter(&mut self, _: &mut Objects) -> bool {
            self.0 = true;
            false
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            !self.0
        }
        fn player(&mut self, p: &mut Player) {
            self.0 |= !matches!(p, Player::You);
        }
        // "Create two 1/1 white Human creature tokens. … create five of
        // those tokens instead": objects no `Objects` names.
        fn effect(&mut self, e: &mut Effect) {
            self.0 |= matches!(
                e,
                Effect::Create { .. }
                    | Effect::Draw { .. }
                    | Effect::Mill { .. }
                    | Effect::LookAtTop { .. }
                    | Effect::RevealTop { .. }
            );
        }
    }
    let mut v = Any(false);
    a.clone().walk(&mut v);
    v.0
}

/// Every violation in one ability, once per place it occurs.
pub fn lint(a: &Ability, cx: Context) -> Vec<Kind> {
    let mut out = Vec::new();
    scope(a.clone(), cx, &mut out);
    out
}

/// One ability as its own scope; an ability granted inside it is another,
/// which can lean on nothing the granting ability said.
fn scope(mut a: Ability, cx: Context, out: &mut Vec<Kind>) {
    let mut v = Lint {
        top: true,
        nested: Vec::new(),
        out: Vec::new(),
        objects: usize::from(cx.prior),
        players: usize::from(cx.prior),
        triggering_spell: false,
        wheres: 0,
        x_chosen: false,
        x_used: false,
        effects: 0,
        pending: Vec::new(),
    };
    match core(&a) {
        Ability::Triggered { trigger, .. } => {
            v.triggering_spell = trigger_event(trigger).is_some_and(event_is_spell);
            // "At the beginning of each player's upkeep, that player …":
            // the step's player, whom no `Player` names.
            if trigger_step(trigger)
                .is_some_and(|w| matches!(w, Whose::Each | Whose::EachOpponent | Whose::EachOther))
                || trigger_event(trigger).is_some_and(event_names_player)
            {
                v.players += 1;
            }
        }
        Ability::Static(s) => statics(s, &mut v.out),
        _ => {}
    }
    a.walk(&mut v);
    out.append(&mut v.out);
    out.extend(v.pending.iter().map(|_| Kind::DanglingThatPlayer));
    if v.x_used && !cx.x_defined && !x_bound(core(&a)) {
        out.push(Kind::UnboundX);
    }
    for n in v.nested {
        scope(n, Context { prior: false, ..cx }, out);
    }
}

struct Lint {
    top: bool,
    nested: Vec<Ability>,
    out: Vec<Kind>,
    /// Objects said so far, and cards said by none: those a draw, a mill
    /// or a look at the top of a library names.
    objects: usize,
    /// Players other than "you" and "that player" said so far.
    players: usize,
    triggering_spell: bool,
    /// How deep in "where X is" the walk is.
    wheres: u32,
    /// "You may pay {X}": the controller chose X (CR 107.3f).
    x_chosen: bool,
    x_used: bool,
    /// How deep in effects the walk is.
    effects: u32,
    /// The depth of each "that player" said before any player: "deals
    /// damage to target player equal to the number of cards in that
    /// player's hand", "Each player loses 1 life for each creature they
    /// control" hold the amount or the trailing "for each" first. The
    /// player the same effect names is theirs, so one is a violation only
    /// if its effect ends naming none.
    pending: Vec<(u32, usize)>,
}

impl Visitor for Lint {
    fn ability(&mut self, a: &mut Ability) -> bool {
        if self.top {
            // "Infusion — …", a harnessed or station ability: a label on
            // the one ability, not another inside it.
            self.top = matches!(
                a,
                Ability::Labelled { .. }
                    | Ability::Harnessed(_)
                    | Ability::KeywordOn { .. }
                    | Ability::Station { .. }
            );
            return true;
        }
        self.nested.push(a.clone());
        false
    }

    fn enter(&mut self, o: &mut Objects) -> bool {
        if o.quant == Quant::Antecedent && self.objects == 0 {
            self.out.push(Kind::DanglingAntecedent);
        }
        self.objects += 1;
        if matches!(o.filter.kind, Some(Noun::Player | Noun::Opponent)) {
            self.players += 1;
        }
        filter(&o.filter, &mut self.out);
        if o.quant == Quant::Triggering && self.triggering_spell && permanent_state(&o.filter) {
            self.out.push(Kind::StateOfTriggeringSpell);
        }
        true
    }

    fn player(&mut self, p: &mut Player) {
        match p {
            Player::That if self.players == 0 => self.pending.push((self.effects, self.players)),
            Player::You | Player::That => {}
            _ => self.players += 1,
        }
    }

    fn effect(&mut self, e: &mut Effect) {
        self.effects += 1;
        // A delayed trigger's "deals combat damage to a player".
        if let Effect::DelayedWhen { event, .. }
        | Effect::DelayedWhenever { event, .. }
        | Effect::DelayedNext { event, .. } = e
        {
            if event_names_player(event) {
                self.players += 1;
            }
        }
        // "Return target permanent to its owner's hand. Then that player
        // discards a card": the owner, whom a zone names.
        if let Effect::Move { to, .. } = e {
            if matches!(
                to,
                Zone::OwnersHand
                    | Zone::OwnersGraveyard
                    | Zone::OwnersLibraryTop
                    | Zone::OwnersLibraryBottom
                    | Zone::OwnersLibraryNthFromTop(_)
            ) {
                self.players += 1;
            }
        }
        match e {
            Effect::Draw { .. }
            | Effect::Mill { .. }
            | Effect::LookAtTop { .. }
            | Effect::RevealTop { .. }
            | Effect::Scry(_)
            | Effect::Surveil(_) => self.objects += 1,
            Effect::Choose(
                Choice::Player | Choice::Opponent | Choice::TargetOpponent | Choice::TargetPlayer,
            ) => self.players += 1,
            Effect::Where { .. } => self.wheres += 1,
            // "You may pay {X}. If you do", "you may sacrifice X Foods":
            // a cost the effect asks for, whose X the controller picks as
            // they pay it (CR 107.3f).
            Effect::May { then, .. } if !self.x_used => {
                if then.first().is_some_and(|f| {
                    matches!(
                        f,
                        Effect::Pay { .. }
                            | Effect::Sacrifice { .. }
                            | Effect::Tap(_)
                            | Effect::Reveal { .. }
                            | Effect::Discard { .. }
                            | Effect::Action { .. }
                    ) && format!("{f:?}").contains('X')
                }) {
                    self.x_chosen = true;
                }
            }
            Effect::Fight { a, b } if a == b => self.out.push(Kind::FightOneObject),
            Effect::FightEachOther(o) if singular(o) => self.out.push(Kind::FightOneObject),
            Effect::Becomes {
                types,
                keeps: Keeps::Supertypes,
                ..
            }
            | Effect::IsNow {
                types,
                keeps: Keeps::Supertypes,
                ..
            } if no_card_type(types) => self.out.push(Kind::TypesWithoutCardType),
            Effect::AlsoIs {
                types: TypeSet::Listed(types),
                ..
            } if no_card_type(types) => self.out.push(Kind::TypesWithoutCardType),
            _ => {}
        }
    }
    fn effect_done(&mut self, e: &mut Effect) {
        let depth = self.effects;
        let players = self.players;
        self.pending
            .retain(|&(d, before)| d < depth || players == before);
        self.effects -= 1;
        if let Effect::Where { .. } = e {
            self.wheres -= 1;
        }
    }

    fn condition(&mut self, c: &mut Condition) {
        let same = match c {
            Condition::AmountIs { n, value, .. } => n == value,
            Condition::Has { what, cmp } => stat_of(cmp.stat, &cmp.value) == Some(what),
            Condition::ControlsMore { who, than, .. } | Condition::HasMore { who, than, .. } => {
                who == than
            }
            _ => false,
        };
        if same {
            self.out.push(Kind::SelfComparison);
        }
        if let Condition::Is { what, filter: f } = c {
            if what.quant == Quant::Triggering && self.triggering_spell && permanent_state(f) {
                self.out.push(Kind::StateOfTriggeringSpell);
            }
        }
    }

    fn amount(&mut self, a: &mut Amount) {
        if *a == Amount::X && self.wheres == 0 && !self.x_chosen {
            self.x_used = true;
        }
    }
}

/// The object a stat of an amount is read from, if the amount is one.
fn stat_of(stat: Stat, a: &Amount) -> Option<&Objects> {
    match (stat, a) {
        (Stat::Power, Amount::PowerOf(o))
        | (Stat::Toughness, Amount::ToughnessOf(o))
        | (Stat::ManaValue, Amount::ManaValueOf(o)) => Some(o),
        _ => None,
    }
}

fn singular(o: &Objects) -> bool {
    matches!(
        o.quant,
        Quant::SelfRef | Quant::Triggering | Quant::Enchanted | Quant::Equipped
    ) || o.quant
        == (Quant::Target {
            n: Amount::N(1),
            up_to: false,
        })
}

/// Subtypes set with no card type: "is a black Dragon". A color or a
/// supertype alone ("is black", "is legendary") sets no type.
fn no_card_type(types: &[Adj]) -> bool {
    types.iter().any(|t| matches!(t, Adj::Subtype(_)))
        && !types.iter().any(|t| matches!(t, Adj::Type(_)))
}

const PERMANENT_ONLY: &[State] = &[
    State::Tapped,
    State::Untapped,
    State::Attacking,
    State::Blocking,
    State::AttackingOrBlocking,
    State::Blocked,
    State::Unblocked,
    State::Enchanted,
    State::Equipped,
];

fn permanent_state(f: &Filter) -> bool {
    f.alts
        .iter()
        .flatten()
        .any(|a| matches!(a, Adj::State(s) if PERMANENT_ONLY.contains(s)))
        || f.posts
            .iter()
            .any(|p| matches!(p, Post::Attacking | Post::Blocking))
}

fn filter(f: &Filter, out: &mut Vec<Kind>) {
    let mut f = Some(f);
    while let Some(g) = f {
        if g.kind == Some(Noun::Spell)
            && g.posts
                .iter()
                .any(|p| matches!(p, Post::In(z) if *z != Zone::Stack))
        {
            out.push(Kind::SpellInZone);
        }
        if matches!(g.kind, Some(Noun::Spell | Noun::Card)) && permanent_state(g) {
            out.push(Kind::PermanentStateOffBattlefield);
        }
        for alt in &g.alts {
            if alt.contains(&Adj::State(State::Equipped))
                && alt.contains(&Adj::Subtype("equipment".into()))
            {
                out.push(Kind::EquippedEquipment);
            }
        }
        if g.kind.is_none() && g.alts == vec![vec![Adj::State(State::Other)]] {
            out.push(Kind::BareOther);
        }
        f = g.or.as_deref();
    }
}

fn event_is_spell(e: &Event) -> bool {
    match e {
        Event::Casts { .. }
        | Event::Copies { .. }
        | Event::WasCast(_)
        | Event::CastFrom { .. }
        | Event::CastUsing { .. }
        | Event::Countered(_) => true,
        Event::While { event, .. } | Event::AndIf { event, .. } => event_is_spell(event),
        Event::FirstTimeEachTurn(event) => event_is_spell(event),
        _ => false,
    }
}

/// Whether something in the ability, rather than the card's mana cost,
/// defines X: an activation, loyalty or keyword cost with an X (CR 107.3a,
/// 606.4), or a "where X is" (CR 107.3c) said anywhere in it.
fn x_bound(a: &Ability) -> bool {
    let has_x = |s: String| s.contains("\"X\"") || s.contains(": X") || s.contains("(X)");
    match a {
        Ability::Activated { cost, .. } => has_x(format!("{cost:?}")),
        Ability::Loyalty { change, .. } => {
            matches!(
                change,
                crate::lex::Signed::PlusX | crate::lex::Signed::MinusX
            )
        }
        Ability::Spree { cost, .. } => cost.0.iter().any(|s| s == "X"),
        Ability::Keywords(_) | Ability::KeywordOn { .. } => true,
        Ability::Static(s) => format!("{s:?}").contains("Where {"),
        _ => false,
    }
}

fn trigger_event(t: &Trigger) -> Option<&Event> {
    match t {
        Trigger::When(e) | Trigger::WhenAndAt(e, _) => Some(e),
        Trigger::InterveningIf(t, _) => trigger_event(t),
        _ => None,
    }
}

fn trigger_step(t: &Trigger) -> Option<&Whose> {
    match t {
        Trigger::At(s) | Trigger::WhenAndAt(_, s) => Some(match s {
            Step::Upkeep(w)
            | Step::DrawStep(w)
            | Step::PrecombatMain(w)
            | Step::Combat(w)
            | Step::EndStep(w)
            | Step::EndOfCombat(w)
            | Step::SecondMain(w)
            | Step::PostcombatMain(w)
            | Step::Main(w)
            | Step::Untap(w) => w,
        }),
        Trigger::InterveningIf(t, _) => trigger_step(t),
        _ => None,
    }
}

/// An event that names a player with no `Player` node: "deals combat
/// damage to a player".
fn event_names_player(e: &Event) -> bool {
    match e {
        Event::DealsCombatDamageToPlayer(_) | Event::DealsCombatDamageTo { .. } => true,
        Event::While { event, .. } | Event::AndIf { event, .. } => event_names_player(event),
        Event::FirstTimeEachTurn(event) => event_names_player(event),
        Event::Or(v) => v.iter().all(event_names_player),
        _ => false,
    }
}

/// The ability a label or a keyword is said of: "Delirium — At the
/// beginning of …" is the triggered ability.
fn core(a: &Ability) -> &Ability {
    match a {
        Ability::Labelled { ability, .. }
        | Ability::KeywordOn { ability, .. }
        | Ability::Station { ability, .. } => core(ability),
        Ability::Harnessed(ability) => core(ability),
        _ => a,
    }
}

/// Every violation on one face, with the line it is on.
pub fn face(e: &Entry) -> Vec<(Kind, String)> {
    let lines = e.lines();
    let cx = face_context(&lines, &e.mana_cost);
    lines
        .iter()
        .zip(cx)
        .filter_map(|(l, cx)| Some((l.parsed.as_ref().ok()?, cx, &l.text)))
        .flat_map(|(a, cx, text)| lint(a, cx).into_iter().map(|k| (k, text.clone())))
        .collect()
}

/// Every violation on the faces, each with its card and line, in the
/// faces' order: read on every core, as the corpus takes a while.
pub fn faces(entries: &[Entry]) -> Vec<(Kind, String, String)> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = entries.len().div_ceil(threads).max(1);
    std::thread::scope(|s| {
        let parts: Vec<_> = entries
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .flat_map(|e| {
                            face(e)
                                .into_iter()
                                .map(|(k, text)| (k, e.card.clone(), text))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        parts
            .into_iter()
            .flat_map(|p| p.join().expect("a lint thread panicked"))
            .collect()
    })
}

/// The checks no walk reaches: a static's type list, through the statics
/// that hold others ("During your turn, this creature is a Bear").
fn statics(ss: &[Static], out: &mut Vec<Kind>) {
    for s in ss {
        match s {
            Static::Is {
                types: TypeSet::Listed(types),
                ..
            } if no_card_type(types) => out.push(Kind::TypesWithoutCardType),
            Static::AsLongAs { then, .. } | Static::If { then, .. } => statics(then, out),
            Static::PermissionRules {
                permissions, rules, ..
            } => {
                statics(permissions, out);
                statics(rules, out);
            }
            Static::Where { inner, .. } | Static::Reflexive { inner, .. } => {
                statics(std::slice::from_ref(inner), out)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objects(quant: Quant, alts: Vec<Vec<Adj>>, kind: Option<Noun>, posts: Vec<Post>) -> Objects {
        Objects {
            quant,
            filter: Filter {
                alts,
                kind,
                posts,
                or: None,
            },
        }
    }

    fn target(alts: Vec<Vec<Adj>>, kind: Option<Noun>, posts: Vec<Post>) -> Objects {
        let quant = Quant::Target {
            n: Amount::N(1),
            up_to: false,
        };
        objects(quant, alts, kind, posts)
    }

    fn kinds(a: Ability) -> Vec<Kind> {
        lint(&a, Context::default())
    }

    #[test]
    fn each_check_fires_on_the_tree_it_names() {
        let creature = || vec![vec![Adj::Type("creature".into())]];
        // "Counter target spell in a graveyard."
        let spell_in_zone = Ability::Spell(vec![Effect::Counter {
            what: target(vec![], Some(Noun::Spell), vec![Post::In(Zone::AGraveyard)]),
            unless: None,
        }]);
        assert_eq!(kinds(spell_in_zone), [Kind::SpellInZone]);
        // "Exile target tapped creature card."
        let tapped_card = Ability::Spell(vec![Effect::Exile(target(
            vec![vec![
                Adj::State(State::Tapped),
                Adj::Type("creature".into()),
            ]],
            Some(Noun::Card),
            vec![],
        ))]);
        assert_eq!(kinds(tapped_card), [Kind::PermanentStateOffBattlefield]);
        // "Destroy target equipped Equipment."
        let equipped = Ability::Spell(vec![Effect::Destroy {
            what: target(
                vec![vec![
                    Adj::State(State::Equipped),
                    Adj::Subtype("equipment".into()),
                ]],
                None,
                vec![],
            ),
            no_regen: false,
        }]);
        assert_eq!(kinds(equipped), [Kind::EquippedEquipment]);
        // "Destroy another."
        let other = Ability::Spell(vec![Effect::Destroy {
            what: objects(
                Quant::One,
                vec![vec![Adj::State(State::Other)]],
                None,
                vec![],
            ),
            no_regen: false,
        }]);
        assert_eq!(kinds(other), [Kind::BareOther]);
        // "If target creature's power is greater than its power, …"
        let me = || target(creature(), None, vec![]);
        let compare = Ability::Spell(vec![Effect::If {
            cond: Condition::Has {
                what: me(),
                cmp: Compare {
                    stat: Stat::Power,
                    op: Op::Gt,
                    value: Amount::PowerOf(Box::new(me())),
                },
            },
            then: vec![],
        }]);
        assert_eq!(kinds(compare), [Kind::SelfComparison]);
        // "Whenever you cast a spell, if it's tapped, …"
        let spell = Ability::Triggered {
            trigger: Trigger::When(Box::new(Event::Casts {
                who: Player::You,
                what: objects(Quant::One, vec![], Some(Noun::Spell), vec![]),
            })),
            effect: vec![Effect::If {
                cond: Condition::Is {
                    what: objects(Quant::Triggering, vec![], None, vec![]),
                    filter: Filter {
                        alts: vec![vec![Adj::State(State::Tapped)]],
                        ..Filter::default()
                    },
                },
                then: vec![],
            }],
            limits: vec![],
        };
        assert_eq!(kinds(spell), [Kind::StateOfTriggeringSpell]);
        // "Target creature fights itself."
        let fight = Ability::Spell(vec![Effect::Fight { a: me(), b: me() }]);
        assert_eq!(kinds(fight), [Kind::FightOneObject]);
        // "Draw X cards." on a card with no X in its cost.
        let x = Ability::Spell(vec![Effect::Draw {
            who: Player::You,
            n: Amount::X,
        }]);
        assert_eq!(kinds(x.clone()), [Kind::UnboundX]);
        let cx = Context {
            x_defined: true,
            prior: false,
        };
        assert_eq!(lint(&x, cx), []);
        // "Destroy it." "That player loses 1 life."
        let it = Ability::Spell(vec![Effect::Destroy {
            what: objects(Quant::Antecedent, vec![], None, vec![]),
            no_regen: false,
        }]);
        assert_eq!(kinds(it), [Kind::DanglingAntecedent]);
        let that = Ability::Spell(vec![Effect::LoseLife {
            who: Player::That,
            n: Amount::N(1),
        }]);
        assert_eq!(kinds(that), [Kind::DanglingThatPlayer]);
    }
}
