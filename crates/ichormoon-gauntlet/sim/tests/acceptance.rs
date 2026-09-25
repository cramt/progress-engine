//! The acceptance test for the shuffler.
//!
//! The rule inherited from this project's predecessor: never touch the shuffling
//! without re-running these. Two hand-rolled PRNGs were rejected there for
//! producing confidently wrong numbers, and both would have passed a casual
//! eyeball. The check that caught them is the hypergeometric distribution, so
//! that is what is asserted here.

use std::convert::Infallible;

use gauntlet_criteria::{Answering, Chosen, Conditionals, LandDetail, Objective, Table};
use gauntlet_criteria::{
    CastingPolicy, Cost, Count, Counted, Delay, Effect, Evaluator, Fetch, Fetched, Grouping, Keep,
    LandDropPolicy, ManaSource, MulliganPolicy, Palette, PathOutcomes, PathView, Plan, Policies,
    Route, Schedule, Trigger, Zone,
};
use gauntlet_sim::{mean_standard_error, simulate, standard_error, SimError};
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
                .map(|f| Count::new(f(view)).expect("a count of a drawn card is in range"))
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

const TRIALS: u32 = 200_000;

#[test]
fn the_shuffler_reproduces_the_documented_land_distribution() {
    // 36 lands in a 99-card library, opening seven: mean 2.5455, SD 1.2331.
    // These are the constants the predecessor's own acceptance test used.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();

    // P(X >= k) for each k, from which the distribution follows.
    let mut ev = Closures(
        (0..=7)
            .map(|k| {
                Box::new(move |v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand)) >= k)
                    as Check
            })
            .collect(),
    );
    let at_least = simulate(
        &g,
        &Schedule::plain(&[7]),
        TRIALS,
        0xC0FFEE,
        only_criteria(8),
        &mut ev,
    )
    .unwrap()
    .proportions;

    let pmf: Vec<f64> = (0..=7)
        .map(|k| at_least[k] - at_least.get(k + 1).copied().unwrap_or(0.0))
        .collect();

    let mean: f64 = pmf.iter().enumerate().map(|(k, p)| k as f64 * p).sum();
    let ex2: f64 = pmf
        .iter()
        .enumerate()
        .map(|(k, p)| (k * k) as f64 * p)
        .sum();
    let sd = (ex2 - mean * mean).sqrt();

    // Three standard errors of the mean, as the documented test specifies.
    let tolerance = 3.0 * 1.2331 / f64::from(TRIALS).sqrt();
    assert!(
        (mean - 2.5455).abs() < tolerance,
        "mean {mean} outside {tolerance} of 2.5455"
    );
    assert!((sd - 1.2331).abs() < 0.02, "sd was {sd}");
}

#[test]
fn sampling_agrees_with_the_exact_engine() {
    // The exact engine is the oracle. Where both can answer, they must agree.
    let cases: [(&[(u64, u32)], u32); 3] = [
        (&[(0b01, 6), (0b10, 8), (0, 85)], 11),
        (&[(0b01, 6), (0b10, 12), (0, 81)], 11),
        (&[(0b11, 2), (0b01, 4), (0b10, 6), (0, 87)], 9),
    ];

    for (cards, draws) in cases {
        let g = Grouping::build(q(&["a", "b"]), cards.to_vec()).unwrap();

        let exact = chip_stats::probability_that(g.group_sizes(), draws, |c| {
            g.count_matching(c, 0) >= 1 && g.count_matching(c, 1) >= 1
        });

        let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
                && v.count_at(0, 1, Counted::In(Zone::Hand)) >= 1
        })]);
        let sampled = simulate(
            &g,
            &Schedule::plain(&[draws]),
            TRIALS,
            42,
            only_criteria(1),
            &mut ev,
        )
        .unwrap()
        .proportions[0];

        let se = standard_error(sampled, TRIALS);
        assert!(
            (sampled - exact.get()).abs() < 4.0 * se,
            "sampled {sampled} vs exact {} ({}x SE)",
            exact.get(),
            (sampled - exact.get()).abs() / se
        );
    }
}

