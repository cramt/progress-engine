//! End-to-end tests for JavaScript criteria against the exact engine.
//!
//! The engine itself is tested with Rust closures in pe-criteria. What is under
//! test here is the bindings: that JS sees the right counts, that query
//! discovery converges, and that the answers match the known fixtures.

use pe_criteria::{Grouping, GroupingError, Outcomes, RunError};
use pe_js::{with_discovery, Criteria, DiscoveryError, JsError};
use pe_stats::Probability;

type Failure = DiscoveryError<GroupingError, RunError<JsError>>;

/// Drive the shipped discovery loop against the exact engine.
///
/// This is the same entry point the binary uses, deliberately: the bug that
/// motivated the horizon half of discovery hid in the gap between what the CLI
/// called and what the tests called.
fn run_exact(c: &mut Criteria, gaps: &[u32], connectors: u32) -> Result<Vec<Probability>, Failure> {
    run_all(c, gaps, connectors).map(|o| o.probabilities)
}

fn run_all(c: &mut Criteria, gaps: &[u32], connectors: u32) -> Result<Outcomes, Failure> {
    let gaps = gaps.to_vec();
    with_discovery(
        c,
        |q| grouping_for(q, connectors),
        |_| gaps.clone(),
        |g, gaps, c| pe_criteria::run(g, gaps, c.plan(), c),
    )
}

fn grouping_for(
    queries: &[String],
    connectors: u32,
) -> Result<Grouping, pe_criteria::GroupingError> {
    // Two disjoint sets plus the rest of the library, mapped onto whichever
    // queries the criteria actually asked about.
    let cards: Vec<(u64, u32)> = queries
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let qty = if q.contains("arm") { 6 } else { connectors };
            (1u64 << i, qty)
        })
        .collect();
    let used: u32 = cards.iter().map(|(_, q)| q).sum();
    let mut all = cards;
    all.push((0, 99 - used));
    Grouping::build(queries.to_vec(), all)
}

#[test]
fn a_javascript_criterion_reproduces_the_known_answer() {
    let src = r#"
        criterion("arm and connector by t5", (t) =>
            t(0).count('cat:"arm"') >= 1 && t(0).count('cat:"connector"') >= 1,
            { atLeast: 0.35 });
    "#;
    let mut c = Criteria::load(src.to_string()).expect("should load");
    assert_eq!(c.criteria().len(), 1);
    assert_eq!(c.criteria()[0].name, "arm and connector by t5");
    assert_eq!(c.criteria()[0].at_least, Some(0.35));

    let strict = run_exact(&mut c, &[11], 8).unwrap();
    assert!(
        (strict[0].percent() - 31.0).abs() < 0.05,
        "strict was {}",
        strict[0].percent()
    );

    let mut c2 = Criteria::load(src.to_string()).unwrap();
    let wide = run_exact(&mut c2, &[11], 12).unwrap();
    assert!(
        (wide[0].percent() - 39.0).abs() < 0.05,
        "wide was {}",
        wide[0].percent()
    );
}

#[test]
fn discovery_finds_queries_hidden_behind_short_circuits() {
    // The second query is only reached when the first succeeds, so a single
    // all-zero probe never sees it. Discovery must still find it.
    let src = r#"
        criterion("both", (t) =>
            t(0).count('cat:"arm"') >= 1 && t(0).count('cat:"connector"') >= 1);
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let r = run_exact(&mut c, &[11], 8).unwrap();
    // If the hidden query had been missed, count() would return 0 forever and
    // the answer would collapse to 0%.
    assert!(r[0].percent() > 1.0, "got {}", r[0].percent());
    assert!(
        (r[0].percent() - 31.0).abs() < 0.05,
        "got {}",
        r[0].percent()
    );
}

#[test]
fn turn_checkpoints_are_cumulative_in_javascript() {
    let src = r#"
        criterion("monotone", (t) =>
            t(1).count('cat:"arm"') >= t(0).count('cat:"arm"'));
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let r = run_exact(&mut c, &[7, 1], 8).unwrap();
    assert!(
        (r[0].get() - 1.0).abs() < 1e-9,
        "should always hold: {}",
        r[0].get()
    );
}

#[test]
fn multiple_criteria_are_evaluated_in_one_pass() {
    let src = r#"
        criterion("a", (t) => t(0).count('cat:"arm"') >= 1, { atLeast: 0.5 });
        criterion("b", (t) => t(0).count('cat:"arm"') >= 2);
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let r = run_exact(&mut c, &[11], 8).unwrap();
    assert_eq!(r.len(), 2);
    assert!(
        (r[0].percent() - 51.6).abs() < 0.05,
        "a was {}",
        r[0].percent()
    );
    assert!(r[1].get() < r[0].get(), "two is harder than one");
}

