//! References back as pointers ([backref] in SEMANTICS.md).
//!
//! A reference ("it", "that creature", "the exiled card", "those cards")
//! means an object some earlier part of the text introduced. This module
//! says which: [`introduces`] is the one definition of what introduces an
//! object, a walk lists every introducer and reference of a face in reading
//! order, and the resolver picks, for each reference, the introducers it
//! can mean and the one it does (the nearest that fits).
//!
//! Reading order is the walk's order (fields in declaration order, which is
//! reading order for almost every node) with three changes: an object is
//! introduced after its own parts, so "enchantments with the same name as
//! that enchantment" cannot point at the set it sits in; an effect's made
//! objects (tokens, the cards a draw or mill moves) are introduced after the
//! effect's parts; and an `If` or `ForEach` whose clause trails
//! (`ClauseOrder::Trailing`) is read effects first.

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use crate::ast::*;
use crate::walk::{Visitor, Walk};

// ---------------------------------------------------------------- introducers

/// A node of a tree that may introduce objects.
pub enum Node<'a> {
    Object(&'a Objects),
    /// An effect, which may make objects no `Objects` names.
    Effect(&'a Effect),
    /// A keyword whose ability exiles a card a linked ability names
    /// (hideaway, CR 702.75a).
    Keyword(&'a Keyword),
}

/// Where a node is said, as far as introducing goes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Place {
    /// Inside an amount: "equal to the number of creatures you control"
    /// counts objects and names none.
    pub in_amount: bool,
    /// Inside a condition that only asks whether such objects exist ("if
    /// you control a Human").
    pub in_existence: bool,
}

/// Whether `node`, said at `at`, introduces an object a later reference
/// may point back at. Every pointer counts introducers by this, so it is
/// the one definition ([backref]):
///
/// - an object introduces unless it is itself a reference (a pointer,
///   `Itself`, the card, the granting card, the ability), a player or a
///   counter, or is said inside an amount or an existence condition;
/// - a target always introduces, wherever it is said: it is chosen as the
///   spell or ability is put on the stack (CR 601.2c);
/// - an effect that makes or moves objects no `Objects` names introduces
///   them: tokens, the cards a draw, mill, look or reveal moves, a copy of a
///   spell, the cards of a zone exiled or discarded whole, the pile a dig
///   reveals, a manifested or cloaked card;
/// - hideaway introduces the card it exiles.
pub fn introduces(node: Node, at: Place) -> bool {
    match node {
        Node::Object(o) => {
            if reference(o).is_some() || not_object(&o.filter) {
                return false;
            }
            if matches!(
                o.quant,
                Quant::SelfRef | Quant::Granter | Quant::ThisAbility | Quant::Itself
            ) {
                return false;
            }
            targeted(o) || !(at.in_amount || at.in_existence)
        }
        Node::Effect(e) => made(e).is_some(),
        Node::Keyword(k) => k.name == "hideaway",
    }
}

/// What a reference back says it may mean, before it is a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hint {
    /// Any earlier object that fits.
    Antecedent,
    /// In a trailing condition: the one object its clause names.
    ClauseObject,
    /// The tokens an earlier effect made.
    Created,
    /// Every object one verb named, which no one `Objects` names.
    Listed,
    /// The last objects of the two clauses before.
    Both,
    /// The object of the trigger's event.
    Triggering,
    /// A spell and the copies an earlier effect made of it.
    OriginalAndCopies,
    /// What a linked ability exiled or created (CR 607.2a).
    Linked,
    /// "from among them": the pool a new choice is made from.
    Pool,
    /// "the rest": what the nearest choice left.
    Rest,
    /// Each one of a set in turn, said inside its filter or amount.
    Itself,
    // Pointers, read by the validator.
    BackRef,
    LinkedBackRef,
    BackRefs,
}

/// Whether an object is a reference back, and what it may mean.
pub fn reference(o: &Objects) -> Option<Hint> {
    Some(match o.quant {
        Quant::Antecedent | Quant::BeforeEach => Hint::Antecedent,
        Quant::ClauseObject => Hint::ClauseObject,
        Quant::Created => Hint::Created,
        Quant::Listed => Hint::Listed,
        Quant::Both => Hint::Both,
        Quant::Triggering => Hint::Triggering,
        Quant::OriginalAndCopies => Hint::OriginalAndCopies,
        Quant::Linked => Hint::Linked,
        Quant::Rest => Hint::Rest,
        Quant::Itself => Hint::Itself,
        Quant::BackRef { .. } => Hint::BackRef,
        Quant::LinkedBackRef { .. } => Hint::LinkedBackRef,
        Quant::BackRefs(_) => Hint::BackRefs,
        _ => return None,
    })
}

/// A target, chosen as the spell or ability is put on the stack.
fn targeted(o: &Objects) -> bool {
    matches!(
        o.quant,
        Quant::Target { .. }
            | Quant::AnyTarget { .. }
            | Quant::AnyOtherTarget
            | Quant::AnyNumberOfTarget
            | Quant::AnyNumberOfOtherTarget
            | Quant::AnotherTarget
            | Quant::SecondTarget
            | Quant::OtherTarget { .. }
    )
}

fn not_object(f: &Filter) -> bool {
    matches!(
        f.kind,
        Some(Noun::Player | Noun::Opponent | Noun::Counter(_))
    )
}

