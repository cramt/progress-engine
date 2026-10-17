//! Binders ([binders] in SEMANTICS.md): every object a reference points
//! back at is bound where it comes to exist, and a reference names the
//! binder by a de Bruijn index over the binders in scope.
//!
//! Two binding times, as the rules have them:
//! - `Ability::Let { bind, ability }`: what is fixed as the ability is put
//!   on the stack or triggers: targets (CR 601.2c), the objects of the
//!   trigger's event, what a cost names (CR 601.2h), a static's objects,
//!   the card hideaway exiled. In scope in the whole ability.
//! - `Effect::With { bind, then }`: what comes to exist or is found as the
//!   ability resolves: a card searched for, revealed, exiled or chosen, a
//!   token made. It sits in the ability's own list of effects and is in
//!   scope in `then`, the rest of that list from the effect that
//!   introduces it.
//! - `Effect::ForEach { each, then }` binds `each` in `then`, always. A
//!   loop's object something names from outside the loop (or from an
//!   object lifted out of it) is bound like any other instead.
//!
//! A described object is lifted into `bind`, and `Quant::Introduced(i)`
//! stands where the text says it in full, so `inline` puts it back there.
//! An object said inside another bound object's words cannot be bound on
//! its own; a reference to it becomes `Unresolved` (one in the corpus).
//!
//! `Quant::Bound(0)` is the innermost binder in scope. In a `bind` list
//! the later entry is the inner one, and each entry sees the ones before
//! it. A binder whose object no `Objects` describes (a token, the cards a
//! draw moves) is `Quant::Made`: what the first effect of `then` makes.
//! Another line's binder (a linked ability, CR 607.2a; a spell's earlier
//! paragraph, CR 608.2c; a die's roll line) is `Quant::LinkedBound { line,
//! binder }`, the `binder`-th binder of that line in walk order.
//!
//! The tree holds no names. `name` derives one from what introduced a
//! binder, which golden files print beside each reference.
//!
//! The conversion reads the pointers `backref::resolve` wrote: the
//! referent of every reference is the one it resolved, only its form
//! changes. Only an object something refers to is bound.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::*;
use crate::backref::{analyse, Kind, Target};
use crate::walk::{Visitor, Walk};

/// Marks a binder by its id while the conversion runs; never in a tree
/// `read_face` returns.
const MARK: &str = "\u{1}binder ";

fn mark(o: &mut Objects, id: usize) {
    o.filter
        .posts
        .push(Post::Named(PrintedName(format!("{MARK}{id}"))));
}

fn take_mark(o: &mut Objects) -> Option<usize> {
    let i = o
        .filter
        .posts
        .iter()
        .position(|p| matches!(p, Post::Named(PrintedName(s)) if s.starts_with(MARK)))?;
    let Post::Named(PrintedName(s)) = o.filter.posts.remove(i) else {
        unreachable!("matched above");
    };
    s[MARK.len()..].parse().ok()
}

fn wrapper(a: &Ability) -> bool {
    matches!(
        a,
        Ability::Labelled { .. }
            | Ability::Harnessed(_)
            | Ability::KeywordOn { .. }
            | Ability::Station { .. }
            | Ability::Let { .. }
    )
}

