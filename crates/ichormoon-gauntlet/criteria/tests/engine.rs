//! Engine tests using plain Rust closures as the evaluator.
//!
//! The JS runtime is deliberately behind a trait so this logic can be verified
//! without booting V8. A failure here is an engine bug; a failure in pe-js is a
//! bindings bug.

use std::convert::Infallible;

use gauntlet_criteria::{
    Activation, Answering, Board, Chosen, Conditionals, Discard, DiscardPolicy, Discards, Mill,
    MillDepth, Objective, Resolves, Table, ToHand,
};
use gauntlet_criteria::{
    Bound, CastingPolicy, Cost, Count, Counted, Criterion, Delay, Effect, Evaluator, Expectation,
    Fetch, Fetched, Grouping, GroupingError, Keep, LandDetail, LandDropPolicy, ManaSource,
    MulliganPolicy, Palette, PathOutcomes, PathView, Plan, Policies, Route, RunError, Schedule,
    Trigger, Zone, MAX_COUNT,
};
use std::sync::Arc;

type Check = Box<dyn FnMut(&PathView<'_>) -> bool + Send>;
type Tally = Box<dyn FnMut(&PathView<'_>) -> u32 + Send>;

struct Closures(Vec<Check>);

impl Evaluator for Closures {
    type Error = Infallible;
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<PathOutcomes, Infallible> {
        Ok(PathOutcomes {
            held: self.0.iter_mut().map(|f| f(view)).collect(),
            counted: Vec::new(),
        })
    }
}

/// The expectation half: closures answering *how many* rather than *whether*.
struct Counters(Vec<Tally>);

impl Evaluator for Counters {
    type Error = Infallible;
    fn evaluate(&mut self, view: &PathView<'_>) -> Result<PathOutcomes, Infallible> {
        Ok(PathOutcomes {
            held: Vec::new(),
            counted: self
                .0
                .iter_mut()
                .map(|f| Count::new(f(view)).expect("test tallies stay in range"))
                .collect(),
        })
    }
}

fn only_criteria(n: usize) -> Plan {
    Plan {
        criteria: n,
        expectations: 0,
    }
}

fn only_expectations(n: usize) -> Plan {
    Plan {
        criteria: 0,
        expectations: n,
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
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
            && v.count_at(0, 1, Counted::In(Zone::Hand)) >= 1
    })]);

    let s = gauntlet_criteria::run(&strict, &Schedule::plain(&[11]), only_criteria(1), &mut ev)
        .unwrap()
        .probabilities;
    let w = gauntlet_criteria::run(&wide, &Schedule::plain(&[11]), only_criteria(1), &mut ev)
        .unwrap()
        .probabilities;

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
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
            && v.count_at(0, 1, Counted::In(Zone::Hand)) >= 1
    })]);
    let o = gauntlet_criteria::run(
        &overlapping,
        &Schedule::plain(&[11]),
        only_criteria(1),
        &mut ev,
    )
    .unwrap()
    .probabilities[0];
    let d = gauntlet_criteria::run(
        &disjoint,
        &Schedule::plain(&[11]),
        only_criteria(1),
        &mut ev,
    )
    .unwrap()
    .probabilities[0];

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
        Box::new(|v: &PathView<'_>| {
            v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
                && v.count_at(0, 1, Counted::In(Zone::Hand)) >= 1
                && v.count_at(1, 0, Counted::In(Zone::Hand)) >= 2
        }),
        Box::new(|v: &PathView<'_>| {
            v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
                && v.count_at(0, 1, Counted::In(Zone::Hand)) >= 1
        }),
    ]);
    let r = gauntlet_criteria::run(&g, &Schedule::plain(&[7, 1]), only_criteria(2), &mut ev)
        .unwrap()
        .probabilities;

    assert!(r[0].get() > 0.0 && r[0].get() < 1.0);
    // Adding the turn-two land requirement can only make it harder.
    assert!(r[0].get() <= r[1].get(), "{} vs {}", r[0].get(), r[1].get());
}

#[test]
fn counts_are_cumulative_across_checkpoints() {
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        // A later checkpoint has seen everything an earlier one did.
        v.count_at(1, 0, Counted::In(Zone::Hand)) >= v.count_at(0, 0, Counted::In(Zone::Hand))
            && v.count_at(2, 0, Counted::In(Zone::Hand))
                >= v.count_at(1, 0, Counted::In(Zone::Hand))
    })]);
    let r = gauntlet_criteria::run(&g, &Schedule::plain(&[7, 1, 1]), only_criteria(1), &mut ev)
        .unwrap()
        .probabilities;
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
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(9, 0, Counted::In(Zone::Hand)) >= 1
    })]);
    let r = gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev)
        .unwrap()
        .probabilities;
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
    let err =
        gauntlet_criteria::run(&g, &Schedule::plain(&[40]), only_criteria(1), &mut ev).unwrap_err();
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
        at_most: None,
    };
    assert_eq!(c.at_least, Some(0.55));
}

#[test]
fn a_criterion_names_the_end_of_its_range_that_was_missed() {
    let range = Criterion {
        name: "keepable, not flooded".into(),
        at_least: Some(0.40),
        at_most: Some(0.60),
    };
    assert!(range.asserts());
    assert_eq!(range.missed(0.39), Some(Bound::AtLeast));
    assert_eq!(range.missed(0.40), None, "a bound is inclusive");
    assert_eq!(range.missed(0.60), None, "at both ends");
    assert_eq!(range.missed(0.61), Some(Bound::AtMost));

    let ceiling = Criterion {
        name: "all-land opener".into(),
        at_least: None,
        at_most: Some(0.05),
    };
    assert_eq!(ceiling.missed(0.0), None);
    assert_eq!(ceiling.missed(0.06), Some(Bound::AtMost));

    let informational = Criterion {
        name: "any ramp".into(),
        at_least: None,
        at_most: None,
    };
    assert!(!informational.asserts());
    assert_eq!(informational.missed(0.0), None);
    assert_eq!(informational.missed(1.0), None);
}

#[test]
fn an_empty_library_is_refused_rather_than_enumerated() {
    // Every card in the list was a commander or outside the deck. This used to
    // underflow the bin count and spin the estimator for u128::MAX iterations.
    let g = Grouping::build(q(&["land"]), []).unwrap();
    let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
    let err =
        gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev).unwrap_err();
    assert!(matches!(err, RunError::EmptyLibrary), "{err}");
}

#[test]
fn a_hand_bigger_than_the_library_is_refused_rather_than_answered_zero() {
    // Enumeration yields no paths at all here, so every criterion would collect
    // zero mass and report a confident 0%.
    let g = Grouping::build(q(&["land"]), [(0b1, 1), (0, 1)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
    })]);
    let err =
        gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev).unwrap_err();
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

#[test]
fn every_path_of_a_run_is_accounted_for() {
    // A criterion that is true everywhere collects the whole enumeration, so
    // its answer is the total probability mass the run checks internally. If
    // this is not 1, some region of the sample space was visited twice or not
    // at all, and every other criterion's answer came from the same enumeration.
    type Shape = (&'static [(u64, u32)], &'static [u32]);
    let shapes: [Shape; 5] = [
        (&[(0b1, 36), (0, 63)], &[7]),
        (&[(0b1, 36), (0b10, 10), (0, 53)], &[7, 1, 1]),
        (&[(0b1, 36), (0b10, 10), (0, 53)], &[7, 2, 3]),
        (&[(0b11, 2), (0b01, 4), (0b10, 6), (0, 87)], &[7, 1, 1, 1]),
        (
            &[(0b1, 20), (0b10, 20), (0b100, 20), (0, 39)],
            &[7, 1, 1, 1, 1, 1],
        ),
    ];
    for (cards, gaps) in shapes {
        let g = Grouping::build(q(&["a", "b", "c"]), cards.iter().copied()).unwrap();
        let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
        let r = gauntlet_criteria::run(&g, &Schedule::plain(gaps), only_criteria(1), &mut ev)
            .unwrap()
            .probabilities;
        assert!(
            (r[0].get() - 1.0).abs() < 1e-12,
            "{cards:?} over gaps {gaps:?} summed to {}",
            r[0].get()
        );
    }
}

#[test]
fn losing_probability_mass_is_refused_rather_than_reported() {
    // There is no input that provokes this today — the guards above catch the
    // known ways to enumerate nothing. It exists for the ways nobody has found
    // yet, so what matters is that it refuses rather than warns, and that the
    // message says the numbers would have been wrong.
    let err: RunError<Infallible> = RunError::MassNotOne { total: 0.9993 };
    let msg = err.to_string();
    assert!(msg.contains("0.9993"), "{msg}");
    assert!(msg.contains("mass"), "{msg}");
    assert!(msg.contains("wrong"), "{msg}");
}

// --- Expectations ---------------------------------------------------------

#[test]
fn an_expectation_reproduces_the_closed_form_mean() {
    // The genuine oracle for this: the mean of a hypergeometric is
    // draws * successes / population, in closed form, and `chip_stats::mean`
    // computes it without enumerating anything. An enumerated expectation walks
    // every composition instead and weights each by its exact probability, so
    // the two share nothing but the answer.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Counters(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand))
    })]);
    let r =
        gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_expectations(1), &mut ev).unwrap();

    let enumerated = r.distributions[0].mean();
    let closed = chip_stats::mean(99, 36, 7);
    assert!(
        (enumerated - closed).abs() < 1e-12,
        "enumerated {enumerated} vs closed form {closed}"
    );
    // And the documented constant the shuffler acceptance test uses.
    assert!((enumerated - 2.545455).abs() < 1e-6, "{enumerated}");
}

#[test]
fn the_distribution_is_the_hypergeometric_bucket_for_bucket() {
    // The mean is one number and could agree by accident. P(exactly k) for every
    // k is the whole answer, and it is the half that a mean cannot show: 2.55
    // lands on average says nothing about how often you keep a one-lander.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Counters(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand))
    })]);
    let r =
        gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_expectations(1), &mut ev).unwrap();
    let d = &r.distributions[0];

    assert_eq!(
        d.probabilities().len(),
        8,
        "seven cards drawn, so 0 through 7"
    );
    for k in 0..=7u32 {
        let want = chip_stats::pmf(99, 36, 7, k);
        let got = d.probabilities()[k as usize];
        assert!((got - want).abs() < 1e-12, "k={k}: {got} vs {want}");
    }
    assert!((d.total() - 1.0).abs() < 1e-12, "summed to {}", d.total());
    assert!((d.sd() - 1.2331509).abs() < 1e-6, "sd was {}", d.sd());
}

#[test]
fn the_distribution_sums_to_one_across_shapes() {
    // A distribution that does not sum to 1 has lost or double-counted a region
    // of the sample space, and every bucket in it is then drawn from the wrong
    // denominator -- the same failure `MassNotOne` guards for probabilities.
    type Shape = (&'static [(u64, u32)], &'static [u32]);
    let shapes: [Shape; 4] = [
        (&[(0b1, 36), (0, 63)], &[7]),
        (&[(0b1, 36), (0b10, 10), (0, 53)], &[7, 1, 1]),
        (&[(0b11, 2), (0b01, 4), (0b10, 6), (0, 87)], &[7, 1, 1, 1]),
        (&[(0b1, 4), (0, 6)], &[10]),
    ];
    for (cards, gaps) in shapes {
        let g = Grouping::build(q(&["a", "b", "c"]), cards.iter().copied()).unwrap();
        let mut ev = Counters(vec![
            Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand))),
            Box::new(|v: &PathView<'_>| {
                v.count_at(0, 0, Counted::In(Zone::Hand))
                    + v.count_at(0, 1, Counted::In(Zone::Hand))
            }),
        ]);
        let r = gauntlet_criteria::run(&g, &Schedule::plain(gaps), only_expectations(2), &mut ev)
            .unwrap();
        for (i, d) in r.distributions.iter().enumerate() {
            assert!(
                (d.total() - 1.0).abs() < 1e-12,
                "{cards:?} over {gaps:?}, expectation {i} summed to {}",
                d.total()
            );
        }
    }
}

#[test]
fn a_criterion_is_the_tail_of_the_expectation_beside_it() {
    // The two kinds come out of one walk over one enumeration, so they cannot be
    // allowed to disagree about the same question asked twice. "At least two
    // lands" must equal the mass this distribution puts at two and above.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();

    let mut counting = Counters(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand))
    })]);
    let d = gauntlet_criteria::run(
        &g,
        &Schedule::plain(&[7]),
        only_expectations(1),
        &mut counting,
    )
    .unwrap();
    let tail: f64 = d.distributions[0].probabilities()[2..].iter().sum();

    let mut checking = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 2
    })]);
    let p = gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut checking)
        .unwrap();

    assert!(
        (tail - p.probabilities[0].get()).abs() < 1e-12,
        "distribution tail {tail} vs criterion {}",
        p.probabilities[0].get()
    );
}

#[test]
fn a_value_with_nowhere_to_go_is_refused_rather_than_bucketed() {
    // Everything `count()` can produce is representable. Everything else is an
    // expectation computing something other than a count, and rounding it to fit
    // a bucket would answer a question nobody asked without saying so.
    for bad in [1.5, -1.0, f64::NAN, f64::INFINITY, 1025.0] {
        let err = Count::from_f64(bad).unwrap_err();
        assert!(
            err.to_string().contains("countable"),
            "{bad} should be refused by name: {err}"
        );
    }
    for good in [0.0, 1.0, 99.0, f64::from(MAX_COUNT)] {
        assert_eq!(Count::from_f64(good).unwrap().get(), good as u32);
    }
    assert!(Count::new(MAX_COUNT + 1).is_err());
}

#[test]
fn an_evaluator_that_answers_the_wrong_shape_is_refused() {
    // The report pairs names to answers by position. An evaluator returning a
    // different number of them would file every question under its neighbour's
    // title, which reads as a wrong number rather than as a bug.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
    let err =
        gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(2), &mut ev).unwrap_err();
    assert!(
        matches!(
            err,
            RunError::WrongShape {
                held: 1,
                counted: 0,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn an_expectation_carries_only_its_name() {
    // Asserted so that adding a threshold field is a deliberate act rather than
    // a plausible-looking convenience: `atLeast` on a criterion is a threshold
    // on a probability, and the same word here would be a threshold in whatever
    // units this expectation happens to count.
    let e = Expectation {
        name: "lands in opener".into(),
    };
    assert_eq!(e.name, "lands in opener");
}

// --- Effects --------------------------------------------------------------

/// Five Loam, five surveil lands and twenty cards that are neither. Bit 0 is
/// the Loam, bit 1 is *the effect applies to this card* — already resolved,
/// which is how the engine always receives it.
///
/// Thirty cards rather than ten because a run with a look slot a turn turns
/// over roughly twice as many as one without, and a deck that runs out is
/// refused rather than answered.
fn surveil_deck() -> Grouping {
    Grouping::build(q(&["loam", "<effect>"]), [(0b01, 5), (0b10, 5), (0, 20)]).unwrap()
}

fn surveil(route: Route) -> Effect {
    Effect {
        matched_by: 1,
        look: 1,
        trigger: Trigger::LandDrop,
        route,
        fetch: None,
        delay: None,
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    }
}

#[test]
fn a_look_that_routes_nothing_moves_nothing() {
    // The default destination is a no-op, and it has to be exactly a no-op: a
    // card left on top is the card the next draw takes, so looking at it and
    // not moving it is indistinguishable from never having looked. If this ever
    // stops holding, the standard library — which ships with no destinations at
    // all — starts changing numbers nobody asked it to change.
    let g = surveil_deck();
    let tally = || {
        Counters(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(3, 0, Counted::In(Zone::Hand))
        })])
    };
    let plain = gauntlet_criteria::run(
        &g,
        &Schedule::build(3, true, Vec::new(), Policies::default()),
        only_expectations(1),
        &mut tally(),
    )
    .unwrap();
    let looking = gauntlet_criteria::run(
        &g,
        &Schedule::build(3, true, vec![surveil(Route::Nowhere)], Policies::default()),
        only_expectations(1),
        &mut tally(),
    )
    .unwrap();
    // Bucket for bucket, to a tolerance rather than bitwise: the looking run
    // enumerates twice as many checkpoints to reach the same answer, so the
    // same probability is assembled from a different number of terms. The two
    // agree to 1e-15, which is the log-gamma round trip and not the routing.
    let (plain, looking) = (
        plain.distributions[0].probabilities(),
        looking.distributions[0].probabilities(),
    );
    assert_eq!(plain.len(), looking.len());
    for (k, (a, b)) in plain.iter().zip(looking).enumerate() {
        assert!((a - b).abs() < 1e-12, "P(exactly {k}) was {a} then {b}");
    }
}

#[test]
fn one_land_drop_a_turn_caps_how_deep_a_turn_can_get() {
    // The reason the land-drop tier is bounded and the mana-gated tier is not.
    // Holding five surveil lands on turn one plays one of them, so by turn T at
    // most T cards can have been binned — whatever the deck, whatever the hand.
    // A model that fired once per copy held would put five in the yard on turn
    // one and the percentage would look entirely reasonable.
    let g = surveil_deck();
    for turn in 1..=3usize {
        let mut ev = Counters(vec![Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 0, Counted::In(Zone::Graveyard))
        })]);
        let r = gauntlet_criteria::run(
            &g,
            &Schedule::build(
                turn as u32,
                true,
                vec![surveil(Route::Everything)],
                Policies::default(),
            ),
            only_expectations(1),
            &mut ev,
        )
        .unwrap();
        let d = r.distributions[0].probabilities();
        for (binned, p) in d.iter().enumerate() {
            assert!(
                binned <= turn || *p == 0.0,
                "turn {turn} put {binned} in the yard with probability {p}"
            );
        }
        assert!(
            d.len() > 1 && d[1] > 0.0,
            "and it is not vacuously capped: {d:?}"
        );
    }
}

#[test]
fn a_routed_card_leaves_the_hand_and_the_library_for_the_yard() {
    // The three zones partition the deck on every path, which is the invariant
    // a routing bug breaks first: a card binned twice, or binned and still
    // counted as drawn, shows up here and nowhere else.
    let g = surveil_deck();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(3, 0, Counted::In(Zone::Hand))
            + v.count_at(3, 0, Counted::In(Zone::Graveyard))
            + v.count_at(3, 0, Counted::In(Zone::Library))
            == 5
    })]);
    let r = gauntlet_criteria::run(
        &g,
        &Schedule::build(
            3,
            true,
            vec![surveil(Route::Matching(0))],
            Policies::default(),
        ),
        only_criteria(1),
        &mut ev,
    )
    .unwrap();
    assert!(
        (r.probabilities[0].get() - 1.0).abs() < 1e-12,
        "the partition held on only {} of the mass",
        r.probabilities[0].get()
    );
}

// --- The mana gate (#10) --------------------------------------------------

/// A land that makes `letters` and is untapped on arrival.
fn untapped(letters: &str) -> ManaSource {
    ManaSource::Land {
        enters_tapped: false,
        produces: Palette::from_letters([letters]),
        lasts: None,
    }
}

fn tapped(letters: &str) -> ManaSource {
    ManaSource::Land {
        enters_tapped: true,
        produces: Palette::from_letters([letters]),
        lasts: None,
    }
}

/// One seven-card hand, played out over three turns that draw nothing.
///
/// The way to write a *hand* down rather than a deck: a seven-card library is
/// the whole opening hand, so there is one deal, every path is that deal, and a
/// probability here is a yes or a no. The turns still happen — a land drop a
/// turn is the thing under test — and nothing is drawn on them, which is the
/// one liberty taken and is what HANDS.md's hands assume anyway.
fn one_hand(cards: &[(u64, ManaSource, u32)]) -> (Grouping, Schedule) {
    assert_eq!(
        cards.iter().map(|(_, _, n)| n).sum::<u32>(),
        7,
        "a hand is seven cards"
    );
    (
        Grouping::with_mana(q(&["lands"]), cards.to_vec()).unwrap(),
        Schedule::plain(&[7, 0, 0, 0]),
    )
}

fn holds(grouping: &Grouping, schedule: &Schedule, check: Check) -> f64 {
    let mut ev = Closures(vec![check]);
    gauntlet_criteria::run(grouping, schedule, only_criteria(1), &mut ev)
        .unwrap()
        .probabilities[0]
        .get()
}

#[test]
fn land_drops_are_one_a_turn_and_do_not_bank() {
    // HANDS.md hand 4. Five Islands and two spells, held from turn 0: five
    // lands in hand on every turn, and one more of them in play on each.
    let (grouping, schedule) = one_hand(&[(0b1, untapped("U"), 5), (0b0, ManaSource::Spell, 2)]);
    for (turn, drawn, played) in [(1, 5, 1), (2, 5, 2), (3, 5, 3)] {
        assert_eq!(
            holds(
                &grouping,
                &schedule,
                Box::new(
                    move |v: &PathView<'_>| v.count_at(turn, 0, Counted::In(Zone::Hand)) == drawn
                        && v.count_at(turn, 0, Counted::In(Zone::Battlefield)) == played
                )
            ),
            1.0,
            "turn {turn}: {drawn} drawn and {played} in play, on every deal"
        );
    }
}

#[test]
fn one_dual_land_is_two_counts_and_one_mana() {
    // HANDS.md hands 6 and 7, which are the same test told twice. Both hands
    // hold "a white source" and "a blue source", because a Hallowed Fountain is
    // both. One of them pays {W}{U} and the other does not, and no arithmetic
    // over the two counts tells them apart.
    let cost = Cost::parse("{W}{U}").unwrap();
    let hand_six = one_hand(&[(0b1, untapped("WU"), 1), (0b0, ManaSource::Spell, 6)]);
    let hand_seven = one_hand(&[
        (0b1, untapped("WU"), 1),
        (0b1, untapped("U"), 1),
        (0b0, ManaSource::Spell, 5),
    ]);
    for (grouping, schedule) in [&hand_six, &hand_seven] {
        // Two land drops by turn 3 in hand seven, one in hand six — the counts
        // that a naive model would read, and they are not what differs.
        assert_eq!(
            holds(
                grouping,
                schedule,
                Box::new(|v: &PathView<'_>| v.count_at(3, 0, Counted::In(Zone::Hand)) >= 1)
            ),
            1.0
        );
    }
    let pays = |(grouping, schedule): &(Grouping, Schedule)| {
        let cost = cost.clone();
        holds(
            grouping,
            schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(3, &cost)),
        )
    };
    assert_eq!(pays(&hand_six), 0.0, "one Fountain cannot pay two pips");
    assert_eq!(
        pays(&hand_seven),
        1.0,
        "the Fountain pays {{W}}, the Island {{U}}"
    );
}

#[test]
fn a_land_that_enters_tapped_makes_no_mana_the_turn_it_arrives() {
    // The other half of hand 7, and the reason hand 12 differs by a whole turn:
    // a tapped land is in play and pays nothing until your next turn.
    let (grouping, schedule) = one_hand(&[(0b1, tapped("WU"), 2), (0b0, ManaSource::Spell, 5)]);
    let castable = |turn: usize, text: &str| {
        let cost = Cost::parse(text).unwrap();
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(turn, &cost)),
        )
    };
    assert_eq!(castable(1, "{W}"), 0.0, "the only land arrived tapped");
    assert_eq!(castable(2, "{W}"), 1.0, "it untapped");
    assert_eq!(
        castable(2, "{W}{U}"),
        0.0,
        "the second one arrived this turn, tapped"
    );
    assert_eq!(castable(3, "{W}{U}"), 1.0);
}