#[test]
fn checkpoints_agree_with_the_exact_engine() {
    // The multi-turn path, which is the fiddlier of the two enumerators.
    let g = Grouping::build(q(&["land", "dork"]), [(0b01, 36), (0b10, 10), (0, 53)]).unwrap();
    let pred = |h: &[Vec<u32>]| h[0][0] >= 1 && h[0][1] >= 1 && h[1][0] >= 2;

    let exact = chip_stats::probability_that_path(g.group_sizes(), &[7, 1], pred);
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
            && v.count_at(0, 1, Counted::In(Zone::Hand)) >= 1
            && v.count_at(1, 0, Counted::In(Zone::Hand)) >= 2
    })]);
    let sampled = simulate(
        &g,
        &Schedule::plain(&[7, 1]),
        TRIALS,
        7,
        only_criteria(1),
        &mut ev,
    )
    .unwrap()
    .proportions[0];

    let se = standard_error(sampled, TRIALS);
    assert!(
        (sampled - exact.get()).abs() < 4.0 * se,
        "sampled {sampled} vs exact {}",
        exact.get()
    );
}

#[test]
fn the_same_seed_deals_the_same_hands() {
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let run = |seed: u64| {
        let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
            v.count_at(0, 0, Counted::In(Zone::Hand)) >= 3
        })]);
        simulate(
            &g,
            &Schedule::plain(&[7]),
            5_000,
            seed,
            only_criteria(1),
            &mut ev,
        )
        .unwrap()
        .proportions[0]
    };
    assert_eq!(run(123), run(123));
    assert_ne!(
        run(123),
        run(124),
        "different seeds should deal differently"
    );
}

#[test]
fn drawing_the_whole_library_is_not_an_infinite_loop() {
    let g = Grouping::build(q(&["land"]), [(0b1, 4), (0, 6)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) == 4
    })]);
    let p = simulate(
        &g,
        &Schedule::plain(&[10]),
        100,
        1,
        only_criteria(1),
        &mut ev,
    )
    .unwrap()
    .proportions[0];
    assert_eq!(p, 1.0, "drawing every card must find every land");
}

#[test]
fn a_hand_bigger_than_the_library_is_refused_rather_than_clamped() {
    // The sampler used to deal what it could and answer 100% for a question the
    // exact engine answered 0%. Both refuse now.
    let g = Grouping::build(q(&["land"]), [(0b1, 1), (0, 1)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
    })]);
    let err = simulate(
        &g,
        &Schedule::plain(&[7]),
        100,
        1,
        only_criteria(1),
        &mut ev,
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            SimError::NotEnoughCards {
                population: 2,
                draws: 7
            }
        ),
        "{err}"
    );

    let exact =
        gauntlet_criteria::run(&g, &Schedule::plain(&[7]), only_criteria(1), &mut ev).unwrap_err();
    assert_eq!(
        exact.to_string(),
        err.to_string(),
        "same question, same answer"
    );
}

#[test]
fn zero_trials_is_refused_rather_than_divided_by() {
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1
    })]);
    assert!(matches!(
        simulate(&g, &Schedule::plain(&[7]), 0, 1, only_criteria(1), &mut ev).unwrap_err(),
        SimError::NoTrials
    ));
}

#[test]
fn a_checkpoint_reached_before_any_draw_sees_an_empty_hand() {
    // Found by the property tests, not by anyone reading the code. The snapshot
    // loop only ran *after* dealing a card, so a leading gap of zero recorded
    // the hand one draw too late: the sampler answered 100% to a question the
    // exact engine answered 0%, on a library of one card.
    let g = Grouping::build(q(&["land"]), [(0b1, 1)]).unwrap();
    let criteria = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand)) >= 1),
            Box::new(|v: &PathView<'_>| v.count_at(1, 0, Counted::In(Zone::Hand)) >= 1),
            Box::new(|v: &PathView<'_>| v.count_at(2, 0, Counted::In(Zone::Hand)) >= 1),
        ])
    };
    // Nothing before the draw, the land after it, and a trailing gap of zero
    // that must not lose it again.
    let sampled = simulate(
        &g,
        &Schedule::plain(&[0, 1, 0]),
        100,
        1,
        only_criteria(3),
        &mut criteria(),
    )
    .unwrap()
    .proportions;
    assert_eq!(sampled, vec![0.0, 1.0, 1.0], "{sampled:?}");

    let exact = gauntlet_criteria::run(
        &g,
        &Schedule::plain(&[0, 1, 0]),
        only_criteria(3),
        &mut criteria(),
    )
    .unwrap()
    .probabilities;
    let exact: Vec<f64> = exact.iter().map(|p| p.get()).collect();
    assert_eq!(exact, sampled, "same question, same answer");
}