fn count_objects<T: Walk + Clone>(x: &T) -> usize {
    struct N(usize);
    impl Visitor for N {
        fn enter(&mut self, _: &mut Objects) -> bool {
            self.0 += 1;
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut n = N(0);
    x.clone().walk(&mut n);
    n.0
}

/// Where a binder goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
    /// The `Let` of the `root`-th scope (an ability that is no wrapper).
    Let { root: usize },
    /// A `With` before the `top`-th effect of that scope's effect list.
    With { root: usize, top: usize },
    /// The loop variable of a `ForEach`: bound where it stands.
    Each,
}

/// A binder of one line, by its temporary id (its index here).
#[derive(Clone, Debug)]
struct Binder {
    /// The object in walk order, or `None` for objects an effect made.
    decl: Option<usize>,
    site: Site,
    /// Its event in the analysis, and where it is a loop's object, the
    /// site it has if it cannot stay one.
    ev: usize,
    lifted: Site,
    /// The references to it: (line, event).
    uses: Vec<(usize, usize)>,
}

/// A reference's new form, with temporary binder ids.
#[derive(Clone, Debug)]
enum To {
    /// A pointer to an object inside another bound object's description,
    /// which no binder can say: no referent.
    Nothing,
    One(Ptr),
    Several(Vec<Ptr>),
    Rest(usize),
}

#[derive(Clone, Copy, Debug)]
enum Ptr {
    Here(usize),
    /// (line, id in that line).
    There(usize, usize),
    Card,
}

/// Binds every object a reference of the face points at, and makes each
/// pointer a reference to its binder.
pub fn bind(lines: &mut [crate::Line], type_line: &str) {
    run(lines, type_line, false);
    if lines.iter().any(|l| l.said.is_some()) {
        run(lines, type_line, true);
    }
}

fn run(lines: &mut [crate::Line], type_line: &str, said: bool) {
    let n = lines.len();
    let mut binders: Vec<Vec<Binder>> = vec![Vec::new(); n];
    let mut ids: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); n];
    let mut refs: Vec<BTreeMap<usize, To>> = vec![BTreeMap::new(); n];
    {
        let abs: Vec<(Option<&Ability>, bool)> = lines
            .iter()
            .map(|l| {
                let p = l.parsed.as_ref().ok();
                (if said { l.said.as_ref().or(p) } else { p }, l.continues)
            })
            .collect();
        let an = analyse(&abs, type_line);
        let mut id_of = |l: usize, ev: usize, binders: &mut Vec<Vec<Binder>>| -> usize {
            *ids[l].entry(ev).or_insert_with(|| {
                let info = an.intro_info(l, ev);
                let lifted = match info.top_effect {
                    Some(top) if !info.cast_time => Site::With {
                        root: info.scope_root,
                        top,
                    },
                    _ => Site::Let {
                        root: info.scope_root,
                    },
                };
                binders[l].push(Binder {
                    decl: info.decl,
                    site: if info.each_of_loop {
                        Site::Each
                    } else {
                        lifted
                    },
                    ev,
                    lifted,
                    uses: Vec::new(),
                });
                binders[l].len() - 1
            })
        };
        for r in &an.resolved {
            if !matches!(
                r.kind,
                Kind::BackRef | Kind::LinkedBackRef | Kind::BackRefs | Kind::Rest
            ) {
                continue;
            }
            let (Some(decl), Some(pick)) = (an.decl(r), &r.pick) else {
                continue;
            };
            let mut ptr = |t: &Target, binders: &mut Vec<Vec<Binder>>| -> Option<Ptr> {
                Some(match t {
                    Target::Intro(l, i) | Target::Linked(l, i) => {
                        let id = id_of(*l, *i, binders);
                        binders[*l][id].uses.push((r.line, r.ev));
                        if *l == r.line {
                            Ptr::Here(id)
                        } else {
                            Ptr::There(*l, id)
                        }
                    }
                    Target::SelfCard => Ptr::Card,
                    _ => return None,
                })
            };
            let to = match (r.kind, pick) {
                (Kind::Rest, t) => match ptr(t, &mut binders) {
                    Some(Ptr::Here(id)) => To::Rest(id),
                    _ => continue,
                },
                (_, Target::Set(v)) => {
                    let Some(ps) = v.iter().map(|t| ptr(t, &mut binders)).collect() else {
                        continue;
                    };
                    To::Several(ps)
                }
                (_, t) => match ptr(t, &mut binders) {
                    Some(p) => To::One(p),
                    None => continue,
                },
            };
            refs[r.line].insert(decl, to);
        }
        // A loop's object stays the loop's binder only if every reference
        // to it is in the loop and none is in an object lifted out of it;
        // otherwise it is bound where the loop stands, like any object.
        loop {
            let mut moved = false;
            #[allow(clippy::needless_range_loop)] // a line is read while another changes
            for l in 0..n {
                for k in 0..binders[l].len() {
                    let b = &binders[l][k];
                    if b.site != Site::Each {
                        continue;
                    }
                    let outside = b.uses.iter().any(|&(rl, rev)| {
                        rl != l
                            || !an.inside_loop(l, rev, b.ev)
                            || binders[l].iter().any(|o| {
                                o.site != Site::Each && o.decl.is_some() && an.within(l, rev, o.ev)
                            })
                    });
                    if outside {
                        binders[l][k].site = binders[l][k].lifted;
                        moved = true;
                    }
                }
            }
            if !moved {
                break;
            }
        }
        // An object said inside another bound object's words goes with
        // them; a reference to it names nothing a binder can.
        #[allow(clippy::needless_range_loop)] // `refs` of another line changes
        for l in 0..n {
            let inner: Vec<usize> = (0..binders[l].len())
                .filter(|&k| {
                    let b = &binders[l][k];
                    b.decl.is_some()
                        && binders[l].iter().any(|o| {
                            o.site != Site::Each && o.decl.is_some() && an.within(l, b.ev, o.ev)
                        })
                })
                .collect();
            for k in inner {
                for (rl, rev) in binders[l][k].uses.clone() {
                    if let Some(d) = an.decl_of(rl, rev) {
                        refs[rl].insert(d, To::Nothing);
                    }
                }
                binders[l][k].decl = None;
                binders[l][k].site = Site::Each;
            }
        }
    }
    // Per line: references and binders marked, binders lifted, sites
    // built, indices counted.
    let mut ordinals: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); n];
    for (l, line) in lines.iter_mut().enumerate() {
        let a = if said {
            line.said.as_mut()
        } else {
            line.parsed.as_mut().ok()
        };
        let Some(a) = a else { continue };
        if binders[l].is_empty() && refs[l].is_empty() {
            continue;
        }
        a.walk(&mut Refs {
            decl: 0,
            refs: &refs[l],
        });
        let mut lift = Lift {
            decl: 0,
            binders: &binders[l],
            by_decl: binders[l]
                .iter()
                .enumerate()
                .filter_map(|(i, b)| b.decl.map(|d| (d, i)))
                .collect(),
            lifted: BTreeMap::new(),
        };
        a.walk(&mut lift);
        let lifted = lift.lifted;
        // Objects an effect made, and lifted descriptions, by site.
        let mut sites: BTreeMap<(usize, Option<usize>), Vec<Objects>> = BTreeMap::new();
        for (id, b) in binders[l].iter().enumerate() {
            let o = match b.decl {
                _ if b.site == Site::Each => continue,
                Some(_) => match lifted.get(&id) {
                    Some(o) => o.clone(),
                    None => continue,
                },
                None => {
                    let mut o = Objects {
                        quant: Quant::Made,
                        filter: Filter::default(),
                    };
                    mark(&mut o, id);
                    o
                }
            };
            let key = match b.site {
                Site::Let { root } => (root, None),
                Site::With { root, top } => (root, Some(top)),
                Site::Each => continue,
            };
            sites.entry(key).or_default().push(o);
        }
        a.walk(&mut Build {
            root: 0,
            skip: false,
            sites: &mut sites,
        });
        let mut scope = Scope::default();
        a.walk(&mut scope);
        ordinals[l] = scope.ordinal;
    }
    // Another line's binders, by their place in that line.
    for (l, line) in lines.iter_mut().enumerate() {
        let a = if said {
            line.said.as_mut()
        } else {
            line.parsed.as_mut().ok()
        };
        let Some(a) = a else { continue };
        let _ = l;
        a.walk(&mut Links {
            ordinals: &ordinals,
        });
    }
}