#[test]
fn a_free_spell_is_castable_with_no_lands_at_all() {
    // Nothing in the library is a land, and turn 0 has no land drop. {0} is
    // still payable, because it asks for nothing.
    let (grouping, schedule) = one_hand(&[(0b0, ManaSource::Spell, 7)]);
    let free = Cost::parse("{0}").unwrap();
    let one = Cost::parse("{1}").unwrap();
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(0, &free))
        ),
        1.0
    );
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(3, &one))
        ),
        0.0
    );
}

// --- The declared land drop (#54) -----------------------------------------

/// HANDS.md hand 12 as one hand: a surveil land that enters tapped, an Island
/// that does not, and a Lantern of Insight that costs `{1}`.
///
/// Bit 0 picks out the surveil land, bit 1 the Island, bit 2 either land —
/// which is the tier a policy always ends in, because a land nobody ranked is
/// still a land.
fn hand_twelve() -> Grouping {
    Grouping::with_mana(
        q(&["surveil", "untapped", "lands"]),
        vec![
            (0b101, tapped("UB"), 1),
            (0b110, untapped("U"), 1),
            (0b000, ManaSource::Spell, 5),
        ],
    )
    .unwrap()
}

#[test]
fn which_land_the_priority_plays_decides_the_turn_the_spell_is_castable() {
    // The hand holds both lands, so both policies play both of them — one on
    // turn 1 and the other on turn 2 — and the only difference is the order.
    // That order is a whole turn of Lantern of Insight, which is the
    // disagreement the refusal was standing in for.
    let grouping = hand_twelve();
    let one = Cost::parse("{1}").unwrap();
    let castable = |prefer: usize, turn: usize| {
        let schedule = Schedule::plain_with(
            &[7, 0, 0],
            Policies::land_drop(LandDropPolicy::new(vec![prefer], 2)),
        );
        let cost = one.clone();
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(turn, &cost)),
        )
    };
    assert_eq!(
        castable(0, 1),
        0.0,
        "surveil first: the only land in play arrived tapped"
    );
    assert_eq!(castable(0, 2), 1.0, "and it has untapped by turn 2");
    assert_eq!(
        castable(1, 1),
        1.0,
        "untapped first: the Island pays on turn 1"
    );
    // The same board read as a count rather than as a cost: the priority also
    // decides *which* land is on the battlefield on turn 1, which is the fact
    // an optimistic reading of the same hand cannot state.
    let in_play = |prefer: usize, query: usize| {
        let schedule = Schedule::plain_with(
            &[7, 0, 0],
            Policies::land_drop(LandDropPolicy::new(vec![prefer], 2)),
        );
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(1, query, Counted::In(Zone::Battlefield)) == 1
            }),
        )
    };
    assert_eq!(in_play(0, 0), 1.0, "surveil first plays the surveil land");
    assert_eq!(in_play(0, 1), 0.0);
    assert_eq!(in_play(1, 1), 1.0, "untapped first plays the Island");
    assert_eq!(in_play(1, 0), 0.0);
}

#[test]
fn a_land_the_priority_never_names_is_still_played() {
    // A priority list is a preference, not a whitelist. This one names only the
    // surveil land and the hand holds none, so every drop falls through to the
    // tier the file did not write — and declining a drop is not something a
    // list can be read as asking for.
    let grouping = hand_twelve();
    let schedule = Schedule::plain_with(
        &[7, 0, 0],
        Policies::land_drop(LandDropPolicy::new(vec![0], 2)),
    );
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(|v: &PathView<'_>| v.count_at(2, 2, Counted::In(Zone::Battlefield)) == 2)
        ),
        1.0,
        "both lands are played by turn 2, ranked or not"
    );
}

// --- Narrowing (#31) ------------------------------------------------------

/// A 99-card library whose lands split by what they make: the shape that makes
/// `can_cast` expensive, and the shape a criterion counting lands cannot see.
fn manabase() -> Grouping {
    Grouping::with_mana(
        q(&["t:land", "cat:Ramp"]),
        vec![
            (0b01, untapped("W"), 8),
            (0b01, untapped("U"), 8),
            (0b01, tapped("WU"), 6),
            (0b01, untapped("B"), 8),
            (0b01, tapped("BG"), 6),
            (0b10, ManaSource::Spell, 10),
            (0b00, ManaSource::Spell, 53),
        ],
    )
    .unwrap()
}

#[test]
fn counting_lands_does_not_pay_for_telling_them_apart() {
    // The group axis of #31, on the case that motivated it. Five mana profiles
    // is five groups a `can_cast` clause needs and a land count cannot see, so
    // dropping them has to be exactly free — not nearly free.
    let grouping = manabase();
    assert_eq!(grouping.group_sizes().len(), 7);
    let schedule = Schedule::plain(&[7, 1, 1]);
    let count = || {
        Box::new(|v: &PathView<'_>| {
            v.count_at(2, 0, Counted::In(Zone::Hand)) >= 4
                && v.count_at(2, 1, Counted::In(Zone::Hand)) >= 1
        }) as Check
    };

    let coarse = grouping.coarsened(0b11, LandDetail::Ignored);
    assert_eq!(
        coarse.group_sizes().len(),
        3,
        "lands, ramp and everything else"
    );
    assert_eq!(coarse.population(), grouping.population());

    let wide = holds(&grouping, &schedule, count());
    let narrow = holds(&coarse, &schedule, count());
    assert!(
        (wide - narrow).abs() < 1e-12,
        "{wide} un-narrowed, {narrow} narrowed"
    );
}

#[test]
fn a_cost_keeps_the_manabase_and_drops_the_queries() {
    // The other direction. A `can_cast` clause reads no query at all — what it
    // reads is what each land makes — so its class keeps the mana detail and
    // throws every query away, and still answers identically.
    let grouping = manabase();
    let schedule = Schedule::plain(&[7, 1, 1]);
    let cost = Cost::parse("{1}{W}{U}").unwrap();
    let cast = || {
        let cost = cost.clone();
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost)) as Check
    };

    let coarse = grouping.coarsened(0, LandDetail::Pips(Palette::ALL));
    assert!(
        coarse.group_sizes().len() < grouping.group_sizes().len(),
        "the queries merge and the profiles do not"
    );

    let wide = holds(&grouping, &schedule, cast());
    let narrow = holds(&coarse, &schedule, cast());
    assert!(
        (wide - narrow).abs() < 1e-12,
        "{wide} un-narrowed, {narrow} narrowed"
    );
}

#[test]
fn what_is_in_play_is_read_turn_by_turn_and_not_collapsed() {
    // The checkpoint axis has a floor. One land drop a turn is
    // use-it-or-lose-it, so five lands drawn by turn four are four lands in
    // play, and no total at turn four can say that — which is why a
    // battlefield clause asks for `PerTurn` and gets every turn up to its own.
    let grouping = manabase();
    let schedule = Schedule::plain(&[7, 1, 1]);
    let in_play = || {
        Box::new(|v: &PathView<'_>| v.count_at(2, 0, Counted::In(Zone::Battlefield)) >= 2) as Check
    };

    let per_turn = schedule.narrowed(&[2], gauntlet_criteria::Reading::PerTurn);
    assert_eq!(per_turn.gaps(), &[7, 1, 1]);
    let wide = holds(&grouping, &schedule, in_play());
    let narrow = holds(&grouping, &per_turn, in_play());
    assert!((wide - narrow).abs() < 1e-12, "{wide} vs {narrow}");

    // And the collapsed reading really would have answered something else, so
    // the distinction is load-bearing rather than defensive.
    let collapsed = schedule.narrowed(&[2], gauntlet_criteria::Reading::Cumulative);
    assert_eq!(collapsed.gaps(), &[0, 0, 9]);
    let wrong = holds(&grouping, &collapsed, in_play());
    assert!(
        (wrong - wide).abs() > 0.01,
        "collapsing the history would have moved this: {wrong} vs {wide}"
    );
}

#[test]
fn a_live_effect_keeps_every_checkpoint_whatever_it_is_asked() {
    // Narrowing less than a caller asked for is always sound; narrowing more
    // is the bug. Routing reads the order cards came off the top, so a
    // schedule with a live effect refuses to merge two draws into one even
    // where the class asking would have allowed it.
    let effects = vec![Effect {
        matched_by: 0,
        look: 1,
        trigger: Trigger::LandDrop,
        route: Route::Everything,
        fetch: None,
        delay: None,
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    }];
    let schedule = Schedule::build(2, false, effects, Policies::default());
    assert_eq!(schedule.gaps(), &[7, 0, 1, 1, 1]);
    assert_eq!(
        schedule
            .narrowed(&[2], gauntlet_criteria::Reading::Cumulative)
            .gaps(),
        &[7, 0, 1, 1, 1],
        "every checkpoint survives, because the order is the question"
    );
}

#[test]
fn draws_after_the_last_turn_anybody_asked_about_are_dropped() {
    let schedule = Schedule::plain(&[7, 1, 1, 1, 1]);
    assert_eq!(
        schedule
            .narrowed(&[2], gauntlet_criteria::Reading::PerTurn)
            .gaps(),
        &[7, 1, 1, 0, 0]
    );
    assert_eq!(
        schedule
            .narrowed(&[1, 3], gauntlet_criteria::Reading::Cumulative)
            .gaps(),
        &[0, 8, 0, 2, 0],
        "eight cards by turn one, ten by turn three, and nothing about turn two"
    );
}

// --- Restricting the palette to the pips a cost demands (#55) -------------

#[test]
fn a_cost_keeps_only_the_colours_it_demands() {
    // The narrowing of #55 on the case that motivated it. `{1}{U}` runs
    // Hall's condition over one pip kind, so a Plains, a Swamp and a
    // black-green tapland are three groups the question cannot tell apart:
    // each pays one generic and none pays the {U}. What survives is whether it
    // makes blue and whether it enters tapped.
    let grouping = manabase();
    let schedule = Schedule::plain(&[7, 1, 1]);
    let cost = Cost::parse("{1}{U}").unwrap();
    let cast = || {
        let cost = cost.clone();
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost)) as Check
    };

    let whole = grouping.coarsened(0, LandDetail::Pips(Palette::ALL));
    let demanded = grouping.coarsened(0, LandDetail::Pips(cost.demands()));
    assert_eq!(whole.group_sizes().len(), 6, "five profiles and the spells");
    assert_eq!(
        demanded.group_sizes().len(),
        5,
        "blue and not-blue, tapped and not, and the spells"
    );
    assert_eq!(demanded.population(), whole.population());

    let wide = holds(&whole, &schedule, cast());
    let narrow = holds(&demanded, &schedule, cast());
    // A narrowed enumeration sums a different set of terms to the same total —
    // fewer, larger ones — so the last bits of the log-gamma round trip land
    // elsewhere. Every figure this tool prints is rounded to six places, so a
    // difference this small cannot reach a report at all.
    assert!(
        (wide - narrow).abs() < 1e-12,
        "{wide} on the whole palette, {narrow} on the pips it demands"
    );
    assert!(wide > 0.0 && wide < 1.0, "and not a degenerate one: {wide}");

    // Two costs in one class join their demands, and the join is still
    // coarser than the manabase.
    let both = grouping.coarsened(
        0,
        LandDetail::Pips(cost.demands().union(Cost::parse("{B}").unwrap().demands())),
    );
    assert_eq!(both.group_sizes().len(), 6);
}

#[test]
fn a_cost_of_pure_generic_cannot_tell_any_two_lands_apart() {
    // `{2}` is paid by any two lands, so the whole manabase collapses to
    // tapped and untapped — but *not* into the spells, because only a land
    // arrives without being cast.
    let grouping = manabase();
    let schedule = Schedule::plain(&[7, 1, 1]);
    let cost = Cost::parse("{2}").unwrap();
    let cast = || {
        let cost = cost.clone();
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost)) as Check
    };
    let demanded = grouping.coarsened(0, LandDetail::Pips(cost.demands()));
    assert_eq!(demanded.group_sizes().len(), 3);
    let whole = holds(
        &grouping.coarsened(0, LandDetail::Pips(Palette::ALL)),
        &schedule,
        cast(),
    );
    assert!((whole - holds(&demanded, &schedule, cast())).abs() < 1e-12);
}

#[test]
fn a_declared_priority_is_not_allowed_to_merge_two_lands_it_ranks_apart() {
    // The negative control, and the reason #55 is not applied everywhere.
    //
    // The priority plays the first land it is holding, and the tie inside one
    // entry goes to the card the decklist names first. Rank `Plains, Island,
    // Swamp` and ask for `{U}`: the Plains and the Swamp are the same source
    // to that cost, so restricting the palette merges them — into a group
    // sitting where the *Plains* did. A hand holding an Island and a Swamp
    // then plays the Swamp where the file said Island, and the answer moves.
    //
    // Nothing here is a bug in the restriction. It is the restriction being
    // unsound in this run, which is why `narrow::Shared::picks_a_land` refuses
    // to apply it and the class keeps the whole palette instead.
    let grouping = Grouping::with_mana(
        q(&["t:land"]),
        vec![
            (0b1, untapped("W"), 2),
            (0b1, untapped("U"), 2),
            (0b1, untapped("B"), 2),
            (0b0, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    // One tier: every land, ranked by the order the decklist reached them.
    let schedule = Schedule::plain_with(
        &[3, 0],
        Policies::land_drop(LandDropPolicy::new(Vec::new(), 0)),
    );
    let cost = Cost::parse("{U}").unwrap();
    let cast = || {
        let cost = cost.clone();
        Box::new(move |v: &PathView<'_>| v.can_cast(1, &cost)) as Check
    };

    let whole = holds(
        &grouping.coarsened(0b1, LandDetail::Pips(Palette::ALL)),
        &schedule,
        cast(),
    );
    let restricted = holds(
        &grouping.coarsened(0b1, LandDetail::Pips(cost.demands())),
        &schedule,
        cast(),
    );
    assert!(
        (whole - restricted).abs() > 0.01,
        "restricting the palette under a declared priority changes which land was played, \
         and this is the case that proves it: {whole} whole, {restricted} restricted"
    );
    // And the direction is the one the argument predicts: merging can only
    // hide the Island behind a group the priority reaches first.
    assert!(restricted < whole);
}

#[test]
fn two_lands_a_cost_cannot_tell_apart_arrive_the_same_way() {
    // The `drop_at` half of what #55 had to prove. Line three of the gate asks
    // whether a land of *this* group arrived this turn, and merging two groups
    // sums their counts — so the test has to stay equivalent. It does because
    // a hand only ever grows: the merged count rises exactly when one of its
    // members does.
    //
    // A Forest arriving on turn 2 pays the generic of `{1}{U}` beside an
    // Island already down, and the run must say so whether the Forest is its
    // own group or merged with the Plains.
    let grouping = Grouping::with_mana(
        q(&["t:land"]),
        vec![
            (0b1, untapped("U"), 1),
            (0b1, untapped("W"), 1),
            (0b1, untapped("G"), 1),
            (0b0, ManaSource::Spell, 4),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain(&[7, 0, 0]);
    let cost = Cost::parse("{1}{U}").unwrap();
    let cast = || {
        let cost = cost.clone();
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost)) as Check
    };
    let whole = grouping.coarsened(0b1, LandDetail::Pips(Palette::ALL));
    let demanded = grouping.coarsened(0b1, LandDetail::Pips(cost.demands()));
    assert_eq!(
        demanded.group_sizes().len(),
        3,
        "blue, not-blue, and spells"
    );
    assert_eq!(holds(&whole, &schedule, cast()), 1.0);
    assert_eq!(holds(&demanded, &schedule, cast()), 1.0);
}

// --- The declared budget (#10) --------------------------------------------

/// What `{U}` costs, as the budget carries it.
fn opt() -> ManaSource {
    ManaSource::Castable {
        cost: Cost::parse("{U}").unwrap().demand(),
        resolves: Resolves::OntoBattlefield,
    }
}

/// HANDS.md hands 1, 2 and 3 as one grouping each: a land of some kind and six
/// Opt, or seven Opt and no land at all.
///
/// Bit 0 picks out the Opts, which is the whole priority — the list is the
/// line, and a card it does not name is not cast.
fn six_opts(land: Option<ManaSource>) -> (Grouping, Schedule) {
    let mut cards = vec![(0b1, opt(), if land.is_some() { 6 } else { 7 })];
    cards.extend(land.map(|l| (0b0, l, 1)));
    (
        Grouping::with_mana(q(&["opts"]), cards).unwrap(),
        Schedule::plain_with(
            &[7, 0, 0, 0],
            Policies::casting(CastingPolicy::new(vec![0])),
        ),
    )
}

#[test]
fn one_island_and_six_opts_casts_one_opt() {
    // HANDS.md hand 1, which is the hand this whole half exists for. Six Opts
    // are in hand on turn 1 and one of them is cast, because the first one
    // spends the Island — so a model reading the hand count reports six times
    // the truth, and it reports it as a perfectly reasonable-looking number.
    let (grouping, schedule) = six_opts(Some(untapped("U")));
    let cast_by = |turn: usize, n: u32| {
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.count_at(turn, 0, Counted::Cast) == n),
        )
    };
    assert_eq!(cast_by(0, 0), 1.0, "turn 0 has played no land");
    assert_eq!(cast_by(1, 1), 1.0, "one Island, one Opt");
    // And the naive reading, on the same path: the cards are all there, and
    // holding them is what it is not.
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(|v: &PathView<'_>| v.count_at(1, 0, Counted::In(Zone::Hand)) == 5)
        ),
        1.0,
        "six drawn, one cast, five left holding"
    );
    // The turns do not bank and they do not compound: one land is one mana
    // every turn, so it is one more Opt every turn.
    assert_eq!(cast_by(2, 2), 1.0);
    assert_eq!(cast_by(3, 3), 1.0);
}

#[test]
fn a_tapland_casts_nothing_the_turn_it_arrives() {
    // HANDS.md hand 2: the same hand with one card changed, and the answer
    // changes from one Opt to none. This is the trade a filtering deck makes —
    // one more card seen, nothing cast — and it is the half of it the gate
    // could already see.
    let (grouping, schedule) = six_opts(Some(tapped("UB")));
    let cast_by = |turn: usize, n: u32| {
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.count_at(turn, 0, Counted::Cast) == n),
        )
    };
    assert_eq!(cast_by(1, 0), 1.0, "it entered tapped");
    assert_eq!(cast_by(2, 1), 1.0, "and it untapped");
}

#[test]
fn seven_opts_and_no_land_casts_nothing_ever() {
    // HANDS.md hand 3. Seven cantrips, and the hand does nothing whatever:
    // there is no mana and there never will be, because nothing here draws or
    // produces. The naive reading is seven cards deep.
    let (grouping, schedule) = six_opts(None);
    for turn in 0..=3 {
        assert_eq!(
            holds(
                &grouping,
                &schedule,
                Box::new(move |v: &PathView<'_>| v.count_at(turn, 0, Counted::Cast) == 0)
            ),
            1.0,
            "turn {turn}"
        );
    }
}

#[test]
fn a_budget_pays_for_the_spells_jointly_rather_than_one_at_a_time() {
    // The reason a bill is added rather than asked twice. One Hallowed
    // Fountain and one Island pay `{1}{W}` and they pay `{1}{U}`, and they do
    // not pay both — which is exactly what two independent `can_cast` answers
    // would have claimed, because each of them is true on its own.
    let white = ManaSource::Castable {
        cost: Cost::parse("{1}{W}").unwrap().demand(),
        resolves: Resolves::OntoBattlefield,
    };
    let blue = ManaSource::Castable {
        cost: Cost::parse("{1}{U}").unwrap().demand(),
        resolves: Resolves::OntoBattlefield,
    };
    let grouping = Grouping::with_mana(
        q(&["white spell", "blue spell"]),
        vec![
            (0b01, white, 1),
            (0b10, blue, 1),
            (0b00, untapped("WU"), 1),
            (0b00, untapped("U"), 1),
            (0b00, ManaSource::Spell, 3),
        ],
    )
    .unwrap();
    // Both spells are wanted and the file says which one first. Two lands pay
    // for one two-drop and leave nothing, so the answer is which one.
    let line = |prefer: Vec<usize>, turn: usize, first: u32, second: u32| {
        let schedule =
            Schedule::plain_with(&[7, 0, 0, 0], Policies::casting(CastingPolicy::new(prefer)));
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(turn, 0, Counted::Cast) == first
                    && v.count_at(turn, 1, Counted::Cast) == second
            }),
        )
    };
    assert_eq!(
        line(vec![0, 1], 2, 1, 0),
        1.0,
        "white first, and the pool is then empty"
    );
    assert_eq!(
        line(vec![1, 0], 2, 0, 1),
        1.0,
        "blue first, same board, other answer"
    );
    // And the one the line could not afford is cast a turn later, because a
    // budget is spent per turn rather than banked: the lands untap, and the
    // spell that lost the argument is still in hand to win it.
    assert_eq!(line(vec![0, 1], 3, 1, 1), 1.0);
}

#[test]
fn a_gate_beside_a_budget_asks_what_the_line_left() {
    // One pool, one accounting. The Island pays for the Opt, so `can_cast` on
    // the same turn is asking whether a second `{U}` exists — and it does not.
    // Answering it against the whole turn's lands would be two claimants on
    // one resource, which is the mistake the land drop taught us not to make.
    let (grouping, schedule) = six_opts(Some(untapped("U")));
    let cost = Cost::parse("{U}").unwrap();
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(1, &cost))
        ),
        0.0,
        "the Opt took the Island"
    );
    // Turn 2 has two lands and casts one more Opt, so there is still nothing
    // spare; turn 3 is three lands and three Opts cast in total.
    let two = Cost::parse("{U}").unwrap();
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(2, &two))
        ),
        0.0
    );
}

#[test]
fn the_budget_and_the_gate_answer_hand_twelve_the_same_way() {
    // HANDS.md hand 12, asked the other way round, and the two answers have to
    // be the same number: there is one Lantern of Insight and it costs `{1}`,
    // so *being able to cast it while holding it* and *casting it* are the same
    // event. The gate reads 62.05% on turn 2 under "surveil first" and the
    // budget has to agree to the digit.
    //
    // This is the check that caught the walk recording its board *after* the
    // budget had already spent from it — which read the previous path's lands
    // and cast spells off a board that hand never had. It showed up as a
    // criterion holding that cannot hold: a spell still in hand on a turn whose
    // mana could have paid for it.
    let grouping = Grouping::with_mana(
        q(&["surveil", "lantern", "t:land", "bolt"]),
        vec![
            (0b0101, tapped("UB"), 1),
            (0b0100, untapped("U"), 1),
            (
                0b0010,
                ManaSource::Castable {
                    cost: Cost::parse("{1}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b1000, ManaSource::Spell, 9),
        ],
    )
    .unwrap();
    let surveil = Effect {
        matched_by: 0,
        look: 1,
        trigger: Trigger::LandDrop,
        route: Route::Matching(3),
        fetch: None,
        delay: None,
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    };
    let land_drop = LandDropPolicy::new(vec![0], 2);
    let gate = Schedule::build(
        2,
        false,
        vec![surveil.clone()],
        Policies::land_drop(land_drop.clone()),
    );
    let budget = Schedule::build(
        2,
        false,
        vec![surveil],
        Policies {
            land_drop: Some(land_drop),
            casting: Some(CastingPolicy::new(vec![1])),
            ..Policies::default()
        },
    );
    let one = Cost::parse("{1}").unwrap();
    let castable = {
        let one = one.clone();
        holds(
            &grouping,
            &gate,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(2, 1, Counted::In(Zone::Hand)) >= 1 && v.can_cast(2, &one)
            }),
        )
    };
    let cast = holds(
        &grouping,
        &budget,
        Box::new(|v: &PathView<'_>| v.count_at(2, 1, Counted::Cast) >= 1),
    );
    assert!((castable - 0.6204545454545).abs() < 1e-9, "{castable}");
    assert!((cast - castable).abs() < 1e-12, "{cast} against {castable}");

    // And the one that cannot hold: a spell the turn's mana could still pay for
    // is not a spell you are holding, because the line already cast it.
    assert_eq!(
        holds(
            &grouping,
            &budget,
            Box::new(
                move |v: &PathView<'_>| v.count_at(2, 1, Counted::In(Zone::Hand)) >= 1
                    && v.can_cast(2, &one)
            )
        ),
        0.0
    );
}

// --- tutors -----------------------------------------------------------------

/// A seven-card library: one tutor costing `{U}`, one card it fetches, one
/// untapped blue source and four blanks. The opening hand is the whole
/// library, so every path is this deal and every probability is a yes or a no
/// — the same trick HANDS.md's hands are asserted with.
fn tutor_hand() -> Grouping {
    Grouping::with_mana(
        q(&["tutor", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b10, ManaSource::Spell, 1),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                    lasts: None,
                },
                1,
            ),
            (0b00, ManaSource::Spell, 4),
        ],
    )
    .unwrap()
}