#[test]
fn the_sampled_distribution_reproduces_the_documented_land_distribution() {
    // The same acceptance test as at the top of this file, asked directly. Above
    // it is reconstructed from eight `>= k` criteria and differenced by hand,
    // which is exactly the workaround an expectation exists to remove -- so this
    // is also the check that the direct route and the workaround agree.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Counters(vec![Box::new(|v: &PathView<'_>| {
        v.count_at(0, 0, Counted::In(Zone::Hand))
    })]);
    let sampled = simulate(
        &g,
        &Schedule::plain(&[7]),
        TRIALS,
        0xC0FFEE,
        only_expectations(1),
        &mut ev,
    )
    .unwrap();
    let d = &sampled.distributions[0];

    let tolerance = 3.0 * 1.2331 / f64::from(TRIALS).sqrt();
    assert!(
        (d.mean() - 2.5455).abs() < tolerance,
        "mean {} outside {tolerance} of 2.5455",
        d.mean()
    );
    assert!((d.sd() - 1.2331).abs() < 0.02, "sd was {}", d.sd());
    assert!((d.total() - 1.0).abs() < 1e-9, "summed to {}", d.total());

    // Bucket for bucket against the closed-form hypergeometric, within the
    // sampler's own error bars. A mean can be right while the shape is wrong,
    // and the shape is the half this feature exists for.
    for k in 0..=7usize {
        let want = chip_stats::pmf(99, 36, 7, k as u32);
        let got = d.probabilities()[k];
        let se = standard_error(got, TRIALS);
        assert!(
            (got - want).abs() < 4.0 * se + 3.0 / f64::from(TRIALS),
            "P(exactly {k}): sampled {got} vs exact {want}"
        );
    }

    // And the error bar the report would quote beside that mean.
    let se = mean_standard_error(d, TRIALS);
    assert!(
        (se - 1.2331 / f64::from(TRIALS).sqrt()).abs() < 1e-4,
        "standard error of the mean was {se}"
    );
}

#[test]
fn expectations_agree_with_the_exact_engine() {
    // The exact engine is the oracle for the sampled one, for the second kind of
    // answer as much as the first. A sampler that quietly answered a different
    // question here -- the mean of something else, or a histogram off by one
    // bucket -- would look entirely plausible on its own.
    let g = Grouping::build(q(&["land", "dork"]), [(0b01, 36), (0b10, 10), (0, 53)]).unwrap();
    let counters = || {
        Counters(vec![
            Box::new(|v: &PathView<'_>| v.count_at(0, 0, Counted::In(Zone::Hand))) as Tally,
            Box::new(|v: &PathView<'_>| {
                v.count_at(1, 0, Counted::In(Zone::Hand))
                    + v.count_at(1, 1, Counted::In(Zone::Hand))
            }) as Tally,
        ])
    };

    let exact = gauntlet_criteria::run(
        &g,
        &Schedule::plain(&[7, 1]),
        only_expectations(2),
        &mut counters(),
    )
    .unwrap();
    let sampled = simulate(
        &g,
        &Schedule::plain(&[7, 1]),
        TRIALS,
        11,
        only_expectations(2),
        &mut counters(),
    )
    .unwrap();

    for (i, (e, s)) in exact
        .distributions
        .iter()
        .zip(&sampled.distributions)
        .enumerate()
    {
        let se = mean_standard_error(s, TRIALS);
        assert!(
            (s.mean() - e.mean()).abs() < 4.0 * se,
            "expectation {i}: sampled mean {} vs exact {} ({:.1}x SE)",
            s.mean(),
            e.mean(),
            (s.mean() - e.mean()).abs() / se
        );
        for k in 0..e.probabilities().len() {
            let want = e.probabilities()[k];
            let got = s.probabilities().get(k).copied().unwrap_or(0.0);
            let bucket_se = standard_error(got, TRIALS);
            assert!(
                (got - want).abs() < 4.0 * bucket_se + 3.0 / f64::from(TRIALS),
                "expectation {i}, P(exactly {k}): sampled {got} vs exact {want}"
            );
        }
    }
}

