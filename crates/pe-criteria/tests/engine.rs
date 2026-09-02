//! Engine tests using plain Rust closures as the evaluator.
//!
//! The JS runtime is deliberately behind a trait so this logic can be verified
//! without booting V8. A failure here is an engine bug; a failure in pe-js is a
//! bindings bug.

use std::convert::Infallible;

use pe_criteria::{Criterion, Evaluator, Grouping, GroupingError, PathView, RunError};

type Check = Box<dyn FnMut(&PathView<'_>) -> bool>;

struct Closures(Vec<Check>);

impl Evaluator for Closures {
    type Error = Infallible;
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<Vec<bool>, Infallible> {
        Ok(self.0.iter_mut().map(|f| f(view)).collect())
    }
}

fn q(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn reproduces_the_thirty_one_versus_thirty_nine_percent_story() {
    // Groups: [safe-arm only, connector only, everything else].
    let strict =
        Grouping::build(q(&["arm", "connector"]), [(0b01, 6), (0b10, 8), (0, 85)]).unwrap();
    let wide = Grouping::build(q(&["arm", "connector"]), [(0b01, 6), (0b10, 12), (0, 81)]).unwrap();

    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count(0, 0) >= 1 && v.count(0, 1) >= 1
    })]);

    let s = pe_criteria::run(&strict, &[11], 1, &mut ev).unwrap();
    let w = pe_criteria::run(&wide, &[11], 1, &mut ev).unwrap();

    assert!(
        (s[0].percent() - 31.0).abs() < 0.05,
        "strict {}",
        s[0].percent()
    );
    assert!(
        (w[0].percent() - 39.0).abs() < 0.05,
        "wide {}",
        w[0].percent()
    );
}

#[test]
fn a_card_can_satisfy_two_queries_at_once() {
    // The case that breaks naive inclusion-exclusion. Two cards are BOTH an
    // arming outlet and a connector, so one draw can satisfy both requirements.
    let overlapping = Grouping::build(
        q(&["arm", "connector"]),
        [(0b11, 2), (0b01, 4), (0b10, 6), (0, 87)],
    )
    .unwrap();
    // Same totals per query (6 arms, 8 connectors) but no card does both.
    let disjoint =
        Grouping::build(q(&["arm", "connector"]), [(0b01, 6), (0b10, 8), (0, 85)]).unwrap();

    assert_eq!(overlapping.matching_total(0), 6);
    assert_eq!(overlapping.matching_total(1), 8);
    assert_eq!(disjoint.matching_total(0), 6);
    assert_eq!(disjoint.matching_total(1), 8);
    assert_eq!(overlapping.population(), disjoint.population());

    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count(0, 0) >= 1 && v.count(0, 1) >= 1
    })]);
    let o = pe_criteria::run(&overlapping, &[11], 1, &mut ev).unwrap()[0];
    let d = pe_criteria::run(&disjoint, &[11], 1, &mut ev).unwrap()[0];

    // Dual-purpose cards make satisfying both requirements strictly likelier,
    // even though the per-query totals are identical.
    assert!(
        o.get() > d.get(),
        "overlapping {} vs disjoint {}",
        o.get(),
        d.get()
    );
}

#[test]
fn the_curve_out_criterion_across_turns() {
    // 36 lands, 10 one-mana dorks, on the play: land + dork on turn one, second
    // land by turn two.
    let g = Grouping::build(q(&["land", "dork"]), [(0b01, 36), (0b10, 10), (0, 53)]).unwrap();
    let mut ev = Closures(vec![
        Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1 && v.count(0, 1) >= 1 && v.count(1, 0) >= 2),
        Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1 && v.count(0, 1) >= 1),
    ]);
    let r = pe_criteria::run(&g, &[7, 1], 2, &mut ev).unwrap();

    assert!(r[0].get() > 0.0 && r[0].get() < 1.0);
    // Adding the turn-two land requirement can only make it harder.
    assert!(r[0].get() <= r[1].get(), "{} vs {}", r[0].get(), r[1].get());
}

#[test]
fn counts_are_cumulative_across_checkpoints() {
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        // A later checkpoint has seen everything an earlier one did.
        v.count(1, 0) >= v.count(0, 0) && v.count(2, 0) >= v.count(1, 0)
    })]);
    let r = pe_criteria::run(&g, &[7, 1, 1], 1, &mut ev).unwrap();
    assert!(
        (r[0].get() - 1.0).abs() < 1e-9,
        "should always hold, got {}",
        r[0].get()
    );
}

#[test]
fn out_of_range_lookups_are_false_not_panics() {
    // A criterion asking about turn 9 of a 1-checkpoint run: the caller is often
    // JavaScript, and this should be false rather than a crash.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(9, 0) >= 1)]);
    let r = pe_criteria::run(&g, &[7], 1, &mut ev).unwrap();
    assert_eq!(r[0].get(), 0.0);

    let g2 = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    assert_eq!(g2.matching_total(99), 0);
}

#[test]
fn impossibly_wide_questions_are_refused_not_hung() {
    let queries: Vec<String> = (0..20).map(|i| format!("q{i}")).collect();
    let cards: Vec<(u64, u32)> = (0..20).map(|i| (1u64 << i, 5)).collect();
    let g = Grouping::build(queries, cards).unwrap();
    let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
    let err = pe_criteria::run(&g, &[40], 1, &mut ev).unwrap_err();
    assert!(matches!(err, RunError::TooWide { .. }));
    // The message must tell you what to do about it.
    let msg = err.to_string();
    assert!(msg.contains("too wide"), "{msg}");
}

#[test]
fn the_query_limit_is_enforced() {
    let queries: Vec<String> = (0..65).map(|i| format!("q{i}")).collect();
    assert_eq!(
        Grouping::build(queries, []).unwrap_err(),
        GroupingError::TooManyQueries(65)
    );
}

#[test]
fn criterion_carries_its_threshold() {
    let c = Criterion {
        name: "t1 dork".into(),
        at_least: Some(0.55),
    };
    assert_eq!(c.at_least, Some(0.55));
}

#[test]
fn an_empty_library_is_refused_rather_than_enumerated() {
    // Every card in the list was a commander or outside the deck. This used to
    // underflow the bin count and spin the estimator for u128::MAX iterations.
    let g = Grouping::build(q(&["land"]), []).unwrap();
    let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
    let err = pe_criteria::run(&g, &[7], 1, &mut ev).unwrap_err();
    assert!(matches!(err, RunError::EmptyLibrary), "{err}");
}

#[test]
fn a_hand_bigger_than_the_library_is_refused_rather_than_answered_zero() {
    // Enumeration yields no paths at all here, so every criterion would collect
    // zero mass and report a confident 0%.
    let g = Grouping::build(q(&["land"]), [(0b1, 1), (0, 1)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1)]);
    let err = pe_criteria::run(&g, &[7], 1, &mut ev).unwrap_err();
    assert!(
        matches!(
            err,
            RunError::NotEnoughCards {
                population: 2,
                draws: 7
            }
        ),
        "{err}"
    );
}
