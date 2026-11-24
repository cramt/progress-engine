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
//!   introduces it; what only a reflexive trigger moves sits in that
//!   trigger's list instead ([when-you-do-binds]).
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

pub(crate) fn count_objects<T: Walk + Clone>(x: &T) -> usize {
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

/// The delayed triggers whose event's objects they bind themselves
/// ([delayed-binds-event]), and a one-shot replacement, which binds the
/// object of each event it replaces the same way ([replacement-binds-event]).
pub(crate) const DELAYED: [&str; 4] = [
    "DelayedWhen",
    "DelayedWhenever",
    "DelayedNext",
    "Replacement",
];

fn delayed(e: &Effect) -> bool {
    matches!(
        e,
        Effect::DelayedWhen { .. }
            | Effect::DelayedWhenever { .. }
            | Effect::DelayedNext { .. }
            | Effect::Replacement { .. }
    )
}

/// "Whenever this creature becomes blocked by a creature of that color
/// this turn, destroy that creature" (Zombie Boa): the object a delayed
/// trigger's event describes is a new one each time it triggers, so the
/// trigger binds it where its event says it, as a `ForEach` binds its
/// object, and never the ability that set the trigger up
/// ([delayed-binds-event]). Every object the event says at its top level
/// that is described (no reference, no target, which is chosen as the
/// ability is put on the stack) is bound, whether or not anything refers
/// to it, so a reader counts the same binders without the resolver.
pub(crate) fn delayed_binds(o: &Objects) -> bool {
    matches!(
        o.quant,
        Quant::One
            | Quant::Each
            | Quant::All
            | Quant::N(_)
            | Quant::OneOrMore
            | Quant::AnyNumber
            | Quant::Another
            | Quant::Plural
            | Quant::Other
            | Quant::AtLeast(_)
            | Quant::AtMost(_)
            | Quant::UpTo(_)
            | Quant::Next
            | Quant::NthEachTurn { .. }
            | Quant::NthDuringTurns { .. }
            | Quant::NthOfGame { .. }
    )
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
                    // "When you next cast a creature spell this turn,
                    // search your library for a creature card with lesser
                    // mana value" (Vivien, Monsters' Advocate): what the
                    // trigger finds is described by its event's spell, and
                    // goes inside the trigger with it (`into_reflexive`).
                    let event = an.event_loop(l, b.ev);
                    let outside = b.uses.iter().any(|&(rl, rev)| {
                        rl != l
                            || !an.inside_loop(l, rev, b.ev)
                            || binders[l].iter().any(|o| {
                                o.site != Site::Each
                                    && o.decl.is_some()
                                    && an.within(l, rev, o.ev)
                                    && !(event && an.inside_loop(l, o.ev, b.ev))
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
            performers: Vec::new(),
            digs: Vec::new(),
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
        a.walk(&mut IntoLoops {
            fresh: binders[l].len(),
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
    /// For each effect the walk is in, the players a `Performed` over
    /// each of them names.
    performers: Vec<Option<Player>>,
    /// For each effect the walk is in, whether it is each player's dig.
    digs: Vec<bool>,
}

/// The players a `Performed` over each of them names.
fn performer(e: &Effect) -> Option<Player> {
    match e {
        Effect::Performed {
            by: by @ (Player::Each | Player::EachOpponent),
            ..
        } => Some(by.clone()),
        _ => None,
    }
}

/// "Each opponent exiles cards from the top of their library until they
/// exile an instant or sorcery card" (Wand of Wonder): each player's own
/// dig, so as many found cards as players ([n-from-each]). "Each opponent
/// may search their library for a land card" (Tempt with Discovery) is
/// each one's own search the same way.
fn digs_each(e: &Effect) -> bool {
    matches!(
        e,
        Effect::UntilFound {
            who: Player::Each | Player::EachOpponent,
            ..
        } | Effect::Search {
            who: Player::Each | Player::EachOpponent,
            ..
        }
    )
}

/// One card found in each library, bound outside the dig.
fn dig_each(o: &mut Objects) {
    if o.quant == Quant::One {
        o.quant = Quant::NFromEach(Amount::N(1));
    }
}

/// `dig_each` undone, where the dig says it.
fn undig_each(o: &mut Objects) {
    if o.quant == Quant::NFromEach(Amount::N(1)) {
        o.quant = Quant::One;
    }
}

/// `distribute` undone, where the object is said inside the players' turns.
fn undistribute(o: &mut Objects, each: &Player) {
    let Quant::NFromEach(n) = &o.quant else {
        return;
    };
    let Some(z) = o.filter.posts.iter_mut().find_map(|p| match p {
        Post::In(Zone::TopOfLibrary(w)) if w == each => Some(w),
        _ => None,
    }) else {
        return;
    };
    *z = Player::That;
    o.quant = Quant::N(n.clone());
}

/// "Each opponent exiles the top card of their library" said once for
/// them all, as a binder outside the players' turns at it: one card from
/// each opponent's library ([n-from-each]), as "the top card of each
/// opponent's library" is (Xander's Pact).
fn distribute(o: &mut Objects, each: &Player) {
    let Quant::N(n) = &o.quant else { return };
    let Some(z) = o.filter.posts.iter_mut().find_map(|p| match p {
        Post::In(Zone::TopOfLibrary(w)) if *w == Player::That => Some(w),
        _ => None,
    }) else {
        return;
    };
    *z = each.clone();
    o.quant = Quant::NFromEach(n.clone());
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
        if let [.., Some(each), _] = &self.performers[..] {
            distribute(&mut lifted, each);
        }
        if self.digs.last() == Some(&true) {
            dig_each(&mut lifted);
        }
        mark(&mut lifted, id);
        self.lifted.insert(id, lifted);
        false
    }
    fn effect(&mut self, e: &mut Effect) {
        self.performers.push(performer(e));
        self.digs.push(digs_each(e));
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.performers.pop();
        self.digs.pop();
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// The temporary ids a `bind` list's entries are marked with.
fn bind_ids(bind: &[Objects]) -> BTreeSet<usize> {
    bind.iter()
        .filter_map(|o| {
            o.filter.posts.iter().find_map(|p| match p {
                Post::Named(PrintedName(s)) if s.starts_with(MARK) => s[MARK.len()..].parse().ok(),
                _ => None,
            })
        })
        .collect()
}

/// Whether `x` says a binder of `ids` (temporary ids, before `Scope`).
fn says<T: Walk + Clone>(x: &T, ids: &BTreeSet<usize>, first_only: bool) -> bool {
    struct Says<'a>(&'a BTreeSet<usize>, bool, bool);
    impl Visitor for Says<'_> {
        fn enter(&mut self, o: &mut Objects) -> bool {
            let id = |i: &u8| self.0.contains(&usize::from(*i));
            self.2 |= match &o.quant {
                Quant::Introduced(i) => id(i),
                Quant::Bound(i) | Quant::Rest { of: i } if !self.1 => id(i),
                Quant::Bounds(ps) if !self.1 => {
                    ps.iter().any(|p| matches!(p, Pointer::Bound(i) if id(i)))
                }
                _ => false,
            };
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut s = Says(ids, first_only, false);
    x.clone().walk(&mut s);
    s.2
}

/// "For each of them, reveal cards … until you reveal a nonland card, ~
/// deals damage equal to that card's mana value …" (Kaboom!): what each
/// pass of a loop finds is bound in the pass, so every pass has its own.
/// A `With` whose `then` starts with the loop, whose binders the loop's
/// first effect says first and nothing outside the loop's body says,
/// goes inside the body.
///
/// One said again after the loop ("You may play the exiled cards",
/// Soulfire Eruption) means what every pass made, not one pass's card: it
/// names a `Made` binder around the loop, what the loop makes
/// ([pass-pile]). `fresh` is the next temporary id free in the line.
fn into_loops(es: &mut Vec<Effect>, fresh: &mut usize) {
    let Some(Effect::With { then, .. }) = es.last_mut() else {
        return;
    };
    into_loops(then, fresh);
    let Some(Effect::With { bind, then }) = es.last() else {
        unreachable!("matched above");
    };
    let ids = bind_ids(bind);
    let movable = ids.len() == bind.len()
        && matches!(then.first(), Some(Effect::ForEach { each, then: body, .. })
            if !says(each, &ids, false)
                && body.first().is_some_and(|f| says(f, &ids, true)));
    if !movable {
        return;
    }
    let after = then[1..].iter().any(|e| says(e, &ids, false));
    // "When you do" after the loop answers the last pass's action
    // (Nihiloor), so its references are one pass's object, not the pile;
    // a reading this rule can't give.
    let reflexive = |e: &Effect| {
        matches!(
            e,
            Effect::WhenYouDo(_)
                | Effect::WhenYouDoIf { .. }
                | Effect::IfYouDo(_)
                | Effect::IfYouDont(_)
        )
    };
    if after
        && !then[1..]
            .iter()
            .all(|e| only_bound(e, &ids) && !reflexive(e))
    {
        return;
    }
    let Some(Effect::With { bind, mut then }) = es.pop() else {
        unreachable!("matched above");
    };
    let Effect::ForEach {
        order,
        each,
        then: body,
    } = then.remove(0)
    else {
        unreachable!("checked above");
    };
    let looped = Effect::ForEach {
        order,
        each,
        then: vec![Effect::With { bind, then: body }],
    };
    if !after {
        es.push(looped);
        es.extend(then);
        return;
    }
    let pile = *fresh;
    *fresh += 1;
    let mut made = Objects {
        quant: Quant::Made,
        filter: Filter::default(),
    };
    mark(&mut made, pile);
    let mut rest = vec![looped];
    for mut e in then {
        e.walk(&mut Repoint {
            ids: &ids,
            to: u8::try_from(pile).unwrap_or(u8::MAX),
        });
        rest.push(e);
    }
    es.push(Effect::With {
        bind: vec![made],
        then: rest,
    });
}

/// Whether every reference `x` makes to a binder of `ids` is a plain
/// `Bound` (never its place, the rest of it, or one of several).
fn only_bound<T: Walk + Clone>(x: &T, ids: &BTreeSet<usize>) -> bool {
    struct Only<'a>(&'a BTreeSet<usize>, bool);
    impl Visitor for Only<'_> {
        fn enter(&mut self, o: &mut Objects) -> bool {
            let id = |i: &u8| self.0.contains(&usize::from(*i));
            self.1 &= match &o.quant {
                Quant::Introduced(i) | Quant::Rest { of: i } => !id(i),
                Quant::Bounds(ps) => !ps.iter().any(|p| matches!(p, Pointer::Bound(i) if id(i))),
                _ => true,
            };
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut s = Only(ids, true);
    x.clone().walk(&mut s);
    s.1
}

/// Points every `Bound` reference to a binder of `ids` at `to`.
struct Repoint<'a> {
    ids: &'a BTreeSet<usize>,
    to: u8,
}

impl Visitor for Repoint<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if let Quant::Bound(i) = o.quant {
            if self.ids.contains(&usize::from(i)) {
                o.quant = Quant::Bound(self.to);
            }
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// A trigger's own list of effects, if `e` is a trigger the ability sets
/// up, and the parts of it said before that list (its event or
/// condition).
fn trigger_list(e: &mut Effect) -> Option<(&mut Vec<Effect>, Vec<Condition>, Option<Event>)> {
    Some(match e {
        Effect::WhenYouDo(then) => (then, Vec::new(), None),
        Effect::WhenYouDoIf { cond, then } => (then, vec![cond.clone()], None),
        Effect::WhenThisWay { event, then } => (then, Vec::new(), Some(event.clone())),
        Effect::Delayed { then, .. } => (then, Vec::new(), None),
        Effect::DelayedWhen { event, then }
        | Effect::DelayedWhenever { event, then }
        | Effect::DelayedNext { event, then, .. } => (then, Vec::new(), Some(event.clone())),
        // Not a trigger, but each pass of "repeat this process" finds its
        // own objects as a trigger's resolution does ([repeat-process]).
        Effect::Process(then) => (then, Vec::new(), None),
        _ => return None,
    })
}

/// "When you do, target opponent exiles the top five cards of their
/// library. You may cast up to two spells from among the exiled cards"
/// (Doom Reigns Supreme), "Until end of turn, whenever a creature you
/// control dies, exile the top card of your library. You may play it …"
/// (Waltz of Rage): what a reflexive or delayed trigger finds, exiles or
/// chooses comes to exist only as that trigger resolves (CR 603.12,
/// 603.7), so a `With` whose binders only such a trigger says goes
/// inside it (through a `During` around it), from the effect that says
/// them first ([when-you-do-binds]).
fn into_reflexive(es: &mut Vec<Effect>) {
    let Some(Effect::With { then, .. }) = es.last_mut() else {
        return;
    };
    into_reflexive(then);
    let Some(Effect::With { bind, then }) = es.last() else {
        unreachable!("matched above");
    };
    let ids = bind_ids(bind);
    if ids.len() != bind.len() {
        return;
    }
    // A token the ability made before the trigger exists already
    // (Ratonhnhaké꞉ton, Iroh, Tea Master); one the trigger makes is made
    // each time it resolves.
    if bind.iter().any(|o| o.quant == Quant::Made) {
        made_in_trigger(es, &ids);
        return;
    }
    // The path to the innermost trigger whose list alone says them.
    let one = |es: &[Effect]| -> Option<usize> {
        let v: Vec<usize> = (0..es.len())
            .filter(|&i| says(&es[i], &ids, false))
            .collect();
        match v[..] {
            [i] => Some(i),
            _ => None,
        }
    };
    let mut path: Vec<usize> = Vec::new();
    let mut deepest = 0;
    let mut cur: Vec<Effect> = then.clone();
    while let Some(i) = one(&cur) {
        path.push(i);
        let mut e = cur[i].clone();
        if let Effect::During { then, .. } = e {
            cur = then;
            continue;
        }
        let Some((list, conds, event)) = trigger_list(&mut e) else {
            break;
        };
        if conds.iter().any(|c| says(c, &ids, false))
            || event.is_some_and(|ev| says(&ev, &ids, false))
        {
            break;
        }
        deepest = path.len();
        cur = std::mem::take(list);
    }
    if deepest == 0 {
        return;
    }
    path.truncate(deepest);
    let Some(Effect::With { bind, mut then }) = es.pop() else {
        unreachable!("matched above");
    };
    let mut list: &mut Vec<Effect> = &mut then;
    for &i in &path {
        let e = &mut list[i];
        list = match e {
            Effect::During { then, .. } => then,
            e => trigger_list(e).expect("on the path").0,
        };
    }
    let k = list
        .iter()
        .position(|e| says(e, &ids, false))
        .expect("checked above");
    let rest = list.split_off(k);
    list.push(Effect::With { bind, then: rest });
    es.extend(then);
}

/// "Until end of turn, whenever a creature you control deals combat damage
/// to a player, copy the exiled card. You may cast the copy" (Surge to
/// Victory), "Whenever you attack this turn, create two … tokens …
/// Sacrifice them at the beginning of the next end step" (Dalkovan
/// Encampment): what a trigger the ability sets up makes, it makes each
/// time it resolves (CR 603.7, 603.12), so the `Made` binder of the one
/// effect in that trigger's list that makes something goes inside it,
/// from that effect on ([delayed-binds-event]). Only along a path of lone
/// `During`s and triggers, whose events and conditions don't say it.
fn made_in_trigger(es: &mut Vec<Effect>, ids: &BTreeSet<usize>) {
    let Some(Effect::With { then, .. }) = es.last() else {
        return;
    };
    let makes = |e: &Effect| crate::backref::made(e).is_some();
    let mut path: Vec<usize> = Vec::new();
    let mut cur: Vec<Effect> = then.clone();
    let k = loop {
        let mk: Vec<usize> = (0..cur.len()).filter(|&i| makes(&cur[i])).collect();
        if !path.is_empty() && !mk.is_empty() {
            match mk[..] {
                [k] if !cur[..k].iter().any(|e| says(e, ids, false)) => break k,
                _ => return,
            }
        }
        if cur.len() != 1 || !mk.is_empty() {
            return;
        }
        let mut e = cur.pop().expect("one");
        if let Effect::During { then, .. } = e {
            path.push(0);
            cur = then;
            continue;
        }
        let Some((list, conds, event)) = trigger_list(&mut e) else {
            return;
        };
        if conds.iter().any(|c| says(c, ids, false))
            || event.is_some_and(|ev| says(&ev, ids, false))
        {
            return;
        }
        path.push(0);
        cur = std::mem::take(list);
    };
    let Some(Effect::With { bind, mut then }) = es.pop() else {
        unreachable!("matched above");
    };
    let mut list: &mut Vec<Effect> = &mut then;
    for _ in &path {
        list = match &mut list[0] {
            Effect::During { then, .. } => then,
            e => trigger_list(e).expect("on the path").0,
        };
    }
    let rest = list.split_off(k);
    list.push(Effect::With { bind, then: rest });
    es.extend(then);
}

/// A branch of `e` that happens only on some resolutions: a mode, an
/// option taken or declined, an "if" whose condition is true. Its list of
/// effects, and whether `e` says anything outside its lists.
fn branches(e: &mut Effect) -> Vec<&mut Vec<Effect>> {
    match e {
        Effect::Modal { modes, .. } => modes.iter_mut().collect(),
        Effect::May { then, .. } | Effect::IfYouDo(then) | Effect::IfYouDont(then) => vec![then],
        Effect::If { then, .. } => vec![then],
        _ => Vec::new(),
    }
}

/// Whether `e` says one of `ids` outside its branches (an `If`'s
/// condition).
fn says_outside_branches(e: &Effect, ids: &BTreeSet<usize>) -> bool {
    match e {
        Effect::If { cond, .. } => says(cond, ids, false),
        _ => false,
    }
}

/// Where a binder of `into_branch` goes: the (effect, branch) steps to its
/// branch, the effect there it is bound from, and the binders.
type BranchMove = (Vec<(usize, usize)>, usize, Vec<usize>);

/// "Choose one — … • Target opponent reveals their hand. You choose a
/// noncreature, nonland card from it. That player discards that card"
/// (Mardu Charm), "you may pay {2}{B}. If you do, return a creature card at
/// random from your graveyard to the battlefield. It gains flying …"
/// (Kheru Lich Lord): an object that only one mode, or only the option
/// taken or the case an "if" names, says comes to exist only where that
/// branch happens, so its binder is bound inside the branch, from the
/// effect that says it first ([branch-binds]). A binder every mode or both
/// sides say stays where it is. Each binder of a `With` goes on its own.
fn into_branch(es: &mut Vec<Effect>) {
    let Some(Effect::With { then, .. }) = es.last_mut() else {
        return;
    };
    into_branch(then);
    let Some(Effect::With { bind, then }) = es.last() else {
        unreachable!("matched above");
    };
    let mut moves = branch_moves(bind, then);
    if moves.is_empty() {
        return;
    }
    let Some(Effect::With { bind, mut then }) = es.pop() else {
        unreachable!("matched above");
    };
    let moved: BTreeSet<usize> = moves.iter().flat_map(|(_, _, ks)| ks.clone()).collect();
    // Innermost and latest first, so an earlier split leaves a later
    // path's indices where they were.
    moves.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (path, first, ks) in moves {
        let mut list: &mut Vec<Effect> = &mut then;
        for &(i, j) in &path {
            list = branches(&mut list[i]).swap_remove(j);
        }
        let rest = list.split_off(first);
        let bound = ks.iter().map(|&k| bind[k].clone()).collect();
        list.push(Effect::With {
            bind: bound,
            then: rest,
        });
    }
    let kept: Vec<Objects> = bind
        .into_iter()
        .enumerate()
        .filter(|(k, _)| !moved.contains(k))
        .map(|(_, o)| o)
        .collect();
    if kept.is_empty() {
        es.extend(then);
    } else {
        es.push(Effect::With { bind: kept, then });
    }
}

/// Which binders of `With { bind, then }` belong inside one branch of
/// `then` ([branch-binds]), and where each goes: what `into_branch` moves,
/// and what the `branch-binder-outside` lint finds left outside. `bind`'s
/// entries carry their temporary ids.
fn branch_moves(bind: &[Objects], then: &[Effect]) -> Vec<BranchMove> {
    if bind_ids(bind).len() != bind.len() {
        return Vec::new();
    }
    // The path of (effect, branch) steps to the innermost branch whose list
    // alone says `ids`, and in it the effect that says them first.
    let path_of = |ids: &BTreeSet<usize>, made: bool| -> Option<(Vec<(usize, usize)>, usize)> {
        let mut path = Vec::new();
        let mut cur: Vec<Effect> = then.to_vec();
        loop {
            let at: Vec<usize> = (0..cur.len())
                .filter(|&i| says(&cur[i], ids, false))
                .collect();
            let [i] = at[..] else { break };
            let mut e = cur[i].clone();
            if says_outside_branches(&e, ids) {
                break;
            }
            let lists = branches(&mut e);
            let inside: Vec<usize> = (0..lists.len())
                .filter(|&j| lists[j].iter().any(|x| says(x, ids, false)))
                .collect();
            let [j] = inside[..] else { break };
            path.push((i, j));
            cur = lists[j].clone();
        }
        if path.is_empty() {
            return None;
        }
        // A token a branch makes is made by its one maker there.
        let first = if made {
            let mk: Vec<usize> = (0..cur.len())
                .filter(|&k| crate::backref::made(&cur[k]).is_some())
                .collect();
            match mk[..] {
                [k] if !cur[..k].iter().any(|e| says(e, ids, false)) => k,
                _ => return None,
            }
        } else {
            cur.iter().position(|e| says(e, ids, false))?
        };
        Some((path, first))
    };
    // "reveals X cards … and you choose one of them" (Hollow Specter): a
    // binder another binder's words say goes with that one, so the two
    // move as one group or stay together ("Exile the top three cards …
    // Choose one of them" inside one mode, You Find Some Prisoners).
    let mut group: Vec<usize> = (0..bind.len()).collect();
    fn root(g: &mut [usize], mut x: usize) -> usize {
        while g[x] != x {
            x = g[x];
        }
        x
    }
    for k in 0..bind.len() {
        let ids = bind_ids(std::slice::from_ref(&bind[k]));
        for (j, b) in bind.iter().enumerate() {
            if j != k && says(b, &ids, false) {
                let (a, b) = (root(&mut group, j), root(&mut group, k));
                group[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for k in 0..bind.len() {
        let r = root(&mut group, k);
        groups.entry(r).or_default().push(k);
    }
    // Where binders `ks` go together, if anywhere: a token the list around
    // the branch makes exists before it, and what a "may" or an "if" says
    // it made ("If you do, you may put a creature card … from among them",
    // Call to the Kindred) may be what the list before it looked at: only a
    // mode, which nothing outside it can have made, takes a made binder.
    // A reflexive trigger's target is chosen as the trigger is put on the
    // stack (CR 603.12, 601.2c), before any "may" in it is answered
    // (Halo Forager), so it never moves into a branch.
    let place = |ks: &[usize]| -> Option<(Vec<(usize, usize)>, usize)> {
        let members: Vec<Objects> = ks.iter().map(|&k| bind[k].clone()).collect();
        if members.iter().any(crate::backref::targeted) {
            return None;
        }
        let made = members.iter().any(|o| o.quant == Quant::Made);
        let (path, first) = path_of(&bind_ids(&members), made)?;
        let only_mode = matches!(then[path[0].0], Effect::Modal { .. })
            && !then
                .iter()
                .enumerate()
                .any(|(i, e)| i != path[0].0 && crate::backref::made(e).is_some());
        (!made || only_mode).then_some((path, first))
    };
    // A group that one branch alone says moves whole; otherwise each
    // binder no other binder's words say may still move on its own.
    let mut moves: Vec<BranchMove> = Vec::new();
    for ks in groups.into_values() {
        let mut each: Vec<BranchMove> = Vec::new();
        match place(&ks).filter(|_| ks.len() > 1) {
            Some((path, first)) => each.push((path, first, ks)),
            None => {
                for k in ks {
                    let ids = bind_ids(std::slice::from_ref(&bind[k]));
                    let said_by_another = bind
                        .iter()
                        .enumerate()
                        .any(|(j, b)| j != k && says(b, &ids, false));
                    if let Some((path, first)) = place(&[k]).filter(|_| !said_by_another) {
                        each.push((path, first, vec![k]));
                    }
                }
            }
        }
        for (path, first, ks) in each {
            match moves.iter_mut().find(|(p, f, _)| *p == path && *f == first) {
                Some((_, _, v)) => {
                    v.extend(ks);
                    v.sort_unstable();
                }
                None => moves.push((path, first, ks)),
            }
        }
    }
    moves
}

/// How many `With`s of a bound tree hold a binder that only one mode or
/// branch inside them says ([branch-binds]): the `With`s `into_branch`
/// would still split. The tree is read back into temporary ids, each
/// binder numbered in walk order, so the decision is `into_branch`'s own.
/// Every `With` is asked, also those `into_branch` never visits (inside a
/// trigger or a branch), so what that pass misses is found too.
pub fn branch_binders_outside(a: &Ability) -> usize {
    if !has_binders(a) {
        return 0;
    }
    let b = bindings(a);
    let mut a = a.clone();
    a.walk(&mut Renumber {
        decl: 0,
        binders: b
            .binders
            .iter()
            .enumerate()
            .map(|(k, info)| (info.decl, k))
            .collect(),
        refs: &b.refs,
    });
    struct Outside(usize);
    impl Visitor for Outside {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
        fn effect(&mut self, e: &mut Effect) {
            if let Effect::With { bind, then } = e {
                if !branch_moves(bind, then).is_empty() {
                    self.0 += 1;
                }
            }
        }
    }
    let mut v = Outside(0);
    a.walk(&mut v);
    v.0
}

/// A bound tree back in the form the binder passes read it in: each
/// binder marked with its number, every reference to it naming that number.
struct Renumber<'a> {
    decl: usize,
    /// By the binder's object in walk order: its number.
    binders: BTreeMap<usize, usize>,
    refs: &'a BTreeMap<usize, Vec<Option<usize>>>,
}

impl Visitor for Renumber<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let d = self.decl;
        self.decl += 1;
        if let Some(&k) = self.binders.get(&d) {
            mark(o, k);
        }
        if let Some(named) = self.refs.get(&d) {
            let id = |n: Option<usize>| n.and_then(|k| u8::try_from(k).ok()).unwrap_or(u8::MAX);
            match &mut o.quant {
                Quant::Bound(i) | Quant::Introduced(i) | Quant::Rest { of: i } => {
                    *i = id(named.first().copied().flatten())
                }
                Quant::Bounds(ps) => {
                    let mut named = named.iter().copied();
                    for p in ps.iter_mut() {
                        if let Pointer::Bound(i) = p {
                            *i = id(named.next().flatten());
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}
/// Runs `into_loops`, `into_reflexive` and `into_branch` on every ability's own list of
/// effects.
struct IntoLoops {
    fresh: usize,
}

impl Visitor for IntoLoops {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, a: &mut Ability) -> bool {
        if let Some(es) = effects_of(a) {
            into_loops(es, &mut self.fresh);
            into_reflexive(es);
            into_branch(es);
        }
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
    /// A loop variable over a set another binder holds ("For each of
    /// those cards"), by its place in `stack`: that binder's id.
    over: BTreeMap<usize, usize>,
    /// Whether each open site is a `ForEach`'s.
    loops: Vec<bool>,
    /// The binder the loop's `each` being walked names, if it names one.
    each_over: Option<usize>,
    /// Whether each open site is a delayed trigger still reading its event.
    events: Vec<bool>,
    /// Conditions and amounts open, which a delayed trigger's event binds
    /// nothing in.
    nest: usize,
}

impl Scope {
    fn open(&mut self, n: usize) {
        self.sites.push((n, self.depth, 0));
        self.loops.push(false);
        self.events.push(false);
    }
    fn close(&mut self) {
        self.loops.pop();
        self.events.pop();
        if let Some((_, _, pushed)) = self.sites.pop() {
            for _ in 0..pushed {
                self.stack.pop();
                self.over.remove(&self.stack.len());
            }
        }
    }
    /// "For each of those cards, pay 4 life or put the card on top of your
    /// library": inside a loop over a set another binder holds, a
    /// reference to that set is the loop's one card of it in turn
    /// ([each-of-those]).
    fn bound_index(&mut self, id: u8) -> u8 {
        let id = usize::from(id);
        for (i, (k, x)) in self.stack.iter().enumerate().rev().enumerate() {
            if *x == Some(id) || x.is_none() && self.over.get(&k) == Some(&id) {
                return u8::try_from(i).unwrap_or(u8::MAX);
            }
        }
        self.misses += 1;
        u8::MAX
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
        let each = self.loops.last() == Some(&true)
            && self
                .sites
                .last()
                .is_some_and(|s| s.0 > 0 && s.1 == self.depth);
        if each {
            self.each_over = match o.quant {
                Quant::Bound(i) => Some(usize::from(i)),
                _ => None,
            };
        }
        match &mut o.quant {
            Quant::Bound(i) => *i = self.bound_index(*i),
            Quant::Introduced(i) => *i = self.index(*i),
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
        if self.events.last() == Some(&true) && self.nest == 0 {
            if let Some(site) = self.sites.last_mut() {
                if site.1 == self.depth && delayed_binds(o) {
                    site.2 += 1;
                    let id = take_mark(o);
                    self.stack.push(id);
                    if let Some(id) = id {
                        self.ordinal.insert(id, self.count);
                    }
                    self.count += 1;
                }
            }
            return;
        }
        if let Some(site) = self.sites.last_mut() {
            if site.1 == self.depth && site.0 > 0 {
                site.0 -= 1;
                site.2 += 1;
                let id = take_mark(o);
                if id.is_none() && self.loops.last() == Some(&true) {
                    if let Some(over) = self.each_over.take() {
                        self.over.insert(self.stack.len(), over);
                    }
                }
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
        // The event is read before the trigger's first effect.
        if let Some(ev) = self.events.last_mut() {
            *ev = false;
        }
        match e {
            Effect::With { bind, .. } => self.open(bind.len()),
            Effect::ForEach { .. } => {
                self.open(1);
                *self.loops.last_mut().expect("opened") = true;
            }
            e if delayed(e) => {
                self.open(0);
                *self.events.last_mut().expect("opened") = true;
            }
            _ => self.open(0),
        }
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.close();
    }
    fn condition(&mut self, _: &mut Condition) {
        self.nest += 1;
    }
    fn condition_done(&mut self, _: &mut Condition) {
        self.nest -= 1;
    }
    fn amount(&mut self, _: &mut Amount) {
        self.nest += 1;
    }
    fn amount_done(&mut self, _: &mut Amount) {
        self.nest -= 1;
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
    /// A delayed trigger's event's object ([delayed-binds-event]).
    Delayed,
    /// A one-shot replacement's event's object ([replacement-binds-event]).
    Replaced,
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
    /// As `Scope`'s.
    events: Vec<bool>,
    nest: usize,
}

impl Read {
    fn open(&mut self, k: SiteKind, n: usize) {
        self.sites.push((k, n, self.depth, 0));
        self.events.push(false);
    }
    fn close(&mut self) {
        self.events.pop();
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
        let event = self.events.last() == Some(&true) && self.nest == 0;
        if self
            .sites
            .last()
            .is_some_and(|s| s.2 == self.depth && (s.1 > 0 && !event || event && delayed_binds(o)))
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
        let event = self.events.last() == Some(&true) && self.nest == 0;
        if let Some(site) = self.sites.last_mut() {
            let binds = if event {
                site.2 == depth && delayed_binds(o)
            } else {
                site.2 == depth && site.1 > 0
            };
            if binds {
                if !event {
                    site.1 -= 1;
                }
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
        if let Some(ev) = self.events.last_mut() {
            *ev = false;
        }
        match e {
            Effect::With { bind, .. } => self.open(SiteKind::With, bind.len()),
            Effect::ForEach { .. } => self.open(SiteKind::Each, 1),
            Effect::Replacement { .. } => {
                self.open(SiteKind::Replaced, 0);
                *self.events.last_mut().expect("opened") = true;
            }
            e if delayed(e) => {
                self.open(SiteKind::Delayed, 0);
                *self.events.last_mut().expect("opened") = true;
            }
            _ => self.open(SiteKind::With, 0),
        }
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.close();
    }
    fn condition(&mut self, _: &mut Condition) {
        self.nest += 1;
    }
    fn condition_done(&mut self, _: &mut Condition) {
        self.nest -= 1;
    }
    fn amount(&mut self, _: &mut Amount) {
        self.nest += 1;
    }
    fn amount_done(&mut self, _: &mut Amount) {
        self.nest -= 1;
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
        events: Vec::new(),
        nest: 0,
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
    a.walk(&mut Subst {
        decl: 0,
        subst,
        performers: Vec::new(),
        digs: Vec::new(),
    });
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
    performers: Vec<Option<Player>>,
    digs: Vec<bool>,
}

impl Visitor for Subst {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let d = self.decl;
        self.decl += 1;
        if let Some(mut w) = self.subst.remove(&d) {
            // Said again by each of the players, of their own library.
            if let [.., Some(each), _] = &self.performers[..] {
                undistribute(&mut w, each);
            }
            if self.digs.last() == Some(&true) {
                undig_each(&mut w);
            }
            *o = w;
            return false;
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect(&mut self, e: &mut Effect) {
        self.performers.push(performer(e));
        self.digs.push(digs_each(e));
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.performers.pop();
        self.digs.pop();
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
    fn effect(&mut self, e: &mut Effect) {
        if let Effect::ForEach { then, .. }
        | Effect::WhenYouDo(then)
        | Effect::WhenYouDoIf { then, .. }
        | Effect::WhenThisWay { then, .. }
        | Effect::Delayed { then, .. }
        | Effect::DelayedWhen { then, .. }
        | Effect::DelayedWhenever { then, .. }
        | Effect::DelayedNext { then, .. }
        | Effect::Process(then)
        | Effect::May { then, .. }
        | Effect::IfYouDo(then)
        | Effect::IfYouDont(then)
        | Effect::If { then, .. } = e
        {
            splice(then);
        }
        if let Effect::Modal { modes, .. } = e {
            modes.iter_mut().for_each(splice);
        }
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
    if info.site == SiteKind::Delayed {
        return "trigger".into();
    }
    if info.site == SiteKind::Replaced {
        return "event".into();
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
            while b.binders.get(bi).is_some_and(|x| {
                matches!(
                    x.site,
                    SiteKind::Each | SiteKind::Delayed | SiteKind::Replaced
                )
            }) {
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