#[test]
fn the_budget_agrees_with_the_exact_engine() {
    // A budget is a new thing for the walk to do on every path, so it is a new
    // way for the two engines to disagree. They share the board, so what this
    // catches is the path being *produced* differently — a sampled hand whose
    // turn boundaries do not line up with the enumerated one's would spend its
    // mana on a different turn and cast a different number of spells.
    //
    // Nineteen lands, twelve one-drops and a library that runs out of neither,
    // asked as "at least two cast by turn 4", which is a question whose answer
    // is neither 0 nor 1.
    let grouping = Grouping::with_mana(
        q(&["cantrip"]),
        vec![
            (
                0b1,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                },
                12,
            ),
            (
                0b0,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                },
                19,
            ),
            (0b0, ManaSource::Spell, 68),
        ],
    )
    .unwrap();
    let schedule = Schedule::build(
        4,
        true,
        Vec::new(),
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let question = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(4, 0, Counted::Cast) >= 2) as Check,
        ])
    };
    let exact = gauntlet_criteria::run(&grouping, &schedule, only_criteria(1), &mut question())
        .unwrap()
        .probabilities[0]
        .get();
    let sampled = simulate(
        &grouping,
        &schedule,
        TRIALS,
        11,
        only_criteria(1),
        &mut question(),
    )
    .unwrap()
    .proportions[0];
    assert!(
        exact > 0.05 && exact < 0.95,
        "a question worth asking: {exact}"
    );
    let se = standard_error(sampled, TRIALS);
    assert!(
        (sampled - exact).abs() < 4.0 * se,
        "sampled {sampled} vs exact {exact} ({}x SE)",
        (sampled - exact).abs() / se
    );
}

#[test]
fn a_tutor_agrees_with_the_exact_engine() {
    // The acceptance test for #18, and the one that matters: a fetch makes the
    // library a population that shrinks, and the two engines shrink it by
    // completely different means. The exact one subtracts from the pool the
    // next gap is dealt out of; the sampler reaches into the undealt tail of a
    // shuffled deck and swaps the card past the end. If those two ever mean
    // different things, this is where it shows.
    //
    // Twelve tutors costing {U}, one card worth fetching, nineteen blue
    // sources. Asked as "the fetched card is in hand by turn 4", which is a
    // question the deck answers by drawing it *or* by going and getting it.
    let grouping = Grouping::with_mana(
        q(&["tutor", "target"]),
        vec![
            (
                0b01,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                },
                12,
            ),
            (0b10, ManaSource::Spell, 4),
            (
                0b00,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                },
                19,
            ),
            (0b00, ManaSource::Spell, 64),
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
    let schedule = Schedule::build(
        4,
        true,
        vec![tutor],
        Policies::casting(CastingPolicy::new(vec![0])),
    );
    let question = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(4, 1, Counted::In(Zone::Hand)) >= 1) as Check,
        ])
    };
    let exact = gauntlet_criteria::run(&grouping, &schedule, only_criteria(1), &mut question())
        .unwrap()
        .probabilities[0]
        .get();
    let sampled = simulate(
        &grouping,
        &schedule,
        TRIALS,
        7,
        only_criteria(1),
        &mut question(),
    )
    .unwrap()
    .proportions[0];
    assert!(
        exact > 0.05 && exact < 0.95,
        "a question worth asking: {exact}"
    );
    let se = standard_error(sampled, TRIALS);
    assert!(
        (sampled - exact).abs() < 4.0 * se,
        "sampled {sampled} vs exact {exact} ({}x SE)",
        (sampled - exact).abs() / se
    );
}