/// The verb of the objects an effect makes that no `Objects` names.
fn made(e: &Effect) -> Option<String> {
    Some(match e {
        Effect::Create { .. }
        | Effect::CreateNamed { .. }
        | Effect::CreateAttached { .. }
        | Effect::CreateThose { .. }
        | Effect::CreateThosePlus { .. }
        | Effect::CreateCopy { .. }
        | Effect::Amass { .. } => TOKEN.into(),
        Effect::RevealTop { .. } => "RevealTop".into(),
        Effect::LookAtTop { .. } => "LookAtTop".into(),
        Effect::Mill { .. } => "Mill".into(),
        Effect::Draw { .. } => "Draw".into(),
        Effect::Copy { .. } => "Copy".into(),
        Effect::ExileZone(_) => "ExileZone".into(),
        Effect::DiscardHand(_) => "DiscardHand".into(),
        Effect::RevealHand(_) => "RevealHand".into(),
        Effect::UntilFound { verb, .. } | Effect::DigUntilTotal { verb, .. } => {
            format!("UntilFound>{verb:?}+Reveal")
        }
        Effect::Action { name, .. } => match name.as_str() {
            "investigate" | "incubate" | "populate" | "amass" => TOKEN.into(),
            "manifest" | "manifest dread" | "cloak" | "venture into the dungeon" => {
                format!("Action>{name}")
            }
            _ => return None,
        },
        _ => return None,
    })
}

const TOKEN: &str = "@token";

