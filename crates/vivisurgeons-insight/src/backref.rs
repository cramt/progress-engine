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

use std::collections::{BTreeMap, BTreeSet};
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
/// - an effect that makes, moves or picks objects no `Objects` names
///   introduces them: tokens, the cards a draw, mill, look or reveal moves,
///   a copy of a spell, the cards of a zone exiled or discarded whole, the
///   pile a dig reveals, a manifested or cloaked card, one of each kind
///   chosen;
/// - hideaway introduces the card it exiles.
pub fn introduces(node: Node, at: Place) -> bool {
    match node {
        Node::Object(o) => {
            if reference(o).is_some() || not_object(&o.filter) {
                return false;
            }
            if matches!(
                o.quant,
                Quant::SelfRef | Quant::Granter | Quant::ThisAbility | Quant::Itself | Quant::Made
            ) {
                return false;
            }
            targeted(o) || !(at.in_amount || at.in_existence)
        }
        Node::Effect(e) => made(e).is_some(),
        Node::Keyword(k) => k.name == "hideaway",
    }
}

/// What kind of reference back an object is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Not yet a pointer: what its words say it may mean.
    Text(Hint),
    /// Each one of a set in turn, said inside its filter or amount: a
    /// bound variable, never a pointer.
    Itself,
    // Pointers, which the resolver checks rather than resolves.
    BackRef,
    LinkedBackRef,
    BackRefs,
    Rest,
    /// A reference to a binder ([binders]), which `binders` reads.
    Bound,
}