#[test]
fn a_tutor_thins_the_library_in_both_engines() {
    // The half of a fetch that is not about the card it found. Four copies of
    // the target and twelve tutors: every tutor that resolves takes one of
    // them out of the library, so the expected number *drawn* has to fall —
    // and it has to fall by the same amount in both engines, which is the
    // thing a sampler that forgot to remove the card would get wrong while
    // still agreeing about what is in hand.
    let library = |fetching: bool| {
        let grouping = Grouping::with_mana(
            q(&["tutor", "target"]),
            vec![
                (
                    0b01,
                    ManaSource::Castable {
                        cost: Cost::parse("{U}").unwrap().demand(),
                    },
                    12,
                ),
                (0b10, ManaSource::Spell, 4),
                (
                    0b00,
                    ManaSource::Land {
                        enters_tapped: false,
                        produces: Palette::from_letters(["U"]),
                    },
                    19,
                ),
                (0b00, ManaSource::Spell, 24),
            ],
        )
        .unwrap();
        let effects = match fetching {
            false => Vec::new(),
            true => vec![Effect {
                matched_by: 0,
                look: 0,
                trigger: Trigger::Cast,
                route: Route::Nowhere,
                fetch: Some(Fetch {
                    prefer: vec![1],
                    to: Fetched::Hand,
                }),
                delay: None,
            }],
        };
        let schedule = Schedule::build(
            4,
            true,
            effects,
            Policies::casting(CastingPolicy::new(vec![0])),
        );
        (grouping, schedule)
    };
    // Still in the library, which is the count a fetch reduces without anyone
    // having drawn anything.
    let question = || {
        Counters(vec![
            Box::new(|v: &PathView<'_>| v.count_at(4, 1, Counted::In(Zone::Library))) as Tally,
        ])
    };
    let left = |fetching: bool| {
        let (grouping, schedule) = library(fetching);
        let exact =
            gauntlet_criteria::run(&grouping, &schedule, only_expectations(1), &mut question())
                .unwrap()
                .distributions[0]
                .mean();
        // Fewer hands than the tests above: this compares two means rather
        // than pinning one, and the prefix replay a fetch costs is paid per
        // checkpoint per hand.
        let trials = TRIALS / 4;
        let sampled = simulate(
            &grouping,
            &schedule,
            trials,
            13,
            only_expectations(1),
            &mut question(),
        )
        .unwrap()
        .distributions[0]
            .clone();
        let se = mean_standard_error(&sampled, trials);
        assert!(
            (sampled.mean() - exact).abs() < 4.0 * se,
            "sampled {} vs exact {exact} ({}x SE)",
            sampled.mean(),
            (sampled.mean() - exact).abs() / se
        );
        exact
    };
    let without = left(false);
    let with = left(true);
    assert!(
        with < without - 0.01,
        "a fetch has to leave fewer behind: {with} against {without}"
    );
}

#[test]
fn a_delayed_fetch_agrees_with_the_exact_engine() {
    // Urza's Saga, and the acceptance test for a delay: the fetch happens two
    // turns after the drop that set it up, so the population shrinks on a turn
    // no card was played on, and the Saga's own mana leaves with it. Asked as
    // "the artifact is on the battlefield by turn 5, and turn 5 can pay {3}",
    // which reads the fetch, the sacrifice and the pool on the same path.
    //
    // Four Sagas rather than one so the delayed fetch fires often enough to
    // be worth comparing, and four targets so it usually has something left
    // to find.
    let grouping = Grouping::with_mana(
        q(&["saga", "target", "land", "<effect saga>"]),
        vec![
            (
                0b1101,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["C"]),
                },
                4,
            ),
            (0b0010, ManaSource::Spell, 4),
            (
                0b0100,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                },
                30,
            ),
            (0b0000, ManaSource::Spell, 61),
        ],
    )
    .unwrap();
    let saga = Effect {
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
    };
    let schedule = Schedule::build(
        5,
        false,
        vec![saga],
        Policies::land_drop(LandDropPolicy::new(vec![0], 2)),
    );
    let three = Cost::parse("{3}").unwrap();
    let question = || {
        let three = three.clone();
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(5, 1, Counted::In(Zone::Battlefield)) >= 1)
                as Check,
            Box::new(move |v: &PathView<'_>| {
                v.count_at(5, 1, Counted::In(Zone::Battlefield)) >= 1 && v.can_cast(5, &three)
            }),
        ])
    };
    let exact = gauntlet_criteria::run(&grouping, &schedule, only_criteria(2), &mut question())
        .unwrap()
        .probabilities;
    let sampled = simulate(
        &grouping,
        &schedule,
        TRIALS / 4,
        17,
        only_criteria(2),
        &mut question(),
    )
    .unwrap()
    .proportions;
    for (exact, sampled) in exact.iter().map(|p| p.get()).zip(sampled) {
        assert!(
            exact > 0.05 && exact < 0.95,
            "a question worth asking: {exact}"
        );
        let se = standard_error(sampled, TRIALS / 4);
        assert!(
            (sampled - exact).abs() < 4.0 * se,
            "sampled {sampled} vs exact {exact} ({}x SE)",
            (sampled - exact).abs() / se
        );
    }
}

