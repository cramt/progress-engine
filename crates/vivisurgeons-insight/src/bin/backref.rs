//! Prototype (branch insight-backref): can every reference back in the
//! corpus be a pointer counted backward, `Quant::BackRef { back }`?
//!
//! Reads every face in `snapshot/`, parses it again, walks each tree in
//! reading order, and for every reference quant lists the introducers it
//! could mean under today's semantics (the quant's doc comment plus filter
//! compatibility). One candidate is DETERMINED, several AMBIGUOUS, none
//! NONE. Then it writes the pointers into a copy of each tree and checks
//! them again from the converted tree alone (the validator).
//!
//! `cargo run --release -p vivisurgeons-insight --bin backref -- [--cards]`
//!
//! Reading order. The generated walk visits fields in declaration order,
//! which is reading order for almost every node (`Where { then, x }`,
//! `Replacement { event, instead }`, `DelayedWhenever { event, then }`,
//! `Triggered { trigger, effect }`, `Activated { cost, effect }` all are).
//! On top of it:
//! - an object that introduces something is introduced after its own
//!   parts (post-order), so "enchantments with the same name as that
//!   enchantment" cannot point at the set it sits in; a reference is read
//!   before its parts;
//! - an effect that makes objects no `Objects` names (Create, RevealTop,
//!   LookAtTop, Mill, Draw, Copy, UntilFound's revealed cards, the
//!   manifest-like keyword actions) introduces them after its parts;
//! - `If { cond, then }` whose cond holds `ClauseObject` is a trailing
//!   "then if cond", and `ForEach { each, then }` whose then holds
//!   `BeforeEach` a trailing "then for each": both are read then-first.
//! The tree does not record whether any other one-clause `If`/`ForEach`,
//! or a `Damage` whose amount names objects ("deals damage to X equal to
//! …"), leads or trails (the round trip accepts both), so `--flip` reads
//! every such node the other way and counts the pointers that move.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use vivisurgeons_insight::ast::*;
use vivisurgeons_insight::golden::{self, Entry};
use vivisurgeons_insight::unparse::objects_text;
use vivisurgeons_insight::walk::{Visitor, Walk};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum RefKind {
    Antecedent,
    Created,
    Listed,
    Itself,
    BeforeEach,
    ClauseObject,
    FromAmongThem,
    Triggering,
    Linked,
    /// `ExiledWith(SelfRef)`/`CreatedWith(SelfRef)` on a quant other than
    /// `Linked` ("all cards exiled with ~").
    LinkedPost,
    /// `Marked(…)`/`MarkedBy`/`MarkedFrom` on a quant that is no reference
    /// ("the exiled card", "each permanent chosen this way").
    Marked,
    OriginalAndCopies,
    Both,
    Rest,
    // What the validator reads.
    BackRef,
    LinkedBackRef,
    BackRefs,
}

#[derive(Clone, Debug, PartialEq)]
enum Role {
    Intro,
    /// An effect's objects no `Objects` names, with the verb.
    Pseudo,
    SelfMention,
    Ref(RefKind),
    /// A reference to a player or a counter: outside this prototype.
    OutOfScope,
    /// Not an introducer under the config (inside an amount or condition),
    /// or a kind that is no object (player, counter).
    Skip,
}

#[derive(Clone, Debug)]
struct Ev {
    decl: Option<usize>,
    role: Role,
    obj: Option<Objects>,
    /// Token types for a Create pseudo.
    token: Option<Token>,
    verb: String,
    in_amount: bool,
    in_cond: bool,
    in_trigger: bool,
    /// Ancestor frames, outermost first.
    frames: Vec<usize>,
    /// Ancestor objects (event ids), outermost first.
    anc: Vec<usize>,
    /// The innermost effect frame.
    effect_frame: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fk {
    Root,
    Effect,
    Cond,
    Amount,
    Ability,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Trail {
    No,
    /// If whose cond holds ClauseObject: read then, cond.
    CondLast,
    /// ForEach whose then holds BeforeEach: read then, each.
    EachLast,
    /// One-clause If/ForEach the tree does not order.
    AmbIf,
    AmbEach,
    /// Damage whose amount names objects.
    AmbDamage,
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
    is_if: bool,
    verb: String,
    effect_index: usize,
}

#[derive(Clone, Copy)]
struct Config {
    intro_in_amount: bool,
    intro_in_cond: bool,
    flip: bool,
}

/// Phase A: the events of one line, in a frame tree.
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
    /// Cost heads, one per object the costs hold, in walk order.
    cost_heads: Vec<String>,
    cost_seen: usize,
    players_that: usize,
    /// Each effect's trail, in walk order: recorded, or forced from a
    /// recording (the validator's "the tree records the order" mode).
    trails: Vec<Trail>,
    force: Option<Vec<Trail>>,
}

fn head(s: &str) -> String {
    s.split(|c: char| c == ' ' || c == '(' || c == '{')
        .next()
        .unwrap_or("")
        .to_string()
}

fn verb_of(e: &Effect) -> String {
    match e {
        Effect::Move { to, .. } => format!("Move>{}", head(&format!("{to:?}"))),
        Effect::UntilFound { verb, .. } => format!("UntilFound>{verb:?}"),
        Effect::Action { name, .. } => format!("Action>{name}"),
        _ => head(&format!("{e:?}")),
    }
}

fn is_ref_quant(q: &Quant) -> Option<RefKind> {
    Some(match q {
        Quant::Antecedent => RefKind::Antecedent,
        Quant::Created => RefKind::Created,
        Quant::Listed => RefKind::Listed,
        Quant::Itself => RefKind::Itself,
        Quant::BeforeEach => RefKind::BeforeEach,
        Quant::ClauseObject => RefKind::ClauseObject,
        Quant::Triggering => RefKind::Triggering,
        Quant::Linked => RefKind::Linked,
        Quant::OriginalAndCopies => RefKind::OriginalAndCopies,
        Quant::Both => RefKind::Both,
        Quant::Rest => RefKind::Rest,
        Quant::BackRef { .. } => RefKind::BackRef,
        Quant::LinkedBackRef { .. } => RefKind::LinkedBackRef,
        Quant::BackRefs(_) => RefKind::BackRefs,
        _ => return None,
    })
}

fn marked_how(f: &Filter) -> Option<Marked> {
    f.posts.iter().find_map(|p| match p {
        Post::Marked(h) | Post::MarkedBy { how: h, .. } | Post::MarkedFrom { how: h, .. } => {
            Some(*h)
        }
        _ => None,
    })
}

fn linked_post(f: &Filter) -> Option<&'static str> {
    f.posts.iter().find_map(|p| match p {
        Post::ExiledWith(o) if o.quant == Quant::SelfRef => Some("Exile"),
        Post::CreatedWith(o) if o.quant == Quant::SelfRef => Some("@token"),
        Post::PutOntoBattlefieldWith(o) if o.quant == Quant::SelfRef => Some("Move>Battlefield"),
        _ => None,
    })
}

