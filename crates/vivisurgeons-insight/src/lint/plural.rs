//! `plural-bound-property`: a property of one object read off a binder
//! that holds a set ([each-itself]).
//!
//! "Return those cards to the battlefield under their owner's control",
//! "The flashback cost is equal to its mana cost", "creatures can't attack
//! you unless their controller pays": the owner, the mana cost and the
//! controller are each object's own, `OwnerOf(Itself)`. Read off the
//! binder of the whole set they are one value for all of it, which no set
//! of several objects has. A `ForEach` binds one object of its set at a
//! time, so its binder is singular; a total ("the total power of those
//! creatures") is `Amount::Total`, no property of one object.

use crate::ast::*;
use crate::binders::{bindings, Bindings, SiteKind};
use crate::walk::{Visitor, Walk};

/// How many properties of one object a bound tree reads off a plural
/// binder.
pub fn count(a: &Ability) -> usize {
    find(&mut a.clone(), false)
}

/// Makes each property of one object read off a plural binder the
/// object's own, `Itself` ([each-itself]): what `count` finds, fixed where
/// the binders are made, so the lint counts only what this pass cannot
/// reach.
pub fn each_itself(a: &mut Ability) {
    find(a, true);
}

fn find(a: &mut Ability, fix: bool) -> usize {
    let b = bindings(a);
    if b.binders.is_empty() {
        return 0;
    }
    let mut v = Find {
        b: &b,
        decl: 0,
        property: false,
        those: false,
        fix,
        inside: 0,
        counters_on: None,
        effects: Vec::new(),
        n: 0,
    };
    a.walk(&mut v);
    v.n
}

struct Find<'a> {
    b: &'a Bindings,
    /// Objects met so far in walk order, as `bindings` counts them.
    decl: usize,
    /// The next object met is the one a property is read off.
    property: bool,
    /// The next player is inside `Player::Those`: "those players", the
    /// controllers of a set, which are several ([those-players]).
    those: bool,
    /// Rewrite each property found to `Itself`.
    fix: bool,
    /// How many objects the walk is inside: a property said in one's
    /// filter is no object an effect acts on.
    inside: usize,
    /// The depth of the object counters are counted on: "the number of
    /// counters on that creature" (Nils, Discipline Enforcer) reads that
    /// creature's counters, a property of one object said as a filter.
    counters_on: Option<usize>,
    /// Per effect walked into: whether it acts on each object of a set in
    /// turn.
    effects: Vec<bool>,
    n: usize,
}

/// A quantity that says several objects, so a binder of it holds a set.
fn plural(q: &Quant) -> bool {
    let many = |n: &Amount| *n != Amount::N(1);
    match q {
        Quant::Each
        | Quant::All
        | Quant::Plural
        | Quant::AnyNumber
        | Quant::OneOrMore
        | Quant::NFromEach(_)
        | Quant::AnyNumberOfTarget
        | Quant::AnyNumberOfOtherTarget
        | Quant::WithOthers(_)
        | Quant::SelfAnd(_)
        | Quant::WithAllOthers(_)
        | Quant::Rest { .. } => true,
        Quant::N(n) | Quant::UpTo(n) => many(n),
        Quant::Target { n, .. } | Quant::OtherTarget { n, .. } => many(n),
        _ => false,
    }
}

impl Visitor for Find<'_> {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let d = self.decl;
        self.decl += 1;
        let on = self.counters_on.take() == Some(self.inside);
        if matches!(o.filter.kind, Some(Noun::Counter(_)))
            && matches!(o.filter.posts.as_slice(), [Post::On(_)])
            && o.filter.alts.is_empty()
        {
            self.counters_on = Some(self.inside + 1);
        }
        if std::mem::take(&mut self.property) || on {
            if let Some([Some(k)]) = self.b.refs.get(&d).map(Vec::as_slice) {
                let info = &self.b.binders[*k];
                if info.site != SiteKind::Each && plural(&info.what.quant) {
                    self.n += 1;
                    // `Itself` is each object an effect acts on in turn, so
                    // only where one does: a `Move`'s "under their owner's
                    // control", a plural source's damage, or a static's
                    // or a continuous effect's own subject. In a filter it would be the object
                    // tested (Hum of the Radix's "each artifact its
                    // controller controls"), and a `GainLife` or a
                    // `Performed` has no object to take in turn (Seeds of
                    // Innocence, Trial of a Time Lord).
                    let outermost = self.inside == usize::from(on);
                    if self.fix && outermost && self.effects.last() != Some(&false) {
                        o.quant = Quant::Itself;
                    }
                }
            }
        }
        self.inside += 1;
        true
    }
    fn leave(&mut self, _: &mut Objects) {
        self.inside -= 1;
    }
    fn effect(&mut self, e: &mut Effect) {
        self.effects.push(matches!(
            e,
            Effect::Move { .. }
                | Effect::Damage { .. }
                | Effect::Modify { .. }
                | Effect::Restrict { .. }
                | Effect::KeywordCostIs { .. }
        ));
    }
    fn effect_done(&mut self, _: &mut Effect) {
        self.effects.pop();
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn amount(&mut self, a: &mut Amount) {
        self.property = matches!(
            a,
            Amount::PowerOf(_)
                | Amount::ToughnessOf(_)
                | Amount::ManaValueOf(_)
                | Amount::BasePowerOf(_)
                | Amount::XOf(_)
        );
    }
    fn cost(&mut self, c: &mut Cost) {
        self.property = matches!(c, Cost::ManaCostOf(_));
    }
    fn player(&mut self, p: &mut Player) {
        if std::mem::take(&mut self.those) {
            return;
        }
        self.those = matches!(p, Player::Those(_));
        self.property = matches!(p, Player::OwnerOf(_) | Player::ControllerOf(_));
    }
}