fn token_of(e: &Effect) -> Option<Token> {
    match e {
        Effect::Create { token, .. }
        | Effect::CreateNamed { token, .. }
        | Effect::CreateAttached { token, .. }
        | Effect::CreateThosePlus { token, .. } => Some(token.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------- the walk

/// The name of a node's variant, from its `Debug`, without formatting the
/// rest of it.
fn head(x: &dyn fmt::Debug) -> String {
    struct Head(String, bool);
    impl fmt::Write for Head {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            if self.1 {
                return Err(fmt::Error);
            }
            for c in s.chars() {
                if c == ' ' || c == '(' || c == '{' {
                    self.1 = true;
                    return Err(fmt::Error);
                }
                self.0.push(c);
            }
            Ok(())
        }
    }
    let mut h = Head(String::new(), false);
    let _ = write!(h, "{x:?}");
    h.0
}

fn verb_of(e: &Effect) -> String {
    match e {
        Effect::Move { to, .. } => format!("Move>{}", head(to)),
        Effect::UntilFound { verb, .. } => format!("UntilFound>{verb:?}"),
        Effect::Action { name, .. } => format!("Action>{name}"),
        _ => head(e),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Role {
    Intro,
    /// Objects an effect made that no `Objects` names.
    Made,
    SelfMention,
    Ref(Hint),
    /// A reference to a player or a counter: no object.
    OutOfScope,
    /// An object that introduces nothing ([`introduces`]).
    Skip,
}

/// Which part of an object a reference is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// The object's quant.
    Quant,
    /// Its `FromAmongThem` post: the pool it is chosen from.
    Pool,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // `decl` and `slot` say where a pointer is written, from the next commit
struct Ev {
    /// The object's index in walk order, if the event is one.
    decl: Option<usize>,
    slot: Slot,
    role: Role,
    obj: Option<Objects>,
    token: Option<Token>,
    verb: String,
    in_trigger: bool,
    /// Ancestor frames, outermost first.
    frames: Vec<usize>,
    /// Ancestor objects (event ids), outermost first.
    anc: Vec<usize>,
    effect_frame: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fk {
    Root,
    Effect,
    Cond,
    Amount,
    Cost,
    Ability,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Trail {
    No,
    /// A trailing "if": its effects, then its condition.
    CondLast,
    /// A trailing "for each": its effects, then its object.
    EachLast,
}

#[derive(Clone, Copy, Debug)]
enum Item {
    Ev(usize),
    Fr(usize),
}

#[derive(Debug)]
struct Frame {
    kind: Fk,
    trail: Trail,
    items: Vec<Item>,
    verb: String,
}

/// The events of one line, in a tree of frames.
struct Collect {
    evs: Vec<Ev>,
    frames: Vec<Frame>,
    stack: Vec<usize>,
    obj_stack: Vec<usize>,
    decl: usize,
    /// Whether each `ability()` call pushed a frame.
    ab_stack: Vec<bool>,
    top: bool,
    /// Effects open at the top ability's level: 0 means trigger or cost.
    effects_open: usize,
    top_kind: &'static str,
    /// Each nested ability open: its kind and how many effects it began.
    nested: Vec<(&'static str, usize)>,
    /// Keywords of a `Keywords` line that introduce.
    keywords: usize,
}

const EXISTENTIAL: [&str; 18] = [
    "YouControl",
    "YouDontControl",
    "PlayerControls",
    "ControlsMore",
    "AmountIs",
    "Threshold",
    "DidThisTurn",
    "DidThisGame",
    "ThereAre",
    "HasMore",
    "Total",
    "PlayerHas",
    "PlayerHasAtMost",
    "PlayerHasFewerThan",
    "ZoneHasCards",
    "NthSpellThisTurn",
    "AttackedWithThisTurn",
    "AttackedWithThisCombat",
];

impl Collect {
    fn new() -> Self {
        Collect {
            evs: Vec::new(),
            frames: vec![Frame {
                kind: Fk::Root,
                trail: Trail::No,
                items: Vec::new(),
                verb: String::new(),
            }],
            stack: vec![0],
            obj_stack: Vec::new(),
            decl: 0,
            ab_stack: Vec::new(),
            top: true,
            effects_open: 0,
            top_kind: "",
            nested: Vec::new(),
            keywords: 0,
        }
    }

    fn push_frame(&mut self, kind: Fk, verb: String) {
        let id = self.frames.len();
        self.frames.push(Frame {
            kind,
            trail: Trail::No,
            items: Vec::new(),
            verb,
        });
        self.stack.push(id);
    }

    fn pop_frame(&mut self) -> usize {
        let id = self.stack.pop().expect("a frame is open");
        let parent = self.here();
        self.frames[parent].items.push(Item::Fr(id));
        id
    }

    fn here(&self) -> usize {
        *self.stack.last().expect("the root frame")
    }

    fn in_kind(&self, k: Fk) -> bool {
        self.stack.iter().any(|&f| self.frames[f].kind == k)
    }

    fn place(&self) -> Place {
        Place {
            in_amount: self.in_kind(Fk::Amount),
            in_existence: self.stack.iter().any(|&f| {
                self.frames[f].kind == Fk::Cond
                    && EXISTENTIAL.contains(&self.frames[f].verb.as_str())
            }),
        }
    }

    /// The verb an object said here is the object of.
    fn cur_verb(&self) -> String {
        for &f in self.stack.iter().rev() {
            match self.frames[f].kind {
                Fk::Effect | Fk::Cost => return self.frames[f].verb.clone(),
                Fk::Ability => break,
                _ => {}
            }
        }
        if let Some((k, n)) = self.nested.last() {
            return if *k == "Triggered" && *n == 0 {
                "@trigger".into()
            } else {
                "@granted".into()
            };
        }
        match self.top_kind {
            "Triggered" if self.effects_open == 0 => "@trigger".into(),
            k => format!("@{k}"),
        }
    }

    fn effect_in_ability(&self) -> bool {
        for &f in self.stack.iter().rev() {
            match self.frames[f].kind {
                Fk::Effect => return true,
                Fk::Ability => return false,
                _ => {}
            }
        }
        false
    }

    fn effect_frame(&self) -> Option<usize> {
        self.stack
            .iter()
            .rev()
            .find(|&&f| self.frames[f].kind == Fk::Effect)
            .copied()
    }

    fn new_ev(
        &mut self,
        decl: Option<usize>,
        slot: Slot,
        role: Role,
        obj: Option<Objects>,
    ) -> usize {
        let verb = self.cur_verb();
        let in_trigger = match self.nested.last() {
            Some((k, n)) => *k == "Triggered" && *n == 0 && !self.effect_in_ability(),
            None => {
                self.top_kind == "Triggered"
                    && self.effects_open == 0
                    && self.effect_frame().is_none()
            }
        };
        let id = self.evs.len();
        self.evs.push(Ev {
            decl,
            slot,
            role,
            obj,
            token: None,
            verb,
            in_trigger,
            frames: self.stack.clone(),
            anc: self.obj_stack.clone(),
            effect_frame: self.effect_frame(),
        });
        id
    }
}

fn marked_how(f: &Filter) -> Option<Marked> {
    f.posts.iter().find_map(|p| match p {
        Post::Marked(h) | Post::MarkedBy { how: h, .. } | Post::MarkedFrom { how: h, .. } => {
            Some(*h)
        }
        _ => None,
    })
}

/// A link post on a reference: "the exiled card" said with
/// `ExiledWith(SelfRef)`, the verb of the linked ability it names.
fn linked_post(f: &Filter) -> Option<&'static str> {
    f.posts.iter().find_map(|p| match p {
        Post::ExiledWith(o) if o.quant == Quant::SelfRef => Some("Exile"),
        Post::CreatedWith(o) if o.quant == Quant::SelfRef => Some(TOKEN),
        Post::PutOntoBattlefieldWith(o) if o.quant == Quant::SelfRef => Some("Move>Battlefield"),
        _ => None,
    })
}

impl Visitor for Collect {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let decl = self.decl;
        self.decl += 1;
        let place = self.place();
        let role = if let Some(k) = reference(o) {
            if not_object(&o.filter) {
                Role::OutOfScope
            } else {
                Role::Ref(k)
            }
        } else if o.quant == Quant::SelfRef {
            Role::SelfMention
        } else if introduces(Node::Object(o), place) {
            Role::Intro
        } else {
            Role::Skip
        };
        let id = self.new_ev(Some(decl), Slot::Quant, role.clone(), Some(o.clone()));
        // "from among them": a reference to a pool, read before the new
        // choice the same object introduces.
        if o.filter.posts.contains(&Post::FromAmongThem) {
            let r = self.new_ev(
                Some(decl),
                Slot::Pool,
                Role::Ref(Hint::Pool),
                Some(o.clone()),
            );
            let parent = self.here();
            self.frames[parent].items.push(Item::Ev(r));
        }
        if matches!(role, Role::Ref(_) | Role::SelfMention | Role::OutOfScope) {
            let parent = self.here();
            self.frames[parent].items.push(Item::Ev(id));
        }
        self.push_frame(Fk::Body, String::new());
        self.obj_stack.push(id);
        true
    }

    fn leave(&mut self, _o: &mut Objects) {
        let id = self.obj_stack.pop().expect("entered");
        self.pop_frame();
        if matches!(self.evs[id].role, Role::Intro | Role::Skip) {
            let parent = self.here();
            self.frames[parent].items.push(Item::Ev(id));
        }
    }

    fn ability(&mut self, a: &mut Ability) -> bool {
        if self.top {
            self.top = matches!(
                a,
                Ability::Labelled { .. }
                    | Ability::Harnessed(_)
                    | Ability::KeywordOn { .. }
                    | Ability::Station { .. }
            );
            if !self.top {
                self.top_kind = match a {
                    Ability::Triggered { .. } => "Triggered",
                    Ability::Activated { .. } => "Activated",
                    Ability::Static(_) => "Static",
                    Ability::Spell(_) => "Spell",
                    Ability::Keywords(_) => "Keywords",
                    _ => "Other",
                };
                if let Ability::Keywords(ks) = a {
                    for k in ks.iter() {
                        if introduces(Node::Keyword(k), Place::default()) {
                            self.keywords += 1;
                        }
                    }
                }
            }
            self.ab_stack.push(false);
            return true;
        }
        self.push_frame(Fk::Ability, String::new());
        self.ab_stack.push(true);
        let k = match a {
            Ability::Triggered { .. } => "Triggered",
            _ => "Other",
        };
        self.nested.push((k, 0));
        true
    }

    fn ability_done(&mut self, _a: &mut Ability) {
        if self.ab_stack.pop() == Some(true) {
            self.pop_frame();
            self.nested.pop();
        }
    }

    fn effect(&mut self, e: &mut Effect) {
        if !self.effect_in_ability() {
            match self.nested.last_mut() {
                Some((_, n)) => *n += 1,
                None => self.effects_open += 1,
            }
        }
        self.push_frame(Fk::Effect, verb_of(e));
    }

    fn effect_done(&mut self, e: &mut Effect) {
        let id = self.pop_frame();
        self.frames[id].trail = match e {
            Effect::If {
                order: ClauseOrder::Trailing,
                ..
            } => Trail::CondLast,
            Effect::ForEach {
                order: ClauseOrder::Trailing,
                ..
            } => Trail::EachLast,
            _ => Trail::No,
        };
        if introduces(Node::Effect(e), self.place()) {
            let ev = self.new_ev(None, Slot::Quant, Role::Made, None);
            self.evs[ev].verb = made(e).expect("introduces");
            self.evs[ev].token = token_of(e);
            let parent = self.here();
            self.frames[parent].items.push(Item::Ev(ev));
        }
    }

    fn condition(&mut self, c: &mut Condition) {
        self.push_frame(Fk::Cond, head(c));
    }
    fn condition_done(&mut self, _c: &mut Condition) {
        self.pop_frame();
    }
    fn amount(&mut self, _a: &mut Amount) {
        self.push_frame(Fk::Amount, String::new());
    }
    fn amount_done(&mut self, _a: &mut Amount) {
        self.pop_frame();
    }
    fn cost(&mut self, c: &mut Cost) {
        // A cost's objects are said before the effect, and "the sacrificed
        // creature" after it means the one the cost took.
        self.push_frame(Fk::Cost, format!("@cost:{}", head(c)));
    }
    fn cost_done(&mut self, _c: &mut Cost) {
        self.pop_frame();
    }
}

/// One scope's events in reading order, and the scopes nested in it.
fn flatten(c: &Collect, f: usize, out: &mut Vec<usize>, nested: &mut Vec<Vec<usize>>) {
    let fr = &c.frames[f];
    let mut items: Vec<Item> = fr.items.clone();
    let is_effect_frame = |it: &Item| matches!(it, Item::Fr(x) if c.frames[*x].kind == Fk::Effect);
    let is_cond = |it: &Item| matches!(it, Item::Fr(x) if c.frames[*x].kind == Fk::Cond);
    let move_last = |items: &mut Vec<Item>, pred: &dyn Fn(&Item) -> bool| {
        let (a, b): (Vec<Item>, Vec<Item>) = items.iter().partition(|i| !pred(i));
        *items = a.into_iter().chain(b).collect();
    };
    match fr.trail {
        Trail::CondLast => move_last(&mut items, &is_cond),
        Trail::EachLast => move_last(&mut items, &|i| !is_effect_frame(i)),
        Trail::No => {}
    }
    for it in items {
        match it {
            Item::Ev(e) => out.push(e),
            Item::Fr(x) if c.frames[x].kind == Fk::Ability => {
                let mut inner = Vec::new();
                flatten(c, x, &mut inner, nested);
                nested.push(inner);
            }
            Item::Fr(x) => flatten(c, x, out, nested),
        }
    }
}

// ---------------------------------------------------------------- fitting

const CARD_TYPES: [&str; 10] = [
    "artifact",
    "creature",
    "enchantment",
    "land",
    "planeswalker",
    "battle",
    "instant",
    "sorcery",
    "kindred",
    "tribal",
];

fn type_words(f: &Filter) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut t, mut s) = (BTreeSet::new(), BTreeSet::new());
    for alt in &f.alts {
        for a in alt {
            match a {
                Adj::Type(x) => {
                    t.insert(x.to_lowercase());
                }
                Adj::Subtype(x) => {
                    s.insert(x.to_lowercase());
                }
                _ => {}
            }
        }
    }
    if let Some(o) = &f.or {
        let (t2, s2) = type_words(o);
        t.extend(t2);
        s.extend(s2);
    }
    (t, s)
}

fn token_words(t: &Token) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut ty, mut sub) = (BTreeSet::new(), BTreeSet::new());
    for w in &t.types {
        let l = w.to_lowercase();
        if CARD_TYPES.contains(&l.as_str()) {
            ty.insert(l);
        } else {
            sub.insert(l);
        }
    }
    if let Some(n) = &t.named {
        sub.insert(n.to_lowercase());
        ty.insert("artifact".into());
    }
    (ty, sub)
}

fn verb_matches(how: Marked, verb: &str) -> bool {
    let has = |s: &str| verb.contains(s);
    match how {
        Marked::Exiled => has("Exile"),
        Marked::Chosen => has("Choose") || has("Choice") || has("Pile") || has("Vote"),
        Marked::Revealed => has("Reveal"),
        Marked::Sacrificed => has("Sacrifice"),
        Marked::Copied => has("Copy"),
        Marked::Returned => has("Move"),
        Marked::Destroyed => has("Destroy"),
        Marked::Found => has("Search") || has("UntilFound"),
        Marked::Tapped => has("Tap"),
        Marked::Milled => has("Mill"),
        Marked::Discarded => has("Discard"),
        Marked::Removed => has("RemoveCounters"),
        Marked::PutOntoBattlefield => has("Move>Battlefield") || has("PutOnto"),
        Marked::Manifested => has("manifest") || has("cloak"),
        Marked::Cast => has("Cast"),
        Marked::Cycled => has("@trigger"),
    }
}

fn is_tokenish(e: &Ev) -> bool {
    if e.role == Role::Made {
        return e.verb == TOKEN;
    }
    e.obj.as_ref().is_some_and(|o| {
        matches!(o.filter.kind, Some(Noun::Token | Noun::Copy))
            || o.filter
                .alts
                .iter()
                .any(|a| a.contains(&Adj::State(State::Token)))
    })
}

/// Whether a reference said with `r`'s filter can mean the introducer `e`.
fn compat(r: &Objects, e: &Ev) -> bool {
    let rf = &r.filter;
    if let Some(how) = marked_how(rf) {
        if !verb_matches(how, &e.verb) {
            return false;
        }
    }
    // The pile a dig revealed is said "the revealed cards", "the rest",
    // never a bare "it" or "that card" (which is the card found).
    if e.role == Role::Made
        && e.verb.starts_with("UntilFound")
        && marked_how(rf) != Some(Marked::Revealed)
    {
        return false;
    }
    let ek = e.obj.as_ref().and_then(|o| o.filter.kind.clone());
    match &rf.kind {
        Some(Noun::Token) if !is_tokenish(e) => return false,
        Some(Noun::Copy)
            if !(e.role == Role::Made && (e.verb == "Copy" || e.verb == TOKEN)
                || matches!(ek, Some(Noun::Copy))) =>
        {
            return false
        }
        Some(Noun::Spell) => {
            let ok = e.role == Role::Made && e.verb == "Copy"
                || matches!(
                    ek,
                    Some(Noun::Spell | Noun::Card | Noun::SpellOrAbility | Noun::Copy)
                )
                || e.verb.contains("Cast");
            if !ok {
                return false;
            }
        }
        Some(Noun::Ability) if !matches!(ek, Some(Noun::Ability | Noun::SpellOrAbility)) => {
            return false
        }
        Some(Noun::Player | Noun::Opponent | Noun::Counter(_)) => return false,
        _ => {}
    }
    if matches!(ek, Some(Noun::Ability))
        && !matches!(rf.kind, Some(Noun::Ability | Noun::SpellOrAbility))
        && !rf.alts.is_empty()
    {
        return false;
    }
    let ctl = |f: &Filter| {
        f.posts.iter().find_map(|p| match p {
            Post::Controller(x) => Some(x.clone()),
            _ => None,
        })
    };
    if let (Some(a), Some(Some(b))) = (ctl(rf), e.obj.as_ref().map(|o| ctl(&o.filter))) {
        if a != b {
            return false;
        }
    }
    let (rt, rs) = type_words(rf);
    let (et, es) = match (&e.token, &e.obj) {
        (Some(t), _) => token_words(t),
        (None, Some(o)) => type_words(&o.filter),
        _ => Default::default(),
    };
    if !rt.is_empty() && !et.is_empty() && rt.is_disjoint(&et) {
        return false;
    }
    if !rs.is_empty() && !es.is_empty() && rs.is_disjoint(&es) {
        return false;
    }
    true
}

/// Whether a reference can be the card itself: only a bare "it"/"its"
/// (Oracle calls the card "this creature", "~", CR 700.7), whose types the
/// type line has.
fn self_compat(r: &Objects, type_line: &str) -> bool {
    let rf = &r.filter;
    if matches!(
        rf.kind,
        Some(Noun::Token | Noun::Player | Noun::Opponent | Noun::Counter(_) | Noun::Ability)
    ) {
        return false;
    }
    if marked_how(rf).is_some() || linked_post(rf).is_some() {
        return false;
    }
    if !rf.alts.is_empty() || rf.kind.is_some() {
        return false;
    }
    let tl = type_line.to_lowercase();
    let words: BTreeSet<&str> = tl
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .collect();
    let (rt, rs) = type_words(rf);
    let types_ok = rt.is_empty() || rt.iter().any(|t| words.contains(t.as_str()));
    let subs_ok = rs.is_empty() || rs.iter().any(|t| words.contains(t.as_str()));
    types_ok && subs_ok
}

// ---------------------------------------------------------------- resolving

/// What a reference may mean.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    /// (line, event) of an introducer.
    Intro(usize, usize),
    /// The card itself.
    SelfCard,
    /// (line, event) of an introducer in another line: a linked ability.
    Linked(usize, usize),
    /// The object whose filter the reference sits in (a bound variable).
    Enclosing(usize),
    /// Several objects at once.
    Set(Vec<Target>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// One candidate.
    Determined,
    /// Several; the nearest is picked.
    Ambiguous,
    /// None.
    None,
}

/// One reference of a face, and what it resolved to.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub line: usize,
    pub ev: usize,
    pub hint: Hint,
    pub class: Class,
    pub cands: Vec<Target>,
    pub pick: Option<Target>,
    pub note: String,
    /// The pick is in an earlier paragraph of the same spell.
    pub cross_line: bool,
}

