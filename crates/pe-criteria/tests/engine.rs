//! Engine tests using plain Rust closures as the evaluator.
//!
//! The JS runtime is deliberately behind a trait so this logic can be verified
//! without booting V8. A failure here is an engine bug; a failure in pe-js is a
//! bindings bug.

use std::convert::Infallible;

use pe_criteria::{
    Cost, Count, Criterion, Effect, Evaluator, Expectation, Grouping, GroupingError,
    LandDropPolicy, ManaSource, Palette, PathOutcomes, PathView, Plan, Route, RunError, Schedule,
    Trigger, Zone, MAX_COUNT,
};

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
        v.count_in(0, 0, Zone::Hand) >= 1 && v.count_in(0, 1, Zone::Hand) >= 1
    })]);

    let s = pe_criteria::run(&strict, &Schedule::plain(&[11]), only_criteria(1), &mut ev)
        .unwrap()
        .probabilities;
    let w = pe_criteria::run(&wide, &Schedule::plain(&[11]), only_criteria(1), &mut ev)
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
        v.count_in(0, 0, Zone::Hand) >= 1 && v.count_in(0, 1, Zone::Hand) >= 1
    })]);
    let o = pe_criteria::run(
        &overlapping,
        &Schedule::plain(&[11]),
        only_criteria(1),
        &mut ev,
    )
    .unwrap()
    .probabilities[0];
    let d = pe_criteria::run(
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
            v.count_in(0, 0, Zone::Hand) >= 1
                && v.count_in(0, 1, Zone::Hand) >= 1
                && v.count_in(1, 0, Zone::Hand) >= 2
        }),
        Box::new(|v: &PathView<'_>| {
            v.count_in(0, 0, Zone::Hand) >= 1 && v.count_in(0, 1, Zone::Hand) >= 1
        }),
    ]);
    let r = pe_criteria::run(&g, &Schedule::plain(&[7, 1]), only_criteria(2), &mut ev)
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
        v.count_in(1, 0, Zone::Hand) >= v.count_in(0, 0, Zone::Hand)
            && v.count_in(2, 0, Zone::Hand) >= v.count_in(1, 0, Zone::Hand)
    })]);
    let r = pe_criteria::run(&g, &Schedule::plain(&[7, 1, 1]), only_criteria(1), &mut ev)
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
        v.count_in(9, 0, Zone::Hand) >= 1
    })]);
    let r = pe_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev)
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
    let err = pe_criteria::run(&g, &Schedule::plain(&[40]), only_criteria(1), &mut ev).unwrap_err();
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
    let err = pe_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev).unwrap_err();
    assert!(matches!(err, RunError::EmptyLibrary), "{err}");
}

#[test]
fn a_hand_bigger_than_the_library_is_refused_rather_than_answered_zero() {
    // Enumeration yields no paths at all here, so every criterion would collect
    // zero mass and report a confident 0%.
    let g = Grouping::build(q(&["land"]), [(0b1, 1), (0, 1)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_in(0, 0, Zone::Hand) >= 1
    })]);
    let err = pe_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev).unwrap_err();
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
        let r = pe_criteria::run(&g, &Schedule::plain(gaps), only_criteria(1), &mut ev)
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
    // draws * successes / population, in closed form, and `pe_stats::mean`
    // computes it without enumerating anything. An enumerated expectation walks
    // every composition instead and weights each by its exact probability, so
    // the two share nothing but the answer.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Counters(vec![Box::new(|v: &PathView<'_>| {
        v.count_in(0, 0, Zone::Hand)
    })]);
    let r = pe_criteria::run(&g, &Schedule::plain(&[7]), only_expectations(1), &mut ev).unwrap();

    let enumerated = r.distributions[0].mean();
    let closed = pe_stats::mean(99, 36, 7);
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
        v.count_in(0, 0, Zone::Hand)
    })]);
    let r = pe_criteria::run(&g, &Schedule::plain(&[7]), only_expectations(1), &mut ev).unwrap();
    let d = &r.distributions[0];

    assert_eq!(
        d.probabilities().len(),
        8,
        "seven cards drawn, so 0 through 7"
    );
    for k in 0..=7u32 {
        let want = pe_stats::pmf(99, 36, 7, k);
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
            Box::new(|v: &PathView<'_>| v.count_in(0, 0, Zone::Hand)),
            Box::new(|v: &PathView<'_>| {
                v.count_in(0, 0, Zone::Hand) + v.count_in(0, 1, Zone::Hand)
            }),
        ]);
        let r =
            pe_criteria::run(&g, &Schedule::plain(gaps), only_expectations(2), &mut ev).unwrap();
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
        v.count_in(0, 0, Zone::Hand)
    })]);
    let d = pe_criteria::run(
        &g,
        &Schedule::plain(&[7]),
        only_expectations(1),
        &mut counting,
    )
    .unwrap();
    let tail: f64 = d.distributions[0].probabilities()[2..].iter().sum();

    let mut checking = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_in(0, 0, Zone::Hand) >= 2
    })]);
    let p = pe_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut checking).unwrap();

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
    let err = pe_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(2), &mut ev).unwrap_err();
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
            v.count_in(3, 0, Zone::Hand)
        })])
    };
    let plain = pe_criteria::run(
        &g,
        &Schedule::build(3, true, Vec::new(), None),
        only_expectations(1),
        &mut tally(),
    )
    .unwrap();
    let looking = pe_criteria::run(
        &g,
        &Schedule::build(3, true, vec![surveil(Route::Nowhere)], None),
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
            v.count_in(turn, 0, Zone::Graveyard)
        })]);
        let r = pe_criteria::run(
            &g,
            &Schedule::build(turn as u32, true, vec![surveil(Route::Everything)], None),
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
        v.count_in(3, 0, Zone::Hand)
            + v.count_in(3, 0, Zone::Graveyard)
            + v.count_in(3, 0, Zone::Library)
            == 5
    })]);
    let r = pe_criteria::run(
        &g,
        &Schedule::build(3, true, vec![surveil(Route::Matching(0))], None),
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
    pe_criteria::run(grouping, schedule, only_criteria(1), &mut ev)
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
                    move |v: &PathView<'_>| v.count_in(turn, 0, Zone::Hand) == drawn
                        && v.count_in(turn, 0, Zone::Battlefield) == played
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
                Box::new(|v: &PathView<'_>| v.count_in(3, 0, Zone::Hand) >= 1)
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
        let schedule = Schedule::plain_under(&[7, 0, 0], LandDropPolicy::new(vec![prefer], 2));
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
        let schedule = Schedule::plain_under(&[7, 0, 0], LandDropPolicy::new(vec![prefer], 2));
        holds(
            &grouping,
            &schedule,
            Box::new(move |v: &PathView<'_>| v.count_in(1, query, Zone::Battlefield) == 1),
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
    let schedule = Schedule::plain_under(&[7, 0, 0], LandDropPolicy::new(vec![0], 2));
    assert_eq!(
        holds(
            &grouping,
            &schedule,
            Box::new(|v: &PathView<'_>| v.count_in(2, 2, Zone::Battlefield) == 2)
        ),
        1.0,
        "both lands are played by turn 2, ranked or not"
    );
}