fn tutor(to: Fetched) -> Effect {
    Effect {
        matched_by: 0,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: Some(Fetch {
            prefer: vec![1],
            to,
        }),
        delay: None,
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    }
}

#[test]
fn a_tutor_puts_the_card_it_names_in_your_hand() {
    // The whole feature, on a hand small enough to check by eye. The target is
    // *not* in the opening seven — the seven cards are the whole library minus
    // it, which cannot happen, so instead: the library is seven and the hand
    // is six, and the one card left out is the target. The tutor is cast on
    // turn 1 off the Island and goes and gets it.
    //
    // Two answers on one deal: without the tutor declared the target is in the
    // library and stays there; with it, it is in hand on turn 1.
    let g = tutor_hand();
    let held = || {
        Closures(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(1, 1, Counted::In(Zone::Hand)) >= 1
        })])
    };
    let line = || Policies::casting(CastingPolicy::new(vec![0]));
    // Six cards of seven, so exactly one card is missing from the hand and it
    // is the target on one deal in seven.
    let gaps = [6, 0];
    let without = gauntlet_criteria::run(
        &g,
        &Schedule::plain_with(&gaps, line()),
        only_criteria(1),
        &mut held(),
    )
    .unwrap()
    .probabilities[0]
        .get();
    let with = gauntlet_criteria::run(
        &g,
        &Schedule::plain_with_fetches(&gaps, vec![tutor(Fetched::Hand(1))], line()),
        only_criteria(1),
        &mut held(),
    )
    .unwrap()
    .probabilities[0]
        .get();
    // Drawn: six of seven cards, so the target is in hand unless it is the one
    // left out — six sevenths.
    assert!(
        (without - 6.0 / 7.0).abs() < 1e-12,
        "drawn alone was {without}"
    );
    // Fetched: the one deal that misses the target also holds the tutor and
    // the Island, because six of seven cards is everything else. So the tutor
    // covers exactly the case the draw missed, and it is certain.
    assert!((with - 1.0).abs() < 1e-12, "with the tutor it was {with}");
}

#[test]
fn a_tutor_takes_its_card_out_of_the_library() {
    // The other half, and the half that needed chip-stats to change. A fetched
    // card is gone from the library whether or not anything asks about the
    // hand, so the count that has to move is the library's.
    let g = tutor_hand();
    let left = || {
        Counters(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(1, 1, Counted::In(Zone::Library))
        })])
    };
    let line = || Policies::casting(CastingPolicy::new(vec![0]));
    let gaps = [6, 0];
    let without = gauntlet_criteria::run(
        &g,
        &Schedule::plain_with(&gaps, line()),
        only_expectations(1),
        &mut left(),
    )
    .unwrap()
    .distributions[0]
        .mean();
    let with = gauntlet_criteria::run(
        &g,
        &Schedule::plain_with_fetches(&gaps, vec![tutor(Fetched::Hand(1))], line()),
        only_expectations(1),
        &mut left(),
    )
    .unwrap()
    .distributions[0]
        .mean();
    // One seventh of deals leave it behind, and the tutor gets every one of
    // them, so nothing is left in the library at all.
    assert!((without - 1.0 / 7.0).abs() < 1e-12, "was {without}");
    assert!(with.abs() < 1e-12, "the tutor left {with} behind");
}

#[test]
fn a_tutor_that_finds_nothing_fetches_nothing() {
    // The total case. A priority naming a card the library no longer holds
    // takes nothing, rather than taking a card that is not there — which in a
    // subtraction is a population that grows.
    let g = Grouping::with_mana(
        q(&["tutor", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                2,
            ),
            (0b10, ManaSource::Spell, 1),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                    lasts: None,
                },
                2,
            ),
            (0b00, ManaSource::Spell, 2),
        ],
    )
    .unwrap();
    // Two tutors, one target, and the whole seven-card library in hand. Both
    // tutors resolve over two turns; the second finds nothing.
    let counted = || {
        Counters(vec![
            Box::new(|v: &PathView<'_>| v.count_at(2, 1, Counted::In(Zone::Hand))) as Tally,
            Box::new(|v: &PathView<'_>| v.count_at(2, 1, Counted::In(Zone::Library))),
        ])
    };
    let out = gauntlet_criteria::run(
        &g,
        &Schedule::plain_with_fetches(
            &[7, 0, 0],
            vec![tutor(Fetched::Hand(1))],
            Policies::casting(CastingPolicy::new(vec![0])),
        ),
        only_expectations(2),
        &mut counted(),
    )
    .unwrap();
    // The whole library is in hand, so the one target is too — and there is
    // nothing left for either tutor to find.
    assert!((out.distributions[0].mean() - 1.0).abs() < 1e-12);
    assert!(out.distributions[1].mean().abs() < 1e-12);
}

/// Buried Alive in an eight-card library: the sorcery at `{U}`, an Island,
/// `cids` creatures it searches for, and blanks to make up eight.
fn buried_alive_library(cids: u32) -> Grouping {
    Grouping::with_mana(
        q(&["buried alive", "cid"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                1,
            ),
            (0b10, ManaSource::Spell, cids),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                    lasts: None,
                },
                1,
            ),
            (0b00, ManaSource::Spell, 6 - cids),
        ],
    )
    .unwrap()
}

/// Cids in the graveyard and in the library on turn 1, with a three-card
/// opener and Buried Alive taking `up_to` of them.
fn buried_alive(cids: u32, up_to: u32) -> (f64, f64) {
    let effect = Effect {
        fetch: Some(Fetch {
            prefer: vec![1],
            to: Fetched::Graveyard(up_to),
        }),
        ..tutor(Fetched::Hand(1))
    };
    let out = gauntlet_criteria::run(
        &buried_alive_library(cids),
        &Schedule::plain_with_fetches(
            &[3, 0],
            vec![effect],
            Policies::casting(CastingPolicy::new(vec![0])),
        ),
        only_expectations(2),
        &mut Counters(vec![
            Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Graveyard))) as Tally,
            Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Library))),
        ]),
    )
    .unwrap();
    (out.distributions[0].mean(), out.distributions[1].mean())
}

#[test]
fn buried_alive_puts_up_to_three_cards_into_the_graveyard() {
    // Issue #137. Three cards of eight in the opener, and the spell is cast
    // on turn 1 only when it and the Island are two of them: 6 of the 56
    // openers, 3/28. The third card is the only other one in hand, so the
    // library holds three or four of the four Cids, and the search takes
    // three either way.
    let (yard, library) = buried_alive(4, 3);
    assert!((yard - 3.0 * 3.0 / 28.0).abs() < 1e-12, "yard was {yard}");
    // Without it, the three-card opener holds 3 × 4/8 of them on average and
    // the library the other 2.5; the search moves the yard's 9/28 out of it.
    assert!(
        (library - (2.5 - 9.0 / 28.0)).abs() < 1e-12,
        "library was {library}"
    );

    // Entomb is the same search for one card.
    let (yard, library) = buried_alive(4, 1);
    assert!((yard - 3.0 / 28.0).abs() < 1e-12, "yard was {yard}");
    assert!((library - (2.5 - 3.0 / 28.0)).abs() < 1e-12);

    // "Up to": with two Cids, a cast finds one where the third card in hand
    // is the other Cid (2 of the 6 openers that cast it) and two otherwise,
    // so 5/3 a cast and 6/56 × 5/3 = 5/28 overall. Never a third.
    let (yard, _) = buried_alive(2, 3);
    assert!((yard - 5.0 / 28.0).abs() < 1e-12, "yard was {yard}");
}

// --- delayed effects --------------------------------------------------------

/// Urza's Saga's third chapter, as an effect: set up by the land drop, two
/// turns later a fetch onto the battlefield, and the Saga sacrificed.
fn saga() -> Effect {
    Effect {
        matched_by: 3,
        look: 0,
        trigger: Trigger::LandDrop,
        route: Route::Nowhere,
        fetch: Some(Fetch {
            prefer: vec![1],
            to: Fetched::Battlefield,
        }),
        delay: Some(Delay {
            turns: 2,
            sacrifice: true,
        }),
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    }
}

#[test]
fn a_saga_fetches_on_its_third_chapter_and_its_mana_goes_with_it() {
    // Ten cards: Urza's Saga, Lantern of Insight, two Islands and six blanks.
    // The opening hand is nine of them and no turn draws, so every deal is the
    // whole library bar one card, and the deal that leaves the Lantern out is
    // the one where it is still in the library for chapter III to find: one
    // deal in ten, and on that deal every question below is a yes or a no.
    //
    // The line: the Saga is played on turn 1 because the priority says so,
    // an Island on turns 2 and 3. Chapter III resolves on turn 3, after the
    // draw and before that turn's land, so the Lantern arrives on turn 3 and
    // not a turn sooner. The Saga taps for {C} in response and is sacrificed,
    // so turn 3 has three mana and turn 4, with nothing new to play, has two.
    // Carrying its own three-turn lifetime, as the card data gives it, and
    // the effect's sacrifice has to end its mana once rather than twice.
    let saga_land = lasting("C", 3);
    let grouping = Grouping::with_mana(
        q(&["saga", "lantern", "land", "<effect saga>"]),
        vec![
            (0b1101, saga_land, 1),
            (0b0010, ManaSource::Spell, 1),
            (0b0100, untapped("U"), 2),
            (0b0000, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[9, 0, 0, 0, 0],
        vec![saga()],
        Policies::land_drop(LandDropPolicy::new(vec![0], 2)),
    );
    let tenth = 1.0 / 10.0;
    let left_out = |v: &PathView<'_>| v.count_at(0, 1, Counted::In(Zone::Hand)) == 0;
    let check = |f: Check| holds(&grouping, &schedule, f);

    // When it arrives. Every other deal has the Lantern in hand already, so
    // chapter III finds nothing and nothing arrives.
    for (turn, expected) in [(2, 0.0), (3, tenth), (4, tenth)] {
        let got = check(Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 1, Counted::In(Zone::Battlefield)) >= 1
        }));
        assert!(
            (got - expected).abs() < 1e-12,
            "Lantern on the battlefield on turn {turn}: {got}, not {expected}"
        );
    }
    // Where it came from: out of the library, and nowhere else.
    let still_there = check(Box::new(|v: &PathView<'_>| {
        v.count_at(3, 1, Counted::In(Zone::Library)) >= 1
    }));
    assert_eq!(still_there, 0.0, "a fetched Lantern is not in the library");

    // The Saga: in play on turn 2 on every deal that holds it, which is nine
    // in ten, and gone by the end of turn 3 on all of them.
    let saga_on = |turn: usize| {
        check(Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 0, Counted::In(Zone::Battlefield)) >= 1
        }))
    };
    assert!((saga_on(2) - 0.9).abs() < 1e-12, "{}", saga_on(2));
    assert_eq!(saga_on(3), 0.0, "sacrificed after chapter III");

    // Its mana is turn 3's and not turn 4's.
    let pays = |turn: usize, cost: &str| {
        let cost = Cost::parse(cost).unwrap();
        check(Box::new(move |v: &PathView<'_>| {
            left_out(v) && v.can_cast(turn, &cost)
        }))
    };
    assert!((pays(3, "{3}") - tenth).abs() < 1e-12, "{}", pays(3, "{3}"));
    assert_eq!(pays(4, "{3}"), 0.0, "the Saga's mana outlived it");
    assert!((pays(4, "{2}") - tenth).abs() < 1e-12, "{}", pays(4, "{2}"));
}

/// A land that makes `letters` for `turns` turns, counting the one it is
/// played on: Urza's Saga is three, because chapter III sacrifices it.
fn lasting(letters: &str, turns: u8) -> ManaSource {
    ManaSource::Land {
        enters_tapped: false,
        produces: Palette::from_letters([letters]),
        lasts: Some(turns),
    }
}

#[test]
fn a_land_that_makes_no_mana_is_a_land_drop_and_not_a_payer() {
    // Maze of Ith. It is played like any land, so it takes a drop and counts
    // as a land in play; it has no mana ability, so it pays for nothing, not
    // even generic. Written as a hand of Maze, one Island and five blanks.
    let (grouping, schedule) = one_hand(&[
        (0b1, lasting("", 0), 1),
        (0b1, untapped("U"), 1),
        (0b0, ManaSource::Spell, 5),
    ]);
    let check = |f: Check| holds(&grouping, &schedule, f);
    assert_eq!(
        check(Box::new(|v: &PathView<'_>| v.count_at(
            2,
            0,
            Counted::In(Zone::Battlefield)
        ) == 2)),
        1.0,
        "both lands are played"
    );
    let pays = |turn: usize, text: &str| {
        let cost = Cost::parse(text).unwrap();
        check(Box::new(move |v: &PathView<'_>| v.can_cast(turn, &cost)))
    };
    assert_eq!(pays(2, "{1}"), 1.0, "the Island pays generic");
    assert_eq!(pays(3, "{2}"), 0.0, "the Maze pays nothing");
}

#[test]
fn a_saga_with_no_effect_declared_still_stops_making_mana_after_chapter_three() {
    // HANDS.md hand 17's "what it costs", on a run that declares no Saga
    // effect — the Lantern north star's reading. Five cards, one a turn: the
    // Saga, a Bolt and three Islands, over turns 0 to 4.
    //
    // On the deal that opens with the Saga and draws the Bolt on turn 1, the
    // Islands arrive on turns 2, 3 and 4 and each takes the drop it arrives
    // on. The Saga then has only turn 1 to go down, and chapter III has
    // sacrificed it by turn 4: four mana on turn 4 is not there, though four
    // land drops were. Three is, from the Islands.
    let grouping = Grouping::with_mana(
        q(&["saga", "island", "bolt"]),
        vec![
            (0b001, lasting("C", 3), 1),
            (0b010, untapped("U"), 3),
            (0b100, ManaSource::Spell, 1),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain(&[1, 1, 1, 1, 1]);
    let this_deal = |v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) == 1
            && v.count_at(1, 2, Counted::In(Zone::Hand)) == 1
    };
    let pays = |turn: usize, text: &str| {
        let cost = Cost::parse(text).unwrap();
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| this_deal(v) && v.can_cast(turn, &cost)),
        )
    };
    let deal = 1.0 / 20.0;
    assert!((pays(4, "{3}") - deal).abs() < 1e-12, "{}", pays(4, "{3}"));
    assert_eq!(pays(4, "{4}"), 0.0, "the Saga's mana outlived it");
    // And on turn 3 it is still there: played on turn 1, sacrificed on 3,
    // tapped with chapter III on the stack.
    assert!((pays(3, "{3}") - deal).abs() < 1e-12, "{}", pays(3, "{3}"));

    // A deal where the Saga can wait for a drop nothing else needs pays in
    // full: the Saga in the opener, Islands on turns 1, 3 and 4 and the Bolt
    // on 2. The Saga goes down on turn 2 and is still there on turn 4.
    let waited = |v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) == 1
            && v.count_at(2, 2, Counted::In(Zone::Hand)) == 1
            && v.count_at(1, 2, Counted::In(Zone::Hand)) == 0
    };
    let four = Cost::parse("{4}").unwrap();
    let got = holds(
        &grouping,
        &schedule,
        Box::new(move |v: &PathView<'_>| waited(v) && v.can_cast(4, &four)),
    );
    assert!((got - deal).abs() < 1e-12, "{got}");
}

#[test]
fn a_saga_played_by_a_declared_priority_stops_making_mana_after_chapter_three() {
    // The same fact through the other reading: the priority plays the Saga
    // on turn 1 and an Island on each turn after, so turn 3 has three mana
    // and turn 4, with a fourth land down, still has three.
    let grouping = Grouping::with_mana(
        q(&["saga", "land"]),
        vec![
            (0b11, lasting("C", 3), 1),
            (0b10, untapped("U"), 4),
            (0b00, ManaSource::Spell, 2),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with(
        &[7, 0, 0, 0, 0],
        Policies::land_drop(LandDropPolicy::new(vec![0], 1)),
    );
    let pays = |turn: usize, text: &str| {
        let cost = Cost::parse(text).unwrap();
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(turn, &cost)),
        )
    };
    assert_eq!(pays(3, "{3}"), 1.0);
    assert_eq!(pays(4, "{3}"), 1.0);
    assert_eq!(pays(4, "{4}"), 0.0, "the Saga's mana outlived it");
}

// --- Mulligans (#7) ----------------------------------------------------------

/// Six lands and six spells, and a mulligan that keeps two to four lands,
/// puts lands back first and stops at five.
fn twelve_card_mulligan(gaps: &[u32]) -> (Grouping, Schedule) {
    let grouping = Grouping::build(q(&["land"]), [(0b1, 6), (0, 6)]).unwrap();
    let policy = MulliganPolicy::new(
        vec![Keep {
            query: 0,
            min: 2,
            max: Some(4),
        }],
        vec![0],
        5,
    );
    let schedule = Schedule::plain_with(
        gaps,
        Policies {
            mulligan: Some(policy),
            ..Policies::default()
        },
    );
    (grouping, schedule)
}

#[test]
fn a_mulligan_on_a_two_group_deck_is_the_number_on_paper() {
    // The known answer #7 and #64 ask for, small enough to do by hand.
    //
    // Seven from twelve is C(12,7) = 792 hands, and by lands held:
    //
    //   k        0   1    2    3    4    5   6
    //   hands    0   6   90  300  300   90   6
    //
    // At seven the rule keeps k in 2..=4: 690 of 792.
    // At six one land goes back, so it keeps k-1 in 2..=4, k in 3..=5: 690
    // again — but a different 690.
    // At five, two go back and the hand is kept whatever it holds.
    //
    // "Three or more lands in the hand kept":
    //   at seven, k in 3..=4:                 600
    //   at six, kept and k-1 >= 3, so 4..=5:  390
    //   at five, k - 2 >= 3, so 5..=6:         96
    let (grouping, schedule) = twelve_card_mulligan(&[7]);
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 3
    })]);
    let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(1), &mut ev).unwrap();

    let hands = 792.0;
    let keep7 = 690.0 / hands;
    let keep6 = 690.0 / hands;
    let reach6 = 1.0 - keep7;
    let reach5 = reach6 * (1.0 - keep6);
    let expected = 600.0 / hands + reach6 * 390.0 / hands + reach5 * 96.0 / hands;
    let got = out.probabilities[0].get();
    assert!((got - expected).abs() < 1e-12, "{got} vs {expected}");

    let mulligan = out.mulligan.expect("a mulligan was declared");
    let kept: Vec<f64> = mulligan.kept.iter().map(|p| p.get()).collect();
    for (got, want) in kept.iter().zip([keep7, reach6 * keep6, reach5]) {
        assert!((got - want).abs() < 1e-12, "kept {kept:?}");
    }
    // Beside it, the number had every seven been kept: k >= 3 is 696 of 792.
    let seven = mulligan.seven[0].get();
    assert!((seven - 696.0 / hands).abs() < 1e-12, "{seven}");
}

#[test]
fn turn_zero_after_a_mulligan_is_the_hand_that_was_kept() {
    // The question #7 left open, settled: turn 0 is the hand you kept, five
    // cards after a mulligan to five, not the seven it was dealt from.
    let (grouping, schedule) = twelve_card_mulligan(&[7, 1]);
    let mut ev = Counters(vec![
        Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand))),
        Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Library))),
    ]);
    let out = gauntlet_criteria::run(&grouping, &schedule, only_expectations(2), &mut ev).unwrap();
    // Every hand kept at six or five put lands back, and those lands are in
    // the library — on the bottom of it, where every count of the library
    // already finds them. So the lands in hand and in the library still sum
    // to six on every path.
    let hand = out.distributions[0].mean();
    let library = out.distributions[1].mean();
    assert!((hand + library - 6.0).abs() < 1e-12, "{hand} + {library}");
    // And no kept hand holds more lands than the rule allows, except at the
    // floor, where a five is kept whatever it holds.
    let p = out.distributions[0].probabilities();
    assert!(p.len() <= 5, "at most four lands in any kept hand: {p:?}");
}

#[test]
fn a_mulligan_with_nothing_to_throw_back_is_the_first_seven() {
    // A rule every seven passes: the answer and the number beside it are the
    // same, and every game keeps at seven.
    let grouping = Grouping::build(q(&["land"]), [(0b1, 6), (0, 6)]).unwrap();
    let policy = MulliganPolicy::new(
        vec![Keep {
            query: 0,
            min: 0,
            max: None,
        }],
        vec![0],
        5,
    );
    let schedule = Schedule::plain_with(
        &[7, 1],
        Policies {
            mulligan: Some(policy),
            ..Policies::default()
        },
    );
    let check = || {
        Closures(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(1, 0, Counted::In(Zone::Hand)) >= 3
        })])
    };
    let with =
        gauntlet_criteria::run(&grouping, &schedule, only_criteria(1), &mut check()).unwrap();
    let without = gauntlet_criteria::run(
        &grouping,
        &Schedule::plain(&[7, 1]),
        only_criteria(1),
        &mut check(),
    )
    .unwrap();
    let m = with.mulligan.expect("declared");
    assert!((with.probabilities[0].get() - without.probabilities[0].get()).abs() < 1e-12);
    assert!((m.seven[0].get() - without.probabilities[0].get()).abs() < 1e-12);
    assert!((m.kept[0].get() - 1.0).abs() < 1e-12, "{:?}", m.kept);
}

#[test]
fn a_tie_inside_one_bottoming_entry_is_priced_rather_than_broken() {
    // One entry naming two cards the hand holds one each of, and one card to
    // put back. Neither is preferred, so each goes back half the time — and a
    // question about one of them sees exactly that half.
    //
    // Seven cards, dealt whole: A, B and five fillers. The rule keeps nothing
    // at seven (it wants six cards or fewer, which only a mulligan makes), so
    // every game goes to six and puts back A or B.
    let grouping = Grouping::build(
        q(&["a", "b", "either", "filler"]),
        [(0b0101, 1), (0b0110, 1), (0b1000, 5)],
    )
    .unwrap();
    let policy = MulliganPolicy::new(
        vec![Keep {
            query: 3,
            min: 0,
            max: Some(4),
        }],
        vec![2],
        6,
    );
    let schedule = Schedule::plain_with(
        &[7],
        Policies {
            mulligan: Some(policy),
            ..Policies::default()
        },
    );
    let mut ev = Closures(vec![
        Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand)) == 1),
        Box::new(|v: &PathView<'_>| v.count_at(0, 1, Counted::In(Zone::Hand)) == 1),
        Box::new(|v: &PathView<'_>| v.count_at(0, 2, Counted::In(Zone::Hand)) == 1),
    ]);
    let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(3), &mut ev).unwrap();
    let p: Vec<f64> = out.probabilities.iter().map(|p| p.get()).collect();
    assert!((p[0] - 0.5).abs() < 1e-12, "A kept half the time: {p:?}");
    assert!((p[1] - 0.5).abs() < 1e-12, "B kept half the time: {p:?}");
    assert!(
        (p[2] - 1.0).abs() < 1e-12,
        "exactly one of them kept always: {p:?}"
    );
}

