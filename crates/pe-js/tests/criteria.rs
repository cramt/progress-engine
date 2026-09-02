//! End-to-end tests for JavaScript criteria against the exact engine.
//!
//! The engine itself is tested with Rust closures in pe-criteria. What is under
//! test here is the bindings: that JS sees the right counts, that query
//! discovery converges, and that the answers match the known fixtures.

use pe_criteria::{Grouping, GroupingError, RunError};
use pe_js::{with_discovery, Criteria, DiscoveryError, JsError};
use pe_stats::Probability;

type Failure = DiscoveryError<GroupingError, RunError<JsError>>;

/// Drive the shipped discovery loop against the exact engine.
///
/// This is the same entry point the binary uses, deliberately: the bug that
/// motivated the horizon half of discovery hid in the gap between what the CLI
/// called and what the tests called.
fn run_exact(c: &mut Criteria, gaps: &[u32], connectors: u32) -> Result<Vec<Probability>, Failure> {
    let gaps = gaps.to_vec();
    with_discovery(
        c,
        |q| grouping_for(q, connectors),
        |_| gaps.clone(),
        |g, gaps, c| {
            let n = c.criteria().len();
            pe_criteria::run(g, gaps, n, c)
        },
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
        |g, gaps, c| {
            let n = c.criteria().len();
            pe_criteria::run(g, gaps, n, c)
        },
    )
    .unwrap();

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
        |g, gaps, c| {
            let n = c.criteria().len();
            pe_criteria::run(g, gaps, n, c)
        },
    )
    .unwrap();

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
