//! CR 613: what every object on the battlefield is, after every continuous
//! effect and counter.
//!
//! Within a layer the effects apply one at a time, each to what the effects
//! before it left, and after each the order of the rest is decided again
//! (CR 613.8c). That fold is Rust: as one Datalog program, an effect that
//! removes what a later effect's filter reads is negation through
//! recursion, which ascent can't stratify. What applies next is ascent:
//! dependency (CR 613.8a), loops (CR 613.8b) and timestamps (CR 613.7).

use std::collections::{BTreeMap, BTreeSet};

use ascent::aggregators::min;
use ascent::ascent_run;

use crate::model::{
    Ability, CardType, Chars, Condition, Counter, Layer, ObjectId, Op, Seen, State, StaticAbility,
    StaticId, Subtype, Timestamp,
};

/// A continuous effect: a static ability of one object (CR 611.3), or what
/// a resolved spell or ability left (CR 611.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Static(ObjectId, StaticId),
    Resolved(usize),
}

/// Every object's characteristics after the layers. A noncreature has no
/// power and toughness, whatever is printed on it (CR 208.3).
pub fn characteristics(state: &State) -> BTreeMap<ObjectId, Chars> {
    let mut run = Run {
        state,
        now: state
            .objects
            .iter()
            .map(|(id, o)| (*id, o.printed.clone()))
            .collect(),
        started: BTreeMap::new(),
    };
    for layer in Layer::ALL {
        let mut pending = run.candidates(layer);
        while let Some(next) = run.next(layer, &pending) {
            pending.remove(&next);
            run.apply(layer, next);
        }
        if layer == Layer::Type {
            run.intrinsic();
        }
        if layer == Layer::ModifyPt {
            run.counters();
        }
    }
    for c in run.now.values_mut() {
        if !c.is(CardType::Creature) {
            c.pt = None;
        }
    }
    run.now
}

struct Run<'a> {
    state: &'a State,
    now: BTreeMap<ObjectId, Chars>,
    /// Each effect that has begun to apply, and the set it applies to from
    /// then on, in every later layer, even if the ability generating it is
    /// removed (CR 613.6).
    started: BTreeMap<Effect, BTreeSet<ObjectId>>,
}