#[test]
fn a_tutor_still_finds_a_card_the_mulligan_put_on_the_bottom() {
    // A card on the bottom of the library is in the library, and a search
    // finds it. Two cards: a tutor that costs nothing and fetches the target,
    // and one copy of the target. The rule throws back any seven holding the
    // target... which is every seven of a seven-card deck, so it goes to six
    // and the target is put back first. Turn 1 casts the tutor, which goes
    // and gets the target from underneath everything.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{0}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b10, ManaSource::Spell, 1),
            (0b00, ManaSource::Spell, 5),
        ],
    )
    .unwrap();
    let tutor = Effect {
        matched_by: 0,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: Some(Fetch {
            prefer: vec![1],
            to: Fetched::Hand(1),
        }),
        delay: None,
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    };
    let policy = MulliganPolicy::new(
        vec![Keep {
            query: 1,
            min: 0,
            max: Some(0),
        }],
        vec![1],
        6,
    );
    let schedule = Schedule::plain_with_fetches(
        &[7, 0],
        vec![tutor],
        Policies {
            casting: Some(CastingPolicy::new(vec![0])),
            mulligan: Some(policy),
            ..Policies::default()
        },
    );
    let mut ev = Closures(vec![
        Box::new(|v: &PathView<'_>| v.count_at(0, 1, Counted::In(Zone::Hand)) == 0),
        Box::new(|v: &PathView<'_>| v.count_at(0, 1, Counted::In(Zone::Library)) == 1),
        Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Hand)) == 1),
        Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Library)) == 0),
    ]);
    let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(4), &mut ev).unwrap();
    let p: Vec<f64> = out.probabilities.iter().map(|p| p.get()).collect();
    assert_eq!(p, vec![1.0, 1.0, 1.0, 1.0], "{p:?}");
}

// --- The best mulligan for an objective (#63) --------------------------------

/// Conditionals for every question of `evaluator`, on one grouping that is
/// its own class, filled to `deepest`.
fn filled<V: Evaluator>(
    grouping: &Grouping,
    schedule: &Schedule,
    plan: Plan,
    evaluator: &mut V,
    deepest: u32,
) -> Table
where
    V::Error: std::fmt::Debug,
{
    let answering = Answering::all(plan);
    let mut conditionals =
        Conditionals::new(grouping, schedule, &answering, evaluator, Table::default()).unwrap();
    conditionals.fill(deepest).unwrap();
    conditionals.into_table()
}

fn every_bit(grouping: &Grouping) -> u64 {
    (0..grouping.queries().len()).fold(0, |b, i| b | 1u64 << i)
}

#[test]
fn the_best_mulligan_for_one_question_is_the_number_on_paper() {
    // The known answer #63 asks for. Six lands and six spells, one question:
    // three or more lands in the hand kept. Seven from twelve is 792 hands,
    // and 696 of them hold three lands or more.
    //
    // The best way to put cards back is spells first, and a hand with three
    // lands never has to give one up: at six it has at least four spells to
    // spare, at five at least three. So at every depth the question holds on
    // exactly the 696 hands that dealt three lands, a = 696/792, and
    //
    //   V_2 = a                    the floor keeps anything
    //   V_1 = a + (1 - a) V_2
    //   V_0 = a + (1 - a) V_1
    //
    // and the thresholds are V_1 at seven and V_2 at six: a hand is kept
    // exactly when it scores 1.
    let grouping = Grouping::build(q(&["land"]), [(0b1, 6), (0, 6)]).unwrap();
    let schedule = Schedule::plain(&[7]);
    let question = || {
        Closures(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(0, 0, Counted::In(Zone::Hand)) >= 3
        })])
    };
    let table = filled(&grouping, &schedule, only_criteria(1), &mut question(), 2);
    let identity: Vec<usize> = (0..grouping.group_sizes().len()).collect();
    let chosen = gauntlet_criteria::optimise(
        grouping.clone(),
        every_bit(&grouping),
        LandDetail::Ignored,
        7,
        5,
        &[Objective {
            weight: 1.0,
            table: &table,
            to_class: &identity,
            class_groups: grouping.group_sizes().len(),
            position: 0,
        }],
    );
    let a = 696.0 / 792.0;
    let v2 = a;
    let v1 = a + (1.0 - a) * v2;
    let v0 = a + (1.0 - a) * v1;
    let strategy = &chosen.strategy;
    assert!(
        (strategy.score() - v0).abs() < 1e-12,
        "{} vs {v0}",
        strategy.score()
    );
    assert_eq!(strategy.thresholds().len(), 2);
    assert!((strategy.thresholds()[0] - v1).abs() < 1e-12);
    assert!((strategy.thresholds()[1] - v2).abs() < 1e-12);
    for (got, want) in strategy
        .kept()
        .iter()
        .zip([a, (1.0 - a) * a, (1.0 - a) * (1.0 - a)])
    {
        assert!((got - want).abs() < 1e-12, "{:?}", strategy.kept());
    }
    assert!((chosen.under[0] - v0).abs() < 1e-12);
    assert!(
        (chosen.alone[0] - v0).abs() < 1e-12,
        "one question is its own optimum"
    );

    // Spells go back first: a hand of three lands and four spells, at the
    // floor, puts two spells back.
    let decision = strategy.decide(&[3, 4], 2);
    assert!(decision.keep);
    assert_eq!(decision.bottoms, vec![(vec![0, 2], 1.0)]);
    // And at seven a two-land hand goes back.
    assert!(!strategy.decide(&[2, 5], 0).keep);

    // Played as a run, the question comes out at the strategy's own score.
    let played = Schedule::plain_with(
        &[7],
        Policies {
            chosen: Some(Chosen(Arc::new(chosen.strategy.clone()))),
            ..Policies::default()
        },
    );
    let out = gauntlet_criteria::run_chosen(
        &grouping,
        every_bit(&grouping),
        LandDetail::Ignored,
        &played,
        &Answering::all(only_criteria(1)),
        &mut question(),
        Table::default(),
    )
    .unwrap();
    assert!((out.probabilities[0].get() - v0).abs() < 1e-12);
    let m = out.mulligan.unwrap();
    assert!((m.seven[0].get() - a).abs() < 1e-12, "the first seven kept");
}

#[test]
fn two_questions_pulling_apart_are_traded_at_their_weights() {
    // Lands and spells again, and two questions that want opposite hands:
    // three lands or more kept, and two or fewer. No hand serves both, so the
    // strategy has to choose, and the weights say how.
    let grouping = Grouping::build(q(&["land"]), [(0b1, 6), (0, 6)]).unwrap();
    let schedule = Schedule::plain(&[7, 1]);
    let questions = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand)) >= 3) as Check,
            Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand)) <= 2) as Check,
        ])
    };
    let table = filled(&grouping, &schedule, only_criteria(2), &mut questions(), 2);
    let identity: Vec<usize> = (0..grouping.group_sizes().len()).collect();
    let objective = |w0: f64, w1: f64| {
        gauntlet_criteria::optimise(
            grouping.clone(),
            every_bit(&grouping),
            LandDetail::Ignored,
            7,
            5,
            &[
                Objective {
                    weight: w0,
                    table: &table,
                    to_class: &identity,
                    class_groups: 2,
                    position: 0,
                },
                Objective {
                    weight: w1,
                    table: &table,
                    to_class: &identity,
                    class_groups: 2,
                    position: 1,
                },
            ],
        )
    };
    let even = objective(1.0, 1.0);
    // The score is the weighted sum of the numbers under the strategy.
    let score = even.under[0] + even.under[1];
    assert!((even.strategy.score() - score).abs() < 1e-12);
    // And neither question does better under a strategy serving both than
    // under the one serving it alone.
    for k in 0..2 {
        assert!(
            even.under[k] <= even.alone[k] + 1e-12,
            "{k}: {:?} {:?}",
            even.under,
            even.alone
        );
    }
    // Weighting one question up moves its number up, never down.
    let lands = objective(10.0, 1.0);
    assert!(lands.under[0] >= even.under[0] - 1e-12);
    assert!(lands.under[1] <= even.under[1] + 1e-12);
}

#[test]
fn no_declared_rule_beats_the_chosen_strategy_on_its_own_objective() {
    // The optimum is an optimum: every declared keep rule is one strategy
    // among the ones the induction searched, so none can score higher.
    let grouping = Grouping::build(q(&["land", "ramp"]), [(0b01, 7), (0b10, 3), (0, 8)]).unwrap();
    let schedule = Schedule::plain(&[7, 1, 1]);
    let questions = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(2, 0, Counted::In(Zone::Hand)) >= 3) as Check,
            Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Hand)) >= 1) as Check,
        ])
    };
    let table = filled(&grouping, &schedule, only_criteria(2), &mut questions(), 2);
    let identity: Vec<usize> = (0..grouping.group_sizes().len()).collect();
    let weights = [2.0, 1.0];
    let chosen = gauntlet_criteria::optimise(
        grouping.clone(),
        every_bit(&grouping),
        LandDetail::Ignored,
        7,
        5,
        &[0, 1].map(|k| Objective {
            weight: weights[k],
            table: &table,
            to_class: &identity,
            class_groups: 3,
            position: k,
        }),
    );
    for (min, max, bottom) in [(2, 5, 0), (1, 7, 1), (3, 4, 0), (0, 7, 0), (2, 3, 1)] {
        let declared = Schedule::plain_with(
            &[7, 1, 1],
            Policies {
                mulligan: Some(MulliganPolicy::new(
                    vec![Keep {
                        query: 0,
                        min,
                        max: Some(max),
                    }],
                    vec![bottom],
                    5,
                )),
                ..Policies::default()
            },
        );
        let out = gauntlet_criteria::run(&grouping, &declared, only_criteria(2), &mut questions())
            .unwrap();
        let score: f64 = out
            .probabilities
            .iter()
            .zip(weights)
            .map(|(p, w)| p.get() * w)
            .sum();
        assert!(
            score <= chosen.strategy.score() + 1e-12,
            "keep {min}..={max} bottoming {bottom} scores {score}, the optimum {}",
            chosen.strategy.score()
        );
    }
}

// --- A spell's draw is a sized gap (ADR-0017) --------------------------------
//
// No card draws yet: the effect library has no words for it. These build the
// effect by hand, which is the only way to reach it, to hold the engine to
// numbers worked out on paper.

/// A spell whose cast draws `draw` cards, matched by query 0.
fn drawing(draw: u32) -> Effect {
    Effect {
        matched_by: 0,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: None,
        delay: None,
        draw,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    }
}

/// One Island, one `{U}` spell that draws a card, one target and two blanks,
/// dealt two and then one, with the line casting the spell.
fn cantrip_and_target(draw: u32) -> (Grouping, Schedule) {
    let grouping = Grouping::with_mana(
        q(&["cantrip", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                1,
            ),
            (0b10, ManaSource::Spell, 1),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                    lasts: None,
                },
                1,
            ),
            (0b00, ManaSource::Spell, 2),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[2, 1],
        vec![drawing(draw)],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    (grouping, schedule)
}

#[test]
fn a_spells_draw_is_dealt_only_on_the_paths_that_cast_it() {
    // Five cards, and by turn 1 three of them are in hand, any three alike.
    //
    //   The target is among them: C(4,2)/C(5,3) = 6/10.
    //   It is not, but the Island and the spell are, with a blank:
    //   2/10 — and then the spell draws one of the two left, the target
    //   half the time: 1/10.
    //
    // 7/10 in hand by turn 1, against the 6/10 of a spell that draws nothing;
    // and the spell is cast on exactly the 3/10 holding the Island and it.
    let answer = |draw: u32| {
        let (grouping, schedule) = cantrip_and_target(draw);
        let mut ev = Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Hand)) == 1),
            Box::new(|v: &PathView<'_>| v.count_at(1, 0, Counted::Cast) == 1),
            Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Library)) == 0),
        ]);
        let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(3), &mut ev).unwrap();
        out.probabilities
            .iter()
            .map(|p| p.get())
            .collect::<Vec<_>>()
    };
    let drew = answer(1);
    assert!((drew[0] - 0.7).abs() < 1e-12, "{drew:?}");
    assert!((drew[1] - 0.3).abs() < 1e-12, "{drew:?}");
    assert!((drew[2] - 0.7).abs() < 1e-12, "{drew:?}");
    let plain = answer(0);
    assert!((plain[0] - 0.6).abs() < 1e-12, "{plain:?}");
    assert!((plain[1] - 0.3).abs() < 1e-12, "{plain:?}");
}