#[test]
fn a_file_with_no_criteria_is_an_error() {
    assert!(matches!(
        Criteria::load("const x = 1;".to_string()),
        Err(JsError::NoCriteria)
    ));
}

#[test]
fn javascript_errors_surface_rather_than_being_swallowed() {
    // A bad registration should fail at load.
    assert!(matches!(
        Criteria::load("criterion(123, () => true);".to_string()),
        Err(JsError::Load(_))
    ));
    assert!(matches!(
        Criteria::load("criterion('x', 'not a function');".to_string()),
        Err(JsError::Load(_))
    ));
    // An out-of-range atLeast is a mistake worth refusing.
    assert!(matches!(
        Criteria::load("criterion('x', () => true, { atLeast: 55 });".to_string()),
        Err(JsError::Load(_))
    ));
}

#[test]
fn a_throwing_criterion_reports_the_message() {
    let src = "criterion('boom', () => { throw new Error('deliberate'); });";
    let mut c = Criteria::load(src.to_string()).unwrap();
    let err = run_exact(&mut c, &[7], 8).unwrap_err();
    assert!(err.to_string().contains("deliberate"), "{err}");
}

#[test]
fn discovery_finds_turns_hidden_behind_short_circuits() {
    // The deepest turn this file names sits behind a `&&` that is false while
    // every count is zero, so the opening probe never reaches it. If the run
    // horizon stayed at what that probe saw, t(3) would fall off the end of the
    // path and count() would answer 0 forever — a confident, silent 0%.
    let src = r#"
        criterion("late", (t) =>
            t(0).count('cat:"arm"') >= 1 && t(3).count('cat:"connector"') >= 1);
    "#;

    // One card per turn after the opening seven, for as many turns as asked.
    let gaps_for = |turns: u32| {
        let mut gaps = vec![7];
        gaps.extend(std::iter::repeat_n(1, turns as usize));
        gaps
    };

    let mut c = Criteria::load(src.to_string()).unwrap();
    let hidden = with_discovery(
        &mut c,
        |q| grouping_for(q, 8),
        gaps_for,
        |g, gaps, c| pe_criteria::run(g, gaps, c.plan(), c),
    )
    .unwrap()
    .probabilities;

    // The same question with both operands evaluated eagerly, which the probe
    // does reach. The two spellings must not disagree.
    let src_eager = r#"
        criterion("late", (t) => {
            const a = t(0).count('cat:"arm"') >= 1;
            const b = t(3).count('cat:"connector"') >= 1;
            return a && b;
        });
    "#;
    let mut c2 = Criteria::load(src_eager.to_string()).unwrap();
    let eager = with_discovery(
        &mut c2,
        |q| grouping_for(q, 8),
        gaps_for,
        |g, gaps, c| pe_criteria::run(g, gaps, c.plan(), c),
    )
    .unwrap()
    .probabilities;

    assert!(
        hidden[0].percent() > 1.0,
        "collapsed to {}",
        hidden[0].percent()
    );
    assert!(
        (hidden[0].get() - eager[0].get()).abs() < 1e-12,
        "short-circuited {} vs eager {}",
        hidden[0].percent(),
        eager[0].percent()
    );
}

// --- Expectations ---------------------------------------------------------

#[test]
fn a_javascript_expectation_reproduces_the_closed_form_mean() {
    // Six arming outlets in a 99-card library, eleven cards seen. The mean of a
    // hypergeometric is draws * successes / population in closed form, and
    // nothing on the path from this JavaScript to that number goes near it.
    let src = r#"
        expect("arms by t5", (t) => t(0).count('cat:"arm"'));
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    assert_eq!(c.expectations().len(), 1);
    assert_eq!(c.expectations()[0].name, "arms by t5");
    assert!(c.criteria().is_empty(), "expect() is not a criterion");

    let r = run_all(&mut c, &[11], 8).unwrap();
    assert!(r.probabilities.is_empty());

    let d = &r.distributions[0];
    let closed = pe_stats::mean(99, 6, 11);
    assert!(
        (d.mean() - closed).abs() < 1e-12,
        "mean {} vs closed form {closed}",
        d.mean()
    );
    assert!((d.total() - 1.0).abs() < 1e-12, "summed to {}", d.total());
    for k in 0..=6u32 {
        let want = pe_stats::pmf(99, 6, 11, k);
        assert!(
            (d.probabilities()[k as usize] - want).abs() < 1e-12,
            "P(exactly {k}) was {}",
            d.probabilities()[k as usize]
        );
    }
}