struct LineData {
    c: Collect,
    /// The top scope then the nested scopes, each in reading order.
    scopes: Vec<Vec<usize>>,
    continues: bool,
    die_row: bool,
}

/// A face's lines read for references.
pub struct Analysis {
    lines: Vec<LineData>,
    pub resolved: Vec<Resolved>,
}

fn counts_as_intro(e: &Ev) -> bool {
    matches!(e.role, Role::Intro | Role::Made)
}

/// The introducers of a line's top scope in reading order, its keyword
/// ones first.
fn line_intros(ld: &LineData) -> Vec<usize> {
    ld.scopes[0]
        .iter()
        .copied()
        .filter(|&e| counts_as_intro(&ld.c.evs[e]))
        .collect()
}

/// Reads each line (`continues`: a spell's later paragraph) and resolves
/// every reference in it. A line that did not parse is `None` and keeps its
/// place, so a line number is the face's.
pub fn analyse(lines: &[(Option<&Ability>, bool)], type_line: &str) -> Analysis {
    let lds: Vec<LineData> = lines
        .iter()
        .map(|(a, continues)| {
            let mut c = Collect::new();
            if let Some(a) = a {
                (*a).clone().walk(&mut c);
            }
            // A keyword that introduces: its card, before anything else.
            for _ in 0..c.keywords {
                let ev = c.new_ev(None, Slot::Quant, Role::Made, None);
                c.evs[ev].verb = "Exile".into();
                c.frames[0].items.insert(0, Item::Ev(ev));
            }
            let mut top = Vec::new();
            let mut nested = Vec::new();
            flatten(&c, 0, &mut top, &mut nested);
            let mut scopes = vec![top];
            scopes.extend(nested);
            LineData {
                c,
                scopes,
                continues: *continues,
                die_row: matches!(a, Some(Ability::DieRow { .. })),
            }
        })
        .collect();
    let resolved = resolve_face(&lds, type_line);
    Analysis {
        lines: lds,
        resolved,
    }
}