/// Two free spells that each draw a card, and three blanks, dealt one and
/// then one.
fn free_cantrips() -> (Grouping, Schedule) {
    let grouping = Grouping::with_mana(
        q(&["cantrip"]),
        vec![
            (
                0b1,
                ManaSource::Castable {
                    cost: Cost::parse("{0}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                2,
            ),
            (0b0, ManaSource::Spell, 3),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[1, 1],
        vec![drawing(1)],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    (grouping, schedule)
}

#[test]
fn a_spell_drawn_mid_line_is_cast_the_same_turn() {
    // The line is read again from its top after each spell resolves. Two
    // cards are seen by turn 1 and every cantrip among them is cast, each
    // showing one more; so both are cast exactly when both sit in the top
    // three: C(3,2)/C(5,2) = 3/10. A line that did not read itself again
    // would leave the drawn one in hand and cast both only when both were in
    // the top two, 1/10.
    let (grouping, schedule) = free_cantrips();
    let mut ev = Closures(vec![
        Box::new(|v: &PathView<'_>| v.count_at(1, 0, Counted::Cast) == 2),
        Box::new(|v: &PathView<'_>| v.count_at(1, 0, Counted::In(Zone::Hand)) == 0),
    ]);
    let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(2), &mut ev).unwrap();
    let p: Vec<f64> = out.probabilities.iter().map(|p| p.get()).collect();
    assert!((p[0] - 0.3).abs() < 1e-12, "{p:?}");
    assert!(
        (p[1] - 1.0).abs() < 1e-12,
        "nothing castable is left in hand: {p:?}"
    );
}

#[test]
fn a_class_with_a_sized_gap_is_as_wide_as_the_paths_it_walks() {
    // [1, 1] over two groups is 2 x 2 = 4 compositions by the static bound,
    // and that is still the width without the draw. With it, the walk takes
    // six paths: blank-blank casts nothing; either order of one cantrip and a
    // blank casts it and turns over a cantrip or a blank, two each; and two
    // cantrips cast both and turn over a blank apiece.
    let (grouping, schedule) = free_cantrips();
    assert_eq!(gauntlet_criteria::width(&grouping, &schedule), 6);
    let plain = Schedule::plain_with_fetches(
        &[1, 1],
        vec![drawing(0)],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    assert_eq!(gauntlet_criteria::width(&grouping, &plain), 4);
}

#[test]
fn a_question_whose_spells_can_draw_the_library_out_is_refused() {
    // Five cards: four dealt by the schedule, and two cantrips that could
    // draw one each. On the games where both are cast, the last turn's draw
    // finds nothing, so the question is refused as a fetch that could empty
    // the library is, rather than answered off the games that did not.
    let (grouping, _) = free_cantrips();
    let schedule = Schedule::plain_with_fetches(
        &[1, 1, 1, 1],
        vec![drawing(1)],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
    let err = gauntlet_criteria::run(&grouping, &schedule, only_criteria(1), &mut ev).unwrap_err();
    assert!(
        matches!(
            err,
            RunError::LibraryRunsOut {
                population: 5,
                draws: 4,
                fetched: 0,
                drawn: 2,
                halves: 0
            }
        ),
        "{err}"
    );
}

#[test]
fn a_question_whose_halves_could_leave_too_little_for_its_draws_is_refused() {
    // Two spells that each mill half the library, on the play to turn 3: an
    // opening seven and two draws. Eleven cards leave four after the opener,
    // and two halves that resolve before either draw leave one, so the
    // second draw could find nothing. Twelve leave five, then two: enough.
    let refused = |blanks| {
        let (grouping, schedule) =
            a_milling_deck_of(Mill::all(MillDepth::HalfLibrary), None, 2, 4, blanks);
        let mut ev = Closures(vec![Box::new(|_: &PathView<'_>| true)]);
        gauntlet_criteria::run(&grouping, &schedule, only_criteria(1), &mut ev).err()
    };
    let err = refused(3).expect("eleven cards are refused");
    assert!(
        matches!(
            err,
            RunError::LibraryRunsOut {
                population: 11,
                draws: 9,
                halves: 2,
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("2 spells each mill half"), "{err}");
    assert!(refused(4).is_none(), "twelve cards are enough");
}

// --- Found by the mutation audit (docs/research/mutation-audit.md) -----------
//
// Each of these pins a line the rest of this file could have had inverted
// without a test failing. All are hands small enough to check by eye.

#[test]
fn the_gate_does_not_play_a_land_that_did_not_arrive_this_turn() {
    // An Island and a Plains, both in hand from turn 0. By turn 2 both are in
    // play, and {U}{U} is still one blue source short: nothing arrived on
    // turn 2 that could be a second Island.
    let (grouping, schedule) = one_hand(&[
        (0b1, untapped("U"), 1),
        (0b1, untapped("W"), 1),
        (0b0, ManaSource::Spell, 5),
    ]);
    let cost = Cost::parse("{U}{U}").unwrap();
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost))
        ),
        0.0
    );
}

#[test]
fn a_land_that_arrives_this_turn_pays_beside_the_ones_already_down() {
    // A Plains, an Island and six blanks: seven dealt, the eighth drawn on
    // turn 2. On every deal both lands are in play by turn 2 and {W}{U} is
    // paid — including the two deals where one of them is the card drawn that
    // turn, which is the only line that plays it.
    let grouping = Grouping::with_mana(
        q(&["lands"]),
        vec![
            (0b1, untapped("W"), 1),
            (0b1, untapped("U"), 1),
            (0b0, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain(&[7, 0, 1]);
    let cost = Cost::parse("{W}{U}").unwrap();
    let paid = holds(
        &grouping,
        &schedule,
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost)),
    );
    assert!((paid - 1.0).abs() < 1e-12, "paid on {paid} of deals");
}

#[test]
fn a_spell_that_is_cast_leaves_the_hand_for_good() {
    // One Opt and two Islands. It is cast on turn 1, and turn 2's two mana
    // cannot cast it again: there is one card, and it is gone.
    let grouping = Grouping::with_mana(
        q(&["opts"]),
        vec![
            (0b1, opt(), 1),
            (0b0, untapped("U"), 2),
            (0b0, ManaSource::Spell, 4),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with(
        &[7, 0, 0, 0],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    for turn in 1..=3 {
        assert_eq!(
            holds(
                &grouping,
                &schedule,
                Box::new(move |v: &PathView<'_>| v.count_at(turn, 0, Counted::Cast) == 1)
            ),
            1.0,
            "turn {turn}"
        );
    }
}

#[test]
fn a_cast_spell_is_counted_in_play_and_not_in_the_library() {
    // HANDS.md hand 1 again, read from the other two zones: the Opt cast on
    // turn 1 left the hand for the battlefield (the engine counts what was
    // cast there; the caller refuses the question for a non-permanent), and
    // none of the six is still in the library, because all six were dealt.
    //
    // With the land drop declared, so that what is in play is read off the
    // line; the test below is the same claim without one.
    let grouping = Grouping::with_mana(
        q(&["opts", "land"]),
        vec![(0b01, opt(), 6), (0b10, untapped("U"), 1)],
    )
    .unwrap();
    let schedule = Schedule::plain_with(
        &[7, 0, 0, 0],
        Policies {
            land_drop: Some(LandDropPolicy::new(vec![1], 1)),
            casting: Some(CastingPolicy::new(vec![0])),
            ..Policies::default()
        },
    );
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(|v: &PathView<'_>| {
                v.count_at(1, 0, Counted::In(Zone::Battlefield)) == 1
                    && v.count_at(1, 0, Counted::In(Zone::Library)) == 0
            })
        ),
        1.0
    );
}

#[test]
fn a_permanent_the_line_names_is_in_play_only_where_it_was_cast() {
    // HANDS.md hand 43 at the engine's seam, and #94. One {1} permanent the
    // line names, two Islands and six blanks, with no land drop declared.
    // Whenever the permanent is held with no Island to pay for it, it is in
    // hand and not in play: the use-it-or-lose-it recurrence is about lands,
    // and a card the line casts gets onto the battlefield only by being cast.
    // So on every deal and every turn the battlefield count is the casting
    // count — and on some deals, those with the permanent seen and no Island,
    // both are zero while the hand holds it.
    let grouping = Grouping::with_mana(
        q(&["permanent"]),
        vec![
            (
                0b1,
                ManaSource::Castable {
                    cost: Cost::parse("{1}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b0, untapped("U"), 2),
            (0b0, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with(
        &[7, 0, 0, 0],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    for turn in 1..=3 {
        let agree = holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(turn, 0, Counted::In(Zone::Battlefield))
                    == v.count_at(turn, 0, Counted::Cast)
            }),
        );
        assert!((agree - 1.0).abs() < 1e-12, "turn {turn}: {agree}");
    }
    // And the deals that hold it uncast exist: the permanent in the seven and
    // both Islands in the two left behind, C(6,6)/C(9,7) = 1/36.
    let held = holds(
        &grouping,
        &schedule,
        Box::new(|v: &PathView<'_>| {
            v.count_at(1, 0, Counted::In(Zone::Hand)) == 1
                && v.count_at(1, 0, Counted::In(Zone::Battlefield)) == 0
        }),
    );
    assert!((held - 1.0 / 36.0).abs() < 1e-12, "held uncast on {held}");
}

#[test]
fn two_tutors_do_not_find_one_card_twice() {
    // Two tutors, one target, two Islands and three blanks: eight cards, seven
    // dealt. On the one deal in eight that leaves the target in the library
    // the first tutor gets it and the second finds nothing, so on every deal
    // exactly one target is in hand by turn 2 and none is left behind.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                2,
            ),
            (0b10, ManaSource::Spell, 1),
            (0b00, untapped("U"), 2),
            (0b00, ManaSource::Spell, 3),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 0],
        vec![tutor(Fetched::Hand(1))],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let once = holds(
        &grouping,
        &schedule,
        Box::new(|v: &PathView<'_>| {
            v.count_at(2, 1, Counted::In(Zone::Hand)) == 1
                && v.count_at(2, 1, Counted::In(Zone::Library)) == 0
        }),
    );
    assert!((once - 1.0).abs() < 1e-12, "held once on {once} of deals");
}

#[test]
fn a_tutor_to_the_battlefield_puts_its_card_in_play_and_out_of_the_library() {
    // The tutor hand with a land query and a declared land drop, so what is in
    // play is read off the line. The target is in the library on one deal in
    // seven, and on that deal the tutor puts it onto the battlefield.
    //
    // Read on turn 1, the turn the tutor resolves, as well as on turn 2: the
    // card is in play and out of the library from the moment it arrives, not
    // from the turn after (#95, HANDS.md hand 44).
    let grouping = Grouping::with_mana(
        q(&["tutor", "target", "land"]),
        vec![
            (
                0b001,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b010, ManaSource::Spell, 1),
            (0b100, untapped("U"), 1),
            (0b000, ManaSource::Spell, 4),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[6, 0, 0],
        vec![tutor(Fetched::Battlefield)],
        Policies {
            land_drop: Some(LandDropPolicy::new(vec![2], 2)),
            casting: Some(CastingPolicy::new(vec![0])),
            ..Policies::default()
        },
    );
    for turn in 1..=2 {
        let in_play = holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(turn, 1, Counted::In(Zone::Battlefield)) == 1
            }),
        );
        assert!(
            (in_play - 1.0 / 7.0).abs() < 1e-12,
            "turn {turn}, in play: {in_play}"
        );
        let gone = holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.count_at(turn, 1, Counted::In(Zone::Library)) == 0),
        );
        assert!(
            (gone - 1.0).abs() < 1e-12,
            "turn {turn}, left behind: {gone}"
        );
    }
}

#[test]
fn a_land_a_cast_puts_onto_the_battlefield_pays_from_the_next_turn() {
    // The pool half of #95. Seven cards, six dealt: a {G} tutor, a Forest, the
    // land it fetches and four blanks. On the deal that leaves the fetched
    // land in the library, turn 1 plays the Forest, casts the tutor off it and
    // puts the land down — after the line has paid, so a {G} asked beside the
    // line on turn 1 is not there on that deal. On turn 2 it is standing, and
    // {G}{G} is paid on every deal but the one that left the Forest behind.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target", "land"]),
        vec![
            (
                0b001,
                ManaSource::Castable {
                    cost: Cost::parse("{G}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                1,
            ),
            (0b100, untapped("G"), 1),
            (0b110, untapped("G"), 1),
            (0b000, ManaSource::Spell, 4),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[6, 0, 0],
        vec![tutor(Fetched::Battlefield)],
        Policies {
            land_drop: Some(LandDropPolicy::new(vec![2], 2)),
            casting: Some(CastingPolicy::new(vec![0])),
            ..Policies::default()
        },
    );
    let green = Cost::parse("{G}").unwrap();
    let two = Cost::parse("{G}{G}").unwrap();
    let left = holds(
        &grouping,
        &schedule,
        Box::new(move |v: &PathView<'_>| v.can_cast(1, &green)),
    );
    // Only on the deal that left the tutor out, where nothing was spent. On
    // the deal that fetched, the Forest paid for the tutor and the land it
    // found is not yet mana: counting it would read 2/7.
    assert!(
        (left - 1.0 / 7.0).abs() < 1e-12,
        "the fetched land paid on the turn it arrived: {left}"
    );
    let next = holds(
        &grouping,
        &schedule,
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &two)),
    );
    assert!((next - 6.0 / 7.0).abs() < 1e-12, "turn 2: {next}");
    let fetched = holds(
        &grouping,
        &schedule,
        Box::new(|v: &PathView<'_>| v.count_at(1, 1, Counted::In(Zone::Battlefield)) == 1),
    );
    // Fetched on 1/7, and played as the drop on the 1/7 that left the Forest
    // out; the other five play the Forest first.
    assert!(
        (fetched - 2.0 / 7.0).abs() < 1e-12,
        "in play on turn 1: {fetched}"
    );
}

#[test]
fn a_card_a_cast_puts_onto_the_battlefield_is_there_that_same_turn() {
    // HANDS.md hand 44, and #95. Ten Islands, a {3}{U}{U} tutor the line casts
    // and the artifact it puts onto the battlefield, which the line does not
    // name: it gets into play by being fetched or not at all. Twelve cards on
    // the play, so turn 5 has seen eleven and made five drops, and the tutor
    // is cast on turn 5 wherever it is among the eleven.
    //
    // The artifact is still in the library on turn 5 exactly when it is the
    // twelfth card, 1/12, and on that deal the tutor is among the eleven and
    // fetches it on turn 5. So on turn 5 it is on the battlefield on 1/12 of
    // deals and in the library on none, and in hand on the other 11/12, where
    // it was drawn and the line never casts it; turn 4 has no tutor cast and
    // so nothing fetched. The same with and without a declared land drop.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target", "land"]),
        vec![
            (
                0b001,
                ManaSource::Castable {
                    cost: Cost::parse("{3}{U}{U}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b010, ManaSource::Spell, 1),
            (0b100, untapped("U"), 10),
        ],
    )
    .unwrap();
    let declared = Policies {
        land_drop: Some(LandDropPolicy::new(vec![2], 2)),
        casting: Some(CastingPolicy::new(vec![0])),
        ..Policies::default()
    };
    let undeclared = Policies::casting(CastingPolicy::new(vec![0]));
    for (label, policies) in [("declared", declared), ("undeclared", undeclared)] {
        let schedule = Schedule::build(5, false, vec![tutor(Fetched::Battlefield)], policies);
        let share = |check: Check| holds(&grouping, &schedule, check);
        let field = Counted::In(Zone::Battlefield);
        let library = Counted::In(Zone::Library);
        for (what, got, want) in [
            (
                "tutor cast by turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 0, Counted::Cast) == 1
                })),
                11.0 / 12.0,
            ),
            (
                "tutor cast by turn 4",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(4, 0, Counted::Cast) == 1
                })),
                0.0,
            ),
            (
                "in play on turn 4",
                share(Box::new(move |v: &PathView<'_>| {
                    v.count_at(4, 1, field) == 1
                })),
                0.0,
            ),
            (
                "in the library on turn 4",
                share(Box::new(move |v: &PathView<'_>| {
                    v.count_at(4, 1, library) == 1
                })),
                2.0 / 12.0,
            ),
            (
                "in hand on turn 4",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(4, 1, Counted::In(Zone::Hand)) == 1
                })),
                10.0 / 12.0,
            ),
            (
                "in play on turn 5",
                share(Box::new(move |v: &PathView<'_>| {
                    v.count_at(5, 1, field) == 1
                })),
                1.0 / 12.0,
            ),
            (
                "in the library on turn 5",
                share(Box::new(move |v: &PathView<'_>| {
                    v.count_at(5, 1, library) == 1
                })),
                0.0,
            ),
            (
                "in hand on turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 1, Counted::In(Zone::Hand)) == 1
                })),
                11.0 / 12.0,
            ),
        ] {
            assert!(
                (got - want).abs() < 1e-12,
                "{label}, {what}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn tezzeret_the_seeker_puts_a_lantern_the_line_also_casts_onto_the_battlefield() {
    // HANDS.md hand 40. Hand 44's deck, with the line reading the Lantern
    // first: a Lantern in hand is cast for {1}, and one still in the library
    // on turn 5 is put down by the Seeker's −1. So it is in play by turn 5 on
    // every deal, and out of the library on every deal. Neither casting moves
    // with the fetch: the Seeker is cast on 37/44, which is 11/12 less the
    // deals where the Lantern is the eleventh card and takes one of turn 5's
    // five mana, (1/12)(10/11); and a fetched Lantern was never cast, so the
    // Lantern's own casting stays 11/12.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target", "land"]),
        vec![
            (
                0b001,
                ManaSource::Castable {
                    cost: Cost::parse("{3}{U}{U}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (
                0b010,
                ManaSource::Castable {
                    cost: Cost::parse("{1}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b100, untapped("U"), 10),
        ],
    )
    .unwrap();
    let line = || CastingPolicy::new(vec![1, 0]);
    for (fetches, field, library) in [(false, 11.0 / 12.0, 1.0 / 12.0), (true, 1.0, 0.0)] {
        let effects = if fetches {
            vec![tutor(Fetched::Battlefield)]
        } else {
            vec![]
        };
        let schedule = Schedule::build(5, false, effects, Policies::casting(line()));
        let share = |check: Check| holds(&grouping, &schedule, check);
        for (what, got, want) in [
            (
                "Seeker cast by turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 0, Counted::Cast) == 1
                })),
                37.0 / 44.0,
            ),
            (
                "Lantern cast by turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 1, Counted::Cast) == 1
                })),
                11.0 / 12.0,
            ),
            (
                "Lantern in play on turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 1, Counted::In(Zone::Battlefield)) == 1
                })),
                field,
            ),
            (
                "Lantern in the library on turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 1, Counted::In(Zone::Library)) == 1
                })),
                library,
            ),
        ] {
            assert!(
                (got - want).abs() < 1e-12,
                "fetches {fetches}, {what}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn a_tutor_billed_at_its_transmute_finds_nothing_before_three_lands() {
    // HANDS.md hand 41 at the engine's seam: Dizzy Spell, the Lantern and ten
    // Islands, the line reading the Lantern first. What the tutor costs is
    // whatever the grouping bills, so the printed {U} and the declared
    // {1}{U}{U} are the same deck with a different cost on one group. Billed
    // {U}, a tutor among the first eight finds the Lantern by turn 2: 60/66.
    // Billed {1}{U}{U}, nothing transmutes before turn 3, so the Lantern is
    // cast by turn 2 only where it was drawn, 8/12. By turn 4 both have it
    // unless both cards are the last two: 65/66.
    for (cost, played, by_two) in [
        ("{U}", 2.0 / 3.0, 60.0 / 66.0),
        ("{1}{U}{U}", 0.0, 8.0 / 12.0),
    ] {
        let grouping = Grouping::with_mana(
            q(&["tutor", "target", "land"]),
            vec![
                (
                    0b001,
                    ManaSource::Castable {
                        cost: Cost::parse(cost).unwrap().demand(),
                        resolves: Resolves::IntoGraveyard,
                    },
                    1,
                ),
                (
                    0b010,
                    ManaSource::Castable {
                        cost: Cost::parse("{1}").unwrap().demand(),
                        resolves: Resolves::OntoBattlefield,
                    },
                    1,
                ),
                (0b100, untapped("U"), 10),
            ],
        )
        .unwrap();
        let schedule = Schedule::build(
            4,
            false,
            vec![tutor(Fetched::Hand(1))],
            Policies::casting(CastingPolicy::new(vec![1, 0])),
        );
        let share = |check: Check| holds(&grouping, &schedule, check);
        for (what, got, want) in [
            (
                "tutor played by turn 2",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(2, 0, Counted::Cast) == 1
                })),
                played,
            ),
            (
                "Lantern cast by turn 2",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(2, 1, Counted::Cast) == 1
                })),
                by_two,
            ),
            (
                "Lantern cast by turn 4",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(4, 1, Counted::Cast) == 1
                })),
                65.0 / 66.0,
            ),
        ] {
            assert!(
                (got - want).abs() < 1e-12,
                "billed {cost}, {what}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn a_delayed_fetch_to_hand_arrives_in_hand_when_it_fires() {
    // The Saga's shape with the card going to hand instead: set up by the land
    // drop on turn 1, resolved on turn 3. The Lantern is still in the library
    // on one deal in ten, and on that deal it is in hand from turn 3.
    let grouping = Grouping::with_mana(
        q(&["saga", "lantern", "land", "<effect saga>"]),
        vec![
            (0b1101, untapped("C"), 1),
            (0b0010, ManaSource::Spell, 1),
            (0b0100, untapped("U"), 2),
            (0b0000, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    let to_hand = Effect {
        fetch: Some(Fetch {
            prefer: vec![1],
            to: Fetched::Hand(1),
        }),
        ..saga()
    };
    let schedule = Schedule::plain_with_fetches(
        &[9, 0, 0, 0],
        vec![to_hand],
        Policies::land_drop(LandDropPolicy::new(vec![0], 2)),
    );
    for (turn, expected) in [(2, 0.0), (3, 0.1)] {
        let fetched = holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(0, 1, Counted::In(Zone::Hand)) == 0
                    && v.count_at(turn, 1, Counted::In(Zone::Hand)) == 1
            }),
        );
        assert!(
            (fetched - expected).abs() < 1e-12,
            "turn {turn}: {fetched}, not {expected}"
        );
    }
}

#[test]
fn a_card_the_mulligan_bottomed_is_found_once_not_twice() {
    // Two free tutors and one target in a seven-card deck. Every seven holds
    // the target, so the mulligan puts it on the bottom; turn 1 casts both
    // tutors, the first finds it underneath everything, and the second finds
    // nothing — one copy, fetched once.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{0}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                2,
            ),
            (0b10, ManaSource::Spell, 1),
            (0b00, ManaSource::Spell, 4),
        ],
    )
    .unwrap();
    let policy = MulliganPolicy::new(
        vec![Keep {
            query: 1,
            min: 0,
            max: Some(0),
        }],
        vec![1],
        6,
    );
    let schedule = Schedule::plain_with_fetches(
        &[7, 0],
        vec![tutor(Fetched::Hand(1))],
        Policies {
            casting: Some(CastingPolicy::new(vec![0])),
            mulligan: Some(policy),
            ..Policies::default()
        },
    );
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(|v: &PathView<'_>| {
                v.count_at(1, 0, Counted::Cast) == 2
                    && v.count_at(1, 1, Counted::In(Zone::Hand)) == 1
            })
        ),
        1.0
    );
}

// --- Rocks and dorks are mana the line cast (ADR-0018) ----------------------
//
// HANDS.md hands 26 to 33, each as the seven-card hand it is written as: one
// deal, five turns that draw nothing, and every answer a yes or a no.

/// A permanent the line casts that adds `adds` of `makes` once it has, after
/// waiting `waits` turns: 0 for a rock, 1 for a dork.
fn source(cost: &str, adds: u32, makes: &str, waits: u32) -> ManaSource {
    ManaSource::RockOrDork {
        cost: Cost::parse(cost).unwrap().demand(),
        adds,
        makes: Palette::from_letters([makes]),
        waits,
    }
}

fn sol_ring() -> ManaSource {
    source("{1}", 2, "C", 0)
}

fn mind_stone() -> ManaSource {
    source("{2}", 1, "C", 0)
}

fn spell(cost: &str) -> ManaSource {
    ManaSource::Castable {
        cost: Cost::parse(cost).unwrap().demand(),
        resolves: Resolves::OntoBattlefield,
    }
}

/// A seven-card hand whose line is `prefer`, as query bits highest first,
/// with `commander` in the command zone where there is one.
fn rock_hand(
    names: &[&str],
    cards: Vec<(u64, ManaSource, u32)>,
    commander: Option<(u64, ManaSource)>,
    prefer: Vec<usize>,
) -> (Grouping, Schedule) {
    let grouping = Grouping::with_mana(q(names), cards).unwrap();
    let grouping = match commander {
        Some((mask, mana)) => grouping.with_command_zone([(mask, mana, 1)]),
        None => grouping,
    };
    let schedule = Schedule::plain_with(
        &[7, 0, 0, 0, 0, 0],
        Policies::casting(CastingPolicy::new(prefer)),
    );
    (grouping, schedule)
}