/// Writes each reference with temporary ids.
struct Refs<'a> {
    decl: usize,
    refs: &'a BTreeMap<usize, To>,
}

fn ptr_quant(p: Ptr) -> Pointer {
    match p {
        Ptr::Here(id) => Pointer::Bound(u8::try_from(id).unwrap_or(u8::MAX)),
        Ptr::There(l, id) => Pointer::LinkedBound {
            line: u8::try_from(l).unwrap_or(u8::MAX),
            binder: u8::try_from(id).unwrap_or(u8::MAX),
        },
        Ptr::Card => Pointer::ThisCard,
    }
}

impl Visitor for Refs<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if let Some(to) = self.refs.get(&self.decl) {
            o.quant = match to {
                To::Nothing => Quant::Unresolved(Hint::Antecedent),
                To::Rest(id) => Quant::Rest {
                    of: u8::try_from(*id).unwrap_or(u8::MAX),
                },
                To::One(p) => match ptr_quant(*p) {
                    Pointer::Bound(i) => Quant::Bound(i),
                    Pointer::LinkedBound { line, binder } => Quant::LinkedBound { line, binder },
                    Pointer::ThisCard => Quant::SelfRef,
                    Pointer::Back(_) | Pointer::Linked { .. } => unreachable!("not written here"),
                },
                To::Several(ps) => Quant::Bounds(ps.iter().map(|p| ptr_quant(*p)).collect()),
            };
        }
        self.decl += 1;
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// Takes each bound object out of its place, leaving a reference to its
/// binder where it was first said.
struct Lift<'a> {
    decl: usize,
    binders: &'a [Binder],
    by_decl: BTreeMap<usize, usize>,
    lifted: BTreeMap<usize, Objects>,
}

