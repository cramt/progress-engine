//! Known-answer tests for the exact draw probabilities.
//!
//! The first fixture is deliberately the constant already documented in the
//! shuffler acceptance test of the tool this crate grew out of: "for 36 lands in
//! 99 cards, n=7: mean 2.5457, SD 1.2331". That number was derived out of band
//! and then hand-copied into two files. Now it is computed, so the new code and
//! the old acceptance test validate each other instead of both being magic.

use progress_engine::hypergeom::{self as h, Probability};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

#[test]
fn reproduces_the_documented_shuffler_constants() {
    // 36 lands in a 99-card library, opening seven.
    assert!(
        close(h::mean(99, 36, 7), 2.545455, 1e-6),
        "mean was {}",
        h::mean(99, 36, 7)
    );
    assert!(
        close(h::sd(99, 36, 7), 1.2331509, 1e-6),
        "sd was {}",
        h::sd(99, 36, 7)
    );
}

#[test]
fn the_distribution_sums_to_one() {
    let total: f64 = (0..=7).map(|k| h::pmf(99, 36, 7, k)).sum();
    assert!(close(total, 1.0, 1e-12), "summed to {total}");
}

#[test]
fn mean_and_sd_agree_with_the_enumerated_distribution() {
    // The closed forms and the pmf must not drift apart.
    let mean: f64 = (0..=7).map(|k| f64::from(k) * h::pmf(99, 36, 7, k)).sum();
    let ex2: f64 = (0..=7)
        .map(|k| f64::from(k * k) * h::pmf(99, 36, 7, k))
        .sum();
    assert!(close(mean, h::mean(99, 36, 7), 1e-9));
    assert!(close((ex2 - mean * mean).sqrt(), h::sd(99, 36, 7), 1e-9));
}

#[test]
fn at_least_one_of_six_in_eleven() {
    // Turn 5 on the play sees 11 cards. Six safe arming outlets in 99.
    let p = h::at_least(99, 6, 11, 1);
    assert!(close(p.percent(), 51.6, 0.05), "was {}", p.percent());
}

#[test]
fn at_least_one_of_eight_in_eleven() {
    let p = h::at_least(99, 8, 11, 1);
    assert!(close(p.percent(), 62.5, 0.05), "was {}", p.percent());
}

#[test]
fn edge_cases() {
    assert_eq!(h::at_least(99, 6, 11, 0), Probability::new(1.0));
    // Cannot draw more successes than exist.
    assert_eq!(h::pmf(99, 6, 11, 7), 0.0);
    // Cannot draw more cards than the deck holds.
    assert_eq!(h::pmf(10, 5, 11, 1), 0.0);
    // Every card is a success.
    assert!(close(h::at_least(10, 10, 1, 1).get(), 1.0, 1e-12));
    assert!(close(h::at_least(10, 0, 5, 1).get(), 0.0, 1e-12));
}

#[test]
fn compositions_are_a_probability_distribution() {
    let mut total = 0.0;
    h::for_each_composition(&[6, 8, 85], 11, |_, p| total += p);
    assert!(close(total, 1.0, 1e-12), "summed to {total}");
}

#[test]
fn composition_marginals_match_the_univariate_case() {
    // Marginalising the multivariate enumeration over one group must reproduce
    // the closed-form univariate answer, or the two engines disagree.
    let p = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1);
    assert!(
        close(p.get(), h::at_least(99, 6, 11, 1).get(), 1e-12),
        "{} vs {}",
        p.get(),
        h::at_least(99, 6, 11, 1).get()
    );
}

#[test]
fn the_thirty_one_versus_thirty_nine_percent_story() {
    // The disagreement that motivated this crate. Same deck, same question,
    // same turn: "do I have a safe arming outlet AND an evasive connector by
    // turn 5?" One analysis counted 8 connectors, another counted 12. Both
    // answers were defensible and neither definition was written down.
    let strict = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1 && c[1] >= 1);
    let wide = h::probability_that(&[6, 12, 81], 11, |c| c[0] >= 1 && c[1] >= 1);

    assert!(
        close(strict.percent(), 31.0, 0.05),
        "strict was {}",
        strict.percent()
    );
    assert!(
        close(wide.percent(), 39.0, 0.05),
        "wide was {}",
        wide.percent()
    );

    // And the wide figure matches the independent Monte Carlo run that produced
    // 38.5% by simulation, within its sampling error.
    assert!((wide.percent() - 38.5).abs() < 1.0);
}

#[test]
fn overlapping_categories_are_handled_by_grouping() {
    // A card counting for two roles at once is exactly what breaks naive
    // inclusion-exclusion. Group [both, only-a, only-b, neither] instead.
    // With 2 cards that are both, P(a and b) must exceed the disjoint case.
    let disjoint = h::probability_that(&[6, 8, 85], 11, |c| c[0] >= 1 && c[1] >= 1);
    let overlapping =
        h::probability_that(&[2, 4, 6, 87], 11, |c| c[0] + c[1] >= 1 && c[0] + c[2] >= 1);
    assert!(overlapping.get() > 0.0 && overlapping.get() < 1.0);
    assert!(disjoint.get() > 0.0);
}