fn cast_by(grouping: &Grouping, schedule: &Schedule, turn: usize, query: usize) -> f64 {
    holds(
        grouping,
        schedule,
        Box::new(move |v: &PathView<'_>| v.count_at(turn, query, Counted::Cast) >= 1),
    )
}

fn left(grouping: &Grouping, schedule: &Schedule, turn: usize, cost: &str) -> f64 {
    let cost = Cost::parse(cost).unwrap();
    holds(
        grouping,
        schedule,
        Box::new(move |v: &PathView<'_>| v.can_cast(turn, &cost)),
    )
}

#[test]
fn hand_26_a_rock_pays_for_what_the_line_casts_after_it() {
    let hand = |prefer| {
        rock_hand(
            &["sol ring", "lantern"],
            vec![
                (0b01, sol_ring(), 1),
                (0b10, spell("{1}"), 1),
                (0b00, untapped("U"), 1),
                (0b00, ManaSource::Spell, 4),
            ],
            None,
            prefer,
        )
    };
    let (g, s) = hand(vec![0, 1]);
    assert_eq!(cast_by(&g, &s, 1, 1), 1.0, "the Lantern, off Sol Ring");
    assert_eq!(cast_by(&g, &s, 1, 0), 1.0, "Sol Ring, off the Island");
    assert_eq!(left(&g, &s, 1, "{1}"), 1.0, "one {{C}} left over");
    // Reversed, the Lantern takes the Island and Sol Ring waits a turn.
    let (g, s) = hand(vec![1, 0]);
    assert_eq!(cast_by(&g, &s, 1, 1), 1.0);
    assert_eq!(cast_by(&g, &s, 1, 0), 0.0);
    assert_eq!(left(&g, &s, 1, "{1}"), 0.0);
    assert_eq!(cast_by(&g, &s, 2, 0), 1.0);
}

#[test]
fn hand_27_a_rock_never_pays_for_itself() {
    let (g, s) = rock_hand(
        &["sol ring", "memory lapse"],
        vec![
            (0b01, sol_ring(), 1),
            (0b10, spell("{1}{U}"), 1),
            (0b00, untapped("U"), 1),
            (0b00, ManaSource::Spell, 4),
        ],
        None,
        vec![0, 1],
    );
    assert_eq!(cast_by(&g, &s, 1, 0), 1.0, "Sol Ring on turn 1");
    assert_eq!(cast_by(&g, &s, 1, 1), 0.0, "no {{U}} left for Memory Lapse");
    assert_eq!(cast_by(&g, &s, 2, 1), 1.0, "the Island untaps");
}

#[test]
fn hand_28_the_line_is_read_again_once_a_rock_grows_the_pool() {
    let (g, s) = rock_hand(
        &["mind stone", "sol ring"],
        vec![
            (0b01, mind_stone(), 1),
            (0b10, sol_ring(), 1),
            (0b00, untapped("U"), 1),
            (0b00, ManaSource::Spell, 4),
        ],
        None,
        vec![0, 1],
    );
    assert_eq!(
        cast_by(&g, &s, 1, 0),
        1.0,
        "Mind Stone, out of Sol Ring's mana"
    );
    assert_eq!(left(&g, &s, 1, "{1}"), 1.0, "and Mind Stone's own {{C}}");
    assert_eq!(left(&g, &s, 1, "{2}"), 0.0);
}

#[test]
fn hands_29_and_30_a_rock_pays_only_the_colours_it_makes() {
    let rashmi = (0b10, spell("{1}{G}{U}{R}"));
    let hand = |rock: ManaSource| {
        rock_hand(
            &["rock", "rashmi"],
            vec![
                (0b01, rock, 1),
                (0b00, untapped("U"), 1),
                (0b00, untapped("G"), 2),
                (0b00, ManaSource::Spell, 3),
            ],
            Some(rashmi),
            vec![0, 1],
        )
    };
    let (g, s) = hand(source("{2}", 1, "CUR", 0));
    assert_eq!(cast_by(&g, &s, 1, 0), 0.0);
    assert_eq!(cast_by(&g, &s, 2, 0), 1.0, "the Talisman on turn 2");
    assert_eq!(cast_by(&g, &s, 2, 1), 0.0);
    assert_eq!(cast_by(&g, &s, 3, 1), 1.0, "Rashmi, red off the Talisman");
    let (g, s) = hand(mind_stone());
    assert_eq!(cast_by(&g, &s, 2, 0), 1.0, "Mind Stone on turn 2");
    assert_eq!(cast_by(&g, &s, 5, 1), 0.0, "four mana and none of it red");
}

#[test]
fn hand_31_a_dork_is_summoning_sick() {
    let loam = ManaSource::Castable {
        cost: Cost::parse("{1}{G}").unwrap().demand(),
        resolves: Resolves::IntoGraveyard,
    };
    let hand = |waits| {
        rock_hand(
            &["mystic", "loam"],
            vec![
                (0b01, source("{G}", 1, "G", waits), 1),
                (0b10, loam, 1),
                (0b00, untapped("G"), 3),
                (0b00, ManaSource::Spell, 2),
            ],
            None,
            vec![0, 1],
        )
    };
    let (g, s) = hand(1);
    assert_eq!(cast_by(&g, &s, 1, 0), 1.0, "the Mystic on turn 1");
    assert_eq!(left(&g, &s, 1, "{G}"), 0.0, "and it cannot tap yet");
    assert_eq!(cast_by(&g, &s, 2, 1), 1.0);
    assert_eq!(left(&g, &s, 2, "{G}"), 1.0, "a {{G}} beside the Loam");
    // The naive reading, a Mystic that taps like a rock, is the other number.
    let (g, s) = hand(0);
    assert_eq!(left(&g, &s, 1, "{G}"), 1.0);
}

#[test]
fn hand_32_a_card_that_is_no_source_is_cast_and_makes_nothing() {
    // Lotus Cobra's mana is a landfall trigger, so it is a spell here.
    let (g, s) = rock_hand(
        &["cobra", "borborygmos"],
        vec![
            (0b01, spell("{1}{G}"), 1),
            (0b00, untapped("G"), 2),
            (0b00, untapped("U"), 1),
            (0b00, untapped("R"), 1),
            (0b00, ManaSource::Spell, 2),
        ],
        Some((0b10, spell("{2}{G}{U}{R}"))),
        vec![0, 1],
    );
    assert_eq!(cast_by(&g, &s, 2, 0), 1.0);
    assert_eq!(cast_by(&g, &s, 5, 1), 0.0, "never five mana");
}

#[test]
fn hand_33_fellwar_stone_makes_nothing_and_mind_stone_makes_one() {
    let hand = |stone: ManaSource| {
        rock_hand(
            &["stone", "trinket mage"],
            vec![
                (0b01, stone, 1),
                (0b10, spell("{2}{U}"), 1),
                (0b00, untapped("U"), 2),
                (0b00, ManaSource::Spell, 3),
            ],
            None,
            vec![0, 1],
        )
    };
    // Fellwar Stone is cast, and counted as making nothing.
    let (g, s) = hand(spell("{2}"));
    assert_eq!(cast_by(&g, &s, 2, 0), 1.0);
    assert_eq!(cast_by(&g, &s, 5, 1), 0.0);
    let (g, s) = hand(mind_stone());
    assert_eq!(cast_by(&g, &s, 2, 1), 0.0);
    assert_eq!(cast_by(&g, &s, 3, 1), 1.0);
}

#[test]
fn a_rock_that_enters_tapped_waits_as_its_effect_says() {
    // `after = 1` on a rock: Sol Ring that enters tapped casts nothing on turn
    // 1 and pays for the Lantern on turn 2 beside the Island's own mana.
    let (g, s) = rock_hand(
        &["sol ring", "lantern"],
        vec![
            (0b01, source("{1}", 2, "C", 1), 1),
            (0b10, spell("{3}"), 1),
            (0b00, untapped("U"), 1),
            (0b00, ManaSource::Spell, 4),
        ],
        None,
        vec![0, 1],
    );
    assert_eq!(cast_by(&g, &s, 1, 0), 1.0);
    assert_eq!(left(&g, &s, 1, "{1}"), 0.0);
    assert_eq!(cast_by(&g, &s, 2, 1), 1.0, "Island and {{C}}{{C}}");
    assert_eq!(left(&g, &s, 2, "{U}"), 0.0, "and nothing left over");
}

#[test]
fn narrowing_never_merges_two_rocks_across_one_it_sees_differently() {
    // Three rocks in one entry of the line, at one cost, in decklist order:
    // red, green, blue. Asked whether a {G} is left on turn 2, a cost that
    // cannot tell red from blue — so narrowed to what it demands, the red
    // and the blue rock look alike. Merged, they would be one group sitting
    // where the red one was, ahead of the green one in the tie order; and a
    // hand holding the green and the blue rock but not the red would cast
    // the blue one first, and have no {G} left. ADR-0018: a narrowing merges
    // two sources only when nothing differently seen sits between them.
    let rock = |makes: &str| source("{2}", 1, makes, 0);
    let grouping = Grouping::with_mana(
        q(&["rocks"]),
        vec![
            (0b1, rock("R"), 1),
            (0b1, rock("G"), 1),
            (0b1, rock("U"), 1),
            (0b0, untapped("W"), 9),
            (0b0, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with(&[7, 1, 1], Policies::casting(CastingPolicy::new(vec![0])));
    let cost = Cost::parse("{G}").unwrap();
    let whole = grouping.coarsened(0b1, LandDetail::Pips(Palette::ALL));
    let narrowed = grouping.coarsened(0b1, LandDetail::Pips(cost.demands()));
    let green = || {
        let cost = cost.clone();
        Box::new(move |v: &PathView<'_>| v.can_cast(2, &cost)) as Check
    };
    let wide = holds(&whole, &schedule, green());
    assert!(wide > 0.0 && wide < 1.0, "a question worth asking: {wide}");
    assert_eq!(holds(&narrowed, &schedule, green()), wide);
    // Two rocks with nothing between them do merge, which is the narrowing
    // still earning its keep.
    let adjacent = Grouping::with_mana(
        q(&["rocks"]),
        vec![
            (0b1, rock("R"), 1),
            (0b1, rock("U"), 1),
            (0b1, rock("G"), 1),
            (0b0, untapped("W"), 9),
            (0b0, ManaSource::Spell, 6),
        ],
    )
    .unwrap();
    let merged = adjacent.coarsened(0b1, LandDetail::Pips(cost.demands()));
    assert_eq!(
        merged.group_sizes().len(),
        adjacent.group_sizes().len() - 1,
        "red and blue are next to each other, and one group to a {{G}}"
    );
    assert_eq!(
        holds(&merged, &schedule, green()),
        holds(
            &adjacent.coarsened(0b1, LandDetail::Pips(Palette::ALL)),
            &schedule,
            green()
        )
    );
}

#[test]
fn a_rock_pays_the_same_under_a_declared_land_drop() {
    // Hands 26 and 27 again, with the land drop declared rather than read
    // generously: the lands standing are the ones the priority played, and a
    // rock's mana still pays only for what the line casts after it.
    let hand = |second: ManaSource| {
        let grouping = Grouping::with_mana(
            q(&["sol ring", "second", "land"]),
            vec![
                (0b001, sol_ring(), 1),
                (0b010, second, 1),
                (0b100, untapped("U"), 3),
                (0b000, ManaSource::Spell, 2),
            ],
        )
        .unwrap();
        let schedule = Schedule::plain_with(
            &[7, 0, 0, 0],
            Policies {
                land_drop: Some(LandDropPolicy::new(vec![], 2)),
                casting: Some(CastingPolicy::new(vec![0, 1])),
                ..Policies::default()
            },
        );
        (grouping, schedule)
    };
    let (g, s) = hand(spell("{1}"));
    assert_eq!(cast_by(&g, &s, 1, 1), 1.0, "the Lantern, off Sol Ring");
    assert_eq!(left(&g, &s, 1, "{1}"), 1.0);
    let (g, s) = hand(spell("{1}{U}"));
    assert_eq!(
        cast_by(&g, &s, 1, 1),
        0.0,
        "Sol Ring does not pay for itself"
    );
    assert_eq!(
        left(&g, &s, 1, "{C}{C}"),
        1.0,
        "its {{C}}{{C}} is still there"
    );
    assert_eq!(cast_by(&g, &s, 2, 1), 1.0);
}

#[test]
fn rock_mana_is_on_top_of_the_land_drops_and_never_instead_of_them() {
    // Hand 27 with three Islands in it. Turn 1 still has one land drop, so
    // one Island pays for Sol Ring and none is left for Memory Lapse's {U}:
    // a matching that counted Islands in hand rather than land drops would
    // cast it on turn 1 — the rock's two mana "making room" for two lands
    // that were never played.
    let (g, s) = rock_hand(
        &["sol ring", "memory lapse"],
        vec![
            (0b01, sol_ring(), 1),
            (0b10, spell("{1}{U}"), 1),
            (0b00, untapped("U"), 3),
            (0b00, ManaSource::Spell, 2),
        ],
        None,
        vec![0, 1],
    );
    assert_eq!(cast_by(&g, &s, 1, 1), 0.0);
    assert_eq!(left(&g, &s, 1, "{U}"), 0.0);
    assert_eq!(left(&g, &s, 1, "{2}"), 1.0, "Sol Ring's two are there");
    assert_eq!(cast_by(&g, &s, 2, 1), 1.0);
    // Turn 2: two Islands and {C}{C}, and Memory Lapse took one of each.
    assert_eq!(left(&g, &s, 2, "{U}{1}"), 1.0);
    assert_eq!(left(&g, &s, 2, "{U}{U}"), 0.0);
}

// --- A spell's mill reaches the graveyard (ADR-0017 §2) ----------------------
//
// HANDS.md hands 19 and 20, each one deal with the library's order written
// down, played on the board both engines play: a path is the history of
// checkpoints, and the sized gap a cast asks for is one more of them.

/// Groups, in this order: Forest, the milling spell, Life from the Loam,
/// Mountain, Island, Beast Within, and Aftermath Analyst where there is one.
/// Queries: 0 the spell, 1 Loam, 2 Forest, 3 land, 4 permanent, 5 Analyst, 6 Beast
/// Within.
fn mill_groups(spell: ManaSource, analysts: u32, blanks: u32) -> Grouping {
    Grouping::with_mana(
        q(&[
            "spell",
            "loam",
            "forest",
            "land",
            "permanent",
            "analyst",
            "beast",
        ]),
        vec![
            (0b011100, untapped("G"), 3),
            (0b000001, spell, 1),
            (0b000010, ManaSource::Spell, 1),
            (0b011000, untapped("R"), 1),
            (0b011000, untapped("U"), 1),
            (0b1000000, ManaSource::Spell, blanks),
            (0b110000, ManaSource::Spell, analysts),
        ],
    )
    .unwrap()
}

fn two_mana(resolves: Resolves) -> ManaSource {
    ManaSource::Castable {
        cost: Cost::parse("{1}{G}").unwrap().demand(),
        resolves,
    }
}

fn milling(mill: Option<Mill>) -> Effect {
    Effect {
        matched_by: 0,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: None,
        delay: None,
        draw: 0,
        mill,
        activation: None,
        discard: None,
        untap: 0,
    }
}

/// A history over [`mill_groups`], one row per checkpoint, each row the cards
/// that checkpoint revealed: `(Forest, spell, Loam, Mountain, Island, Beast
/// Within, Analyst)`, cut to the `groups` the grouping has.
fn dealt(groups: usize, rows: &[[u32; 7]]) -> Vec<Vec<u32>> {
    let mut total = [0u32; 7];
    rows.iter()
        .map(|row| {
            for (t, r) in total.iter_mut().zip(row) {
                *t += r;
            }
            total[..groups].to_vec()
        })
        .collect()
}

const NOTHING: [u32; 7] = [0; 7];
const FOREST: [u32; 7] = [1, 0, 0, 0, 0, 0, 0];
const MOUNTAIN: [u32; 7] = [0, 0, 0, 1, 0, 0, 0];
const LOAM: [u32; 7] = [0, 0, 1, 0, 0, 0, 0];
const BEAST: [u32; 7] = [0, 0, 0, 0, 0, 1, 0];

#[test]
fn hand_19_aftermath_analyst_mills_the_loam_before_you_could_draw_it() {
    // Forest x2, the Analyst and Beast Within x4 in hand; the library, top
    // first, Beast Within, Mountain, Life from the Loam, Island, Forest. On the
    // play, turns 2 to 4 draw one each. The Analyst is the spell here, cast on
    // turn 2 off two Forests, and it mills three.
    let grouping = mill_groups(two_mana(Resolves::OntoBattlefield), 0, 5);
    let schedule = |mill| {
        Schedule::plain_with_fetches(
            &[7, 0, 1, 1, 1],
            vec![milling(mill)],
            Policies::casting(CastingPolicy::new(vec![0])),
        )
    };
    let opener = [2, 1, 0, 0, 0, 4, 0];

    // Today, and what a spell that does nothing looks like: turn 3 draws the
    // Mountain and turn 4 the Loam, into hand.
    let today = schedule(None);
    let mut board = Board::new(&grouping, &today);
    board.walk(&dealt(6, &[opener, NOTHING, BEAST, MOUNTAIN, LOAM]));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(2, 0, Counted::Cast), 1);
    assert_eq!(board.count_at(4, 1, Counted::In(Zone::Graveyard)), 0);
    assert_eq!(board.count_at(4, 1, Counted::In(Zone::Hand)), 1);
    assert_eq!(board.count_at(3, 2, Counted::In(Zone::Battlefield)), 2);

    // Mill 3. Replaying up to turn 2's draw, the walk stops at the cast and
    // asks for three cards; dealt, they go to the graveyard, and turn 3 draws
    // the Forest that was under them. Turn 4 finds the library empty.
    let milled = schedule(Some(Mill::all(3)));
    let mut board = Board::new(&grouping, &milled);
    board.walk(&dealt(6, &[opener, NOTHING, BEAST]));
    assert_eq!(board.next_gap(), 3, "the cast asks for its mill");
    let three = [0, 0, 1, 1, 1, 0, 0];
    board.walk(&dealt(6, &[opener, NOTHING, BEAST, three, FOREST, NOTHING]));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(
        board.count_at(2, 0, Counted::Cast),
        1,
        "what a spell does cannot change whether it was paid for"
    );
    assert_eq!(board.count_at(2, 1, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(4, 1, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(4, 1, Counted::In(Zone::Hand)), 0);
    assert_eq!(board.count_at(4, 1, Counted::In(Zone::Library)), 0);
    assert_eq!(board.count_at(2, 3, Counted::In(Zone::Graveyard)), 2);
    assert_eq!(board.count_at(3, 2, Counted::In(Zone::Battlefield)), 3);
}

/// Hand 19's number on paper: Forest x2, Aftermath Analyst, Life from the
/// Loam and Beast Within x8, twelve cards on the play, to turn 2.
fn analyst_twelve(mill: Option<Mill>) -> (Grouping, Schedule) {
    let grouping = Grouping::with_mana(
        q(&["analyst", "loam"]),
        vec![
            (0b00, untapped("G"), 2),
            (0b01, two_mana(Resolves::OntoBattlefield), 1),
            (0b10, ManaSource::Spell, 1),
            (0b00, ManaSource::Spell, 8),
        ],
    )
    .unwrap();
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 1],
        vec![milling(mill)],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    (grouping, schedule)
}

#[test]
fn hand_19_on_paper_the_mill_takes_the_loam_one_time_in_three() {
    // The Analyst is cast on turn 2 exactly when it and both Forests are in
    // the top eight: C(9,5)/C(12,8) = 126/495. The other nine cards fill five
    // seen slots, three milled and one left over, and the Loam is equally
    // likely to be in any of them: in hand 5/9 of those deals, milled 3/9,
    // still in the library 1/9.
    let answer = |mill| {
        let (grouping, schedule) = analyst_twelve(mill);
        let cast = |v: &PathView<'_>| v.count_at(2, 0, Counted::Cast) == 1;
        let mut ev = Closures(vec![
            Box::new(move |v: &PathView<'_>| cast(v)),
            Box::new(move |v: &PathView<'_>| {
                cast(v) && v.count_at(2, 1, Counted::In(Zone::Hand)) == 1
            }),
            Box::new(move |v: &PathView<'_>| {
                cast(v) && v.count_at(2, 1, Counted::In(Zone::Graveyard)) == 1
            }),
            Box::new(move |v: &PathView<'_>| {
                cast(v) && v.count_at(2, 1, Counted::In(Zone::Library)) == 1
            }),
        ]);
        let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(4), &mut ev).unwrap();
        out.probabilities
            .iter()
            .map(|p| p.get() * 495.0)
            .collect::<Vec<_>>()
    };
    let milled = answer(Some(Mill::all(3)));
    for (got, want) in milled.iter().zip([126.0, 70.0, 42.0, 14.0]) {
        assert!((got - want).abs() < 1e-9, "{milled:?} of 495");
    }
    let today = answer(None);
    for (got, want) in today.iter().zip([126.0, 70.0, 0.0, 56.0]) {
        assert!((got - want).abs() < 1e-9, "{today:?} of 495");
    }
}

#[test]
fn traumatize_mills_half_the_library_it_finds_which_the_path_decides() {
    // Hand 19's deck and deal, with the spell milling half its library,
    // rounded down. Cast on turn 2 after that turn's draw, it finds four cards
    // and mills two; the Island and the Forest under them are turns 3 and 4.
    let grouping = mill_groups(two_mana(Resolves::IntoGraveyard), 0, 5);
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 1, 1, 1],
        vec![milling(Some(Mill::all(MillDepth::HalfLibrary)))],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let island = [0, 0, 0, 0, 1, 0, 0];
    let two = [0, 0, 1, 1, 0, 0, 0];
    let mut board = Board::new(&grouping, &schedule);
    board.walk(&dealt(6, &[[2, 1, 0, 0, 0, 4, 0], NOTHING, BEAST]));
    assert_eq!(board.next_gap(), 2, "half of the four it finds");
    board.walk(&dealt(
        6,
        &[[2, 1, 0, 0, 0, 4, 0], NOTHING, BEAST, two, island, FOREST],
    ));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(2, 0, Counted::Cast), 1);
    assert_eq!(board.count_at(2, 1, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(
        board.count_at(2, 3, Counted::In(Zone::Graveyard)),
        1,
        "the Mountain"
    );
    assert_eq!(board.count_at(4, 3, Counted::In(Zone::Library)), 0);

    // One Forest short, it waits for turn 3's draw to find the second, and
    // by then the library is three cards: it mills one.
    let mut board = Board::new(&grouping, &schedule);
    board.walk(&dealt(6, &[[1, 1, 1, 0, 0, 4, 0], NOTHING, BEAST, FOREST]));
    assert_eq!(board.count_at(2, 0, Counted::Cast), 0);
    assert_eq!(board.next_gap(), 1, "half of three, rounded down");
}

#[test]
fn traumatize_on_paper_mills_two_of_the_four_it_finds() {
    // Hand 19's number with half the library milled: cast on turn 2, the
    // spell finds the four cards under the top eight and mills two. The other
    // nine cards fill five seen slots, two milled and two left, and the Loam
    // is in any of them alike: in hand 5/9, milled 2/9, in the library 2/9
    // of the 126 deals in 495 that cast it.
    let (grouping, schedule) = analyst_twelve(Some(Mill::all(MillDepth::HalfLibrary)));
    let cast = |v: &PathView<'_>| v.count_at(2, 0, Counted::Cast) == 1;
    let mut ev = Closures(vec![
        Box::new(move |v: &PathView<'_>| cast(v)),
        Box::new(move |v: &PathView<'_>| cast(v) && v.count_at(2, 1, Counted::In(Zone::Hand)) == 1),
        Box::new(move |v: &PathView<'_>| {
            cast(v) && v.count_at(2, 1, Counted::In(Zone::Graveyard)) == 1
        }),
        Box::new(move |v: &PathView<'_>| {
            cast(v) && v.count_at(2, 1, Counted::In(Zone::Library)) == 1
        }),
    ]);
    let out = gauntlet_criteria::run(&grouping, &schedule, only_criteria(4), &mut ev).unwrap();
    let got: Vec<f64> = out.probabilities.iter().map(|p| p.get() * 495.0).collect();
    for (got, want) in got.iter().zip([126.0, 70.0, 28.0, 28.0]) {
        assert!((got - want).abs() < 1e-9, "{got} of 495, want {want}");
    }
}

#[test]
fn hand_20_malevolent_rumble_keeps_a_permanent_and_the_loam_is_not_one() {
    // Forest x2, Malevolent Rumble and Beast Within x4 in hand; the library,
    // top first, Beast Within, Life from the Loam, Mountain, Beast Within,
    // Aftermath Analyst, Beast Within. Turn 2 draws the first, plays the
    // second Forest and casts Rumble: it reveals the next four, at most one
    // permanent card goes to hand, and the rest go to the graveyard because
    // the card says so. The file chooses the permanent.
    let grouping = mill_groups(two_mana(Resolves::IntoGraveyard), 1, 6);
    let (spell, loam, land, permanent, analyst, beast) = (0, 1, 3, 4, 5, 6);
    let rumble = |prefer: Vec<usize>| {
        Schedule::plain_with_fetches(
            &[7, 0, 1, 1],
            vec![milling(Some(Mill {
                cards: MillDepth::Exactly(4),
                to_hand: ToHand::Chosen {
                    up_to: 1,
                    of: Some(permanent),
                    prefer,
                },
                returns: None,
            }))],
            Policies::casting(CastingPolicy::new(vec![spell])),
        )
    };
    let opener = [2, 1, 0, 0, 0, 4, 0];
    let four = [0, 0, 1, 1, 0, 1, 1];
    let path = dealt(7, &[opener, NOTHING, BEAST, four, BEAST]);
    // (nothing declared, lands, the Loam and then lands)
    let columns = [vec![], vec![land], vec![loam, land]];
    let mut rows = Vec::new();
    for prefer in columns {
        let schedule = rumble(prefer);
        let mut board = Board::new(&grouping, &schedule);
        board.walk(&path[..3]);
        assert_eq!(board.next_gap(), 4, "Rumble reveals four");
        board.walk(&path);
        assert_eq!(board.next_gap(), 0);
        assert_eq!(board.count_at(2, spell, Counted::Cast), 1);
        let yard = |query| board.count_at(2, query, Counted::In(Zone::Graveyard));
        rows.push((
            yard(loam),
            // The revealed cards, and not Rumble, which is a sorcery that
            // resolved into the graveyard too.
            yard(loam) + yard(land) + yard(analyst) + yard(beast),
            board.count_at(3, land, Counted::In(Zone::Battlefield)),
            board.count_at(2, analyst, Counted::In(Zone::Hand)),
        ));
    }
    assert_eq!(
        rows,
        [(1, 4, 2, 0), (1, 3, 3, 0), (1, 3, 3, 0)],
        "an unrouted Rumble bins all four; the kept land is played on turn 3; \
         and the Loam, a sorcery, is not a card Rumble may keep"
    );
}

#[test]
fn a_land_kept_mid_line_waits_for_the_next_turns_drop_even_when_this_turns_was_not_made() {
    // Hand 20's Rumble a turn later, with nothing to play on turn 3: two
    // Forests are down by turn 2, turn 3 draws the Rumble, and the line casts
    // it off the two, keeping the Mountain. The drop came before the line, so
    // the Mountain is not on the battlefield until turn 4.
    let grouping = mill_groups(two_mana(Resolves::IntoGraveyard), 1, 7);
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 1, 1, 1],
        vec![milling(Some(Mill {
            cards: MillDepth::Exactly(4),
            to_hand: ToHand::Chosen {
                up_to: 1,
                of: Some(4),
                prefer: vec![3],
            },
            returns: None,
        }))],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let opener = [2, 0, 0, 0, 0, 5, 0];
    let rumble = [0, 1, 0, 0, 0, 0, 0];
    let four = [0, 0, 1, 1, 0, 1, 1];
    let mut board = Board::new(&grouping, &schedule);
    board.walk(&dealt(7, &[opener, NOTHING, BEAST, rumble, four, BEAST]));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(3, 0, Counted::Cast), 1);
    assert_eq!(board.count_at(3, 3, Counted::In(Zone::Hand)), 3);
    assert_eq!(board.count_at(3, 3, Counted::In(Zone::Battlefield)), 2);
    assert_eq!(board.count_at(4, 3, Counted::In(Zone::Battlefield)), 3);
}

#[test]
fn wrenn_and_sevens_lands_go_to_hand_whatever_the_file_asks() {
    // Hand 20's deal, with a spell whose four go to hand if they are lands and
    // to the graveyard if not: the card chooses, so there is no list to read.
    let grouping = mill_groups(two_mana(Resolves::OntoBattlefield), 1, 6);
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 1, 1],
        vec![milling(Some(Mill {
            cards: MillDepth::Exactly(4),
            to_hand: ToHand::Every(3),
            returns: None,
        }))],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let opener = [2, 1, 0, 0, 0, 4, 0];
    let four = [0, 0, 1, 1, 0, 1, 1];
    let mut board = Board::new(&grouping, &schedule);
    board.walk(&dealt(7, &[opener, NOTHING, BEAST, four, BEAST]));
    let at = |query, zone| board.count_at(2, query, Counted::In(zone));
    assert_eq!(at(1, Zone::Graveyard), 1, "the Loam is milled");
    assert_eq!(at(5, Zone::Graveyard), 1, "and so is the Analyst");
    assert_eq!(at(6, Zone::Graveyard), 1);
    assert_eq!(at(3, Zone::Graveyard), 0, "the Mountain is not");
    assert_eq!(at(3, Zone::Hand), 3, "it is in hand beside the two Forests");
    assert_eq!(board.count_at(3, 3, Counted::In(Zone::Battlefield)), 3);
}

// --- A mill nothing reads is dealt last (ADR-0017 §4) ------------------------
//
// A narrowing, so the whole of its specification is that it moves no number:
// the same questions, answered with the mill dealt where it fired and dealt
// last over only what the questions read of the graveyard and the library.

/// Green spells matched by query 0 at `{1}{G}`, whose cast mills `mill`; a
/// target, query 1; Forests, query 2; blanks.
fn a_milling_deck(mill: Mill, fetch: Option<Fetch>) -> (Grouping, Schedule) {
    a_milling_deck_of(mill, fetch, 4, 16, 38)
}

/// [`a_milling_deck`] with `millers` spells, `forests` Forests and `blanks`
/// blanks.
fn a_milling_deck_of(
    mill: Mill,
    fetch: Option<Fetch>,
    millers: u32,
    forests: u32,
    blanks: u32,
) -> (Grouping, Schedule) {
    let grouping = Grouping::with_mana(
        q(&["miller", "target", "land"]),
        vec![
            (0b001, two_mana(Resolves::IntoGraveyard), millers),
            (0b010, ManaSource::Spell, 2),
            (0b100, untapped("G"), forests),
            (0b000, ManaSource::Spell, blanks),
        ],
    )
    .unwrap();
    let effect = Effect {
        fetch,
        ..milling(Some(mill))
    };
    let schedule = Schedule::build(
        3,
        false,
        vec![effect],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    (grouping, schedule)
}

/// Every zone the target and the lands can be in, on two turns, and the
/// mill fired or not.
fn every_zone() -> Closures {
    let mut checks: Vec<Check> = Vec::new();
    for turn in [2, 3] {
        for query in [1, 2] {
            for zone in [Zone::Graveyard, Zone::Hand, Zone::Library] {
                checks.push(Box::new(move |v: &PathView<'_>| {
                    v.count_at(turn, query, Counted::In(zone)) >= 1
                }));
            }
        }
        checks.push(Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 1, Counted::In(Zone::Library)) == 2
        }));
        checks.push(Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 2, Counted::In(Zone::Graveyard)) >= 2
        }));
        checks.push(Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 2, Counted::In(Zone::Battlefield)) >= 3
        }));
        checks.push(Box::new(move |v: &PathView<'_>| {
            v.count_at(turn, 0, Counted::Cast) >= 2
        }));
    }
    Closures(checks)
}