impl Visitor for Lift<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let d = self.decl;
        self.decl += 1;
        let Some(&id) = self.by_decl.get(&d) else {
            return true;
        };
        if self.binders[id].site == Site::Each {
            mark(o, id);
            return true;
        }
        // Its parts go with it: count them as met.
        self.decl += count_objects(o) - 1;
        let mut lifted = std::mem::replace(
            o,
            Objects {
                quant: Quant::Introduced(u8::try_from(id).unwrap_or(u8::MAX)),
                filter: Filter::default(),
            },
        );
        mark(&mut lifted, id);
        self.lifted.insert(id, lifted);
        false
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// Puts each scope's `With`s and `Let`.
struct Build<'a> {
    root: usize,
    skip: bool,
    sites: &'a mut BTreeMap<(usize, Option<usize>), Vec<Objects>>,
}

impl Visitor for Build<'_> {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, a: &mut Ability) -> bool {
        if self.skip {
            self.skip = false;
            return true;
        }
        if wrapper(a) {
            return true;
        }
        let root = self.root;
        self.root += 1;
        let withs: Vec<(usize, Vec<Objects>)> = self
            .sites
            .range((root, Some(0))..=(root, Some(usize::MAX)))
            .map(|(k, v)| (k.1.expect("a With site"), v.clone()))
            .collect();
        let mut lets = self.sites.remove(&(root, None)).unwrap_or_default();
        if !withs.is_empty() {
            match a.effects_mut() {
                Some(es) => {
                    for (top, bind) in withs.into_iter().rev() {
                        let top = top.min(es.len());
                        let then = es.split_off(top);
                        es.push(Effect::With { bind, then });
                    }
                }
                // No list of effects to scope it in: the whole ability.
                None => {
                    for (_, bind) in withs {
                        lets.extend(bind);
                    }
                }
            }
        }
        if !lets.is_empty() {
            let inner = std::mem::replace(a, Ability::Keywords(Vec::new()));
            *a = Ability::Let {
                bind: lets,
                ability: Box::new(inner),
            };
            self.skip = true;
        }
        true
    }
}

/// The binders in scope at each point of a walk, outermost first, and
/// each reference's index counted over them.
#[derive(Default)]
pub(crate) struct Scope {
    /// Binders in scope: their temporary id, `None` for a loop variable
    /// nothing named.
    pub(crate) stack: Vec<Option<usize>>,
    /// Sites open: how many binders each still binds, the object depth its
    /// binders sit at, and how many it pushed.
    sites: Vec<(usize, usize, usize)>,
    depth: usize,
    /// Each binder's place in the line, by temporary id.
    pub(crate) ordinal: BTreeMap<usize, usize>,
    count: usize,
    /// References whose binder is not in scope.
    pub(crate) misses: usize,
}

impl Scope {
    fn open(&mut self, n: usize) {
        self.sites.push((n, self.depth, 0));
    }
    fn close(&mut self) {
        if let Some((_, _, pushed)) = self.sites.pop() {
            for _ in 0..pushed {
                self.stack.pop();
            }
        }
    }
    fn index(&mut self, id: u8) -> u8 {
        match self
            .stack
            .iter()
            .rev()
            .position(|x| *x == Some(usize::from(id)))
        {
            Some(i) => u8::try_from(i).unwrap_or(u8::MAX),
            None => {
                self.misses += 1;
                u8::MAX
            }
        }
    }
}