fn resolve_face(lds: &[LineData], type_line: &str) -> Vec<Resolved> {
    let mut out = resolve_pass(lds, type_line);
    // A linked reference whose object the other ability names by a
    // reference of its own (Wormfang Crab: "an opponent chooses a
    // permanent … and exiles it"): that reference's pick.
    let picks: Vec<(usize, String, Target)> = out
        .iter()
        .filter_map(|q| match &q.pick {
            Some(t @ Target::Intro(..)) => {
                Some((q.line, lds[q.line].c.evs[q.ev].verb.clone(), t.clone()))
            }
            _ => None,
        })
        .collect();
    for r in out.iter_mut().filter(|r| r.class == Class::None) {
        let o = lds[r.line].c.evs[r.ev].obj.clone().expect("a reference");
        let lp = linked_post(&o.filter);
        let mh = marked_how(&o.filter);
        let want = |x: &str| match (lp, mh) {
            (Some(v), _) => x.contains(v),
            (None, Some(h)) => verb_matches(h, x),
            _ => false,
        };
        for (l, verb, t) in &picks {
            if *l != r.line && want(verb) {
                if let Target::Intro(a, b) = t {
                    let t = Target::Linked(*a, *b);
                    if !r.cands.contains(&t) {
                        r.cands.push(t);
                    }
                }
            }
        }
        if !r.cands.is_empty() {
            r.class = if r.cands.len() == 1 {
                Class::Determined
            } else {
                Class::Ambiguous
            };
            r.pick = Some(r.cands[0].clone());
            r.note = "linked through the other ability's reference".into();
        }
    }
    out
}

