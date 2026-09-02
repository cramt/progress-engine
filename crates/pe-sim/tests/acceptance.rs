//! The acceptance test for the shuffler.
//!
//! The rule inherited from this project's predecessor: never touch the shuffling
//! without re-running these. Two hand-rolled PRNGs were rejected there for
//! producing confidently wrong numbers, and both would have passed a casual
//! eyeball. The check that caught them is the hypergeometric distribution, so
//! that is what is asserted here.

use std::convert::Infallible;

use pe_criteria::{Evaluator, Grouping, PathView};
use pe_sim::{simulate, standard_error, SimError};

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

const TRIALS: u32 = 200_000;

#[test]
fn the_shuffler_reproduces_the_documented_land_distribution() {
    // 36 lands in a 99-card library, opening seven: mean 2.5455, SD 1.2331.
    // These are the constants the predecessor's own acceptance test used.
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();

    // P(X >= k) for each k, from which the distribution follows.
    let mut ev = Closures(
        (0..=7)
            .map(|k| Box::new(move |v: &PathView<'_>| v.count(0, 0) >= k) as Check)
            .collect(),
    );
    let at_least = simulate(&g, &[7], TRIALS, 0xC0FFEE, &mut ev).unwrap();

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

        let exact = pe_stats::probability_that(g.group_sizes(), draws, |c| {
            g.count_matching(c, 0) >= 1 && g.count_matching(c, 1) >= 1
        });

        let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
            v.count(0, 0) >= 1 && v.count(0, 1) >= 1
        })]);
        let sampled = simulate(&g, &[draws], TRIALS, 42, &mut ev).unwrap()[0];

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

    let exact = pe_stats::probability_that_path(g.group_sizes(), &[7, 1], pred);
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| {
        v.count(0, 0) >= 1 && v.count(0, 1) >= 1 && v.count(1, 0) >= 2
    })]);
    let sampled = simulate(&g, &[7, 1], TRIALS, 7, &mut ev).unwrap()[0];

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
        let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) >= 3)]);
        simulate(&g, &[7], 5_000, seed, &mut ev).unwrap()[0]
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
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) == 4)]);
    let p = simulate(&g, &[10], 100, 1, &mut ev).unwrap()[0];
    assert_eq!(p, 1.0, "drawing every card must find every land");
}

#[test]
fn a_hand_bigger_than_the_library_is_refused_rather_than_clamped() {
    // The sampler used to deal what it could and answer 100% for a question the
    // exact engine answered 0%. Both refuse now.
    let g = Grouping::build(q(&["land"]), [(0b1, 1), (0, 1)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1)]);
    let err = simulate(&g, &[7], 100, 1, &mut ev).unwrap_err();
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

    let exact = pe_criteria::run(&g, &[7], 1, &mut ev).unwrap_err();
    assert_eq!(
        exact.to_string(),
        err.to_string(),
        "same question, same answer"
    );
}

#[test]
fn zero_trials_is_refused_rather_than_divided_by() {
    let g = Grouping::build(q(&["land"]), [(0b1, 36), (0, 63)]).unwrap();
    let mut ev = Closures(vec![Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1)]);
    assert!(matches!(
        simulate(&g, &[7], 0, 1, &mut ev).unwrap_err(),
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
            Box::new(|v: &PathView<'_>| v.count(0, 0) >= 1),
            Box::new(|v: &PathView<'_>| v.count(1, 0) >= 1),
            Box::new(|v: &PathView<'_>| v.count(2, 0) >= 1),
        ])
    };
    // Nothing before the draw, the land after it, and a trailing gap of zero
    // that must not lose it again.
    let sampled = simulate(&g, &[0, 1, 0], 100, 1, &mut criteria()).unwrap();
    assert_eq!(sampled, vec![0.0, 1.0, 1.0], "{sampled:?}");

    let exact = pe_criteria::run(&g, &[0, 1, 0], 3, &mut criteria()).unwrap();
    let exact: Vec<f64> = exact.iter().map(|p| p.get()).collect();
    assert_eq!(exact, sampled, "same question, same answer");
}