#[test]
fn a_mulligan_agrees_with_the_exact_engine() {
    // The acceptance test for #7. The two engines get to a mulligan's number
    // by different roads: the exact one sums an enumeration per depth,
    // weighted by the chance of reaching it, and prices a tie inside one
    // bottoming entry by walking every way it could fall; the sampler plays
    // the games — deals, puts back the first-dealt cards of the entry, asks
    // the rule, deals again. Everything a mulligan touches is in here at once:
    //
    // - a tie that matters: the bottoming entry is "a land", the deck's lands
    //   are Islands and Mountains, and only an Island pays for the tutor;
    // - a tutor, so the card that went back decides what a later turn can
    //   fetch, and the branch has to come before the rest of the path;
    // - the keep-your-seven number beside each answer, and how often each
    //   hand size was kept.
    let island = ManaSource::Land {
        enters_tapped: false,
        produces: Palette::from_letters(["U"]),
    };
    let mountain = ManaSource::Land {
        enters_tapped: false,
        produces: Palette::from_letters(["R"]),
    };
    let grouping = Grouping::with_mana(
        q(&["land", "tutor", "target"]),
        vec![
            (0b001, island, 8),
            (0b001, mountain, 8),
            (
                0b010,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                },
                4,
            ),
            (0b100, ManaSource::Spell, 2),
            (0b000, ManaSource::Spell, 18),
        ],
    )
    .unwrap();
    let tutor = Effect {
        matched_by: 1,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: Some(Fetch {
            prefer: vec![2],
            to: Fetched::Hand,
        }),
        delay: None,
    };
    let mulligan = MulliganPolicy::new(
        vec![Keep {
            query: 0,
            min: 2,
            max: Some(4),
        }],
        vec![0],
        5,
    );
    let schedule = Schedule::build(
        3,
        false,
        vec![tutor],
        Policies {
            casting: Some(CastingPolicy::new(vec![1])),
            mulligan: Some(mulligan),
            ..Policies::default()
        },
    );
    let question = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(3, 2, Counted::In(Zone::Hand)) >= 1) as Check,
            Box::new(|v: &PathView<'_>| v.count_at(2, 1, Counted::Cast) >= 1) as Check,
            Box::new(|v: &PathView<'_>| v.count_at(3, 0, Counted::In(Zone::Battlefield)) >= 3)
                as Check,
        ])
    };
    let exact =
        gauntlet_criteria::run(&grouping, &schedule, only_criteria(3), &mut question()).unwrap();
    let sampled = simulate(
        &grouping,
        &schedule,
        TRIALS,
        17,
        only_criteria(3),
        &mut question(),
    )
    .unwrap();
    let agree = |what: &str, exact: f64, sampled: f64| {
        let se = standard_error(sampled, TRIALS);
        assert!(
            (sampled - exact).abs() < 4.0 * se.max(1e-6),
            "{what}: sampled {sampled} vs exact {exact} ({}x SE)",
            (sampled - exact).abs() / se
        );
    };
    let mulligan = exact.mulligan.as_ref().expect("declared");
    let sampled_mulligan = sampled.mulligan.as_ref().expect("declared");
    for (i, (e, s)) in exact
        .probabilities
        .iter()
        .zip(&sampled.proportions)
        .enumerate()
    {
        assert!(
            e.get() > 0.05 && e.get() < 0.95,
            "criterion {i} is a question worth asking: {}",
            e.get()
        );
        agree(&format!("criterion {i}"), e.get(), *s);
        agree(
            &format!("criterion {i}, keep seven"),
            mulligan.seven[i].get(),
            sampled_mulligan.seven[i],
        );
    }
    assert_eq!(mulligan.kept.len(), 3, "seven, six and five");
    assert!(
        mulligan.kept[1].get() > 0.05,
        "a mulligan that fires: {:?}",
        mulligan.kept
    );
    for (depth, (e, s)) in mulligan.kept.iter().zip(&sampled_mulligan.kept).enumerate() {
        agree(&format!("kept at depth {depth}"), e.get(), *s);
    }
}