fn resolve_pass(lds: &[LineData], type_line: &str) -> Vec<Resolved> {
    let mut out = Vec::new();
    let intros_per_line: Vec<Vec<usize>> = lds.iter().map(line_intros).collect();
    // Earlier paragraphs of the same spell (CR 608.2c).
    let mut prior: Vec<(usize, usize)> = Vec::new();
    let mut prior_self = false;
    // A die's rows are alternatives: each continues the roll above them
    // (CR 706.3a), none the row before it.
    let mut roll: (Vec<(usize, usize)>, bool) = (Vec::new(), false);
    for (li, ld) in lds.iter().enumerate() {
        if ld.die_row {
            (prior, prior_self) = roll.clone();
        } else if !ld.continues {
            prior.clear();
            prior_self = false;
        }
        for (si, scope) in ld.scopes.iter().enumerate() {
            let mut intros: Vec<(usize, usize)> = if si == 0 { prior.clone() } else { Vec::new() };
            let mut self_at: Option<usize> = if si == 0 && prior_self { Some(0) } else { None };
            // References said so far, at their position, with their pick.
            let mut earlier: Vec<(usize, Vec<Target>)> = Vec::new();
            // Every object said so far as what it settled on.
            let mut mentions: Vec<((usize, usize), Target)> = Vec::new();
            for &ei in scope {
                let e = &ld.c.evs[ei];
                if counts_as_intro(e) {
                    intros.push((li, ei));
                    mentions.push(((li, ei), Target::Intro(li, ei)));
                    continue;
                }
                match &e.role {
                    Role::SelfMention => {
                        self_at = Some(intros.len());
                        mentions.push(((li, ei), Target::SelfCard));
                    }
                    Role::Ref(k) => {
                        let r = e.obj.clone().expect("a reference is an object");
                        let pos = intros.len();
                        let (pick, class, cands, note) = resolve_one(
                            lds,
                            li,
                            ei,
                            *k,
                            &r,
                            scope,
                            &intros,
                            self_at,
                            &earlier,
                            &mentions,
                            &intros_per_line,
                            type_line,
                        );
                        let cross_line = matches!(&pick, Some(Target::Intro(l, _)) if *l != li);
                        if *k != Hint::Pool {
                            // Only what it settled on chains on: "its owner"
                            // after "the exiled card" is that card.
                            earlier.push((pos, pick.iter().cloned().collect()));
                            if let Some(p) = &pick {
                                mentions.push(((li, ei), p.clone()));
                            }
                        }
                        out.push(Resolved {
                            line: li,
                            ev: ei,
                            hint: *k,
                            class,
                            cands,
                            pick,
                            note,
                            cross_line,
                        });
                    }
                    _ => {}
                }
            }
            if si == 0 {
                prior = intros.clone();
                prior_self |= self_at.is_some();
                if !ld.die_row {
                    roll = (prior.clone(), prior_self);
                }
            }
        }
    }
    out
}