#[test]
fn criteria_and_expectations_share_one_pass() {
    // Both kinds registered in one file, answered off one enumeration. The
    // criterion must equal the tail of the distribution beside it, or the two
    // halves of the report are describing different runs.
    let src = r#"
        criterion("at least two arms", (t) => t(0).count('cat:"arm"') >= 2);
        expect("arms", (t) => t(0).count('cat:"arm"'));
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let r = run_all(&mut c, &[11], 8).unwrap();

    let tail: f64 = r.distributions[0].probabilities()[2..].iter().sum();
    assert!(
        (tail - r.probabilities[0].get()).abs() < 1e-12,
        "criterion {} vs distribution tail {tail}",
        r.probabilities[0].get()
    );
}

#[test]
fn discovery_finds_the_queries_and_turns_an_expectation_names() {
    // Expectations go through the same discovery loop as criteria. If they did
    // not, an expectation naming a query nobody grouped by would count zero on
    // every path and report a confident, perfectly shaped distribution entirely
    // at zero -- this project's defining failure with a histogram drawn on it.
    let src = r#"
        expect("arms by t2", (t) => t(2).count('cat:"arm"'));
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let r = run_all(&mut c, &[7, 1, 1], 8).unwrap();
    assert!(
        r.distributions[0].mean() > 0.0,
        "collapsed to a distribution entirely at zero"
    );
    let closed = pe_stats::mean(99, 6, 9);
    assert!(
        (r.distributions[0].mean() - closed).abs() < 1e-12,
        "mean {} vs closed form {closed}",
        r.distributions[0].mean()
    );
}

#[test]
fn a_criterion_that_answers_with_a_number_is_an_error_not_a_coercion() {
    // `Boolean(3)` is true and `Boolean(0)` is false, so a criterion that forgot
    // its comparison used to answer "at least one arm" while looking like it
    // answered "how many arms" -- a plausible number for a question nobody
    // asked. The mistake has to name itself instead.
    let src = r#"
        criterion("arms", (t) => t(0).count('cat:"arm"'));
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let err = run_all(&mut c, &[11], 8).unwrap_err().to_string();
    assert!(err.contains("criterion"), "{err}");
    assert!(err.contains("arms"), "it must name the offender: {err}");
    assert!(err.contains("expect()"), "and say what to do: {err}");
}

#[test]
fn an_expectation_that_answers_with_a_bool_is_an_error_not_a_coercion() {
    // The mirror image. `true` is 1 and `false` is 0, so this would report a
    // mean that is really a probability, in a column headed "mean".
    let src = r#"
        expect("has an arm", (t) => t(0).count('cat:"arm"') >= 1);
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let err = run_all(&mut c, &[11], 8).unwrap_err().to_string();
    assert!(err.contains("expect"), "{err}");
    assert!(
        err.contains("has an arm"),
        "it must name the offender: {err}"
    );
    assert!(err.contains("criterion()"), "and say what to do: {err}");
}

#[test]
fn an_expectation_with_no_bucket_to_go_in_is_refused_by_name() {
    // Everything count() can produce is a whole number of cards. An expectation
    // computing a rate is a real thing somebody will write, and bucketing 0.545
    // at a width nobody chose would answer a different question in silence.
    let src = r#"
        expect("arms per card", (t) => t(0).count('cat:"arm"') / 11);
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let err = run_all(&mut c, &[11], 8).unwrap_err().to_string();
    assert!(err.contains("arms per card"), "{err}");
    assert!(err.contains("countable"), "{err}");

    // And the same for a value too large to histogram.
    let src = r#"
        expect("arms, scaled", (t) => (t(0).count('cat:"arm"') + 1) * 100000);
    "#;
    let mut c = Criteria::load(src.to_string()).unwrap();
    let err = run_all(&mut c, &[11], 8).unwrap_err().to_string();
    assert!(err.contains("arms, scaled"), "{err}");
    assert!(err.contains("countable"), "{err}");
}

#[test]
fn a_bad_expectation_registration_fails_at_load() {
    assert!(matches!(
        Criteria::load("expect(123, () => 1);".to_string()),
        Err(JsError::Load(_))
    ));
    assert!(matches!(
        Criteria::load("expect('x', 'not a function');".to_string()),
        Err(JsError::Load(_))
    ));
}

#[test]
fn a_file_of_only_expectations_is_a_file_with_something_in_it() {
    // "No criteria" means nothing was registered at all, not "no criterion()".
    // A file that only asks how many is a perfectly good file.
    let src = r#"expect("arms", (t) => t(0).count('cat:"arm"'));"#;
    assert!(Criteria::load(src.to_string()).is_ok());
}