/// Whether an object is a reference back, and what kind.
pub fn reference(o: &Objects) -> Option<Kind> {
    Some(match o.quant {
        Quant::Antecedent | Quant::BeforeEach => Kind::Text(Hint::Antecedent),
        Quant::ClauseObject => Kind::Text(Hint::ClauseObject),
        Quant::Created => Kind::Text(Hint::Created),
        Quant::Listed => Kind::Text(Hint::Listed),
        Quant::Both => Kind::Text(Hint::Both),
        Quant::Triggering => Kind::Text(Hint::Triggering),
        Quant::OriginalAndCopies => Kind::Text(Hint::OriginalAndCopies),
        Quant::Linked => Kind::Text(Hint::Linked),
        Quant::Unresolved(h) => Kind::Text(h),
        Quant::Itself => Kind::Itself,
        Quant::BackRef { .. } => Kind::BackRef,
        Quant::LinkedBackRef { .. } => Kind::LinkedBackRef,
        Quant::BackRefs(_) => Kind::BackRefs,
        Quant::Rest { .. } => Kind::Rest,
        Quant::Bound(_) | Quant::Introduced(_) | Quant::LinkedBound { .. } | Quant::Bounds(_) => {
            Kind::Bound
        }
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
        // "chooses an artifact, a creature, an enchantment, and a
        // planeswalker from among …": the picks, which only filters name.
        Effect::Choose(Choice::OneOfEach { .. }) => "Choose".into(),
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
    Ref(Kind),
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
    /// The ability (no wrapper) it is said in, by walk order.
    scope_root: usize,
    /// The index of the effect it is said in within that ability's own
    /// list of effects, if any.
    top_effect: Option<usize>,
    /// It is a `ForEach`'s loop object.
    each_of_loop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fk {
    Root,
    Effect,
    Cond,
    Amount,
    Cost,
    /// A static ability, which may hold a condition said after it.
    Static,
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
    /// The abilities (no wrappers) open, by walk order, and the index of
    /// the effect each is in within its own list.
    roots: Vec<(usize, Option<usize>)>,
    next_root: usize,
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
            roots: Vec::new(),
            next_root: 0,
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
        let (scope_root, top_effect) = self.roots.last().copied().unwrap_or((0, None));
        let each_of_loop = self.frames[self.here()].kind == Fk::Effect
            && self.frames[self.here()].verb == "ForEach"
            && self.frames[self.here()].items.is_empty()
            && decl.is_some();
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
            scope_root,
            top_effect,
            each_of_loop,
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
                Role::Ref(Kind::Text(Hint::Pool)),
                // The pool is said "them": its own words say nothing of it.
                Some(Objects {
                    quant: Quant::Unresolved(Hint::Pool),
                    filter: Filter::default(),
                }),
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
                    | Ability::Let { .. }
            );
            if !self.top {
                self.roots.push((self.next_root, None));
                self.next_root += 1;
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
        self.roots.push((self.next_root, None));
        self.next_root += 1;
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
            self.roots.pop();
        }
    }

    fn effect(&mut self, e: &mut Effect) {
        if !self.effect_in_ability() {
            let n = match self.nested.last_mut() {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    self.effects_open += 1;
                    self.effects_open
                }
            };
            if let Some(r) = self.roots.last_mut() {
                r.1 = Some(n - 1);
            }
        }
        self.push_frame(Fk::Effect, verb_of(e));
    }

    fn effect_done(&mut self, e: &mut Effect) {
        // The objects it made are said in it.
        let top = self.roots.last().and_then(|r| r.1);
        let id = self.pop_frame();
        if !self.effect_in_ability() {
            if let Some(r) = self.roots.last_mut() {
                r.1 = None;
            }
        }
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
            self.evs[ev].top_effect = top;
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
    fn statik(&mut self, _s: &mut Static) {
        self.push_frame(Fk::Static, String::new());
    }
    fn statik_done(&mut self, s: &mut Static) {
        let id = self.pop_frame();
        if let Static::If {
            order: ClauseOrder::Trailing,
            ..
        }
        | Static::AsLongAs {
            order: ClauseOrder::Trailing,
            ..
        } = s
        {
            self.frames[id].trail = Trail::CondLast;
        }
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
    pub kind: Kind,
    pub class: Class,
    pub cands: Vec<Target>,
    pub pick: Option<Target>,
    pub note: String,
    /// The pick is in an earlier paragraph of the same spell.
    pub cross_line: bool,
    /// The pick as pointers counted from where the reference is: one, or
    /// several for a set. `None` where it has no pick, or is a bound
    /// variable.
    pub pointer: Option<Vec<Pointer>>,
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
    let intros_per_line: Vec<Vec<usize>> = lds.iter().map(line_intros).collect();
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
            r.pointer = pointer_of(&r.cands[0], &[], &intros_per_line);
            r.note = "linked through the other ability's reference".into();
        }
    }
    // A pointer to such an object fits "the exiled card" though the
    // object was introduced by another verb: the other ability exiled it
    // by a reference of its own.
    for r in out
        .iter_mut()
        .filter(|r| r.kind == Kind::LinkedBackRef && !r.note.is_empty())
    {
        let Some(Target::Linked(l, i)) = r.pick.clone() else {
            continue;
        };
        let mut o = lds[r.line].c.evs[r.ev].obj.clone().expect("a reference");
        let Some(how) = marked_how(&o.filter) else {
            continue;
        };
        o.filter.posts.retain(|p| !matches!(p, Post::Marked(_)));
        let named = picks
            .iter()
            .any(|(pl, verb, t)| *pl == l && *t == Target::Intro(l, i) && verb_matches(how, verb));
        if named && compat(&o, &lds[l].c.evs[i]) {
            r.note.clear();
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
                        let pointer = pick
                            .as_ref()
                            .and_then(|t| pointer_of(t, &intros, &intros_per_line));
                        if *k != Kind::Text(Hint::Pool) {
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
                            kind: *k,
                            class,
                            cands,
                            pick,
                            note,
                            cross_line,
                            pointer,
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

/// `t` as pointers from the end of `intros`, where the reference is.
fn pointer_of(
    t: &Target,
    intros: &[(usize, usize)],
    intros_per_line: &[Vec<usize>],
) -> Option<Vec<Pointer>> {
    Some(match t {
        Target::Intro(l, i) => {
            let at = intros.iter().rposition(|x| *x == (*l, *i))?;
            vec![Pointer::Back(u8::try_from(intros.len() - at).ok()?)]
        }
        Target::SelfCard => vec![Pointer::ThisCard],
        Target::Linked(l, i) => {
            let nth = intros_per_line[*l].iter().position(|x| x == i)? + 1;
            vec![Pointer::Linked {
                line: u8::try_from(*l).ok()?,
                nth: u8::try_from(nth).ok()?,
            }]
        }
        Target::Set(v) => {
            let mut out = Vec::new();
            for t in v {
                out.extend(pointer_of(t, intros, intros_per_line)?);
            }
            out
        }
        Target::Enclosing(_) => return None,
    })
}

/// The line, and within the reference's own line the effect, a listed
/// object was said in.
type Group = (usize, Option<usize>);

#[allow(clippy::too_many_arguments)]
fn resolve_one(
    lds: &[LineData],
    li: usize,
    ei: usize,
    k: Kind,
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
    // "Search your library for a card, exile it …. You may cast the exiled
    // card": an object one verb introduced and a reference exiled fits
    // "the exiled card" too.
    let unmarked = {
        let mut u = r.clone();
        u.filter.posts.retain(|p| !matches!(p, Post::Marked(_)));
        u
    };
    let named_so = |t: (usize, usize)| {
        marked_how(&r.filter).is_some_and(|how| {
            compat(&unmarked, get(t))
                && mentions.iter().any(|((l, i), m)| {
                    *m == Target::Intro(t.0, t.1) && verb_matches(how, &get((*l, *i)).verb)
                })
        })
    };
    // A pointer is checked, not resolved: it lands where it says, on an
    // introducer its words fit.
    let back = |b: u8, cands: &mut Vec<Target>, note: &mut String, check: bool| {
        let b = usize::from(b);
        if b == 0 || b > intros.len() {
            *note = format!("back {b} out of range ({} introduced before)", intros.len());
            return;
        }
        let t = intros[intros.len() - b];
        if check && !compat(r, get(t)) && !named_so(t) {
            *note = format!("back {b} lands on an object its words do not fit");
        }
        cands.push(Target::Intro(t.0, t.1));
    };
    let linked = |line: u8, nth: u8, cands: &mut Vec<Target>, note: &mut String| {
        let l = usize::from(line);
        match intros_per_line
            .get(l)
            .and_then(|v| v.get(usize::from(nth).wrapping_sub(1)))
        {
            Some(&i) => {
                if !compat(r, &lds[l].c.evs[i]) {
                    *note = format!(
                        "line {l} object {nth} does not fit its words (said with {})",
                        lds[l].c.evs[i].verb
                    );
                }
                cands.push(Target::Linked(l, i));
            }
            None => *note = format!("line {l} introduces no object {nth}"),
        }
    };
    match (&r.quant, k) {
        (Quant::BackRef { back: b }, _) => back(*b, &mut cands, &mut note, true),
        (Quant::Rest { of: b }, _) => back(*b, &mut cands, &mut note, false),
        (Quant::LinkedBackRef { line, nth }, _) => linked(*line, *nth, &mut cands, &mut note),
        (Quant::BackRefs(ps), _) => {
            let mut set = Vec::new();
            for p in ps {
                match p {
                    Pointer::Back(b) => back(*b, &mut set, &mut note, true),
                    Pointer::ThisCard => set.push(Target::SelfCard),
                    Pointer::Linked { line, nth } => linked(*line, *nth, &mut set, &mut note),
                    // A binder is read by `binders`, not counted here.
                    Pointer::Bound(_) | Pointer::LinkedBound { .. } => {}
                }
            }
            cands.push(Target::Set(set));
        }
        _ => {}
    }
    if matches!(
        k,
        Kind::BackRef | Kind::Rest | Kind::LinkedBackRef | Kind::BackRefs
    ) {
        let class = if cands.is_empty() {
            Class::None
        } else {
            Class::Determined
        };
        return (cands.first().cloned(), class, cands, note);
    }
    match k {
        Kind::Text(Hint::Created) => push_pool(&mut cands, &is_tokenish),
        Kind::Text(Hint::Triggering) => push_pool(&mut cands, &|x| x.in_trigger),
        // The one object the clause the condition trails names: what the
        // frame holding the condition says outside it, an object or a
        // reference's pick ("counter that spell if its mana value …").
        Kind::Text(Hint::ClauseObject) => {
            let inner = e
                .frames
                .iter()
                .rposition(|&f| ld.c.frames[f].kind == Fk::Cond);
            // "unless it has the same name as …": the outermost condition
            // of the clause, where the innermost names nothing.
            let outer = e
                .frames
                .iter()
                .rposition(|&f| matches!(ld.c.frames[f].kind, Fk::Effect | Fk::Static))
                .and_then(|s| {
                    (s + 1..e.frames.len()).find(|&j| ld.c.frames[e.frames[j]].kind == Fk::Cond)
                });
            for ci in [inner, outer].into_iter().flatten() {
                if !cands.is_empty() || ci == 0 {
                    continue;
                }
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
        Kind::Itself => {
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
        Kind::Text(Hint::Listed) | Kind::Text(Hint::Both) => {
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
            if k == Kind::Text(Hint::Listed) {
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
        _ if k == Kind::Text(Hint::Linked) || linked_post(&r.filter).is_some() => {
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
        Kind::Text(Hint::Pool) => push_pool(&mut cands, &|_| true),
        // "the rest": what the nearest choice left, of the pool it chose
        // from.
        Kind::Text(Hint::Rest) => {
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
            let copies = k == Kind::Text(Hint::OriginalAndCopies);
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
    if cands.is_empty() && !matches!(k, Kind::Itself | Kind::Text(Hint::Rest)) {
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
    // What no rule above found, read again more widely. Each is tried only
    // where nothing fit, so no reference that had a referent moves.
    if cands.is_empty() {
        match k {
            Kind::Text(Hint::Antecedent | Hint::ClauseObject | Hint::Linked)
                if marked_how(&r.filter).is_some() =>
            {
                for &(l, i) in intros.iter().rev() {
                    if named_so((l, i)) {
                        cands.push(Target::Intro(l, i));
                    }
                }
                if !cands.is_empty() {
                    note = "a reference before it did what its words say".into();
                }
            }
            // "When you next cast a spell with {X} …, gain half X life": the
            // object of a delayed trigger's event.
            Kind::Text(Hint::Triggering) => {
                for &(l, i) in intros.iter().rev() {
                    let x = get((l, i));
                    let delayed = x
                        .effect_frame
                        .is_some_and(|f| lds[l].c.frames[f].verb.starts_with("Delayed"));
                    if delayed && compat(r, x) {
                        cands.push(Target::Intro(l, i));
                    }
                }
            }
            _ => {}
        }
    }
    // "permanents that had a counter put on them this way", "target spell
    // with mana value … the number of cards in its controller's
    // graveyard": a reference inside the filter of the set it describes,
    // with nothing before it, is each one of that set in turn.
    if cands.is_empty() && matches!(k, Kind::Text(Hint::Antecedent | Hint::ClauseObject)) {
        if let Some(&anc) = e
            .anc
            .iter()
            .rev()
            .find(|&&a| matches!(ld.c.evs[a].role, Role::Intro | Role::Skip))
        {
            cands.push(Target::Enclosing(anc));
            note = "in its set's own filter".into();
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
            let s = match (&e.obj, e.role == Role::Made) {
                (_, true) => match e.verb.as_str() {
                    TOKEN => "the token(s) created".to_string(),
                    "Exile" => "the card hideaway exiled".to_string(),
                    "RevealTop" => "the cards revealed".to_string(),
                    "LookAtTop" => "the cards looked at".to_string(),
                    "Mill" => "the cards milled".to_string(),
                    "Draw" => "the cards drawn".to_string(),
                    "Copy" => "the copy".to_string(),
                    "ExileZone" => "the cards exiled".to_string(),
                    "DiscardHand" => "the cards discarded".to_string(),
                    "RevealHand" => "the cards revealed".to_string(),
                    "Choose" => "the ones chosen".to_string(),
                    v if v.starts_with("UntilFound") => "the cards revealed".to_string(),
                    v => format!("the card of {}", v.trim_start_matches("Action>")),
                },
                (Some(o), _) => crate::unparse::objects_text(o),
                _ => "?".into(),
            };
            if e.in_trigger {
                format!("{s} [trigger]")
            } else {
                s
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

    /// The objects (by walk order) of line `line` that point at what its
    /// trigger's event names: "it" in "Whenever you cast a spell, if it's
    /// tapped".
    pub fn pointing_at_trigger(&self, line: usize) -> BTreeSet<usize> {
        self.resolved
            .iter()
            .filter(|r| r.line == line && r.kind == Kind::BackRef)
            .filter_map(|r| match r.pick {
                Some(Target::Intro(l, i)) if self.lines[l].c.evs[i].in_trigger => {
                    self.lines[r.line].c.evs[r.ev].decl
                }
                _ => None,
            })
            .collect()
    }

    /// The reference's filter kind.
    pub fn kind(&self, r: &Resolved) -> Option<Noun> {
        self.lines[r.line].c.evs[r.ev]
            .obj
            .as_ref()
            .and_then(|o| o.filter.kind.clone())
    }
}

// ---------------------------------------------------------------- rewriting

/// What one line's tree becomes once its references are pointers.
#[derive(Default, Debug)]
pub struct LinePlan {
    /// Object (in walk order) to its new quant.
    quants: std::collections::BTreeMap<usize, Quant>,
    /// Each `FromAmongThem` in walk order, the pool it points at.
    pools: Vec<Quant>,
}

/// A reference's new quant, from what it resolved to.
fn new_quant(r: &Resolved, o: &Objects) -> Option<Quant> {
    let Kind::Text(h) = r.kind else {
        return None;
    };
    // "the exiled cards used to craft it" with nothing before it: every
    // card CR 702.167c says that of, which the face that crafted it (the
    // other one) exiled. A set by what was done to it, not a pointer.
    if r.pick.is_none()
        && o.filter
            .posts
            .iter()
            .any(|p| matches!(p, Post::UsedToCraft(x) if x.quant == Quant::SelfRef))
    {
        return Some(Quant::All);
    }
    if matches!(r.pick, Some(Target::Enclosing(_))) {
        return Some(Quant::Itself);
    }
    Some(match (h, r.pointer.as_deref()) {
        (Hint::Rest, Some([Pointer::Back(b)])) => Quant::Rest { of: *b },
        (Hint::Rest, _) | (_, None | Some([])) => Quant::Unresolved(h),
        (_, Some([Pointer::Back(b)])) => Quant::BackRef { back: *b },
        (_, Some([Pointer::ThisCard])) => Quant::SelfRef,
        (_, Some([Pointer::Linked { line, nth }])) => Quant::LinkedBackRef {
            line: *line,
            nth: *nth,
        },
        (_, Some(v)) => Quant::BackRefs(v.to_vec()),
    })
}

impl Analysis {
    /// Each line's rewrite, in the order of the lines given.
    pub fn plan(&self) -> Vec<LinePlan> {
        let mut plans: Vec<LinePlan> = (0..self.lines.len()).map(|_| LinePlan::default()).collect();
        let mut pools: Vec<Vec<(usize, Quant)>> = vec![Vec::new(); self.lines.len()];
        for r in &self.resolved {
            let e = &self.lines[r.line].c.evs[r.ev];
            let Some(o) = &e.obj else {
                continue;
            };
            let (Some(decl), Some(q)) = (e.decl, new_quant(r, o)) else {
                continue;
            };
            match e.slot {
                Slot::Quant => {
                    plans[r.line].quants.insert(decl, q);
                }
                Slot::Pool => pools[r.line].push((decl, q)),
            }
        }
        for (plan, mut ps) in plans.iter_mut().zip(pools) {
            ps.sort_by_key(|p| p.0);
            plan.pools = ps.into_iter().map(|p| p.1).collect();
        }
        plans
    }

    /// Every pointer that does not land where it says, and every
    /// reference with nothing to point at, as text: what `tests/backref.rs`
    /// checks.
    pub fn problems(&self) -> Vec<(Kind, String)> {
        self.resolved
            .iter()
            .filter(|r| {
                r.kind != Kind::Itself
                    && (r.class == Class::None
                        || matches!(
                            r.kind,
                            Kind::BackRef | Kind::Rest | Kind::LinkedBackRef | Kind::BackRefs
                        ) && !r.note.is_empty())
            })
            .map(|r| {
                (
                    r.kind,
                    format!("line {}: {} ({})", r.line, self.reference_text(r), r.note),
                )
            })
            .collect()
    }
}

impl LinePlan {
    pub fn apply(&self, a: &mut Ability) {
        struct Quants<'a>(usize, &'a LinePlan);
        impl Visitor for Quants<'_> {
            fn enter(&mut self, o: &mut Objects) -> bool {
                if let Some(q) = self.1.quants.get(&self.0) {
                    let pointer = !matches!(q, Quant::Unresolved(_));
                    o.quant = q.clone();
                    // A link post said which ability to look in; the
                    // pointer says that now, and the word is "exiled".
                    if pointer {
                        // Each link post held one object, `SelfRef`, which
                        // the walk no longer meets: count it as met.
                        let before = o.filter.posts.len();
                        o.filter.posts.retain(
                            |p| !matches!(p, Post::CreatedWith(x) if x.quant == Quant::SelfRef),
                        );
                        self.0 += before - o.filter.posts.len();
                        for p in &mut o.filter.posts {
                            if matches!(p, Post::ExiledWith(x) if x.quant == Quant::SelfRef) {
                                *p = Post::Marked(Marked::Exiled);
                                self.0 += 1;
                            }
                        }
                    }
                }
                self.0 += 1;
                true
            }
            fn ability(&mut self, _: &mut Ability) -> bool {
                true
            }
        }
        struct Pools<'a>(std::slice::Iter<'a, Quant>);
        impl Visitor for Pools<'_> {
            fn enter(&mut self, o: &mut Objects) -> bool {
                for p in &mut o.filter.posts {
                    if *p == Post::FromAmongThem {
                        if let Some(q) = self.0.next() {
                            *p = Post::FromAmong(Box::new(Objects {
                                quant: q.clone(),
                                filter: Filter::default(),
                            }));
                        }
                    }
                }
                true
            }
            fn ability(&mut self, _: &mut Ability) -> bool {
                true
            }
        }
        a.walk(&mut Quants(0, self));
        if !self.pools.is_empty() {
            a.walk(&mut Pools(self.pools.iter()));
        }
    }
}

/// Makes every reference of a face's lines a pointer where an introducer
/// fits ([backref]): the parser's last step, in `read_face`. A second round
/// reads the pointers the first wrote, so "its owner" after "the exiled
/// card" of a linked ability points where that card does.
pub fn resolve(lines: &mut [crate::Line], type_line: &str) {
    rounds(lines, type_line, false);
    if lines.iter().any(|l| l.said.is_some()) {
        rounds(lines, type_line, true);
    }
}

/// Resolves the parsed trees, or (`said`) the trees as the lines' own
/// words say them, beside the others' parsed trees.
fn rounds(lines: &mut [crate::Line], type_line: &str, said: bool) {
    for _ in 0..2 {
        let plans = {
            let abs: Vec<(Option<&Ability>, bool)> = lines
                .iter()
                .map(|l| {
                    let parsed = l.parsed.as_ref().ok();
                    let a = if said {
                        l.said.as_ref().or(parsed)
                    } else {
                        parsed
                    };
                    (a, l.continues)
                })
                .collect();
            analyse(&abs, type_line).plan()
        };
        let mut left = false;
        for (l, p) in lines.iter_mut().zip(&plans) {
            let a = if said {
                l.said.as_mut()
            } else {
                l.parsed.as_mut().ok()
            };
            if let Some(a) = a {
                p.apply(a);
                left |= unresolved(a);
            }
        }
        if !left {
            return;
        }
    }
}

fn unresolved(a: &Ability) -> bool {
    struct Any(bool);
    impl Visitor for Any {
        fn enter(&mut self, o: &mut Objects) -> bool {
            self.0 |= matches!(o.quant, Quant::Unresolved(_));
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut any = Any(false);
    a.clone().walk(&mut any);
    any.0
}

/// Each line's pointers in walk order (which is `Debug` order), with what
/// each points at in words: what `golden` writes beside them.
pub fn notes(lines: &[crate::Line], type_line: &str) -> Vec<Vec<String>> {
    let abs: Vec<(Option<&Ability>, bool)> = lines
        .iter()
        .map(|l| (l.parsed.as_ref().ok(), l.continues))
        .collect();
    let an = analyse(&abs, type_line);
    let mut by: Vec<Vec<(usize, String)>> = vec![Vec::new(); lines.len()];
    for r in &an.resolved {
        if !matches!(
            r.kind,
            Kind::BackRef | Kind::LinkedBackRef | Kind::BackRefs | Kind::Rest
        ) {
            continue;
        }
        let (Some(decl), Some(t)) = (an.lines[r.line].c.evs[r.ev].decl, &r.pick) else {
            continue;
        };
        by[r.line].push((decl, an.describe(t)));
    }
    by.into_iter()
        .map(|mut v| {
            v.sort_by_key(|x| x.0);
            v.into_iter().map(|x| x.1).collect()
        })
        .collect()
}

/// `dbg`, a tree's `Debug`, with `/* … */` after each pointer saying what
/// it points at. Unchanged where the pointers and notes do not pair up.
pub fn annotate(dbg: &str, notes: &[String]) -> String {
    const PATS: [&str; 4] = [
        "BackRefs([",
        "LinkedBackRef { ",
        "BackRef { back: ",
        "Rest { back: ",
    ];
    let mut out = String::new();
    let mut rest = dbg;
    let mut k = 0;
    while let Some((i, p)) = PATS
        .iter()
        .filter_map(|p| rest.find(p).map(|i| (i, *p)))
        .min_by_key(|x| x.0)
    {
        let close = if p == "BackRefs([" { "])" } else { "}" };
        let Some(j) = rest[i..].find(close).map(|j| i + j + close.len()) else {
            break;
        };
        out.push_str(&rest[..j]);
        let Some(n) = notes.get(k) else {
            return dbg.to_string();
        };
        out.push_str(" /* ");
        out.push_str(&n.replace("*/", "* /"));
        out.push_str(" */");
        k += 1;
        rest = &rest[j..];
    }
    if k != notes.len() {
        return dbg.to_string();
    }
    out.push_str(rest);
    out
}

/// What an introducer is, for `binders`.
#[derive(Clone, Copy, Debug)]
pub struct IntroInfo {
    /// The object in walk order; `None` for objects an effect made.
    pub decl: Option<usize>,
    /// The ability (no wrapper) it is said in, by walk order.
    pub scope_root: usize,
    /// The effect of that ability's own list it is said in.
    pub top_effect: Option<usize>,
    /// Fixed as the ability is put on the stack: a target (CR 601.2c).
    pub cast_time: bool,
    /// A `ForEach`'s loop object.
    pub each_of_loop: bool,
}

impl Analysis {
    pub fn intro_info(&self, line: usize, ev: usize) -> IntroInfo {
        let e = &self.lines[line].c.evs[ev];
        IntroInfo {
            decl: e.decl,
            scope_root: e.scope_root,
            top_effect: e.top_effect,
            cast_time: e.obj.as_ref().is_some_and(targeted),
            each_of_loop: e.each_of_loop,
        }
    }

    /// Whether the event `ev` of `line` is said inside the loop whose
    /// object is `each` (an event of the same line).
    pub fn inside_loop(&self, line: usize, ev: usize, each: usize) -> bool {
        let evs = &self.lines[line].c.evs;
        evs[each]
            .frames
            .last()
            .is_some_and(|f| evs[ev].frames.contains(f))
    }

    /// Whether the event `ev` of `line` is said inside the object `obj`.
    pub fn within(&self, line: usize, ev: usize, obj: usize) -> bool {
        self.lines[line].c.evs[ev].anc.contains(&obj)
    }

    /// An event's object in walk order.
    pub fn decl_of(&self, line: usize, ev: usize) -> Option<usize> {
        self.lines[line].c.evs[ev].decl
    }

    /// A reference's object in walk order.
    pub fn decl(&self, r: &Resolved) -> Option<usize> {
        self.lines[r.line].c.evs[r.ev].decl
    }

    /// The verb each object of `line` is said with, by walk order.
    pub fn verbs(&self, line: usize) -> BTreeMap<usize, String> {
        self.lines[line]
            .c
            .evs
            .iter()
            .filter_map(|e| e.decl.map(|d| (d, e.verb.clone())))
            .collect()
    }

    /// The references of `line` in reading order, by walk order.
    pub fn reading_order(&self, line: usize) -> Vec<usize> {
        self.lines[line]
            .scopes
            .iter()
            .flatten()
            .filter_map(|&e| {
                let ev = &self.lines[line].c.evs[e];
                matches!(ev.role, Role::Ref(_)).then_some(ev.decl).flatten()
            })
            .collect()
    }
}

/// Whether a reference said with `r`'s words can mean the object `b`
/// describes, by kind and type alone.
pub fn fits(r: &Objects, b: &Objects) -> bool {
    let mut r = r.clone();
    r.filter.posts.retain(|p| !matches!(p, Post::Marked(_)));
    let e = Ev {
        decl: None,
        slot: Slot::Quant,
        role: Role::Intro,
        obj: Some(b.clone()),
        token: None,
        verb: String::new(),
        in_trigger: false,
        frames: Vec::new(),
        anc: Vec::new(),
        effect_frame: None,
        scope_root: 0,
        top_effect: None,
        each_of_loop: false,
    };
    // "cast target nonland permanent card …, and mana … to cast that
    // spell": a card cast is the spell.
    let card_cast = r.filter.kind == Some(Noun::Spell)
        && matches!(b.filter.kind, Some(Noun::Card | Noun::PermanentCard) | None);
    b.quant == Quant::Made || card_cast || compat(&r, &e)
}

/// One ability's events in reading order, with no resolving: what
/// `binders` reads the order and verbs of mentions from.
fn read_alone(a: &Ability) -> (Collect, Vec<usize>) {
    let mut c = Collect::new();
    a.clone().walk(&mut c);
    let mut top = Vec::new();
    let mut nested = Vec::new();
    flatten(&c, 0, &mut top, &mut nested);
    top.extend(nested.into_iter().flatten());
    (c, top)
}

/// The references of one ability in reading order, by walk order.
pub fn reading_order_of(a: &Ability) -> Vec<usize> {
    let (c, order) = read_alone(a);
    order
        .into_iter()
        .filter_map(|e| {
            let ev = &c.evs[e];
            matches!(ev.role, Role::Ref(_)).then_some(ev.decl).flatten()
        })
        .collect()
}

/// The verb each object of one ability is said with, by walk order.
pub fn verbs_of(a: &Ability) -> BTreeMap<usize, String> {
    let (c, _) = read_alone(a);
    c.evs
        .iter()
        .filter_map(|e| e.decl.map(|d| (d, e.verb.clone())))
        .collect()
}