impl Visitor for Scope {
    fn enter(&mut self, o: &mut Objects) -> bool {
        match &mut o.quant {
            Quant::Bound(i) | Quant::Introduced(i) => *i = self.index(*i),
            Quant::Rest { of } => *of = self.index(*of),
            Quant::Bounds(ps) => {
                for p in ps.iter_mut() {
                    if let Pointer::Bound(i) = p {
                        *i = self.index(*i);
                    }
                }
            }
            _ => {}
        }
        self.depth += 1;
        true
    }
    fn leave(&mut self, o: &mut Objects) {
        self.depth -= 1;
        if let Some(site) = self.sites.last_mut() {
            if site.1 == self.depth && site.0 > 0 {
                site.0 -= 1;
                site.2 += 1;
                let id = take_mark(o);
                self.stack.push(id);
                if let Some(id) = id {
                    self.ordinal.insert(id, self.count);
                }
                self.count += 1;
            }
        }
    }
    fn ability(&mut self, a: &mut Ability) -> bool {
        if let Ability::Let { bind, .. } = a {
            self.open(bind.len());
        } else {
            self.open(0);
        }
        true
    }
    fn ability_done(&mut self, _: &mut Ability) {
        self.close();
    }
    fn effect(&mut self, e: &mut Effect) {
        match e {
            Effect::With { bind, .. } => self.open(bind.len()),
            Effect::ForEach { .. } => self.open(1),
            _ => self.open(0),
        }
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.close();
    }
}

/// Another line's binders, by their place in that line.
struct Links<'a> {
    ordinals: &'a [BTreeMap<usize, usize>],
}

impl Links<'_> {
    fn place(&self, line: u8, binder: u8) -> u8 {
        self.ordinals
            .get(usize::from(line))
            .and_then(|m| m.get(&usize::from(binder)))
            .and_then(|&o| u8::try_from(o).ok())
            .unwrap_or(u8::MAX)
    }
}