fn not_object(f: &Filter) -> bool {
    matches!(
        f.kind,
        Some(Noun::Player | Noun::Opponent | Noun::Counter(_))
    )
}

/// The verb an effect's made objects carry, if it makes any no `Objects`
/// names.
fn pseudo_verb(e: &Effect) -> Option<String> {
    Some(match e {
        Effect::Create { .. }
        | Effect::CreateNamed { .. }
        | Effect::CreateAttached { .. }
        | Effect::CreateThose { .. }
        | Effect::CreateThosePlus { .. }
        | Effect::CreateCopy { .. }
        | Effect::Amass { .. } => "@token".into(),
        Effect::RevealTop { .. } => "RevealTop".into(),
        Effect::LookAtTop { .. } => "LookAtTop".into(),
        Effect::Mill { .. } => "Mill".into(),
        Effect::Draw { .. } => "Draw".into(),
        Effect::Copy { .. } => "Copy".into(),
        Effect::UntilFound { verb, .. } | Effect::DigUntilTotal { verb, .. } => {
            format!("UntilFound>{verb:?}+Reveal")
        }
        Effect::Action { name, .. } => {
            let n = name.as_str();
            if [
                "manifest",
                "manifest dread",
                "cloak",
                "investigate",
                "incubate",
                "populate",
                "amass",
                "venture into the dungeon",
            ]
            .contains(&n)
            {
                if ["investigate", "incubate", "populate", "amass"].contains(&n) {
                    "@token".into()
                } else {
                    format!("Action>{n}")
                }
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn token_of(e: &Effect) -> Option<Token> {
    match e {
        Effect::Create { token, .. }
        | Effect::CreateNamed { token, .. }
        | Effect::CreateAttached { token, .. }
        | Effect::CreateThosePlus { token, .. } => Some(token.clone()),
        _ => None,
    }
}

impl Collect {
    fn new() -> Self {
        Collect {
            evs: Vec::new(),
            frames: vec![Frame {
                kind: Fk::Root,
                trail: Trail::No,
                items: Vec::new(),
                is_if: false,
                verb: String::new(),
                effect_index: 0,
            }],
            stack: vec![0],
            obj_stack: Vec::new(),
            decl: 0,
            ab_stack: Vec::new(),
            top: true,
            effects_open: 0,
            top_kind: "",
            cost_heads: Vec::new(),
            cost_seen: 0,
            players_that: 0,
            trails: Vec::new(),
            force: None,
        }
    }

    fn push_frame(&mut self, kind: Fk, verb: String, is_if: bool) {
        let id = self.frames.len();
        self.frames.push(Frame {
            kind,
            trail: Trail::No,
            items: Vec::new(),
            is_if,
            verb,
            effect_index: 0,
        });
        self.stack.push(id);
    }

    fn pop_frame(&mut self) -> usize {
        let id = self.stack.pop().unwrap();
        let parent = *self.stack.last().unwrap();
        self.frames[parent].items.push(Item::Fr(id));
        id
    }

    fn here(&self) -> usize {
        *self.stack.last().unwrap()
    }

    fn in_kind(&self, k: Fk) -> bool {
        self.stack.iter().any(|&f| self.frames[f].kind == k)
    }

    fn cur_verb(&self) -> String {
        for &f in self.stack.iter().rev() {
            match self.frames[f].kind {
                Fk::Effect => return self.frames[f].verb.clone(),
                Fk::Ability => break,
                _ => {}
            }
        }
        if self.in_kind(Fk::Ability) {
            return "@granted".into();
        }
        match self.top_kind {
            "Triggered" if self.effects_open == 0 => "@trigger".into(),
            "Activated" if self.effects_open == 0 => {
                let h = self
                    .cost_heads
                    .get(self.cost_seen)
                    .cloned()
                    .unwrap_or_default();
                format!("@cost:{h}")
            }
            k => format!("@{k}"),
        }
    }

    fn effect_frame(&self) -> Option<usize> {
        self.stack
            .iter()
            .rev()
            .find(|&&f| self.frames[f].kind == Fk::Effect)
            .copied()
    }

    fn new_ev(&mut self, decl: Option<usize>, role: Role, obj: Option<Objects>) -> usize {
        let verb = self.cur_verb();
        let in_trigger = !self.in_kind(Fk::Ability)
            && self.top_kind == "Triggered"
            && self.effects_open == 0
            && self.effect_frame().is_none();
        let id = self.evs.len();
        self.evs.push(Ev {
            decl,
            role,
            obj,
            token: None,
            verb,
            in_amount: self.in_kind(Fk::Amount),
            in_cond: self.in_kind(Fk::Cond),
            in_trigger,
            frames: self.stack.clone(),
            anc: self.obj_stack.clone(),
            effect_frame: self.effect_frame(),
        });
        id
    }
}

struct CountObjs(usize);
impl Visitor for CountObjs {
    fn enter(&mut self, _: &mut Objects) -> bool {
        self.0 += 1;
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

fn contains_quant<T: Walk + Clone>(x: &T, q: &Quant) -> bool {
    struct Has<'a>(&'a Quant, bool);
    impl Visitor for Has<'_> {
        fn enter(&mut self, o: &mut Objects) -> bool {
            self.1 |= o.quant == *self.0;
            true
        }
    }
    let mut h = Has(q, false);
    x.clone().walk(&mut h);
    h.1
}

fn has_objects<T: Walk + Clone>(x: &T) -> bool {
    let mut c = CountObjs(0);
    x.clone().walk(&mut c);
    c.0 > 0
}

impl Visitor for Collect {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let decl = self.decl;
        self.decl += 1;
        let top_level_cost = self.top_kind == "Activated"
            && self.effects_open == 0
            && !self.in_kind(Fk::Ability)
            && self.effect_frame().is_none();
        let role = if let Some(k) = is_ref_quant(&o.quant) {
            if not_object(&o.filter) {
                Role::OutOfScope
            } else {
                Role::Ref(k)
            }
        } else if o.quant == Quant::SelfRef {
            Role::SelfMention
        } else if marked_how(&o.filter).is_some() && !not_object(&o.filter) {
            Role::Ref(RefKind::Marked)
        } else if linked_post(&o.filter).is_some() {
            Role::Ref(RefKind::LinkedPost)
        } else if not_object(&o.filter) {
            Role::Skip
        } else {
            Role::Intro
        };
        let id = self.new_ev(Some(decl), role.clone(), Some(o.clone()));
        if top_level_cost {
            // Every object a cost holds took one head, in walk order.
            self.cost_seen += 1;
        }
        // "from among them": a reference to a pool, read before the new
        // choice the same object introduces.
        let from_among = o.filter.posts.contains(&Post::FromAmongThem);
        let mut pre = Vec::new();
        if from_among {
            let r = self.new_ev(None, Role::Ref(RefKind::FromAmongThem), Some(o.clone()));
            pre.push(Item::Ev(r));
        }
        let parent = self.here();
        self.frames[parent].items.extend(pre);
        if matches!(role, Role::Ref(_) | Role::SelfMention | Role::OutOfScope) {
            self.frames[parent].items.push(Item::Ev(id));
        }
        self.push_frame(Fk::Body, String::new(), false);
        self.obj_stack.push(id);
        true
    }

    fn leave(&mut self, _o: &mut Objects) {
        let id = self.obj_stack.pop().unwrap();
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
                if let Ability::Activated { cost, .. } = a {
                    for c in cost.iter() {
                        let mut n = CountObjs(0);
                        c.clone().walk(&mut n);
                        let h = head(&format!("{c:?}"));
                        for _ in 0..n.0 {
                            self.cost_heads.push(h.clone());
                        }
                    }
                }
            }
            self.ab_stack.push(false);
            return true;
        }
        self.push_frame(Fk::Ability, String::new(), false);
        self.ab_stack.push(true);
        true
    }

    fn ability_done(&mut self, _a: &mut Ability) {
        if self.ab_stack.pop() == Some(true) {
            self.pop_frame();
        }
    }

    fn effect(&mut self, e: &mut Effect) {
        if !self.in_kind(Fk::Ability) && self.effect_frame().is_none() {
            self.effects_open += 1;
        }
        let is_if = matches!(e, Effect::If { .. });
        self.push_frame(Fk::Effect, verb_of(e), is_if);
        // The effect's index in walk order, kept in its frame's verb slot
        // would be confusing: keep a parallel list instead.
        self.trails.push(Trail::No);
        let id = self.here();
        self.frames[id].effect_index = self.trails.len() - 1;
    }

    fn effect_done(&mut self, e: &mut Effect) {
        let id = self.pop_frame();
        self.frames[id].trail = match e {
            Effect::If { cond, then } if contains_quant(cond, &Quant::ClauseObject) => {
                let _ = then;
                Trail::CondLast
            }
            Effect::If { then, .. } if then.len() == 1 => Trail::AmbIf,
            Effect::ForEach { then, .. }
                if then.iter().any(|t| contains_quant(t, &Quant::BeforeEach)) =>
            {
                Trail::EachLast
            }
            Effect::ForEach { then, .. } if then.len() == 1 => Trail::AmbEach,
            Effect::Damage { n, to, .. } if has_objects(n) && !to.is_empty() => Trail::AmbDamage,
            _ => Trail::No,
        };
        let k = self.frames[id].effect_index;
        if let Some(f) = &self.force {
            if let Some(t) = f.get(k) {
                self.frames[id].trail = *t;
            }
        }
        self.trails[k] = self.frames[id].trail;
        if let Some(v) = pseudo_verb(e) {
            // The made objects: introduced after the effect's own parts.
            let ev = self.new_ev(None, Role::Pseudo, None);
            self.evs[ev].verb = v;
            self.evs[ev].token = token_of(e);
            // new_ev saw the stack without this effect's frame; keep it so.
            let parent = self.here();
            self.frames[parent].items.push(Item::Ev(ev));
        }
    }

    fn condition(&mut self, _c: &mut Condition) {
        self.push_frame(Fk::Cond, String::new(), false);
    }
    fn condition_done(&mut self, _c: &mut Condition) {
        self.pop_frame();
    }
    fn amount(&mut self, _a: &mut Amount) {
        self.push_frame(Fk::Amount, String::new(), false);
    }
    fn amount_done(&mut self, _a: &mut Amount) {
        self.pop_frame();
    }
    fn player(&mut self, p: &mut Player) {
        if *p == Player::That {
            self.players_that += 1;
        }
    }
}

/// One scope's events in reading order, and the scopes nested in it.
fn flatten(c: &Collect, f: usize, cfg: Config, out: &mut Vec<usize>, nested: &mut Vec<Vec<usize>>) {
    let fr = &c.frames[f];
    let mut items: Vec<Item> = fr.items.clone();
    let is_effect_frame = |it: &Item| matches!(it, Item::Fr(x) if c.frames[*x].kind == Fk::Effect);
    let is_cond = |it: &Item| matches!(it, Item::Fr(x) if c.frames[*x].kind == Fk::Cond);
    let is_amount = |it: &Item| matches!(it, Item::Fr(x) if c.frames[*x].kind == Fk::Amount);
    let move_last = |items: &mut Vec<Item>, pred: &dyn Fn(&Item) -> bool| {
        let (a, b): (Vec<Item>, Vec<Item>) = items.iter().partition(|i| !pred(i));
        *items = a.into_iter().chain(b).collect();
    };
    match fr.trail {
        Trail::CondLast => move_last(&mut items, &is_cond),
        Trail::AmbIf if cfg.flip => move_last(&mut items, &is_cond),
        Trail::EachLast => move_last(&mut items, &|i| !is_effect_frame(i)),
        Trail::AmbEach if cfg.flip => move_last(&mut items, &|i| !is_effect_frame(i)),
        Trail::AmbDamage if cfg.flip => move_last(&mut items, &is_amount),
        _ => {}
    }
    for it in items {
        match it {
            Item::Ev(e) => out.push(e),
            Item::Fr(x) if c.frames[x].kind == Fk::Ability => {
                let mut inner = Vec::new();
                flatten(c, x, cfg, &mut inner, nested);
                nested.push(inner);
            }
            Item::Fr(x) => flatten(c, x, cfg, out, nested),
        }
    }
}

fn collect(a: &Ability, force: Option<Vec<Trail>>) -> Collect {
    let mut c = Collect::new();
    c.force = force;
    a.clone().walk(&mut c);
    c
}

// ------------------------------------------------------------ compatibility

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
    if e.role == Role::Pseudo {
        return e.verb == "@token";
    }
    e.obj.as_ref().is_some_and(|o| {
        matches!(o.filter.kind, Some(Noun::Token | Noun::Copy))
            || o.filter
                .alts
                .iter()
                .any(|a| a.contains(&Adj::State(State::Token)))
    })
}

/// Whether the reference `r` (its filter) can mean the introducer `e`.
fn compat(r: &Objects, e: &Ev) -> bool {
    let rf = &r.filter;
    if let Some(how) = marked_how(rf) {
        if !verb_matches(how, &e.verb) {
            return false;
        }
    }
    // The pile UntilFound revealed is said "the revealed cards", "the
    // rest", never a bare "it" or "that card" (which is the card found).
    if e.role == Role::Pseudo
        && e.verb.starts_with("UntilFound")
        && marked_how(rf) != Some(Marked::Revealed)
    {
        return false;
    }
    let ek = e.obj.as_ref().and_then(|o| o.filter.kind.clone());
    match &rf.kind {
        Some(Noun::Token) if !is_tokenish(e) => return false,
        Some(Noun::Copy)
            if !(e.role == Role::Pseudo && (e.verb == "Copy" || e.verb == "@token")
                || matches!(ek, Some(Noun::Copy))) =>
        {
            return false
        }
        Some(Noun::Spell) => {
            let ok = e.role == Role::Pseudo && e.verb == "Copy"
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

fn self_compat(r: &Objects, type_line: &str) -> bool {
    let tl = type_line.to_lowercase();
    let words: BTreeSet<&str> = tl
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .collect();
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
    let (rt, rs) = type_words(rf);
    let types_ok = rt.is_empty() || rt.iter().any(|t| words.contains(t.as_str()));
    let subs_ok = rs.is_empty() || rs.iter().any(|t| words.contains(t.as_str()));
    types_ok && subs_ok
}

// ------------------------------------------------------------ resolution

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Target {
    /// (line, event id) of an introducer.
    Intro(usize, usize),
    SelfCard,
    /// (line, event id) in another line: a linked ability.
    Linked(usize, usize),
    /// The object whose filter the reference sits in.
    Enclosing(usize),
    Set(Vec<(usize, usize)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Class {
    Determined,
    Ambiguous,
    None,
}

struct Resolved {
    line: usize,
    ev: usize,
    kind: RefKind,
    class: Class,
    cands: Vec<Target>,
    pick: Option<Target>,
    /// For a BackRef pick: how many introducers were read before the ref.
    pos: usize,
    note: String,
    cross_line: bool,
}

struct LineData {
    c: Collect,
    /// Top scope then nested scopes, each in reading order.
    scopes: Vec<Vec<usize>>,
    continues: bool,
    die_row: bool,
}

fn counts_as_intro(e: &Ev, cfg: Config) -> bool {
    match e.role {
        Role::Intro => (cfg.intro_in_amount || !e.in_amount) && (cfg.intro_in_cond || !e.in_cond),
        Role::Pseudo => true,
        _ => false,
    }
}

fn line_data(
    lines: &[(Ability, bool)],
    cfg: Config,
    force: Option<&[Vec<Trail>]>,
) -> Vec<LineData> {
    lines
        .iter()
        .enumerate()
        .map(|(li, (a, continues))| {
            let c = collect(a, force.map(|f| f[li].clone()));
            let mut top = Vec::new();
            let mut nested = Vec::new();
            flatten(&c, 0, cfg, &mut top, &mut nested);
            let mut scopes = vec![top];
            scopes.extend(nested);
            LineData {
                c,
                scopes,
                continues: *continues,
                die_row: matches!(a, Ability::DieRow { .. }),
            }
        })
        .collect()
}

/// The introducers of each line's top scope, in reading order.
fn line_intros(ld: &LineData, cfg: Config) -> Vec<usize> {
    ld.scopes[0]
        .iter()
        .copied()
        .filter(|&e| counts_as_intro(&ld.c.evs[e], cfg))
        .collect()
}

fn resolve_face(lds: &[LineData], type_line: &str, cfg: Config) -> Vec<Resolved> {
    let mut out = Vec::new();
    let intros_per_line: Vec<Vec<usize>> = lds.iter().map(|l| line_intros(l, cfg)).collect();
    // Prior paragraphs of the same spell (CR 608.2c), or a die's rows.
    let mut prior: Vec<(usize, usize)> = Vec::new();
    let mut prior_self = false;
    for (li, ld) in lds.iter().enumerate() {
        if !ld.continues && !ld.die_row {
            prior.clear();
            prior_self = false;
        }
        for (si, scope) in ld.scopes.iter().enumerate() {
            let mut intros: Vec<(usize, usize)> = if si == 0 { prior.clone() } else { Vec::new() };
            let base = intros.len();
            let mut self_at: Option<usize> = if si == 0 && prior_self { Some(0) } else { None };
            let mut earlier: Vec<(Objects, Vec<Target>)> = Vec::new();
            for &ei in scope {
                let e = &ld.c.evs[ei];
                if counts_as_intro(e, cfg) {
                    intros.push((li, ei));
                    continue;
                }
                match &e.role {
                    Role::SelfMention => self_at = Some(intros.len()),
                    Role::Ref(k) => {
                        let r = e.obj.clone().unwrap();
                        let pos = intros.len();
                        let get = |(l, i): (usize, usize)| &lds[l].c.evs[i];
                        let mut cands: Vec<Target> = Vec::new();
                        let mut note = String::new();
                        let push_pool = |cands: &mut Vec<Target>, pred: &dyn Fn(&Ev) -> bool| {
                            for &(l, i) in intros.iter().rev() {
                                if pred(get((l, i))) && compat(&r, get((l, i))) {
                                    cands.push(Target::Intro(l, i));
                                }
                            }
                        };
                        match k {
                            RefKind::Created => push_pool(&mut cands, &|x| is_tokenish(x)),
                            RefKind::Triggering => push_pool(&mut cands, &|x| x.in_trigger),
                            RefKind::ClauseObject => {
                                let iff = e
                                    .frames
                                    .iter()
                                    .rev()
                                    .find(|&&f| ld.c.frames[f].is_if)
                                    .copied();
                                push_pool(&mut cands, &|x| {
                                    iff.is_some_and(|f| x.frames.contains(&f))
                                });
                            }
                            RefKind::Itself => {
                                if let Some(&anc) = e
                                    .anc
                                    .iter()
                                    .rev()
                                    .find(|&&a| matches!(ld.c.evs[a].role, Role::Intro))
                                {
                                    cands.push(Target::Enclosing(anc));
                                    note = "in its set's own filter".into();
                                } else {
                                    let ef = e.effect_frame;
                                    push_pool(&mut cands, &|x| {
                                        ef.is_some() && x.frames.contains(&ef.unwrap())
                                    });
                                }
                            }
                            RefKind::Listed | RefKind::Both => {
                                // The latest effect with two or more objects,
                                // or (Both) the last object of each of the two
                                // effects before.
                                let mut groups: Vec<(Option<usize>, Vec<(usize, usize)>)> =
                                    Vec::new();
                                for &(l, i) in intros.iter() {
                                    let x = get((l, i));
                                    if !compat(&r, x) {
                                        continue;
                                    }
                                    let key = if l == li { x.effect_frame } else { None };
                                    match groups.last_mut() {
                                        Some((g, v)) if *g == key && l == v[0].0 => v.push((l, i)),
                                        _ => groups.push((key, vec![(l, i)])),
                                    }
                                }
                                if *k == RefKind::Listed {
                                    if let Some((_, v)) =
                                        groups.iter().rev().find(|(_, v)| v.len() >= 2)
                                    {
                                        cands.push(Target::Set(v.clone()));
                                    }
                                } else if groups.len() >= 2 {
                                    let n = groups.len();
                                    let set = vec![
                                        *groups[n - 2].1.last().unwrap(),
                                        *groups[n - 1].1.last().unwrap(),
                                    ];
                                    cands.push(Target::Set(set));
                                }
                            }
                            RefKind::Linked | RefKind::LinkedPost => {
                                let verb = linked_post(&r.filter).unwrap_or("?");
                                let vm = |x: &Ev| {
                                    if verb == "@token" {
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
                                        if vm(x) && compat(&r, x) {
                                            cands.push(Target::Linked(lj, i));
                                        }
                                    }
                                }
                                if verb == "?" {
                                    note = format!(
                                        "first post {:?}",
                                        r.filter.posts.first().map(|p| head(&format!("{p:?}")))
                                    );
                                }
                            }
                            RefKind::FromAmongThem => push_pool(&mut cands, &|_| true),
                            // "the rest": what the nearest choice left, of
                            // the pool it chose from (its doc).
                            RefKind::Rest => {
                                let choice = intros.iter().rev().find(|&&(l, i)| {
                                    let x = get((l, i));
                                    x.role == Role::Pseudo && x.verb != "@token"
                                        || x.obj.as_ref().is_some_and(|o| {
                                            o.filter.posts.iter().any(|p| {
                                                matches!(
                                                    p,
                                                    Post::FromAmongThem | Post::FromAmong(_)
                                                )
                                            }) || matches!(
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
                            // Antecedent, BeforeEach, Marked, Rest,
                            // OriginalAndCopies: any earlier object that fits.
                            _ => {
                                let copies = *k == RefKind::OriginalAndCopies;
                                push_pool(&mut cands, &|x| !copies || x.verb.contains("Copy"));
                                if self_at.is_some() && self_compat(&r, type_line) {
                                    cands.push(Target::SelfCard);
                                }
                                // A reference back to a reference: the same
                                // object (a chain), when it fits.
                                for (eo, ts) in earlier.iter() {
                                    let _ = eo;
                                    for t in ts {
                                        let ok = match t {
                                            Target::Linked(l, i) => compat(&r, &lds[*l].c.evs[*i]),
                                            Target::SelfCard => self_compat(&r, type_line),
                                            _ => false,
                                        };
                                        if ok && !cands.contains(t) {
                                            cands.push(t.clone());
                                        }
                                    }
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
                        // Today's implicit reading: the nearest that fits
                        // (the self mention counts at its position).
                        let pick = match class {
                            Class::None => None,
                            Class::Determined => Some(cands[0].clone()),
                            Class::Ambiguous => {
                                let nearest_intro =
                                    cands.iter().find(|t| matches!(t, Target::Intro(..)));
                                let ipos = nearest_intro.and_then(|t| match t {
                                    Target::Intro(l, i) => {
                                        intros.iter().position(|x| *x == (*l, *i))
                                    }
                                    _ => None,
                                });
                                match (self_at, ipos, cands.contains(&Target::SelfCard)) {
                                    (Some(s), Some(p), true) if s > p => Some(Target::SelfCard),
                                    (_, Some(_), _) => nearest_intro.cloned(),
                                    _ => Some(cands[0].clone()),
                                }
                            }
                        };
                        let cross_line = matches!(&pick, Some(Target::Intro(l, _)) if *l != li);
                        if *k != RefKind::FromAmongThem {
                            earlier.push((r.clone(), cands.clone()));
                        }
                        let _ = base;
                        out.push(Resolved {
                            line: li,
                            ev: ei,
                            kind: *k,
                            class,
                            cands,
                            pick,
                            pos,
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
            }
        }
    }
    out
}

// ------------------------------------------------------------ conversion

fn back_of(intros: &[(usize, usize)], pos: usize, t: (usize, usize)) -> Option<u8> {
    let i = intros[..pos].iter().rposition(|x| *x == t)?;
    u8::try_from(pos - i).ok()
}

/// The intros a scope reads before position `pos`, across prior lines.
fn scope_intros(
    lds: &[LineData],
    li: usize,
    ev: usize,
    cfg: Config,
) -> (Vec<(usize, usize)>, usize) {
    let mut start = li;
    while start > 0 && (lds[start].continues || lds[start].die_row) {
        start -= 1;
    }
    let ld = &lds[li];
    let si = ld.scopes.iter().position(|s| s.contains(&ev)).unwrap();
    let mut intros = Vec::new();
    if si == 0 {
        for (lj, l) in lds.iter().enumerate().take(li).skip(start) {
            for i in line_intros(l, cfg) {
                intros.push((lj, i));
            }
        }
    }
    let mut pos = 0;
    for &e in &ld.scopes[si] {
        if e == ev {
            pos = intros.len();
            break;
        }
        if counts_as_intro(&ld.c.evs[e], cfg) {
            intros.push((li, e));
        }
    }
    (intros, pos)
}

fn new_quant(lds: &[LineData], r: &Resolved, cfg: Config) -> Option<Quant> {
    let t = r.pick.as_ref()?;
    // A pool reference and "the rest" are not the object pointed at but
    // one chosen from it or what a choice left: a bare BackRef would lose
    // that, so they stay (they would want `Rest(BackRef)`, `FromAmong(BackRef)`).
    if matches!(r.kind, RefKind::FromAmongThem | RefKind::Rest) {
        return None;
    }
    let (intros, pos) = scope_intros(lds, r.line, r.ev, cfg);
    Some(match t {
        Target::Intro(l, i) => Quant::BackRef {
            back: back_of(&intros, pos, (*l, *i))?,
        },
        Target::SelfCard => Quant::SelfRef,
        Target::Linked(l, i) => {
            let nth = line_intros(&lds[*l], cfg).iter().position(|x| x == i)? + 1;
            Quant::LinkedBackRef {
                ability: u8::try_from(*l).ok()?,
                nth: u8::try_from(nth).ok()?,
            }
        }
        Target::Set(v) => Quant::BackRefs(
            v.iter()
                .map(|x| back_of(&intros, pos, *x))
                .collect::<Option<Vec<u8>>>()?,
        ),
        Target::Enclosing(_) => return None,
    })
}

struct Rewrite<'a> {
    decl: usize,
    map: &'a BTreeMap<usize, Quant>,
}
impl Visitor for Rewrite<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if let Some(q) = self.map.get(&self.decl) {
            // The filter stays, so the walk's count of objects stays too.
            o.quant = q.clone();
        }
        self.decl += 1;
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

fn describe(lds: &[LineData], t: &Target) -> String {
    let one = |l: usize, i: usize| -> String {
        let e = &lds[l].c.evs[i];
        let s = match (&e.obj, e.role == Role::Pseudo) {
            (_, true) => match e.verb.as_str() {
                "@token" => "the token(s) created".to_string(),
                v => format!("the cards of {v}"),
            },
            (Some(o), _) => objects_text(o),
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
            .map(|(l, i)| one(*l, *i))
            .collect::<Vec<_>>()
            .join(" + "),
    }
}

// ------------------------------------------------------------ validator

#[derive(Default)]
struct Validation {
    checked: usize,
    out_of_range: usize,
    incompatible: usize,
    moved: usize,
    linked_bad: usize,
    examples: Vec<String>,
}

fn validate(
    card: &str,
    conv: &[(Ability, bool)],
    intended: &BTreeMap<(usize, usize), Target>,
    type_line: &str,
    cfg: Config,
    force: Option<&[Vec<Trail>]>,
    v: &mut Validation,
) {
    let _ = type_line;
    let lds = line_data(conv, cfg, force);
    for (li, ld) in lds.iter().enumerate() {
        for (ei, e) in ld.c.evs.iter().enumerate() {
            let Some(o) = &e.obj else { continue };
            let Role::Ref(k) = e.role else { continue };
            if !matches!(
                k,
                RefKind::BackRef | RefKind::LinkedBackRef | RefKind::BackRefs
            ) {
                continue;
            }
            v.checked += 1;
            let (intros, pos) = scope_intros(&lds, li, ei, cfg);
            let mut fail = |what: &str, v: &mut Validation| {
                if v.examples.len() < 200 {
                    v.examples
                        .push(format!("{card} line {li}: {what} {:?}", o.quant));
                }
            };
            match &o.quant {
                Quant::BackRef { back } => {
                    let b = *back as usize;
                    if b == 0 || b > pos {
                        v.out_of_range += 1;
                        fail("out of range", v);
                        continue;
                    }
                    let (l, i) = intros[pos - b];
                    if !compat(o, &lds[l].c.evs[i]) {
                        v.incompatible += 1;
                        fail("incompatible", v);
                    }
                    if intended.get(&(li, ei)) != Some(&Target::Intro(l, i)) {
                        v.moved += 1;
                        fail("lands elsewhere than converted", v);
                    }
                }
                Quant::BackRefs(bs) => {
                    for &b in bs {
                        let b = b as usize;
                        if b == 0 || b > pos {
                            v.out_of_range += 1;
                            fail("out of range", v);
                        } else if !compat(o, &lds[intros[pos - b].0].c.evs[intros[pos - b].1]) {
                            v.incompatible += 1;
                            fail("incompatible", v);
                        }
                    }
                }
                Quant::LinkedBackRef { ability, nth } => {
                    let l = *ability as usize;
                    let ok = l < lds.len()
                        && line_intros(&lds[l], cfg)
                            .get(*nth as usize - 1)
                            .is_some_and(|&i| compat(o, &lds[l].c.evs[i]));
                    if !ok {
                        v.linked_bad += 1;
                        fail("linked target missing or incompatible", v);
                    }
                }
                _ => {}
            }
        }
    }
}

// ------------------------------------------------------------ driver

fn compact(s: &str) -> String {
    s.replace(
        ", filter: Filter { alts: [], kind: None, posts: [], or: None }",
        "",
    )
    .replace("Filter { alts: [], kind: None, posts: [], or: None }", "F0")
    .replace(", posts: [], or: None", "")
    .replace(", or: None", "")
}

fn annotate(dbg: &str, notes: &[String]) -> String {
    let mut out = String::new();
    let mut rest = dbg;
    let mut k = 0;
    let pats = ["BackRef { back: ", "LinkedBackRef { ", "BackRefs(["];
    loop {
        let next = pats
            .iter()
            .filter_map(|p| rest.find(p).map(|i| (i, *p)))
            .min_by_key(|x| x.0);
        let Some((i, p)) = next else { break };
        let close = if p == "BackRefs([" { "])" } else { "}" };
        let j = i + rest[i..].find(close).unwrap() + close.len();
        out.push_str(&rest[..j]);
        if let Some(n) = notes.get(k) {
            let _ = write!(out, " /* {n} */");
        }
        k += 1;
        rest = &rest[j..];
    }
    out.push_str(rest);
    out
}

const CARDS: [&str; 23] = [
    "Jace, Reality Sculptor",
    "Magmatic Core",
    "Invasive Maneuvers",
    "Torbran, Thane of Red Fell",
    "Kindred Summons",
    "Sowing Salt",
    "Promise of Loyalty",
    "Dark Confidant",
    "Bucolic Ranch",
    "Conductive Machete",
    "Prossh, Skyraider of Kher",
    "Tyrannical Pitlord",
    "Scheming Fence",
    "Zenos yae Galvus",
    "Diabolic Servitude",
    "Havengul Laboratory",
    "Contraband Livestock",
    "Oblivion Ring",
    "Fiend Hunter",
    "Journey to Nowhere",
    "Jace, Vryn's Prodigy",
    "Dire Tactics",
    "Echoing Calm",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let show_cards = args.iter().any(|a| a == "--cards");
    let show_fail = args.iter().any(|a| a == "--failures");
    let examples_of = args
        .iter()
        .position(|a| a == "--examples")
        .and_then(|i| args.get(i + 1).cloned());
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/snapshot");
    let mut faces: Vec<Entry> = Vec::new();
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for p in paths {
        let text = std::fs::read_to_string(&p).unwrap();
        for (e, _) in golden::read(&text).unwrap() {
            faces.push(e);
        }
    }
    let base = Config {
        intro_in_amount: false,
        intro_in_cond: true,
        flip: false,
    };
    let configs = [
        ("default (no introducers inside amounts)", base),
        (
            "every non-reference object introduces",
            Config {
                intro_in_amount: true,
                ..base
            },
        ),
        (
            "nor inside conditions",
            Config {
                intro_in_cond: false,
                ..base
            },
        ),
    ];
    let parsed: Vec<(Entry, Vec<(Ability, bool)>, bool)> = faces
        .into_iter()
        .map(|e| {
            let lines = e.lines();
            let all = lines.iter().all(|l| l.parsed.is_ok());
            let abs = lines
                .into_iter()
                .filter_map(|l| l.parsed.ok().map(|a| (a, l.continues)))
                .collect();
            (e, abs, all)
        })
        .collect();
    eprintln!("{} faces", parsed.len());

    for (ci, (name, cfg)) in configs.iter().enumerate() {
        let mut table: BTreeMap<(RefKind, Class), usize> = BTreeMap::new();
        let mut none_why: BTreeMap<String, usize> = BTreeMap::new();
        let mut amb_n: BTreeMap<usize, usize> = BTreeMap::new();
        let mut amb_self: usize = 0;
        let mut cross = 0;
        let mut out_of_scope = 0;
        let mut players_that = 0;
        let mut flipped = 0;
        let mut flip_total = 0;
        let mut v = Validation::default();
        // The same check where the tree records each If/ForEach's order.
        let mut vk = Validation::default();
        let mut converted_n = 0;
        let mut example_lines: Vec<String> = Vec::new();
        for (e, abs, _) in &parsed {
            let lds = line_data(abs, *cfg, None);
            for ld in &lds {
                players_that += ld.c.players_that;
                out_of_scope +=
                    ld.c.evs
                        .iter()
                        .filter(|x| x.role == Role::OutOfScope)
                        .count();
            }
            let res = resolve_face(&lds, &e.type_line, *cfg);
            let mut maps: Vec<BTreeMap<usize, Quant>> = vec![BTreeMap::new(); abs.len()];
            let mut intended: BTreeMap<(usize, usize), Target> = BTreeMap::new();
            for r in &res {
                *table.entry((r.kind, r.class)).or_default() += 1;
                if r.cross_line {
                    cross += 1;
                }
                match r.class {
                    Class::None => {
                        let ev = &lds[r.line].c.evs[r.ev];
                        let o = ev.obj.as_ref().unwrap();
                        let ctx = if ev.verb == "@granted" {
                            "in a granted/quoted ability".to_string()
                        } else {
                            format!("verb {}", ev.verb)
                        };
                        let key = format!("{:?} | {ctx} | kind {:?}", r.kind, o.filter.kind);
                        *none_why.entry(key).or_default() += 1;
                    }
                    Class::Ambiguous => {
                        *amb_n.entry(r.cands.len().min(5)).or_default() += 1;
                        if r.cands.len() == 2 && r.cands.contains(&Target::SelfCard) {
                            amb_self += 1;
                        }
                    }
                    Class::Determined => {}
                }
                if let Some(want) = &examples_of {
                    let tag = format!("{:?}:{:?}", r.kind, r.class);
                    if &tag == want && example_lines.len() < 25 && ci == 0 {
                        let ev = &lds[r.line].c.evs[r.ev];
                        example_lines.push(format!(
                            "{} | {} | ref {} | cands: {}",
                            e.card,
                            ld_text(e, r.line).chars().take(200).collect::<String>(),
                            ev.obj.as_ref().map(objects_text).unwrap_or_default(),
                            r.cands
                                .iter()
                                .map(|t| describe(&lds, t))
                                .collect::<Vec<_>>()
                                .join(" ; ")
                        ));
                    }
                }
                if let (Some(q), Some(d)) = (new_quant(&lds, r, *cfg), lds[r.line].c.evs[r.ev].decl)
                {
                    if let Some(Target::Intro(..)) = &r.pick {
                        intended.insert((r.line, r.ev), r.pick.clone().unwrap());
                    }
                    maps[r.line].insert(d, q);
                    converted_n += 1;
                }
            }
            // The converted face, checked from its own tree.
            let conv: Vec<(Ability, bool)> = abs
                .iter()
                .zip(&maps)
                .map(|((a, c), m)| {
                    let mut a = a.clone();
                    a.walk(&mut Rewrite { decl: 0, map: m });
                    (a, *c)
                })
                .collect();
            validate(&e.card, &conv, &intended, &e.type_line, *cfg, None, &mut v);
            let trails: Vec<Vec<Trail>> = lds.iter().map(|l| l.c.trails.clone()).collect();
            validate(
                &e.card,
                &conv,
                &intended,
                &e.type_line,
                *cfg,
                Some(&trails),
                &mut vk,
            );

            // Fragility: the same refs with every unordered node flipped.
            if ci == 0 {
                let fcfg = Config { flip: true, ..*cfg };
                let flds = line_data(abs, fcfg, None);
                let fres = resolve_face(&flds, &e.type_line, fcfg);
                for (a, b) in res.iter().zip(&fres) {
                    if a.class == Class::Determined && matches!(a.pick, Some(Target::Intro(..))) {
                        flip_total += 1;
                        let qa = new_quant(&lds, a, *cfg);
                        let qb = new_quant(&flds, b, fcfg);
                        if qa != qb || a.pick != b.pick {
                            flipped += 1;
                        }
                    }
                }
            }

            if show_cards && ci == 0 && CARDS.iter().any(|c| e.card.starts_with(c)) {
                println!("\n### {} ({})", e.card, e.face);
                for (li, ((a, _), (ca, _))) in abs.iter().zip(&conv).enumerate() {
                    let rs: Vec<&Resolved> = res.iter().filter(|r| r.line == li).collect();
                    if rs.is_empty() {
                        continue;
                    }
                    let ld = &lds[li];
                    println!("> {}", ld_text(e, li));
                    println!("old: {}", compact(&format!("{a:?}")));
                    // Notes in Debug order: refs sorted by decl.
                    let mut by_decl: Vec<&&Resolved> = rs
                        .iter()
                        .filter(|r| {
                            ld.c.evs[r.ev].decl.is_some()
                                && maps[li].contains_key(&ld.c.evs[r.ev].decl.unwrap())
                                && !matches!(
                                    maps[li][&ld.c.evs[r.ev].decl.unwrap()],
                                    Quant::SelfRef
                                )
                        })
                        .collect();
                    by_decl.sort_by_key(|r| ld.c.evs[r.ev].decl);
                    let notes: Vec<String> = by_decl
                        .iter()
                        .map(|r| describe(&lds, r.pick.as_ref().unwrap()))
                        .collect();
                    println!("new: {}", annotate(&compact(&format!("{ca:?}")), &notes));
                    for r in rs {
                        let ev = &ld.c.evs[r.ev];
                        println!(
                            "  - {:?} \"{}\" -> {:?}; candidates: [{}]; pick: {}{}",
                            r.kind,
                            ev.obj.as_ref().map(objects_text).unwrap_or_default(),
                            r.class,
                            r.cands
                                .iter()
                                .map(|t| describe(&lds, t))
                                .collect::<Vec<_>>()
                                .join(" | "),
                            r.pick
                                .as_ref()
                                .map(|t| describe(&lds, t))
                                .unwrap_or("-".into()),
                            if r.note.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", r.note)
                            }
                        );
                    }
                }
            }
        }
        if show_cards && ci == 0 {
            println!();
        }
        println!("\n== config: {name}");
        println!(
            "{:<18} {:>8} {:>8} {:>8} {:>8} {:>7}",
            "quant", "DETERM", "AMBIG", "NONE", "total", "%det"
        );
        let kinds: BTreeSet<RefKind> = table.keys().map(|k| k.0).collect();
        let (mut td, mut ta, mut tn) = (0, 0, 0);
        for k in kinds {
            let d = table.get(&(k, Class::Determined)).copied().unwrap_or(0);
            let a = table.get(&(k, Class::Ambiguous)).copied().unwrap_or(0);
            let n = table.get(&(k, Class::None)).copied().unwrap_or(0);
            td += d;
            ta += a;
            tn += n;
            println!(
                "{:<18} {d:>8} {a:>8} {n:>8} {:>8} {:>6.1}%",
                format!("{k:?}"),
                d + a + n,
                100.0 * d as f64 / (d + a + n) as f64
            );
        }
        let t = td + ta + tn;
        println!(
            "{:<18} {td:>8} {ta:>8} {tn:>8} {t:>8} {:>6.1}%",
            "ALL",
            100.0 * td as f64 / t as f64
        );
        println!("ambiguous by candidate count (5 = 5+): {amb_n:?}; of the 2-way, this-card vs one object: {amb_self}");
        println!("picks reaching into an earlier paragraph: {cross}; player/counter refs skipped: {out_of_scope}; Player::That: {players_that}");
        println!("converted {converted_n}; validator: checked {} out-of-range {} incompatible {} landed-elsewhere {} linked-bad {}", v.checked, v.out_of_range, v.incompatible, v.moved, v.linked_bad);
        println!("validator, order recorded: checked {} out-of-range {} incompatible {} landed-elsewhere {} linked-bad {}", vk.checked, vk.out_of_range, vk.incompatible, vk.moved, vk.linked_bad);
        if ci == 0 && show_fail {
            for x in v.examples.iter() {
                println!("  ! {x}");
            }
            for x in vk.examples.iter() {
                println!("  !k {x}");
            }
        }
        if ci == 0 {
            println!("flip unordered If/ForEach/Damage: {flipped} of {flip_total} determined BackRefs move");
            let mut why: Vec<_> = none_why.into_iter().collect();
            why.sort_by(|a, b| b.1.cmp(&a.1));
            println!("NONE by kind/context (top 30):");
            for (k, n) in why.iter().take(30) {
                println!("  {n:>5} {k}");
            }
            for l in &example_lines {
                println!("  ex: {l}");
            }
        }
    }
}

fn ld_text(e: &Entry, li: usize) -> String {
    // Lines as `read_face` gives them; bullets joined to their line.
    e.lines()
        .get(li)
        .map(|l| l.text.replace('\n', " "))
        .unwrap_or_default()
}