/// The line, and within the reference's own line the effect, a listed
/// object was said in.
type Group = (usize, Option<usize>);

#[allow(clippy::too_many_arguments)]
fn resolve_one(
    lds: &[LineData],
    li: usize,
    ei: usize,
    k: Hint,
    r: &Objects,
    scope: &[usize],
    intros: &[(usize, usize)],
    self_at: Option<usize>,
    earlier: &[(usize, Vec<Target>)],
    mentions: &[((usize, usize), Target)],
    intros_per_line: &[Vec<usize>],
    type_line: &str,
) -> (Option<Target>, Class, Vec<Target>, String) {
    let ld = &lds[li];
    let e = &ld.c.evs[ei];
    let get = |(l, i): (usize, usize)| &lds[l].c.evs[i];
    let mut cands: Vec<Target> = Vec::new();
    let mut note = String::new();
    // "create X tokens, where X is its power": X is worked out before the
    // tokens exist (CR 107.3, 608.2h), so the X of a `Where` cannot mean
    // what its effect makes.
    let where_frame = e.frames.windows(2).rev().find_map(|w| {
        (ld.c.frames[w[0]].verb == "Where" && ld.c.frames[w[1]].kind == Fk::Amount).then_some(w[0])
    });
    let made_by_where =
        |x: &Ev| where_frame.is_some_and(|w| x.role == Role::Made && x.frames.contains(&w));
    let push_pool = |cands: &mut Vec<Target>, pred: &dyn Fn(&Ev) -> bool| {
        for &(l, i) in intros.iter().rev() {
            if pred(get((l, i))) && compat(r, get((l, i))) && !made_by_where(get((l, i))) {
                cands.push(Target::Intro(l, i));
            }
        }
    };
    let fits = |t: &Target, x: &Ev| match t {
        Target::Intro(..) => compat(r, x),
        Target::SelfCard => self_compat(r, type_line),
        Target::Linked(a, b) => compat(r, get((*a, *b))),
        _ => true,
    };
    match k {
        Hint::Created => push_pool(&mut cands, &is_tokenish),
        Hint::Triggering => push_pool(&mut cands, &|x| x.in_trigger),
        // The one object the clause the condition trails names: what the
        // frame holding the condition says outside it, an object or a
        // reference's pick ("counter that spell if its mana value …").
        Hint::ClauseObject => {
            let ci = e
                .frames
                .iter()
                .rposition(|&f| ld.c.frames[f].kind == Fk::Cond);
            if let Some(ci) = ci.filter(|&c| c > 0) {
                let (holder, cond) = (e.frames[ci - 1], e.frames[ci]);
                let inside = |x: &Ev| x.frames.contains(&holder) && !x.frames.contains(&cond);
                for ((l, i), t) in mentions.iter().rev() {
                    let x = get((*l, *i));
                    if *l == li && inside(x) && !cands.contains(t) && fits(t, x) {
                        cands.push(t.clone());
                    }
                }
                if cands.is_empty()
                    && scope.iter().any(|&j| {
                        let x = &ld.c.evs[j];
                        j != ei
                            && inside(x)
                            && matches!(x.role, Role::Intro | Role::Ref(_) | Role::SelfMention)
                    })
                {
                    note = "its clause comes after it in reading order".into();
                }
            }
        }
        Hint::Itself => {
            if let Some(&anc) = e
                .anc
                .iter()
                .rev()
                .find(|&&a| matches!(ld.c.evs[a].role, Role::Intro | Role::Skip))
            {
                cands.push(Target::Enclosing(anc));
                note = "in its set's own filter".into();
            } else {
                let ef = e.effect_frame;
                push_pool(&mut cands, &|x| ef.is_some_and(|f| x.frames.contains(&f)));
            }
        }
        Hint::Listed | Hint::Both => {
            // The latest effect with two or more objects, or (Both) the
            // last object of each of the two effects before.
            let mut groups: Vec<(Group, Vec<Target>)> = Vec::new();
            for ((l, i), t) in mentions.iter() {
                let x = get((*l, *i));
                if matches!(t, Target::Intro(..)) && !compat(r, x) {
                    continue;
                }
                let key = (*l, if *l == li { x.effect_frame } else { None });
                match groups.last_mut() {
                    Some((g, v)) if *g == key => {
                        if !v.contains(t) {
                            v.push(t.clone())
                        }
                    }
                    _ => groups.push((key, vec![t.clone()])),
                }
            }
            if k == Hint::Listed {
                if let Some((_, v)) = groups.iter().rev().find(|(_, v)| v.len() >= 2) {
                    cands.push(Target::Set(v.clone()));
                }
            } else if groups.len() >= 2 {
                let n = groups.len();
                let last = |g: &Vec<Target>| g.last().expect("a group is never empty").clone();
                cands.push(Target::Set(vec![
                    last(&groups[n - 2].1),
                    last(&groups[n - 1].1),
                ]));
            }
        }
        _ if k == Hint::Linked || linked_post(&r.filter).is_some() => {
            let verb = linked_post(&r.filter).unwrap_or("?");
            let vm = |x: &Ev| {
                if verb == TOKEN {
                    is_tokenish(x)
                } else {
                    x.verb.contains(verb)
                }
            };
            push_pool(&mut cands, &vm);
            for (lj, ints) in intros_per_line.iter().enumerate() {
                if lj == li {
                    continue;
                }
                for &i in ints {
                    let x = &lds[lj].c.evs[i];
                    if vm(x) && compat(r, x) {
                        cands.push(Target::Linked(lj, i));
                    }
                }
            }
        }
        Hint::Pool => push_pool(&mut cands, &|_| true),
        // "the rest": what the nearest choice left, of the pool it chose
        // from.
        Hint::Rest => {
            let choice = intros.iter().rev().find(|&&(l, i)| {
                let x = get((l, i));
                x.role == Role::Made && x.verb != TOKEN
                    || x.obj.as_ref().is_some_and(|o| {
                        o.filter
                            .posts
                            .iter()
                            .any(|p| matches!(p, Post::FromAmongThem | Post::FromAmong(_)))
                            || matches!(
                                o.quant,
                                Quant::One
                                    | Quant::N(_)
                                    | Quant::UpTo(_)
                                    | Quant::AnyNumber
                                    | Quant::Target { .. }
                            )
                    })
            });
            if let Some(&(l, i)) = choice {
                cands.push(Target::Intro(l, i));
            }
        }
        // Antecedent, OriginalAndCopies: any earlier object that fits.
        _ => {
            let copies = k == Hint::OriginalAndCopies;
            push_pool(&mut cands, &|x| !copies || x.verb.contains("Copy"));
            if self_at.is_some() && self_compat(r, type_line) {
                cands.push(Target::SelfCard);
            }
            // A reference back to a reference: the same object, when it
            // fits.
            for (_, ts) in earlier.iter() {
                for t in ts {
                    let ok = match t {
                        Target::Linked(l, i) => compat(r, &lds[*l].c.evs[*i]),
                        Target::SelfCard => self_compat(r, type_line),
                        _ => false,
                    };
                    if ok && !cands.contains(t) {
                        cands.push(t.clone());
                    }
                }
            }
        }
    }
    // "The chosen creature" with no choice in the ability: a linked
    // ability's choice (CR 607.2d).
    if cands.is_empty() && !matches!(k, Hint::Itself | Hint::Rest) {
        if let Some(how) = marked_how(&r.filter) {
            for (lj, ints) in intros_per_line.iter().enumerate() {
                if lj == li {
                    continue;
                }
                for &i in ints {
                    let x = &lds[lj].c.evs[i];
                    if verb_matches(how, &x.verb) && compat(r, x) {
                        cands.push(Target::Linked(lj, i));
                    }
                }
            }
            if !cands.is_empty() {
                note = "linked by its Marked post".into();
            }
        }
    }
    let mut seen = BTreeSet::new();
    cands.retain(|t| seen.insert(t.clone()));
    let class = match cands.len() {
        0 => Class::None,
        1 => Class::Determined,
        _ => Class::Ambiguous,
    };
    // The nearest that fits: an introducer at its index, the card at its
    // last mention, a chained object at the reference that named it.
    let pick = match class {
        Class::None => None,
        Class::Determined => Some(cands[0].clone()),
        Class::Ambiguous => {
            let rec = |t: &Target| -> f64 {
                let chained = earlier
                    .iter()
                    .filter(|(_, ts)| ts.contains(t))
                    .map(|(p, _)| *p as f64 - 0.5)
                    .fold(f64::MIN, f64::max);
                let own = match t {
                    Target::Intro(l, i) => intros
                        .iter()
                        .position(|x| *x == (*l, *i))
                        .map_or(f64::MIN, |p| p as f64),
                    Target::SelfCard => self_at.map_or(f64::MIN, |s| s as f64 - 0.5),
                    _ => -1e9,
                };
                own.max(chained)
            };
            cands
                .iter()
                .max_by(|a, b| rec(a).total_cmp(&rec(b)))
                .cloned()
        }
    };
    (pick, class, cands, note)
}