fn every_zone_answered(grouping: &Grouping, schedule: &Schedule) -> Vec<f64> {
    let mut ev = every_zone();
    let n = ev.0.len();
    gauntlet_criteria::run(grouping, schedule, only_criteria(n), &mut ev)
        .unwrap()
        .probabilities
        .iter()
        .map(|p| p.get())
        .collect()
}

#[test]
fn a_mill_nothing_reads_is_dealt_last_and_moves_no_number() {
    let (grouping, in_place) = a_milling_deck(Mill::all(3), None);
    // The graveyard and the library are read for the target and the lands,
    // and for nothing else.
    let last = in_place.clone().deferring(0b110);
    let (a, b) = (
        every_zone_answered(&grouping, &in_place),
        every_zone_answered(&grouping, &last),
    );
    assert!(
        a[0] > 0.01 && a[0] < 0.99,
        "the mill reaches the target: {a:?}"
    );
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            (x - y).abs() < 1e-12,
            "question {i}: {x} in place, {y} last"
        );
    }
    let (wide, narrow) = (
        gauntlet_criteria::width(&grouping, &in_place),
        gauntlet_criteria::width(&grouping, &last),
    );
    assert!(
        narrow < wide,
        "dealt last over three bins rather than in place over four groups: {narrow} vs {wide}"
    );
}

#[test]
fn half_a_library_nothing_reads_is_dealt_last_and_moves_no_number() {
    // Two Traumatizes in twenty cards: half of what is left, some six cards,
    // is one block over every group in place, and dealt last it is the same
    // block over three bins, sized where it fired.
    let (grouping, in_place) = a_milling_deck_of(Mill::all(MillDepth::HalfLibrary), None, 2, 7, 9);
    let last = in_place.clone().deferring(0b110);
    let (a, b) = (
        every_zone_answered(&grouping, &in_place),
        every_zone_answered(&grouping, &last),
    );
    assert!(
        a[0] > 0.01 && a[0] < 0.99,
        "the mill reaches the target: {a:?}"
    );
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            (x - y).abs() < 1e-12,
            "question {i}: {x} in place, {y} last"
        );
    }
    let (wide, narrow) = (
        gauntlet_criteria::width(&grouping, &in_place),
        gauntlet_criteria::width(&grouping, &last),
    );
    assert!(narrow < wide, "{narrow} vs {wide}");
}

#[test]
fn a_mill_that_chooses_from_its_cards_is_dealt_where_it_fired() {
    // Rumble's shape reads its own block to choose the card it keeps, so the
    // block is not one nothing reads, and a class that would defer a mill
    // leaves this one where it is.
    let rumble = Mill {
        cards: MillDepth::Exactly(4),
        to_hand: ToHand::Chosen {
            up_to: 1,
            of: Some(2),
            prefer: vec![2],
        },
        returns: None,
    };
    let (grouping, in_place) = a_milling_deck(rumble, None);
    let last = in_place.clone().deferring(0b110);
    assert_eq!(
        gauntlet_criteria::width(&grouping, &last),
        gauntlet_criteria::width(&grouping, &in_place)
    );
    // Nor Wrenn and Seven's, which sends every land among them to hand.
    let wrenn = Mill {
        cards: MillDepth::Exactly(4),
        to_hand: ToHand::Every(2),
        returns: None,
    };
    let (grouping, in_place) = a_milling_deck(wrenn, None);
    let last = in_place.clone().deferring(0b110);
    assert_eq!(
        gauntlet_criteria::width(&grouping, &last),
        gauntlet_criteria::width(&grouping, &in_place)
    );
}

#[test]
fn a_mill_beside_a_tutor_is_dealt_where_it_fired() {
    // A tutor reads the library, and a card dealt last is still in the
    // library to it: it could find the card the mill had already binned.
    let fetch = Fetch {
        prefer: vec![1],
        to: Fetched::Hand(1),
    };
    let (grouping, in_place) = a_milling_deck(Mill::all(3), Some(fetch));
    let last = in_place.clone().deferring(0b110);
    assert_eq!(
        gauntlet_criteria::width(&grouping, &last),
        gauntlet_criteria::width(&grouping, &in_place)
    );
}

/// HANDS.md hand 42's deck at the engine's seam: Expedition Map, the Lantern,
/// Urza's Saga, five Islands and eight blanks, sixteen cards. Queries: the
/// Map, the Lantern, the Saga, any land, and one bit per effect.
fn hand_forty_two() -> Grouping {
    Grouping::with_mana(
        q(&[
            "map",
            "lantern",
            "saga",
            "land",
            "<effect map>",
            "<effect saga>",
        ]),
        vec![
            (
                0b010001,
                ManaSource::Castable {
                    cost: Cost::parse("{1}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b000010, ManaSource::Spell, 1),
            (0b101100, lasting("C", 3), 1),
            (0b001000, untapped("U"), 5),
            (0b000000, ManaSource::Spell, 8),
        ],
    )
    .unwrap()
}

/// Expedition Map: `{2}`, `{T}`, sacrifice it: a land card to hand. Here it
/// finds the Saga.
fn expedition_map() -> Effect {
    Effect {
        matched_by: 4,
        look: 0,
        trigger: Trigger::Activate,
        route: Route::Nowhere,
        fetch: Some(Fetch {
            prefer: vec![2],
            to: Fetched::Hand(1),
        }),
        delay: None,
        draw: 0,
        mill: None,
        activation: Some(Activation {
            cost: Cost::parse("{2}").unwrap().demand(),
            sacrifice: true,
        }),
        discard: None,
        untap: 0,
    }
}

/// Hand 17's chapter III, on the Saga's effect bit.
fn chapter_three() -> Effect {
    Effect {
        matched_by: 5,
        ..saga()
    }
}

fn map_and_saga_policies() -> Policies {
    Policies {
        land_drop: Some(LandDropPolicy::new(vec![2], 3)),
        casting: Some(CastingPolicy::new(vec![0])),
        ..Policies::default()
    }
}

#[test]
fn expedition_map_goes_and_gets_urzas_saga_before_the_drop_it_is_played_on() {
    // HANDS.md hand 42, both columns, on the play to turn 5. The line names
    // only the Map, so the Lantern arrives by chapter III or not at all. The
    // Map's casting cannot move; the Saga played by turn 3 and the Lantern
    // on the battlefield by turn 5 must, to the brute-forced fractions
    // (docs/research/tutor-routes-hands.py): the activation paid before
    // turn 3's drop is what makes the fetched Saga that turn's land.
    let grouping = hand_forty_two();
    for (effects, saga_by_three, lantern_by_five) in [
        (vec![chapter_three()], 9.0 / 16.0, 1.0 / 4.0),
        (
            vec![expedition_map(), chapter_three()],
            26249.0 / 34320.0,
            3977.0 / 12870.0,
        ),
    ] {
        let activated = effects.len() == 2;
        let schedule = Schedule::build(5, false, effects, map_and_saga_policies());
        let share = |check: Check| holds(&grouping, &schedule, check);
        for (what, got, want) in [
            (
                "Map cast on turn 1",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(1, 0, Counted::Cast) == 1
                })),
                4921.0 / 11440.0,
            ),
            (
                // Played by turn 3: on the battlefield on one of turns 1 to 3,
                // because chapter III takes it off again on the third.
                "Saga played by turn 3",
                share(Box::new(|v: &PathView<'_>| {
                    (1..=3).any(|t| v.count_at(t, 2, Counted::In(Zone::Battlefield)) >= 1)
                })),
                saga_by_three,
            ),
            (
                "Lantern on the battlefield by turn 5",
                share(Box::new(|v: &PathView<'_>| {
                    v.count_at(5, 1, Counted::In(Zone::Battlefield)) >= 1
                })),
                lantern_by_five,
            ),
        ] {
            assert!(
                (got - want).abs() < 1e-12,
                "activated: {activated}, {what}: {got}, not {want}"
            );
        }
    }
}

#[test]
fn a_sacrificed_map_leaves_the_battlefield_and_is_counted_nowhere() {
    // The same deck. A Map the line cast is on the battlefield until it is
    // activated; sacrificed, it is in no zone a criterion asks about, and it
    // was still cast. Turn 1 never activates it — one Island pays {1}, not
    // {2} — so on turn 1 every Map cast is in play.
    let grouping = hand_forty_two();
    let schedule = Schedule::build(
        5,
        false,
        vec![expedition_map(), chapter_three()],
        map_and_saga_policies(),
    );
    let share = |check: Check| holds(&grouping, &schedule, check);
    let in_play_turn_one = share(Box::new(|v: &PathView<'_>| {
        v.count_at(1, 0, Counted::In(Zone::Battlefield)) == 1
    }));
    assert!((in_play_turn_one - 4921.0 / 11440.0).abs() < 1e-12);
    let gone = share(Box::new(|v: &PathView<'_>| {
        v.count_at(5, 0, Counted::Cast) == 1
            && [
                Zone::Battlefield,
                Zone::Graveyard,
                Zone::Hand,
                Zone::Library,
            ]
            .into_iter()
            .all(|zone| v.count_at(5, 0, Counted::In(zone)) == 0)
    }));
    let cast = share(Box::new(|v: &PathView<'_>| {
        v.count_at(5, 0, Counted::Cast) == 1
    }));
    let in_play = share(Box::new(|v: &PathView<'_>| {
        v.count_at(5, 0, Counted::In(Zone::Battlefield)) == 1
    }));
    assert!(gone > 0.5, "{gone}");
    assert!(
        (gone + in_play - cast).abs() < 1e-12,
        "a cast Map is in play or gone: {gone} + {in_play} against {cast}"
    );
}

#[test]
fn an_activation_is_paid_once_per_permanent_per_turn() {
    // A tutor that taps and stays: {1}, {T}: a card to hand. Two Islands in
    // play and three copies of the Lantern in the library; seven cards dealt
    // and nothing drawn, so every deal is the one hand. The tapper is cast on
    // turn 1 for {1}; turn 2 has two Islands, and the tapper finds one
    // Lantern — not two, though {1}{1} is payable — and one more on turn 3.
    let grouping = Grouping::with_mana(
        q(&["tapper", "lantern", "land", "<effect tapper>"]),
        vec![
            (
                0b1001,
                ManaSource::Castable {
                    cost: Cost::parse("{1}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (0b0010, ManaSource::Spell, 3),
            (0b0100, untapped("U"), 2),
            (0b0000, ManaSource::Spell, 4),
        ],
    )
    .unwrap();
    let tapper = Effect {
        matched_by: 3,
        fetch: Some(Fetch {
            prefer: vec![1],
            to: Fetched::Hand(1),
        }),
        activation: Some(Activation {
            cost: Cost::parse("{1}").unwrap().demand(),
            sacrifice: false,
        }),
        discard: None,
        untap: 0,
        ..expedition_map()
    };
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 0, 0],
        vec![tapper],
        Policies {
            land_drop: Some(LandDropPolicy::new(vec![], 2)),
            casting: Some(CastingPolicy::new(vec![0])),
            ..Policies::default()
        },
    );
    // The deals that hold the tapper, both Islands and at most one Lantern,
    // so the library has two for it to find.
    let dealt = |v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) == 1
            && v.count_at(0, 2, Counted::In(Zone::Hand)) == 2
            && v.count_at(0, 1, Counted::In(Zone::Hand)) <= 1
    };
    let some = holds(&grouping, &schedule, Box::new(dealt));
    assert!(some > 0.0);
    for (turn, lanterns) in [(1, 0), (2, 1), (3, 2)] {
        let off = holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| {
                dealt(v)
                    && v.count_at(turn, 1, Counted::In(Zone::Hand))
                        - v.count_at(0, 1, Counted::In(Zone::Hand))
                        != lanterns
            }),
        );
        assert_eq!(off, 0.0, "turn {turn}: not {lanterns} Lanterns found");
    }
}

#[test]
fn what_an_activation_paid_before_the_drop_spent_is_not_paid_again_by_the_drop() {
    // Eight cards, seven dealt and nothing drawn: two Opts, the Map, a
    // Brainstorm, two Islands, the Saga and a blank, on the deal that leaves
    // the Saga in the library. The line is [Opt, Map, Brainstorm]. Turn 1: an
    // Island and an Opt. Turn 2: an Island, an Opt and the Map, and nothing
    // left for its {2}. Turn 3, before the drop, both Islands pay the Map's
    // {2} and the Saga comes to hand; it is the drop. Brainstorm wants {U},
    // and what is left is the Saga's {C}: it waits for turn 4. Paid as one
    // bill with the Saga in it, the Saga would take the {2}'s generic and an
    // Island the {U}, which is a line nobody could have played.
    let grouping = Grouping::with_mana(
        q(&[
            "opt",
            "map",
            "saga",
            "land",
            "<effect map>",
            "<effect saga>",
            "brainstorm",
        ]),
        vec![
            (
                0b0000001,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                2,
            ),
            (
                0b0010010,
                ManaSource::Castable {
                    cost: Cost::parse("{1}").unwrap().demand(),
                    resolves: Resolves::OntoBattlefield,
                },
                1,
            ),
            (
                0b1000000,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                1,
            ),
            (0b0101100, lasting("C", 3), 1),
            (0b0001000, untapped("U"), 2),
            (0b0000000, ManaSource::Spell, 1),
        ],
    )
    .unwrap();
    let map = Effect {
        matched_by: 4,
        ..expedition_map()
    };
    let saga_waits = Effect {
        fetch: None,
        ..chapter_three()
    };
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 0, 0, 0],
        vec![map, saga_waits],
        Policies {
            land_drop: Some(LandDropPolicy::new(vec![2], 3)),
            casting: Some(CastingPolicy::new(vec![0, 1, 6])),
            ..Policies::default()
        },
    );
    let left_out = |v: &PathView<'_>| v.count_at(0, 2, Counted::In(Zone::Hand)) == 0;
    let share = |check: Check| holds(&grouping, &schedule, check);
    let eighth = 1.0 / 8.0;
    for (what, got, want) in [
        (
            "the Saga fetched and played on turn 3",
            share(Box::new(move |v: &PathView<'_>| {
                left_out(v) && v.count_at(3, 2, Counted::In(Zone::Battlefield)) == 1
            })),
            eighth,
        ),
        (
            "Brainstorm by turn 3",
            share(Box::new(move |v: &PathView<'_>| {
                left_out(v) && v.count_at(3, 6, Counted::Cast) == 1
            })),
            0.0,
        ),
        (
            "Brainstorm by turn 4",
            share(Box::new(move |v: &PathView<'_>| {
                left_out(v) && v.count_at(4, 6, Counted::Cast) == 1
            })),
            eighth,
        ),
    ] {
        assert!((got - want).abs() < 1e-12, "{what}: {got}, not {want}");
    }
}

// --- discard (ADR-0017 §3) --------------------------------------------------
//
// HANDS.md hands 21 to 24, each dealt as it is written: the opener, what each
// turn draws, and the block a spell's draw turns over.

/// The queries hands 21 to 24 ask: 0 Frantic Search, 1 Life from the Loam,
/// 2 land, 3 Beast Within, 4 Desperate Ravings, 5 Borborygmos and Fblthp,
/// 6 Forest.
const FRANTIC: usize = 0;
const THE_LOAM: usize = 1;
const LAND: usize = 2;
const BEAST_WITHIN: usize = 3;
const RAVINGS: usize = 4;
const BORBORYGMOS: usize = 5;
const A_FOREST: usize = 6;

/// What each card of hands 21 to 24 is to the engine: the queries it answers
/// and what it does for mana.
fn discard_card(name: &str) -> (u64, ManaSource) {
    let castable = |cost: &str, resolves| ManaSource::Castable {
        cost: Cost::parse(cost).unwrap().demand(),
        resolves,
    };
    match name {
        "Island" => (1 << LAND, untapped("U")),
        "Forest" => (1 << LAND | 1 << A_FOREST, untapped("G")),
        "Mountain" => (1 << LAND, untapped("R")),
        "Frantic Search" => (1 << FRANTIC, castable("{2}{U}", Resolves::IntoGraveyard)),
        "Life from the Loam" => (1 << THE_LOAM, castable("{1}{G}", Resolves::IntoGraveyard)),
        "Beast Within" => (1 << BEAST_WITHIN, ManaSource::Spell),
        "Desperate Ravings" => (1 << RAVINGS, castable("{1}{R}", Resolves::IntoGraveyard)),
        "Borborygmos and Fblthp" => (
            1 << BORBORYGMOS,
            castable("{2}{G}{U}{R}", Resolves::OntoBattlefield),
        ),
        _ => panic!("{name} is not in hands 21 to 24"),
    }
}

/// A deck of hands 21 to 24's cards, in the order they are given, and the
/// commander in the command zone where there is one.
struct DiscardDeck {
    grouping: Grouping,
    names: Vec<&'static str>,
}