impl Visitor for Links<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        match &mut o.quant {
            Quant::LinkedBound { line, binder } => *binder = self.place(*line, *binder),
            Quant::Bounds(ps) => {
                for p in ps.iter_mut() {
                    if let Pointer::LinkedBound { line, binder } = p {
                        *binder = self.place(*line, *binder);
                    }
                }
            }
            _ => {}
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

// ---------------------------------------------------------------- reading

/// What a binder is.
#[derive(Clone, Debug)]
pub struct BinderInfo {
    /// What it binds: a description (`Made` for objects an effect made).
    pub what: Objects,
    pub site: SiteKind,
    /// The object in walk order (in the bound tree).
    pub decl: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    Let,
    With,
    Each,
}

/// Each binder of a bound tree and the binder each reference names.
#[derive(Default)]
pub struct Bindings {
    pub binders: Vec<BinderInfo>,
    /// By the reference's object in walk order: the binders it names, in
    /// order (`None` where an index is out of scope).
    pub refs: BTreeMap<usize, Vec<Option<usize>>>,
    /// The reference's own words, by the same key.
    pub words: BTreeMap<usize, Objects>,
    /// References said inside a binder's description.
    pub in_bind: BTreeSet<usize>,
}

struct Read {
    out: Bindings,
    stack: Vec<usize>,
    /// Sites open: (kind, binders still to bind, object depth, pushed).
    sites: Vec<(SiteKind, usize, usize, usize)>,
    depth: usize,
    decl: usize,
    /// The depths of the binder descriptions being read.
    binds: Vec<usize>,
}

impl Read {
    fn open(&mut self, k: SiteKind, n: usize) {
        self.sites.push((k, n, self.depth, 0));
    }
    fn close(&mut self) {
        if let Some((_, _, _, pushed)) = self.sites.pop() {
            for _ in 0..pushed {
                self.stack.pop();
            }
        }
    }
    fn at(&self, i: u8) -> Option<usize> {
        let i = usize::from(i);
        (i < self.stack.len()).then(|| self.stack[self.stack.len() - 1 - i])
    }
}

impl Visitor for Read {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let named = match &o.quant {
            Quant::Bound(i) | Quant::Introduced(i) | Quant::Rest { of: i } => {
                Some(vec![self.at(*i)])
            }
            Quant::Bounds(ps) => Some(
                ps.iter()
                    .filter_map(|p| match p {
                        Pointer::Bound(i) => Some(self.at(*i)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        };
        if let Some(v) = named {
            self.out.refs.insert(self.decl, v);
            self.out.words.insert(self.decl, o.clone());
            if !self.binds.is_empty() {
                self.out.in_bind.insert(self.decl);
            }
        }
        if self
            .sites
            .last()
            .is_some_and(|s| s.2 == self.depth && s.1 > 0)
        {
            self.binds.push(self.depth);
        }
        self.decl += 1;
        self.depth += 1;
        true
    }
    fn leave(&mut self, o: &mut Objects) {
        self.depth -= 1;
        let depth = self.depth;
        if self.binds.last() == Some(&depth) {
            self.binds.pop();
        }
        if let Some(site) = self.sites.last_mut() {
            if site.2 == depth && site.1 > 0 {
                site.1 -= 1;
                site.3 += 1;
                let kind = site.0;
                self.out.binders.push(BinderInfo {
                    what: o.clone(),
                    site: kind,
                    decl: self.decl - count_objects(o),
                });
                self.stack.push(self.out.binders.len() - 1);
            }
        }
    }
    fn ability(&mut self, a: &mut Ability) -> bool {
        match a {
            Ability::Let { bind, .. } => self.open(SiteKind::Let, bind.len()),
            _ => self.open(SiteKind::Let, 0),
        }
        true
    }
    fn ability_done(&mut self, _: &mut Ability) {
        self.close();
    }
    fn effect(&mut self, e: &mut Effect) {
        match e {
            Effect::With { bind, .. } => self.open(SiteKind::With, bind.len()),
            Effect::ForEach { .. } => self.open(SiteKind::Each, 1),
            _ => self.open(SiteKind::With, 0),
        }
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.close();
    }
}

/// Reads a bound tree's binders and what each reference names.
pub fn bindings(a: &Ability) -> Bindings {
    let mut r = Read {
        out: Bindings::default(),
        stack: Vec::new(),
        sites: Vec::new(),
        depth: 0,
        decl: 0,
        binds: Vec::new(),
    };
    a.clone().walk(&mut r);
    r.out
}

/// The tree with its binders said where the text first says them and
/// with no `Let` or `With` left: the shape the text has, which the round
/// trip renders. A reference other than a first mention stays a `Bound`,
/// whose index then counts nothing.
pub fn inline(a: &Ability) -> Ability {
    if !has_binders(a) {
        return a.clone();
    }
    let b = bindings(a);
    let subst: BTreeMap<usize, Objects> = b
        .refs
        .iter()
        .filter(|(d, _)| matches!(b.words[*d].quant, Quant::Introduced(_)))
        .filter_map(|(d, v)| match v[..] {
            [Some(binder)] => Some((*d, b.binders[binder].what.clone())),
            _ => None,
        })
        .collect();
    let mut a = a.clone();
    a.walk(&mut Subst { decl: 0, subst });
    a.walk(&mut Unwrap);
    a
}

fn has_binders(a: &Ability) -> bool {
    struct Has(bool);
    impl Visitor for Has {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn ability(&mut self, a: &mut Ability) -> bool {
            self.0 |= matches!(a, Ability::Let { .. });
            true
        }
        fn effect(&mut self, e: &mut Effect) {
            self.0 |= matches!(e, Effect::With { .. });
        }
    }
    let mut h = Has(false);
    a.clone().walk(&mut h);
    h.0
}

struct Subst {
    decl: usize,
    subst: BTreeMap<usize, Objects>,
}

impl Visitor for Subst {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let d = self.decl;
        self.decl += 1;
        if let Some(w) = self.subst.remove(&d) {
            *o = w;
            return false;
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

struct Unwrap;

fn splice(es: &mut Vec<Effect>) {
    if let Some(Effect::With { .. }) = es.last() {
        if let Some(Effect::With { mut then, .. }) = es.pop() {
            splice(&mut then);
            es.extend(then);
        }
    }
}

impl Visitor for Unwrap {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, a: &mut Ability) -> bool {
        while let Ability::Let { ability, .. } = a {
            let inner = std::mem::replace(ability.as_mut(), Ability::Keywords(Vec::new()));
            *a = inner;
        }
        if let Some(es) = effects_of(a) {
            splice(es);
        }
        true
    }
}

/// An ability's own list of effects, through wrappers.
fn effects_of(a: &mut Ability) -> Option<&mut Vec<Effect>> {
    match a {
        Ability::DieRow { effect: es, .. } => Some(es),
        Ability::Let { ability, .. } => effects_of(ability),
        _ => a.effects_mut(),
    }
}

/// A plain name for each binder of a bound tree, from what introduced
/// it, with a number after it where one scope has two of a name.
pub fn names(a: &Ability) -> Vec<String> {
    let b = bindings(a);
    let verbs = crate::backref::verbs_of(a);
    // The verb a binder's first mention is said with.
    let mut first_verb: BTreeMap<usize, String> = BTreeMap::new();
    for (d, v) in &b.refs {
        if let [Some(binder)] = v[..] {
            if let Some(verb) = verbs.get(d) {
                if matches!(b.words[d].quant, Quant::Introduced(_)) {
                    first_verb.insert(binder, verb.clone());
                } else {
                    first_verb.entry(binder).or_insert_with(|| verb.clone());
                }
            }
        }
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    b.binders
        .iter()
        .enumerate()
        .map(|(i, info)| {
            let base = name_of(info, first_verb.get(&i).map(String::as_str).unwrap_or(""));
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                base
            } else {
                format!("{base}{n}")
            }
        })
        .collect()
}

fn name_of(info: &BinderInfo, verb: &str) -> String {
    if info.site == SiteKind::Each {
        return "each".into();
    }
    let o = &info.what;
    if matches!(
        o.quant,
        Quant::Target { .. }
            | Quant::AnyTarget { .. }
            | Quant::AnyOtherTarget
            | Quant::AnyNumberOfTarget
            | Quant::AnyNumberOfOtherTarget
            | Quant::AnotherTarget
            | Quant::SecondTarget
            | Quant::OtherTarget { .. }
    ) {
        return "target".into();
    }
    if o.quant == Quant::Made {
        return "made".into();
    }
    let v = verb.trim_start_matches("@cost:");
    let by = |s: &str| v.contains(s);
    if verb.starts_with("@trigger") {
        "trigger".into()
    } else if by("Sacrifice") {
        "sacrificed".into()
    } else if by("Discard") {
        "discarded".into()
    } else if by("Exile") {
        "exiled".into()
    } else if by("Search") {
        "found".into()
    } else if by("Reveal") || by("UntilFound") {
        "revealed".into()
    } else if by("Choose") || by("Choice") || by("Vote") || by("Pile") {
        "chosen".into()
    } else if by("Destroy") {
        "destroyed".into()
    } else if by("Tap") {
        "tapped".into()
    } else if by("Create") {
        "created".into()
    } else if by("Cast") {
        "cast".into()
    } else if by("Move") {
        "moved".into()
    } else if matches!(o.quant, Quant::Enchanted) {
        "enchanted".into()
    } else if matches!(o.quant, Quant::Equipped) {
        "equipped".into()
    } else {
        "it".into()
    }
}

/// A bound tree's `Debug` with each reference followed by the name of
/// the binder it names, and each binder by its name: what golden files
/// print.
pub fn annotate(a: &Ability, dbg: &str) -> String {
    if !has_binders(a) {
        return dbg.to_string();
    }
    let b = bindings(a);
    let names = names(a);
    // References in walk order, which is `Debug` order.
    let notes: Vec<String> = b
        .refs
        .values()
        .map(|v| {
            v.iter()
                .map(|x| x.map_or("?".into(), |i| names[i].clone()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    const PATS: [&str; 4] = ["Bounds([", "Bound(", "Introduced(", "Rest { of: "];
    let mut out = String::new();
    let mut rest = dbg;
    let mut k = 0;
    while let Some((i, p)) = PATS
        .iter()
        .filter_map(|p| rest.find(p).map(|i| (i, *p)))
        .min_by_key(|x| (x.0, std::cmp::Reverse(x.1.len())))
    {
        let close = match p {
            "Bounds([" => "])",
            "Bound(" | "Introduced(" => ")",
            _ => "}",
        };
        let Some(j) = rest[i..].find(close).map(|j| i + j + close.len()) else {
            break;
        };
        out.push_str(&rest[..j]);
        let Some(n) = notes.get(k) else {
            return dbg.to_string();
        };
        out.push_str(" /* ");
        out.push_str(n);
        out.push_str(" */");
        k += 1;
        rest = &rest[j..];
    }
    if k != notes.len() {
        return dbg.to_string();
    }
    out.push_str(rest);
    // Each binder's name before it: "bind: [/* target */ Objects …".
    let mut named = String::new();
    let mut rest = out.as_str();
    let mut bi = 0;
    while let Some(i) = rest.find("bind: [") {
        let start = i + "bind: [".len();
        named.push_str(&rest[..start]);
        rest = &rest[start..];
        // The entries of this list, each an `Objects { … }` or `Made`.
        while names.get(bi).is_some() && rest.starts_with("Objects {") {
            // Names follow binders in walk order; skip loop variables,
            // which are not in a `bind` list.
            while b.binders.get(bi).is_some_and(|x| x.site == SiteKind::Each) {
                bi += 1;
            }
            let Some(n) = names.get(bi) else { break };
            named.push_str(&format!("/* {n} */ "));
            bi += 1;
            let end = matching(rest);
            named.push_str(&rest[..end]);
            rest = &rest[end..];
            if let Some(r) = rest.strip_prefix(", ") {
                named.push_str(", ");
                rest = r;
            } else {
                break;
            }
        }
    }
    named.push_str(rest);
    named
}

/// The length of the `Objects { … }` at the start of `s`, braces matched.
fn matching(s: &str) -> usize {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    s.len()
}

/// The references of a bound tree (by walk order) to a binder the
/// trigger's event names: "it" in "Whenever you cast a spell, if it's
/// tapped".
pub fn trigger_refs(a: &Ability) -> BTreeSet<usize> {
    let b = bindings(a);
    let verbs = crate::backref::verbs_of(a);
    let said_in_trigger = |d: &usize| verbs.get(d).is_some_and(|v| v == "@trigger");
    let named: BTreeSet<usize> = b
        .refs
        .iter()
        .filter(|(d, _)| said_in_trigger(d))
        .flat_map(|(_, v)| v.iter().flatten().copied())
        .collect();
    b.refs
        .iter()
        .filter(|(d, v)| !said_in_trigger(d) && v.iter().flatten().any(|i| named.contains(i)))
        .map(|(d, _)| *d)
        .collect()
}

/// How many references of a bound tree to what the trigger's event names
/// are tested for, or said with, a state `state` picks out (a permanent's
/// state of the spell that triggered the ability).
pub fn trigger_state_refs(a: &Ability, state: fn(&Filter) -> bool) -> usize {
    struct V {
        refs: BTreeSet<usize>,
        decl: usize,
        pending: Option<bool>,
        state: fn(&Filter) -> bool,
        n: usize,
    }
    impl Visitor for V {
        fn enter(&mut self, o: &mut Objects) -> bool {
            let hit = self.refs.contains(&self.decl);
            self.decl += 1;
            if self.pending.take() == Some(true) && hit {
                self.n += 1;
            }
            if hit && (self.state)(&o.filter) {
                self.n += 1;
            }
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
        fn condition(&mut self, c: &mut Condition) {
            if let Condition::Is { filter, .. } = c {
                self.pending = Some((self.state)(filter));
            }
        }
    }
    let mut v = V {
        refs: trigger_refs(a),
        decl: 0,
        pending: None,
        state,
        n: 0,
    };
    a.clone().walk(&mut v);
    v.n
}