impl Analysis {
    /// A plain account of what `t` is, for a reader of a tree.
    pub fn describe(&self, t: &Target) -> String {
        let one = |l: usize, i: usize| -> String {
            let e = &self.lines[l].c.evs[i];
            match (&e.obj, e.role == Role::Made) {
                (_, true) => match e.verb.as_str() {
                    TOKEN => "the token(s) created".to_string(),
                    "Exile" => "the card hideaway exiled".to_string(),
                    v => format!("the cards of {v}"),
                },
                (Some(o), _) => crate::unparse::objects_text(o),
                _ => "?".into(),
            }
        };
        match t {
            Target::Intro(l, i) => one(*l, *i),
            Target::SelfCard => "this card".into(),
            Target::Linked(l, i) => format!("line {l}: {}", one(*l, *i)),
            Target::Enclosing(_) => "the set it sits in".into(),
            Target::Set(v) => v
                .iter()
                .map(|t| self.describe(t))
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }

    /// The reference an entry of `resolved` is, as its filter says it.
    pub fn reference_text(&self, r: &Resolved) -> String {
        self.lines[r.line].c.evs[r.ev]
            .obj
            .as_ref()
            .map(crate::unparse::objects_text)
            .unwrap_or_default()
    }

    /// The verb the reference is said with ("@granted" inside a quoted
    /// ability).
    pub fn verb(&self, r: &Resolved) -> &str {
        &self.lines[r.line].c.evs[r.ev].verb
    }

    /// The reference's filter kind.
    pub fn kind(&self, r: &Resolved) -> Option<Noun> {
        self.lines[r.line].c.evs[r.ev]
            .obj
            .as_ref()
            .and_then(|o| o.filter.kind.clone())
    }
}