impl DiscardDeck {
    fn new(cards: &[(&'static str, u32)], commander: Option<&'static str>) -> DiscardDeck {
        let grouping = Grouping::with_mana(
            q(&[
                "frantic",
                "loam",
                "land",
                "beast",
                "ravings",
                "borborygmos",
                "forest",
            ]),
            cards.iter().map(|&(name, n)| {
                let (mask, mana) = discard_card(name);
                (mask, mana, n)
            }),
        )
        .unwrap();
        let mut names: Vec<&'static str> = cards.iter().map(|&(name, _)| name).collect();
        let grouping = match commander {
            Some(name) => {
                let (mask, mana) = discard_card(name);
                names.push(name);
                grouping.with_command_zone([(mask, mana, 1)])
            }
            None => grouping,
        };
        assert_eq!(names.len(), grouping.group_sizes().len());
        DiscardDeck { grouping, names }
    }

    /// A history, one list of cards per checkpoint: what that checkpoint
    /// turned over.
    fn dealt(&self, rows: &[&[(&str, u32)]]) -> Vec<Vec<u32>> {
        let mut total = vec![0u32; self.names.len()];
        rows.iter()
            .map(|row| {
                for &(name, n) in *row {
                    let g = self.names.iter().position(|&m| m == name).unwrap();
                    total[g] += n;
                }
                total.clone()
            })
            .collect()
    }

    /// The same counts as one opener, for a walk resumed from it.
    fn opener(&self, cards: &[(&str, u32)]) -> Vec<u32> {
        self.dealt(&[cards]).remove(0)
    }
}

fn discarding(matched_by: usize, draw: u32, discard: Discard, untap: u32) -> Effect {
    Effect {
        matched_by,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: None,
        delay: None,
        draw,
        mill: None,
        activation: None,
        discard: Some(discard),
        untap,
    }
}

/// Frantic Search: draw two, then discard two, then untap three lands.
fn frantic_search() -> Effect {
    discarding(
        FRANTIC,
        2,
        Discard {
            cards: Discards::Exactly(2),
            at_random: false,
            only: None,
        },
        3,
    )
}

/// Every hand here plays its lands by a declared drop — the discard is the
/// second claimant on the lands in hand — and casts `line`.
fn discard_schedule(
    gaps: &[u32],
    effects: Vec<Effect>,
    land_drop: Vec<usize>,
    line: Vec<usize>,
    discard: Option<Vec<usize>>,
) -> Schedule {
    Schedule::plain_with_fetches(
        gaps,
        effects,
        Policies {
            land_drop: Some(LandDropPolicy::new(land_drop, LAND)),
            casting: Some(CastingPolicy::new(line)),
            discard: discard.map(DiscardPolicy::new),
            ..Policies::default()
        },
    )
}

#[test]
fn hand_21_frantic_search_and_the_discard_list_decides_where_the_loam_goes() {
    // Island x3, Frantic Search and Beast Within x3 in hand; the library, top
    // first, Beast Within, Beast Within, Life from the Loam, Forest. On the
    // play turns 2 and 3 draw a Beast Within each, and turn 3's third Island
    // casts Frantic Search: it draws the Loam and the Forest, then discards
    // two of the seven in hand.
    let deck = DiscardDeck::new(
        &[
            ("Island", 3),
            ("Forest", 1),
            ("Frantic Search", 1),
            ("Life from the Loam", 1),
            ("Beast Within", 5),
        ],
        None,
    );
    let opener: &[(&str, u32)] = &[("Island", 3), ("Frantic Search", 1), ("Beast Within", 3)];
    let beast: &[(&str, u32)] = &[("Beast Within", 1)];
    let drawn: &[(&str, u32)] = &[("Life from the Loam", 1), ("Forest", 1)];
    let prefix = deck.dealt(&[opener, &[], beast, beast]);
    let path = deck.dealt(&[opener, &[], beast, beast, drawn]);
    // (the Loam and then lands, Beast Within)
    let columns = [vec![THE_LOAM, LAND], vec![BEAST_WITHIN]];
    let mut rows = Vec::new();
    for prefer in columns {
        let schedule = discard_schedule(
            &[7, 0, 1, 1],
            vec![frantic_search()],
            vec![],
            vec![FRANTIC],
            Some(prefer),
        );
        let mut board = Board::new(&deck.grouping, &schedule);
        board.walk(&prefix);
        assert_eq!(board.next_gap(), 2, "Frantic Search draws two");
        board.walk(&path);
        assert_eq!(board.next_gap(), 0);
        let at = |query, zone| board.count_at(3, query, Counted::In(zone));
        rows.push((
            board.count_at(3, FRANTIC, Counted::Cast),
            at(THE_LOAM, Zone::Graveyard),
            at(THE_LOAM, Zone::Hand),
            at(LAND, Zone::Graveyard),
        ));
    }
    assert_eq!(
        rows,
        [(1, 1, 0, 1), (1, 0, 1, 0)],
        "the list bins the Loam and the Forest it drew, or two Beast Within and keeps the Loam"
    );
}

#[test]
fn hand_22_desperate_ravings_discards_at_random_whatever_the_list_says() {
    // Mountain x2, Desperate Ravings and Beast Within x4 in hand; the library
    // is Beast Within, Life from the Loam and a Forest. Turn 2 draws one of
    // them, plays the second land and casts Ravings, which draws the other
    // two: whichever order they came in, the hand is five Beast Within, the
    // Loam and a land, and one of the seven goes at random.
    let deck = DiscardDeck::new(
        &[
            ("Mountain", 2),
            ("Forest", 1),
            ("Desperate Ravings", 1),
            ("Life from the Loam", 1),
            ("Beast Within", 5),
        ],
        None,
    );
    let opener = deck.opener(&[
        ("Mountain", 2),
        ("Desperate Ravings", 1),
        ("Beast Within", 4),
    ]);
    let ravings = discarding(
        RAVINGS,
        2,
        Discard {
            cards: Discards::Exactly(1),
            at_random: true,
            only: None,
        },
        0,
    );
    let answering = Answering::all(only_criteria(3));
    let mut columns = Vec::new();
    for prefer in [Some(vec![THE_LOAM]), None] {
        let schedule = discard_schedule(
            &[7, 0, 1],
            vec![ravings.clone()],
            vec![],
            vec![RAVINGS],
            prefer,
        );
        let at = |query, zone| move |v: &PathView<'_>| v.count_at(2, query, Counted::In(zone)) == 1;
        let mut ev = Closures(vec![
            Box::new(at(THE_LOAM, Zone::Graveyard)),
            Box::new(at(THE_LOAM, Zone::Hand)),
            Box::new(at(LAND, Zone::Graveyard)),
        ]);
        let mut conditionals = Conditionals::new(
            &deck.grouping,
            &schedule,
            &answering,
            &mut ev,
            Table::default(),
        )
        .unwrap();
        let rest = conditionals.get(&opener, &vec![0; opener.len()]).unwrap();
        columns.push(rest.held.clone());
    }
    for held in &columns {
        for (got, want) in held.iter().zip([1.0 / 7.0, 6.0 / 7.0, 1.0 / 7.0]) {
            assert!((got - want).abs() < 1e-12, "{columns:?}");
        }
    }
}

/// Borborygmos and Fblthp's enter: draw a card, then discard any number of
/// land cards.
fn borborygmos() -> Effect {
    discarding(
        BORBORYGMOS,
        1,
        Discard {
            cards: Discards::AnyNumber,
            at_random: false,
            only: Some(LAND),
        },
        0,
    )
}

#[test]
fn hand_23_borborygmos_and_fblthp_cannot_discard_the_loam() {
    // Forest x2, Island x2, Mountain x2 and Life from the Loam, and nothing
    // drawn until turn 5, whose five drops leave a Mountain in hand. Turn 5
    // casts the commander out of the command zone, and it enters: draw the
    // Forest, then any number of land cards may go.
    let deck = DiscardDeck::new(
        &[
            ("Island", 2),
            ("Forest", 3),
            ("Mountain", 2),
            ("Life from the Loam", 1),
        ],
        Some("Borborygmos and Fblthp"),
    );
    let opener: &[(&str, u32)] = &[
        ("Island", 2),
        ("Forest", 2),
        ("Mountain", 2),
        ("Life from the Loam", 1),
    ];
    let prefix = deck.dealt(&[opener, &[], &[], &[], &[], &[]]);
    let path = deck.dealt(&[opener, &[], &[], &[], &[], &[], &[("Forest", 1)]]);
    // (the Loam and then lands, the Loam alone, no list)
    let columns = [Some(vec![THE_LOAM, LAND]), Some(vec![THE_LOAM]), None];
    let mut rows = Vec::new();
    for prefer in columns {
        let schedule = discard_schedule(
            &[7, 0, 0, 0, 0, 0],
            vec![borborygmos()],
            vec![],
            vec![BORBORYGMOS],
            prefer,
        );
        let mut board = Board::new(&deck.grouping, &schedule);
        board.walk(&prefix);
        assert_eq!(board.next_gap(), 1, "it draws a card as it enters");
        board.walk(&path);
        assert_eq!(board.next_gap(), 0);
        let at = |query, zone| board.count_at(5, query, Counted::In(zone));
        rows.push((
            board.count_at(5, BORBORYGMOS, Counted::Cast),
            at(THE_LOAM, Zone::Graveyard),
            at(LAND, Zone::Graveyard),
            at(THE_LOAM, Zone::Hand),
        ));
    }
    assert_eq!(
        rows,
        [(1, 0, 2, 1), (1, 0, 0, 1), (1, 0, 0, 1)],
        "the card says land cards, so the Loam stays whatever the list says; any number is \
         every land the list names; and with no list nothing goes"
    );
}

#[test]
fn hand_24_a_spell_drawn_mid_line_is_cast_and_a_land_drawn_mid_line_waits() {
    // A Forest and two Islands down by turn 3, the last of them turn 3's drop,
    // and Frantic Search and Beast Within x2 in hand; the library, top first,
    // Life from the Loam, Forest. The three lands cast Frantic Search, which
    // draws both and untaps them, and the line is read again from its top.
    let deck = DiscardDeck::new(
        &[
            ("Island", 2),
            ("Forest", 2),
            ("Frantic Search", 1),
            ("Life from the Loam", 1),
            ("Beast Within", 2),
        ],
        None,
    );
    let opener: &[(&str, u32)] = &[
        ("Island", 2),
        ("Forest", 1),
        ("Frantic Search", 1),
        ("Beast Within", 2),
    ];
    let drawn: &[(&str, u32)] = &[("Life from the Loam", 1), ("Forest", 1)];
    let path = deck.dealt(&[opener, &[], &[], &[], drawn]);
    // (Beast Within, the Loam and then Beast Within)
    let columns = [vec![BEAST_WITHIN], vec![THE_LOAM, BEAST_WITHIN]];
    let mut rows = Vec::new();
    for prefer in columns {
        let schedule = discard_schedule(
            &[6, 0, 0, 0],
            vec![frantic_search()],
            vec![A_FOREST],
            vec![FRANTIC, THE_LOAM],
            Some(prefer),
        );
        let mut board = Board::new(&deck.grouping, &schedule);
        board.walk(&path);
        assert_eq!(board.next_gap(), 0);
        let forests = |zone| board.count_at(3, A_FOREST, Counted::In(zone));
        rows.push((
            board.count_at(3, FRANTIC, Counted::Cast),
            board.count_at(3, THE_LOAM, Counted::Cast),
            board.count_at(3, THE_LOAM, Counted::In(Zone::Graveyard)),
            board.count_at(3, LAND, Counted::In(Zone::Battlefield)),
            // Held and not played: the hand counts a land from the draw on.
            forests(Zone::Hand) - forests(Zone::Battlefield),
        ));
    }
    assert_eq!(
        rows,
        [(1, 1, 1, 3, 1), (1, 0, 1, 3, 1)],
        "the untap pays for the Loam it drew; the list that bins it casts nothing more; and \
         the Forest it drew waits in hand either way"
    );
}

// --- Attack and landfall triggers mill (#89) ---------------------------------
//
// HANDS.md hands 58 to 60: a mill that fires off an attack, off a land
// entering, and a mill that returns the lands it finds. Each is a sized gap
// on the paths where it fires, played on the board both engines play.

/// Groups, in this order: Forest, the trigger's creature, Life from the Loam,
/// Mountain, Island, Beast Within. Queries: 0 the creature, 1 Loam, 2 Forest,
/// 3 land, 4 Beast Within.
fn trigger_groups(creature: ManaSource, forests: u32, beasts: u32) -> Grouping {
    Grouping::with_mana(
        q(&["creature", "loam", "forest", "land", "beast"]),
        vec![
            (0b01100, untapped("G"), forests),
            (0b00001, creature, 1),
            (0b00010, ManaSource::Spell, 1),
            (0b01000, untapped("R"), 1),
            (0b01000, untapped("U"), 1),
            (0b10000, ManaSource::Spell, beasts),
        ],
    )
    .unwrap()
}

fn creature(cost: &str) -> ManaSource {
    ManaSource::Castable {
        cost: Cost::parse(cost).unwrap().demand(),
        resolves: Resolves::OntoBattlefield,
    }
}

/// A history over [`trigger_groups`], one row per checkpoint: `(Forest,
/// creature, Loam, Mountain, Island, Beast Within)`.
fn dealt6(rows: &[[u32; 6]]) -> Vec<Vec<u32>> {
    let mut total = [0u32; 6];
    rows.iter()
        .map(|row| {
            for (t, r) in total.iter_mut().zip(row) {
                *t += r;
            }
            total.to_vec()
        })
        .collect()
}

const NONE6: [u32; 6] = [0; 6];
const BEAST6: [u32; 6] = [0, 0, 0, 0, 0, 1];
const LOAM6: [u32; 6] = [0, 0, 1, 0, 0, 0];
const ISLAND6: [u32; 6] = [0, 0, 0, 0, 1, 0];
const MOUNTAIN6: [u32; 6] = [0, 0, 0, 1, 0, 0];

/// Six: "Whenever Six attacks, mill three cards. You may put a land card
/// from among them into your hand." The file keeps a land.
fn six(trigger: Trigger) -> Effect {
    Effect {
        trigger,
        ..milling(Some(Mill {
            cards: MillDepth::Exactly(3),
            to_hand: ToHand::Chosen {
                up_to: 1,
                of: Some(3),
                prefer: vec![3],
            },
            returns: None,
        }))
    }
}

#[test]
fn hand_58_six_attacks_the_turn_after_it_is_cast() {
    // Forest x3, Six and Beast Within x3 in hand; the library, top first,
    // Beast Within x3, then Loam, Mountain, Beast Within, then Island, then
    // Beast Within, Forest, Beast Within. On the play. Six is cast on turn 3
    // off three Forests and is summoning-sick; it attacks on turn 4 and 5.
    let grouping = trigger_groups(creature("{2}{G}"), 4, 9);
    let (six_q, loam, land, beast) = (0, 1, 3, 4);
    let schedule = |effects| {
        Schedule::plain_with_fetches(
            &[7, 0, 1, 1, 1, 1],
            effects,
            Policies::casting(CastingPolicy::new(vec![six_q])),
        )
    };
    let opener = [3, 1, 0, 0, 0, 3];

    // Today Six does nothing, and turn 5 draws the Loam.
    let today = schedule(vec![]);
    let mut board = Board::new(&grouping, &today);
    board.walk(&dealt6(&[opener, NONE6, BEAST6, BEAST6, BEAST6, LOAM6]));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(3, six_q, Counted::Cast), 1);
    assert_eq!(board.count_at(5, loam, Counted::In(Zone::Hand)), 1);
    assert_eq!(board.count_at(5, loam, Counted::In(Zone::Graveyard)), 0);

    let attacks = schedule(vec![six(Trigger::Attack)]);
    let mut board = Board::new(&grouping, &attacks);
    // Summoning-sick: the turn it is cast asks for nothing.
    board.walk(&dealt6(&[opener, NONE6, BEAST6, BEAST6]));
    assert_eq!(
        board.next_gap(),
        0,
        "Six does not attack the turn it is cast"
    );
    board.walk(&dealt6(&[opener, NONE6, BEAST6, BEAST6, BEAST6]));
    assert_eq!(board.next_gap(), 3, "it attacks on turn 4 and mills three");
    let first = [0, 0, 1, 1, 0, 1];
    let second = [1, 0, 0, 0, 0, 2];
    let path = dealt6(&[
        opener, NONE6, BEAST6, BEAST6, BEAST6, first, ISLAND6, second,
    ]);
    board.walk(&path);
    assert_eq!(board.next_gap(), 0);
    assert_eq!(
        board.count_at(3, six_q, Counted::Cast),
        1,
        "what a creature does cannot change whether it was paid for"
    );
    assert_eq!(board.count_at(3, loam, Counted::In(Zone::Graveyard)), 0);
    assert_eq!(board.count_at(4, loam, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(5, loam, Counted::In(Zone::Hand)), 0);
    assert_eq!(board.count_at(4, beast, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(5, beast, Counted::In(Zone::Graveyard)), 3);
    assert_eq!(
        board.count_at(5, land, Counted::In(Zone::Graveyard)),
        0,
        "each attack keeps the land it found"
    );
    // The Mountain kept on turn 4 came after that turn's drop, and is played
    // on turn 5, and the Forest kept on turn 5 waits for turn 6.
    assert_eq!(board.count_at(4, land, Counted::In(Zone::Battlefield)), 3);
    assert_eq!(board.count_at(5, land, Counted::In(Zone::Battlefield)), 4);
}

/// Icetill Explorer: "Landfall — Whenever a land you control enters, mill a
/// card."
fn explorer() -> Effect {
    Effect {
        trigger: Trigger::Landfall,
        ..milling(Some(Mill::all(1)))
    }
}

#[test]
fn hand_59_icetill_explorer_mills_one_for_each_land_after_it() {
    // Forest x4, Icetill Explorer and Beast Within x2 in hand; the library,
    // top first, Beast Within, Beast Within, Mountain, Beast Within, Life from
    // the Loam, Beast Within. On the play. Turn 4 plays the fourth Forest and
    // then casts the Explorer, so that drop fires nothing; turn 5's Mountain
    // enters with it on the battlefield and mills the Loam. Turn 6 has no
    // land to play and mills nothing.
    let grouping = trigger_groups(creature("{2}{G}{G}"), 4, 6);
    let (explorer_q, loam, land, beast) = (0, 1, 3, 4);
    let schedule = |effects| {
        Schedule::plain_with_fetches(
            &[7, 0, 1, 1, 1, 1, 1],
            effects,
            Policies::casting(CastingPolicy::new(vec![explorer_q])),
        )
    };
    let opener = [4, 1, 0, 0, 0, 2];

    let today = schedule(vec![]);
    let mut board = Board::new(&grouping, &today);
    board.walk(&dealt6(&[
        opener, NONE6, BEAST6, BEAST6, MOUNTAIN6, BEAST6, LOAM6,
    ]));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(4, explorer_q, Counted::Cast), 1);
    assert_eq!(board.count_at(6, loam, Counted::In(Zone::Hand)), 1);

    let landfall = schedule(vec![explorer()]);
    let mut board = Board::new(&grouping, &landfall);
    board.walk(&dealt6(&[opener, NONE6, BEAST6, BEAST6, MOUNTAIN6]));
    assert_eq!(
        board.next_gap(),
        0,
        "the drop the turn it is cast came before it"
    );
    board.walk(&dealt6(&[opener, NONE6, BEAST6, BEAST6, MOUNTAIN6, BEAST6]));
    assert_eq!(board.next_gap(), 1, "turn 5's land mills one");
    let path = dealt6(&[
        opener, NONE6, BEAST6, BEAST6, MOUNTAIN6, BEAST6, LOAM6, BEAST6,
    ]);
    board.walk(&path);
    assert_eq!(board.next_gap(), 0, "no land on turn 6, so no mill");
    assert_eq!(board.count_at(4, explorer_q, Counted::Cast), 1);
    assert_eq!(board.count_at(4, loam, Counted::In(Zone::Graveyard)), 0);
    assert_eq!(board.count_at(5, loam, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(6, loam, Counted::In(Zone::Hand)), 0);
    assert_eq!(board.count_at(6, beast, Counted::In(Zone::Graveyard)), 0);
    assert_eq!(board.count_at(5, land, Counted::In(Zone::Battlefield)), 5);
}

/// Groups, in this order: Forest, Lumra, Life from the Loam, Mountain,
/// Island, Beast Within, and a second creature. Queries: 0 Lumra, 1 Loam,
/// 2 Forest, 3 land, 4 Beast Within, 5 the second creature.
fn lumra_groups(second: ManaSource, forests: u32, beasts: u32) -> Grouping {
    Grouping::with_mana(
        q(&["lumra", "loam", "forest", "land", "beast", "second"]),
        vec![
            (0b001100, untapped("G"), forests),
            (0b000001, creature("{4}{G}{G}"), 1),
            (0b000010, ManaSource::Spell, 1),
            (0b001000, untapped("R"), 1),
            (0b001000, untapped("U"), 1),
            (0b010000, ManaSource::Spell, beasts),
            (0b100000, second, 1),
        ],
    )
    .unwrap()
}

/// Lumra, Bellow of the Woods: "When Lumra enters, mill four cards. Then
/// return all land cards from your graveyard to the battlefield tapped."
fn lumra(returns: Option<usize>) -> Effect {
    milling(Some(Mill {
        returns,
        ..Mill::all(4)
    }))
}

/// `(Forest, Mountain, Island, Loam, Beast Within)` as a row over
/// [`lumra_groups`].
fn lumra_row(forest: u32, mountain: u32, island: u32, loam: u32, beast: u32) -> [u32; 7] {
    [forest, 0, loam, mountain, island, beast, 0]
}

/// Rows over [`lumra_groups`], summed into a history.
fn lumra_path(rows: &[[u32; 7]]) -> Vec<Vec<u32>> {
    let mut total = [0u32; 7];
    rows.iter()
        .map(|r| {
            for (t, x) in total.iter_mut().zip(r) {
                *t += x;
            }
            total.to_vec()
        })
        .collect()
}

#[test]
fn hand_60_lumra_returns_every_land_in_the_graveyard_and_the_loam_stays() {
    // Forest x5, Lumra and Aftermath Analyst in hand; the library, top first,
    // Beast Within, then the Analyst's three: Mountain, Beast Within, Beast
    // Within; then Beast Within x3, Forest, and Lumra's four: Life from the
    // Loam, Island, Beast Within, Forest. On the play. The Analyst is cast on
    // turn 2 and bins the Mountain; Lumra is cast on turn 6 off six Forests,
    // mills four, and every land in the graveyard comes back, the Mountain
    // the Analyst milled with them.
    let grouping = lumra_groups(creature("{1}{G}"), 7, 7);
    let (lumra_q, loam, land, beast, analyst) = (0, 1, 3, 4, 5);
    let schedule = |returns| {
        let analysts = Effect {
            matched_by: analyst,
            ..milling(Some(Mill::all(3)))
        };
        Schedule::plain_with_fetches(
            &[7, 0, 1, 1, 1, 1, 1],
            vec![lumra(returns), analysts],
            Policies::casting(CastingPolicy::new(vec![lumra_q, analyst])),
        )
    };
    let b = lumra_row(0, 0, 0, 0, 1);
    let path = lumra_path(&[
        [5, 1, 0, 0, 0, 0, 1],
        [0; 7],
        b,
        lumra_row(0, 1, 0, 0, 2),
        b,
        b,
        b,
        lumra_row(1, 0, 0, 0, 0),
        lumra_row(1, 0, 1, 1, 1),
    ]);
    let mut rows = Vec::new();
    for returns in [None, Some(land)] {
        let schedule = schedule(returns);
        let mut board = Board::new(&grouping, &schedule);
        board.walk(&path[..8]);
        assert_eq!(board.next_gap(), 4, "Lumra mills four");
        board.walk(&path);
        assert_eq!(board.next_gap(), 0);
        assert_eq!(board.count_at(6, lumra_q, Counted::Cast), 1);
        assert_eq!(board.count_at(5, land, Counted::In(Zone::Graveyard)), 1);
        let at = |query, zone| board.count_at(6, query, Counted::In(zone));
        rows.push((
            at(loam, Zone::Graveyard),
            at(land, Zone::Graveyard),
            at(beast, Zone::Graveyard),
            at(land, Zone::Battlefield),
            at(land, Zone::Library),
        ));
    }
    assert_eq!(
        rows,
        [(1, 3, 3, 6, 0), (1, 0, 3, 9, 0)],
        "a mill that returns nothing leaves three lands in the yard; Lumra puts all \
         three onto the battlefield, the Analyst's Mountain with its own two, \
         and the Loam, a sorcery, stays"
    );
}

#[test]
fn each_land_lumra_returns_fires_a_landfall() {
    // Forest x5, Lumra and Icetill Explorer in hand; the library, top first,
    // Beast Within x2, Forest, Beast Within, Forest, Beast Within, Beast
    // Within, then Lumra's four: the Loam, Island, Mountain, Beast Within,
    // then Beast Within x3. The Explorer is cast on turn 4 and mills one on
    // turn 5's drop, a Forest, and one on turn 6's; Lumra, cast on turn 6,
    // returns the Island, the Mountain and that Forest, and each fires the
    // Explorer once more.
    let grouping = lumra_groups(creature("{2}{G}{G}"), 7, 9);
    let (lumra_q, loam, land, beast, explorer_q) = (0, 1, 3, 4, 5);
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 1, 1, 1, 1, 1],
        vec![
            lumra(Some(land)),
            Effect {
                matched_by: explorer_q,
                ..explorer()
            },
        ],
        Policies::casting(CastingPolicy::new(vec![lumra_q, explorer_q])),
    );
    let b = lumra_row(0, 0, 0, 0, 1);
    let f = lumra_row(1, 0, 0, 0, 0);
    let path = lumra_path(&[
        [5, 1, 0, 0, 0, 0, 1],
        [0; 7],
        b,
        b,
        f,                        // turn 4's draw; four Forests down, and the Explorer is cast
        b,                        // turn 5's draw, and a fifth Forest is played: a landfall
        f,                        // which mills a Forest
        b,                        // turn 6's draw, and the sixth Forest is played
        b,                        // which mills this
        lumra_row(0, 1, 1, 1, 1), // Lumra's four
        b,
        b,
        b, // one each for the Island, the Mountain and the milled Forest
    ]);
    let mut board = Board::new(&grouping, &schedule);
    board.walk(&path);
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(4, explorer_q, Counted::Cast), 1);
    assert_eq!(board.count_at(6, lumra_q, Counted::Cast), 1);
    assert_eq!(board.count_at(5, land, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(6, loam, Counted::In(Zone::Graveyard)), 1);
    assert_eq!(board.count_at(6, land, Counted::In(Zone::Graveyard)), 0);
    assert_eq!(board.count_at(6, beast, Counted::In(Zone::Graveyard)), 5);
    assert_eq!(board.count_at(6, land, Counted::In(Zone::Battlefield)), 9);
}

#[test]
fn a_fetchland_is_two_lands_entering_and_fires_a_landfall_for_each() {
    // Forest x4, Icetill Explorer, a fetchland and Beast Within in hand, and
    // the declared drop plays Forests first. The Explorer is cast on turn 4;
    // turn 5 plays the fetchland, which enters, and then the Forest it finds
    // enters in its place: two lands, two mills.
    let grouping = Grouping::with_mana(
        q(&["explorer", "forest", "land", "fetchland"]),
        vec![
            (0b0110, untapped("G"), 5),
            (0b0001, creature("{2}{G}{G}"), 1),
            (0b1100, untapped("G"), 1),
            (0b0000, ManaSource::Spell, 7),
        ],
    )
    .unwrap();
    let fetchland = Effect {
        matched_by: 3,
        look: 0,
        trigger: Trigger::LandDrop,
        route: Route::Nowhere,
        fetch: Some(Fetch {
            prefer: vec![1],
            to: Fetched::Battlefield,
        }),
        delay: None,
        draw: 0,
        mill: None,
        activation: None,
        discard: None,
        untap: 0,
    };
    let schedule = Schedule::plain_with_fetches(
        &[7, 0, 1, 1, 1, 1],
        vec![explorer(), fetchland],
        Policies {
            land_drop: Some(LandDropPolicy::new(vec![1], 2)),
            casting: Some(CastingPolicy::new(vec![0])),
            ..Policies::default()
        },
    );
    let history = |rows: &[[u32; 4]]| {
        let mut total = [0u32; 4];
        rows.iter()
            .map(|r| {
                for (t, x) in total.iter_mut().zip(r) {
                    *t += x;
                }
                total.to_vec()
            })
            .collect::<Vec<_>>()
    };
    let (opener, none, beast) = ([4, 1, 1, 1], [0; 4], [0, 0, 0, 1]);
    let mut board = Board::new(&grouping, &schedule);
    board.walk(&history(&[opener, none, beast, beast, beast, beast]));
    assert_eq!(board.next_gap(), 1, "the fetchland entering mills one");
    board.walk(&history(&[opener, none, beast, beast, beast, beast, beast]));
    assert_eq!(board.next_gap(), 1, "and the Forest it found, one more");
    board.walk(&history(&[
        opener, none, beast, beast, beast, beast, beast, beast,
    ]));
    assert_eq!(board.next_gap(), 0);
    assert_eq!(board.count_at(4, 0, Counted::Cast), 1);
    assert_eq!(board.count_at(5, 1, Counted::In(Zone::Battlefield)), 5);
    let milled = board.count_at(5, 2, Counted::In(Zone::Library));
    assert_eq!(milled, 0, "every Forest and the fetchland left the library");
}

#[test]
fn a_mill_that_returns_lands_is_dealt_where_it_fired() {
    // Lumra's shape reads the graveyard it filled: a land dealt last would
    // not be there for it to return.
    let lumra = Mill {
        returns: Some(2),
        ..Mill::all(3)
    };
    let (grouping, in_place) = a_milling_deck(lumra, None);
    let last = in_place.clone().deferring(0b110);
    assert_eq!(
        gauntlet_criteria::width(&grouping, &last),
        gauntlet_criteria::width(&grouping, &in_place)
    );
}
