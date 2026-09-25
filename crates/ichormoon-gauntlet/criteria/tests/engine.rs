//! Engine tests using plain Rust closures as the evaluator.
//!
//! The JS runtime is deliberately behind a trait so this logic can be verified
//! without booting V8. A failure here is an engine bug; a failure in pe-js is a
//! bindings bug.

use std::convert::Infallible;

use gauntlet_criteria::{Answering, Chosen, Conditionals, Objective, Table};
use gauntlet_criteria::{
    CastingPolicy, Cost, Count, Counted, Criterion, Delay, Effect, Evaluator, Expectation, Fetch,
    Fetched, Grouping, GroupingError, Keep, LandDetail, LandDropPolicy, ManaSource, MulliganPolicy,
    Palette, PathOutcomes, PathView, Plan, Policies, Route, RunError, Schedule, Trigger, Zone,
    MAX_COUNT,
};
use std::sync::Arc;

type Check = Box<dyn FnMut(&PathView<'_>) -> bool>;
type Tally = Box<dyn FnMut(&PathView<'_>) -> u32>;

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
    };
    assert_eq!(c.at_least, Some(0.55));
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
    }
}

fn tapped(letters: &str) -> ManaSource {
    ManaSource::Land {
        enters_tapped: true,
        produces: Palette::from_letters([letters]),
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
    };
    let blue = ManaSource::Castable {
        cost: Cost::parse("{1}{U}").unwrap().demand(),
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
                },
                1,
            ),
            (0b10, ManaSource::Spell, 1),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
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
        &Schedule::plain_with_fetches(&gaps, vec![tutor(Fetched::Hand)], line()),
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
        &Schedule::plain_with_fetches(&gaps, vec![tutor(Fetched::Hand)], line()),
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
                },
                2,
            ),
            (0b10, ManaSource::Spell, 1),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
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
            vec![tutor(Fetched::Hand)],
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
    let saga_land = ManaSource::Land {
        enters_tapped: false,
        produces: Palette::from_letters(["C"]),
    };
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
            to: Fetched::Hand,
        }),
        delay: None,
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
