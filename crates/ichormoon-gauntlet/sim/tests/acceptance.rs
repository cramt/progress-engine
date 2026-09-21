//! The acceptance test for the shuffler.
//!
//! The rule inherited from this project's predecessor: never touch the shuffling
//! without re-running these. Two hand-rolled PRNGs were rejected there for
//! producing confidently wrong numbers, and both would have passed a casual
//! eyeball. The check that caught them is the hypergeometric distribution, so
//! that is what is asserted here.

use std::convert::Infallible;

use gauntlet_criteria::{
    CastingPolicy, Cost, Count, Counted, Effect, Evaluator, Fetch, Fetched, Grouping, ManaSource,
    Palette, PathOutcomes, PathView, Plan, Policies, Route, Schedule, Trigger, Zone,
};
use gauntlet_sim::{mean_standard_error, simulate, standard_error, SimError};

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