#[test]
fn a_chosen_strategy_agrees_with_the_exact_engine_on_a_class_it_does_not_read() {
    // The acceptance test for #63 and #64 together. A strategy is chosen for
    // one question on the grouping that question reads — lands against
    // everything else — and then a *different* question is answered under it,
    // on a grouping that cannot see lands at all but can see a card some of
    // the lands are. The exact engine reads each opener on the join of the
    // two, lets the strategy decide on its own groups, and spreads what goes
    // back across the finer ones by a uniform choice; the sampler reads the
    // opener it dealt and puts back the lands it dealt first. If the spread is
    // wrong, the second question is where it shows.
    let full = Grouping::build(
        q(&["land", "x"]),
        [(0b11, 5), (0b01, 6), (0b10, 4), (0b00, 15)],
    )
    .unwrap();
    let plain = Schedule::build(2, false, Vec::new(), Policies::default());
    let with_opener = plain.narrowed(&[0, 2], gauntlet_criteria::Reading::Cumulative);
    let questions = || {
        Closures(vec![
            Box::new(|v: &PathView<'_>| v.count_at(2, 0, Counted::In(Zone::Hand)) >= 3) as Check,
            Box::new(|v: &PathView<'_>| v.count_at(2, 1, Counted::In(Zone::Hand)) >= 1) as Check,
        ])
    };
    let plan = only_criteria(2);

    // The objective's class: lands, and nothing else.
    let lands = full.coarsened(0b01, LandDetail::Ignored);
    let objective_answering = Answering::some(plan, vec![0], Vec::new()).unwrap();
    let mut ev = questions();
    let mut conditionals = Conditionals::new(
        &lands,
        &with_opener,
        &objective_answering,
        &mut ev,
        Table::default(),
    )
    .unwrap();
    conditionals.fill(2).unwrap();
    let table = conditionals.into_table();
    let identity: Vec<usize> = (0..lands.group_sizes().len()).collect();
    let chosen = gauntlet_criteria::optimise(
        lands.clone(),
        0b01,
        LandDetail::Ignored,
        7,
        5,
        &[Objective {
            weight: 1.0,
            table: &table,
            to_class: &identity,
            class_groups: lands.group_sizes().len(),
            position: 0,
        }],
    );
    assert!(
        chosen.strategy.kept()[0] < 0.95,
        "a strategy that mulligans: {:?}",
        chosen.strategy.kept()
    );
    let strategy = Chosen(Arc::new(chosen.strategy.clone()));
    let played = Schedule::build(
        2,
        false,
        Vec::new(),
        Policies {
            chosen: Some(strategy),
            ..Policies::default()
        },
    );

    // Each question on its own class, as a run would ask it.
    let mut exact = Vec::new();
    let mut seven = Vec::new();
    for (i, keep) in [(0usize, 0b01u64), (1, 0b10)] {
        let answering = Answering::some(plan, vec![i], Vec::new()).unwrap();
        let class_schedule = played.narrowed(&[2], gauntlet_criteria::Reading::Cumulative);
        let out = gauntlet_criteria::run_chosen(
            &full,
            keep,
            LandDetail::Ignored,
            &class_schedule,
            &answering,
            &mut questions(),
            Table::default(),
        )
        .unwrap();
        exact.push(out.probabilities[0].get());
        seven.push(out.mulligan.unwrap().seven[0].get());
    }
    assert!(
        (exact[0] - chosen.under[0]).abs() < 1e-12,
        "the run plays the strategy's own number: {} vs {}",
        exact[0],
        chosen.under[0]
    );

    let sampled = simulate(&full, &played, TRIALS, 23, plan, &mut questions()).unwrap();
    let mulligan = sampled.mulligan.as_ref().expect("a strategy was played");
    for i in 0..2 {
        let se = standard_error(sampled.proportions[i], TRIALS);
        assert!(
            (sampled.proportions[i] - exact[i]).abs() < 4.0 * se,
            "question {i}: sampled {} vs exact {} ({}x SE)",
            sampled.proportions[i],
            exact[i],
            (sampled.proportions[i] - exact[i]).abs() / se
        );
        let se = standard_error(mulligan.seven[i], TRIALS);
        assert!(
            (mulligan.seven[i] - seven[i]).abs() < 4.0 * se,
            "question {i}, keep seven: sampled {} vs exact {}",
            mulligan.seven[i],
            seven[i]
        );
    }
    for (depth, (e, s)) in chosen
        .strategy
        .kept()
        .iter()
        .zip(&mulligan.kept)
        .enumerate()
    {
        let se = standard_error(*s, TRIALS);
        assert!(
            (e - s).abs() < 4.0 * se.max(1e-6),
            "kept at depth {depth}: {s} vs {e}"
        );
    }
}