impl Run<'_> {
    fn ops(&self, e: Effect) -> &[Op] {
        match e {
            Effect::Static(_, s) => &self.state.statics[&s].ops,
            Effect::Resolved(i) => &self.state.resolved[i].ops,
        }
    }

    fn has_part_in(&self, e: Effect, layer: Layer) -> bool {
        self.ops(e).iter().any(|op| op.layer() == layer)
    }

    fn timestamp(&self, e: Effect) -> Timestamp {
        match e {
            // CR 613.7a, without abilities granted by a later effect.
            Effect::Static(o, _) => self.state.objects[&o].timestamp,
            Effect::Resolved(i) => self.state.resolved[i].timestamp,
        }
    }

    /// The effects that may apply in this layer: those already applying,
    /// every resolved one, and the static abilities objects have now.
    fn candidates(&self, layer: Layer) -> BTreeSet<Effect> {
        let statics = self.now.iter().flat_map(|(o, c)| {
            c.abilities.iter().filter_map(move |a| match a {
                Ability::Static(s) => Some(Effect::Static(*o, *s)),
                Ability::Keyword(_) | Ability::Unread(_) | Ability::Intrinsic(_) => None,
            })
        });
        let resolved = (0..self.state.resolved.len()).map(Effect::Resolved);
        statics
            .chain(resolved)
            .chain(self.started.keys().copied())
            .filter(|e| self.has_part_in(*e, layer))
            .collect()
    }

    /// A static ability's effect exists while its object has the ability
    /// and its condition holds, read from what the layers have made of
    /// the battlefield so far (CR 611.3a, 613).
    fn exists(&self, e: Effect, now: &BTreeMap<ObjectId, Chars>) -> bool {
        match e {
            Effect::Static(o, s) => {
                now[&o].abilities.contains(&Ability::Static(s))
                    && self.state.statics[&s]
                        .condition
                        .as_ref()
                        .is_none_or(|c| self.holds(c, o, now))
            }
            Effect::Resolved(_) => true,
        }
    }

    /// "As long as you control a Mountain": counted among the permanents
    /// the source's controller controls.
    fn holds(&self, cond: &Condition, source: ObjectId, now: &BTreeMap<ObjectId, Chars>) -> bool {
        let source_chars = &now[&source];
        let cond = match cond {
            Condition::YouControl(c) => c,
            Condition::YourTurn => return self.state.active == Some(source_chars.controller),
        };
        let n = now
            .iter()
            .filter(|(id, c)| {
                c.controller == source_chars.controller
                    && cond.any_of.iter().any(|f| {
                        f.matches(&Seen {
                            id: **id,
                            chars: c,
                            source,
                            source_chars,
                            source_attached_to: self.state.objects[&source].attached_to,
                            token: self.state.objects[*id].token,
                            attacking: self.state.attacking.contains(*id),
                        })
                    })
            })
            .count();
        cond.count.holds(n as u32)
    }

    fn matches(
        &self,
        ability: &StaticAbility,
        source: ObjectId,
        id: ObjectId,
        chars: &Chars,
        source_chars: &Chars,
    ) -> bool {
        ability.filter.matches(&Seen {
            id,
            chars,
            source,
            source_chars,
            source_attached_to: self.state.objects[&source].attached_to,
            token: self.state.objects[&id].token,
            attacking: self.state.attacking.contains(&id),
        })
    }

    /// What the effect applies to: the set it began with, or for one that
    /// hasn't begun, what its filter matches now.
    /// An object that has left the battlefield is gone, and a new object
    /// is never in a set fixed before it arrived (CR 400.7, 611.2c).
    fn affects(&self, e: Effect, now: &BTreeMap<ObjectId, Chars>) -> BTreeSet<ObjectId> {
        let here = |set: &BTreeSet<ObjectId>| -> BTreeSet<ObjectId> {
            set.iter()
                .copied()
                .filter(|o| now.contains_key(o))
                .collect()
        };
        if let Some(set) = self.started.get(&e) {
            return here(set);
        }
        if !self.exists(e, now) {
            return BTreeSet::new();
        }
        match e {
            Effect::Resolved(i) => here(&self.state.resolved[i].affects),
            Effect::Static(source, s) => now
                .iter()
                .filter(|(id, c)| {
                    self.matches(&self.state.statics[&s], source, **id, c, &now[&source])
                })
                .map(|(id, _)| *id)
                .collect(),
        }
    }

    /// The effect's parts in this layer applied alone to what it affects
    /// now: the objects it would change, as they would be.
    fn alone(&self, layer: Layer, e: Effect) -> BTreeMap<ObjectId, Chars> {
        self.affects(e, &self.now)
            .into_iter()
            .filter_map(|o| {
                let mut c = self.now[&o].clone();
                for op in self.ops(e).iter().filter(|op| op.layer() == layer) {
                    op.apply(&mut c, &self.state.objects[&o].printed);
                }
                (c != self.now[&o]).then_some((o, c))
            })
            .collect()
    }

    /// CR 613.8a, for an effect `b` that hasn't begun: would applying `a`
    /// change `b`'s existence, or whether `b` applies to `o`? A resolved
    /// effect's set was fixed as it began, so only a static can depend.
    fn depends(&self, b: Effect, o: ObjectId, after_a: &Chars) -> bool {
        let Effect::Static(source, s) = b else {
            return false;
        };
        let before = &self.now[&o];
        if o == source
            && before.abilities.contains(&Ability::Static(s))
            && !after_a.abilities.contains(&Ability::Static(s))
        {
            return true;
        }
        let ability = &self.state.statics[&s];
        if let Some(cond) = &ability.condition {
            let mut after = self.now.clone();
            after.insert(o, after_a.clone());
            if self.holds(cond, source, &self.now) != self.holds(cond, source, &after) {
                return true;
            }
        }
        let source_after = if o == source {
            after_a
        } else {
            &self.now[&source]
        };
        self.matches(ability, source, o, before, &self.now[&source])
            != self.matches(ability, source, o, after_a, source_after)
    }

    /// Which pending effect applies next.
    fn next(&self, layer: Layer, pending: &BTreeSet<Effect>) -> Option<Effect> {
        if pending.is_empty() {
            return None;
        }
        let alone: BTreeMap<Effect, BTreeMap<ObjectId, Chars>> = pending
            .iter()
            .map(|e| match layer.has_dependencies() {
                true => (*e, self.alone(layer, *e)),
                false => (*e, BTreeMap::new()),
            })
            .collect();
        let changes: Vec<(Effect, ObjectId)> = alone
            .iter()
            .flat_map(|(e, objects)| objects.keys().map(move |o| (*e, *o)))
            .collect();
        let started = &self.started;
        let depends = |b: &Effect, a: &Effect, o: &ObjectId| self.depends(*b, *o, &alone[a][o]);
        let ordered = |e: &Effect| (self.timestamp(*e), *e);

        let program = ascent_run! {
            relation pending(Effect);
            relation unstarted(Effect);
            relation changes(Effect, ObjectId);
            relation dep(Effect, Effect);
            relation reach(Effect, Effect);
            relation waits(Effect);
            relation ready((Timestamp, Effect));
            relation first((Timestamp, Effect));

            pending(*e) <-- for e in pending.iter();
            unstarted(e) <-- pending(e), if !started.contains_key(e);
            changes(*a, *o) <-- for (a, o) in changes.iter();

            // CR 613.8a: b depends on a if applying a would change b's
            // existence or what b applies to. Both are in this layer.
            dep(b, a) <-- changes(a, o), unstarted(b), if a != b, if depends(b, a, o);

            reach(b, a) <-- dep(b, a);
            reach(c, a) <-- dep(c, b), reach(b, a);

            // CR 613.8b: a dependent effect waits for those it depends on,
            // unless they are in a loop, which applies in timestamp order.
            waits(b) <-- dep(b, a), !reach(a, b);

            ready(ordered(e)) <-- pending(e), !waits(e);
            first(m) <-- agg m = min(x) in ready(x);
        };
        program.first.into_iter().next().map(|((_, e),)| e)
    }

    fn apply(&mut self, layer: Layer, e: Effect) {
        let set = self.affects(e, &self.now);
        if !self.exists(e, &self.now) && !self.started.contains_key(&e) {
            return;
        }
        self.started.entry(e).or_insert_with(|| set.clone());
        let ops: Vec<Op> = self
            .ops(e)
            .iter()
            .filter(|op| op.layer() == layer)
            .cloned()
            .collect();
        for o in set {
            // An object that left the battlefield since the effect began
            // is gone, and a new object if it returns (CR 400.7).
            let Some(object) = self.state.objects.get(&o) else {
                continue;
            };
            let printed = &object.printed;
            if let Some(c) = self.now.get_mut(&o) {
                for op in &ops {
                    op.apply(c, printed);
                }
            }
        }
    }

    /// A land has a mana ability for each basic land type it has once
    /// layer 4 is done, and none for a type it lost (CR 305.6, 305.7).
    fn intrinsic(&mut self) {
        for c in self.now.values_mut() {
            c.abilities.retain(|a| !matches!(a, Ability::Intrinsic(_)));
            if !c.is(CardType::Land) {
                continue;
            }
            let types: Vec<_> = c
                .subtypes
                .iter()
                .filter_map(|s| match s {
                    Subtype::BasicLand(t) => Some(Ability::Intrinsic(*t)),
                    _ => None,
                })
                .collect();
            c.abilities.extend(types);
        }
    }

    /// Counters are layer 7c too (CR 613.4c), and adding commutes, so
    /// where they fall among 7c's effects doesn't matter.
    fn counters(&mut self) {
        for (id, o) in &self.state.objects {
            let (mut power, mut toughness) = (0, 0);
            for (k, n) in &o.counters {
                let (dp, dt) = match k {
                    Counter::PlusOne => (1, 1),
                    Counter::MinusOne => (-1, -1),
                    Counter::Pt(p, t) => (*p, *t),
                    Counter::Named(_) => (0, 0),
                };
                power += dp * *n as i32;
                toughness += dt * *n as i32;
            }
            if let Some(p) = self.now.get_mut(id).and_then(|c| c.pt.as_mut()) {
                p.power += power;
                p.toughness += toughness;
            }
        }
    }
}
