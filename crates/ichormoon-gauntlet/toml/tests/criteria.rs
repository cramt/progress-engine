//! The criteria file as the two engines see it.
//!
//! The engine itself is tested with Rust closures in gauntlet-criteria and the
//! sampler against closed-form hypergeometrics in gauntlet-sim. What is under test
//! here is the layer between a file on disk and those engines: that a clause
//! reads the count it names at the turn it names, that the two TOML spellings
//! of the same document are the same document, that a file asking something
//! unanswerable refuses instead, and that the exact and sampled engines running
//! the same file still agree.

use gauntlet_criteria::{
    CastingPolicy, Cost, Delay, Effect, Grouping, ManaSource, Outcomes, Palette, Policies,
    Resolves, Route, RunError, Schedule, Trigger, Zone, ZoneError,
};
use gauntlet_toml::{
    Criteria, Destination, EffectLibrary, ErrorKind, MAX_TURN, STANDARD_LIBRARY,
    STANDARD_LIBRARY_ORIGIN,
};

/// A synthetic library where every query the file names has cards of its own
/// and nothing overlaps: `each` copies per query, the rest matching nothing.
fn grouping_for(criteria: &Criteria, each: u32) -> Grouping {
    let queries = criteria.queries();
    let mut cards: Vec<(u64, u32)> = (0..queries.len()).map(|i| (1u64 << i, each)).collect();
    let used: u32 = cards.iter().map(|(_, qty)| qty).sum();
    cards.push((0, 99 - used));
    Grouping::build(queries.to_vec(), cards).expect("a grouping")
}

/// The opening seven, then one card per turn up to whatever the file asked for.
fn schedule(criteria: &Criteria) -> Schedule {
    // On the draw, so every turn past the opener sees one more card and the
    // helper does not have to special-case turn one.
    Schedule::build(criteria.horizon(), true, Vec::new(), Policies::default())
}

fn parse(source: &str) -> Criteria {
    Criteria::parse(source, "test.criteria.toml").expect("should parse")
}

fn refuse(source: &str) -> ErrorKind {
    Criteria::parse(source, "test.criteria.toml")
        .expect_err("should be refused")
        .kind
}

fn run_exact(source: &str, each: u32) -> Outcomes {
    let mut criteria = parse(source);
    let grouping = grouping_for(&criteria, each);
    let schedule = schedule(&criteria);
    let plan = criteria.plan();
    gauntlet_criteria::run(&grouping, &schedule, plan, &mut criteria).expect("should run")
}

fn percent(outcomes: &Outcomes, index: usize) -> f64 {
    outcomes.probabilities[index].get() * 100.0
}

#[test]
fn a_clause_reproduces_the_closed_form_answer() {
    // Six cards in a 99-card library, seven drawn. P(at least one) is one minus
    // the hypergeometric probability of exactly none, and nothing on the path
    // from this TOML to that number goes near that arithmetic.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "an outlet in the opener"
        at_least = 0.35
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
        6,
    );
    let want = (1.0 - chip_stats::pmf(99, 6, 7, 0)) * 100.0;
    assert!(
        (percent(&out, 0) - want).abs() < 1e-9,
        "{} vs {want}",
        percent(&out, 0)
    );
}

#[test]
fn both_toml_spellings_are_the_same_question() {
    // An inline table and an expanded [[criterion.require]] table are the same
    // TOML document. A generator writes one, a person writes the other, and the
    // day the two answer differently is the day a saved file loaded back into a
    // builder quietly changes a deck's numbers.
    let inline = r#"
        [[criterion]]
        name = "two by turn 2"
        at_least = 0.3
        require = [
          { turn = 1, query = 'cat:"arm"', min = 1 },
          { turn = 2, query = 'cat:"connector"', min = 1 },
        ]
    "#;
    let expanded = r#"
        [[criterion]]
        name = "two by turn 2"
        at_least = 0.3

          [[criterion.require]]
          turn = 1
          query = 'cat:"arm"'
          min = 1

          [[criterion.require]]
          turn = 2
          query = 'cat:"connector"'
          min = 1
    "#;
    assert_eq!(parse(inline).queries(), parse(expanded).queries());
    assert_eq!(parse(inline).horizon(), parse(expanded).horizon());
    assert_eq!(
        percent(&run_exact(inline, 6), 0),
        percent(&run_exact(expanded, 6), 0)
    );
}

#[test]
fn a_turn_is_everything_seen_by_then() {
    // Cumulative, not per-turn: turn 3 has seen the opening seven plus three
    // draws. The same clause at a later turn can only get easier.
    let by = |turn: u32| {
        let src = format!(
            r#"
            [[criterion]]
            name = "an outlet"
            require = [{{ turn = {turn}, query = 'cat:"arm"', min = 1 }}]
            "#
        );
        percent(&run_exact(&src, 6), 0)
    };
    assert!(by(0) < by(1), "{} then {}", by(0), by(1));
    assert!(by(1) < by(3), "{} then {}", by(1), by(3));
}

#[test]
fn clauses_are_anded_and_a_range_is_two_sided() {
    // Three questions off one enumeration, and the bounded one must sit inside
    // the open one rather than beside it.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "at least two"
        require = [{ turn = 0, query = 'cat:"arm"', min = 2 }]

        [[criterion]]
        name = "two or three"
        require = [{ turn = 0, query = 'cat:"arm"', min = 2, max = 3 }]

        [[criterion]]
        name = "at most one"
        require = [{ turn = 0, query = 'cat:"arm"', max = 1 }]
        "#,
        20,
    );
    assert_eq!(out.probabilities.len(), 3);
    assert!(percent(&out, 1) < percent(&out, 0), "the range is narrower");
    // And it is exactly two or exactly three, in closed form: a range that
    // read its upper bound backwards would still be narrower.
    let range = (chip_stats::pmf(99, 20, 7, 2) + chip_stats::pmf(99, 20, 7, 3)) * 100.0;
    assert!(
        (percent(&out, 1) - range).abs() < 1e-9,
        "{} vs {range}",
        percent(&out, 1)
    );
    // "at most one" is the complement of "at least two", so the two partition
    // every hand and have to sum to 100.
    assert!(
        (percent(&out, 0) + percent(&out, 2) - 100.0).abs() < 1e-9,
        "{} + {}",
        percent(&out, 0),
        percent(&out, 2)
    );
}

#[test]
fn an_expectation_reproduces_the_closed_form_distribution() {
    // Six arming outlets in a 99-card library, eleven cards seen. The mean of a
    // hypergeometric is draws * successes / population in closed form.
    let out = run_exact(
        r#"
        [[expect]]
        name = "arms by turn 4"
        turn = 4
        query = 'cat:"arm"'
        "#,
        6,
    );
    assert!(out.probabilities.is_empty(), "expect() is not a criterion");
    let d = &out.distributions[0];
    let closed = chip_stats::mean(99, 6, 11);
    assert!(
        (d.mean() - closed).abs() < 1e-12,
        "mean {} vs closed form {closed}",
        d.mean()
    );
    assert!((d.total() - 1.0).abs() < 1e-12, "summed to {}", d.total());
    for k in 0..=6u32 {
        let want = chip_stats::pmf(99, 6, 11, k);
        assert!(
            (d.probabilities()[k as usize] - want).abs() < 1e-12,
            "P(exactly {k}) was {}",
            d.probabilities()[k as usize]
        );
    }
}

#[test]
fn criteria_and_expectations_share_one_pass() {
    // Both kinds in one file, answered off one enumeration. The criterion must
    // equal the tail of the distribution beside it, or the two halves of the
    // report are describing different runs.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "at least one"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]

        [[expect]]
        name = "how many"
        turn = 0
        query = 'cat:"arm"'
        "#,
        6,
    );
    let tail: f64 = out.distributions[0].probabilities()[1..].iter().sum();
    assert!(
        (out.probabilities[0].get() - tail).abs() < 1e-12,
        "{} vs {tail}",
        out.probabilities[0].get()
    );
}

#[test]
fn a_query_named_twice_is_one_query() {
    // The enumeration grows with the number of *distinct* queries, and two
    // criteria asking about lands are one column in the grouping, not two.
    let c = parse(
        r#"
        [[criterion]]
        name = "one"
        require = [{ turn = 0, query = "t:land", min = 1 }]

        [[criterion]]
        name = "two"
        require = [{ turn = 4, query = "t:land", min = 2 }]

        [[expect]]
        name = "three"
        turn = 2
        query = "t:land"
        "#,
    );
    assert_eq!(c.queries(), ["t:land"]);
    assert_eq!(c.horizon(), 4, "the deepest turn anything names");
}

#[test]
fn the_whole_query_set_is_known_before_anything_runs() {
    // The property the JavaScript front end could not have, and the reason a
    // too-wide refusal can now name what the file asked for: nothing has to be
    // evaluated to learn which queries are in play.
    let c = parse(
        r#"
        [[criterion]]
        name = "deep"
        require = [
          { turn = 0, query = "t:land", min = 1 },
          { turn = 6, query = 'cat:"Ramp"', min = 1 },
        ]

        [[expect]]
        name = "wider"
        turn = 1
        query = "produces:g"
        "#,
    );
    assert_eq!(c.queries(), ["t:land", r#"cat:"Ramp""#, "produces:g"]);
    assert_eq!(c.asked_by("produces:g"), Some("wider"));
    assert_eq!(c.asked_by("t:land"), Some("deep"));
}

#[test]
fn a_question_too_wide_to_enumerate_names_every_query_it_asks_about() {
    // Issue #36. The refusal used to report the queries discovered so far,
    // because the only way to learn them was to run the file and the run was
    // refused part-way through. There is no part-way any more.
    let mut criteria = parse(
        r#"
        [[criterion]]
        name = "far too much at once"
        require = [
          { turn = 8, query = "a", min = 1 },
          { turn = 8, query = "b", min = 1 },
          { turn = 8, query = "c", min = 1 },
          { turn = 8, query = "d", min = 1 },
          { turn = 8, query = "e", min = 1 },
          { turn = 8, query = "f", min = 1 },
          { turn = 8, query = "g", min = 1 },
        ]
        "#,
    );
    let grouping = grouping_for(&criteria, 6);
    let schedule = schedule(&criteria);
    let plan = criteria.plan();
    let err =
        gauntlet_criteria::run(&grouping, &schedule, plan, &mut criteria).expect_err("too wide");
    assert!(matches!(err, RunError::TooWide { .. }), "{err}");
    let msg = err.to_string();
    for query in ["a", "b", "c", "d", "e", "f", "g"] {
        assert!(msg.contains(query), "should name {query:?}: {msg}");
    }
}

#[test]
fn the_exact_and_sampled_engines_answer_the_same_file_the_same_way() {
    // The middle of the three levels the two engines are held to each other at.
    // One criteria file, one grouping, two engines: anything this layer gets
    // wrong about which count a clause reads would move both answers together
    // and go unnoticed at the other two levels.
    //
    // A disjunction is in here for the same reason, and its probability is
    // neither 0 nor 1 on purpose: an engine that ignored `any_of` entirely, or
    // that took the first branch and stopped, would land somewhere a sampler
    // could not follow it to.
    //
    // Zones are in here deliberately. `gauntlet_sim` is generic over `Evaluator` and
    // never mentions a zone, which is either the reason it cannot disagree with
    // the exact engine or the reason it silently ignores zones entirely — and
    // only a clause whose answer *moves* with the zone tells those apart. The
    // library clause is that clause; the graveyard one would agree at zero
    // whether or not either engine had ever heard of it.
    let source = r#"
        [[criterion]]
        name = "an outlet in the opener"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]

        [[criterion]]
        name = "two by turn 3"
        require = [
          { turn = 0, query = 'cat:"arm"', min = 1 },
          { turn = 3, query = 'cat:"connector"', min = 2 },
        ]

        [[criterion]]
        name = "most arms still in the deck by turn 3"
        require = [{ turn = 3, query = 'cat:"arm"', zone = "library", min = 7 }]

        [[criterion]]
        name = "an arm in the yard by turn 3"
        require = [{ turn = 3, query = 'cat:"arm"', zone = "graveyard", min = 1 }]

        [[criterion]]
        name = "either route by turn 3"
        any_of = [
          { require = [{ turn = 0, query = 'cat:"arm"', min = 2 }] },
          { require = [{ turn = 3, query = 'cat:"connector"', min = 2 }] },
        ]

        [[expect]]
        name = "arms by turn 3"
        turn = 3
        query = 'cat:"arm"'

        [[expect]]
        name = "arms left in the deck by turn 3"
        turn = 3
        query = 'cat:"arm"'
        zone = "library"
    "#;
    let mut criteria = parse(source);
    let grouping = grouping_for(&criteria, 8);
    let schedule = schedule(&criteria);
    let plan = criteria.plan();

    let exact = gauntlet_criteria::run(&grouping, &schedule, plan, &mut criteria).expect("exact");
    // The disjunction has to be a question the sampler could get wrong. At 0 or
    // 1 it agrees with anything.
    let disjunction = exact.probabilities[4].get();
    assert!(
        (0.05..0.95).contains(&disjunction),
        "the any_of criterion should be genuinely uncertain, was {disjunction}"
    );
    let trials = 200_000;
    let sampled = gauntlet_sim::simulate(&grouping, &schedule, trials, 7, plan, &mut criteria)
        .expect("sampled");

    for (i, want) in exact.probabilities.iter().enumerate() {
        let got = sampled.proportions[i];
        let se = gauntlet_sim::standard_error(got, trials);
        // Not-greater-than rather than strictly-less-than, because a zone
        // nothing routes into gives both engines a standard error of exactly
        // zero, and "agrees to within zero" is the strongest form of agreement
        // rather than a failure to agree.
        assert!(
            (got - want.get()).abs() <= 5.0 * se,
            "criterion {i}: sampled {got} vs exact {}, {:.2} SE away",
            want.get(),
            (got - want.get()).abs() / se
        );
    }
    for (i, want) in exact.distributions.iter().enumerate() {
        let got = &sampled.distributions[i];
        let se = gauntlet_sim::mean_standard_error(got, trials);
        assert!(
            (got.mean() - want.mean()).abs() <= 5.0 * se,
            "expectation {i}: sampled mean {} vs exact {}",
            got.mean(),
            want.mean()
        );
    }
}

#[test]
fn a_file_run_against_a_spell_that_draws_agrees_in_both_engines() {
    // The criteria layer with a sized gap firing (ADR-0017). No `[[effect]]`
    // key draws yet, so the drawing spell is built by hand beside the file's
    // own queries; what is under test is that the file's clauses read the
    // same board in both engines once a cast has dealt cards mid-turn.
    let source = r#"
        [[criterion]]
        name = "a target in hand by turn 3"
        require = [{ turn = 3, query = 'cat:"target"', min = 1 }]

        [[criterion]]
        name = "no drawer cast by turn 3"
        require = [{ turn = 3, cast = 'cat:"drawer"', max = 0 }]

        [[criterion]]
        name = "two drawers cast by turn 3"
        require = [{ turn = 3, cast = 'cat:"drawer"', min = 2 }]

        [[expect]]
        name = "targets left in the library by turn 3"
        turn = 3
        query = 'cat:"target"'
        zone = "library"
    "#;
    let mut criteria = parse(source);
    let bit = |query: &str| {
        1u64 << criteria
            .queries()
            .iter()
            .position(|q| q == query)
            .expect("the file names it")
    };
    let (target, drawer) = (bit(r#"cat:"target""#), bit(r#"cat:"drawer""#));
    let grouping = Grouping::with_mana(
        criteria.queries().to_vec(),
        vec![
            (
                drawer,
                ManaSource::Castable {
                    cost: Cost::parse("{U}").unwrap().demand(),
                    resolves: Resolves::IntoGraveyard,
                },
                5,
            ),
            (target, ManaSource::Spell, 3),
            (
                0,
                ManaSource::Land {
                    enters_tapped: false,
                    produces: Palette::from_letters(["U"]),
                    lasts: None,
                },
                17,
            ),
            (0, ManaSource::Spell, 35),
        ],
    )
    .unwrap();
    let drawing = Effect {
        matched_by: drawer.trailing_zeros() as usize,
        look: 0,
        trigger: Trigger::Cast,
        route: Route::Nowhere,
        fetch: None,
        delay: None,
        draw: 1,
    };
    let schedule = Schedule::build(
        criteria.horizon(),
        false,
        vec![drawing],
        Policies::casting(CastingPolicy::new(vec![drawer.trailing_zeros() as usize])),
    );
    let plan = criteria.plan();
    let exact = gauntlet_criteria::run(&grouping, &schedule, plan, &mut criteria).expect("exact");
    let p = |i: usize| exact.probabilities[i].get();
    assert!(
        p(1) > 0.05 && p(2) > 0.05,
        "the gap fires on some paths and not on others: {:?}",
        exact.probabilities
    );
    let trials = 100_000;
    let sampled = gauntlet_sim::simulate(&grouping, &schedule, trials, 11, plan, &mut criteria)
        .expect("sampled");
    for (i, want) in exact.probabilities.iter().enumerate() {
        let got = sampled.proportions[i];
        let se = gauntlet_sim::standard_error(got, trials);
        assert!(
            (got - want.get()).abs() <= 5.0 * se,
            "criterion {i}: sampled {got} vs exact {}, {:.2} SE away",
            want.get(),
            (got - want.get()).abs() / se
        );
    }
    let (want, got) = (&exact.distributions[0], &sampled.distributions[0]);
    let se = gauntlet_sim::mean_standard_error(got, trials);
    assert!(
        (got.mean() - want.mean()).abs() <= 5.0 * se,
        "targets left: sampled mean {} vs exact {}",
        got.mean(),
        want.mean()
    );
}

// --- Disjunction -----------------------------------------------------------

#[test]
fn both_toml_spellings_of_any_of_are_the_same_question() {
    // The same promise `require` makes, one level deeper: a generator emits
    // inline tables and a person writes sections, and TOML says the two are one
    // document. The nested [[criterion.any_of.require]] form is the one a hand
    // reaches for and the one most likely to be read by a different parser, so
    // it is asserted rather than assumed.
    let inline = r#"
        [[criterion]]
        name = "either route"
        at_least = 0.3
        any_of = [
          { require = [{ turn = 1, query = 'cat:"arm"', min = 1 }] },
          { require = [
              { turn = 2, query = 'cat:"connector"', min = 1 },
              { turn = 3, query = 'cat:"arm"', min = 2 },
          ] },
        ]
    "#;
    let expanded = r#"
        [[criterion]]
        name = "either route"
        at_least = 0.3

          [[criterion.any_of]]
          require = [{ turn = 1, query = 'cat:"arm"', min = 1 }]

          [[criterion.any_of]]

            [[criterion.any_of.require]]
            turn = 2
            query = 'cat:"connector"'
            min = 1

            [[criterion.any_of.require]]
            turn = 3
            query = 'cat:"arm"'
            min = 2
    "#;
    assert_eq!(parse(inline).queries(), parse(expanded).queries());
    assert_eq!(parse(inline).horizon(), parse(expanded).horizon());
    let answer = percent(&run_exact(inline, 6), 0);
    assert_eq!(answer, percent(&run_exact(expanded, 6), 0));
    assert!((0.0..100.0).contains(&answer), "{answer} is a real number");
}

#[test]
fn a_disjunction_is_the_union_and_not_the_sum_of_its_routes() {
    // The routes a deck has are not mutually exclusive — a hand can hold the
    // card *and* the tutor for it — so adding the branches would double-count
    // every hand that has both, and with routes this likely the total would
    // exceed 100%. A percentage above one is the confidently-wrong number this
    // project exists to prevent, so it gets an assertion of its own.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "either route"
        any_of = [
          { require = [{ turn = 0, query = 'cat:"arm"', min = 1 }] },
          { require = [{ turn = 3, query = 'cat:"connector"', min = 1 }] },
        ]

        [[criterion]]
        name = "route a"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]

        [[criterion]]
        name = "route b"
        require = [{ turn = 3, query = 'cat:"connector"', min = 1 }]

        [[criterion]]
        name = "both routes"
        require = [
          { turn = 0, query = 'cat:"arm"', min = 1 },
          { turn = 3, query = 'cat:"connector"', min = 1 },
        ]
        "#,
        20,
    );
    let (union, a, b, both) = (
        percent(&out, 0),
        percent(&out, 1),
        percent(&out, 2),
        percent(&out, 3),
    );
    assert!(
        a + b > 100.0,
        "the branches overlap enough to matter: {a} + {b}"
    );
    assert!(union <= 100.0, "a probability, not a total: {union}");
    // Inclusion-exclusion, computed the long way round from three separate
    // criteria and matched against the one the engine answered in a single
    // walk. Nothing in `Predicate` does this arithmetic; it falls out of each
    // path contributing its probability once.
    assert!(
        (union - (a + b - both)).abs() < 1e-9,
        "union {union} vs |a| + |b| - |a and b| = {}",
        a + b - both
    );
    assert!(union > a.max(b), "and it is more than either route alone");
}

#[test]
fn a_branch_that_subsumes_another_adds_nothing_to_it() {
    // The same query at two turns: everything the opener has, turn 3 has too.
    // The union is exactly the wider branch, and a sum would be nearly twice
    // it.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "either turn"
        any_of = [
          { require = [{ turn = 0, query = 'cat:"arm"', min = 1 }] },
          { require = [{ turn = 3, query = 'cat:"arm"', min = 1 }] },
        ]

        [[criterion]]
        name = "the wider branch alone"
        require = [{ turn = 3, query = 'cat:"arm"', min = 1 }]
        "#,
        20,
    );
    assert_eq!(percent(&out, 0), percent(&out, 1));
}

#[test]
fn require_and_any_of_are_a_conjunction_of_the_two() {
    // The combination the shape is for: a precondition every route shares,
    // then the routes. It has to be no likelier than either half on its own.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "precondition and a route"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        any_of = [
          { require = [{ turn = 2, query = 'cat:"connector"', min = 1 }] },
          { require = [{ turn = 3, query = 'cat:"spark"', min = 1 }] },
        ]

        [[criterion]]
        name = "the precondition alone"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]

        [[criterion]]
        name = "the routes alone"
        any_of = [
          { require = [{ turn = 2, query = 'cat:"connector"', min = 1 }] },
          { require = [{ turn = 3, query = 'cat:"spark"', min = 1 }] },
        ]
        "#,
        12,
    );
    let (both, precondition, routes) = (percent(&out, 0), percent(&out, 1), percent(&out, 2));
    assert!(both < precondition, "{both} vs {precondition}");
    assert!(both < routes, "{both} vs {routes}");
    assert!(both > 0.0, "and it is not a confident zero");
}

#[test]
fn one_branch_is_legal_and_is_just_that_branch() {
    // A generator building a disjunction one route at a time passes through
    // this state, and it is a well-formed question rather than a half-written
    // one.
    let single = run_exact(
        r#"
        [[criterion]]
        name = "one route"
        any_of = [{ require = [{ turn = 2, query = 'cat:"arm"', min = 2 }] }]
        "#,
        9,
    );
    let plain = run_exact(
        r#"
        [[criterion]]
        name = "the same thing"
        require = [{ turn = 2, query = 'cat:"arm"', min = 2 }]
        "#,
        9,
    );
    assert_eq!(percent(&single, 0), percent(&plain, 0));
}

#[test]
fn a_branch_names_its_queries_and_turns_to_the_whole_file() {
    // A query only a branch mentions still has to be in the set the grouping is
    // built from and the too-wide refusal lists, and a turn only a branch
    // mentions still has to be inside the run horizon. Otherwise the criterion
    // reads a count of zero from a query nobody grouped and fails quietly.
    let c = parse(
        r#"
        [[criterion]]
        name = "routes"
        require = [{ turn = 0, query = "t:land", min = 1 }]
        any_of = [
          { require = [{ turn = 4, query = 'cat:"Ramp"', zone = "graveyard", min = 1 }] },
          { require = [{ turn = 6, query = "t:land", min = 3 }] },
        ]
        "#,
    );
    assert_eq!(c.queries(), ["t:land", r#"cat:"Ramp""#]);
    assert_eq!(c.horizon(), 6, "the deepest turn any branch names");
    assert_eq!(c.asked_by(r#"cat:"Ramp""#), Some("routes"));
    assert_eq!(c.zones(), [Zone::Hand, Zone::Graveyard]);
    assert_eq!(c.zone_asked_by(Zone::Graveyard), Some("routes"));
}

#[test]
fn a_disjunction_costs_no_query_a_conjunction_would_not() {
    // #47 is the reason this is measured rather than assumed. The queries are
    // what the enumeration is built from, so a disjunction over routes that
    // name the same queries as a set of separate criteria has to hand the
    // engine the same set — the alternation is in the predicate, not in the
    // grouping.
    let routes = r#"
        [[criterion]]
        name = "a"
        require = [{ turn = 5, query = 'name:"Lantern of Insight"', min = 1 }]

        [[criterion]]
        name = "b"
        require = [{ turn = 4, query = 'name:"Trinket Mage"', min = 1 }]

        [[criterion]]
        name = "c"
        require = [{ turn = 3, query = "name:\"Urza's Saga\"", min = 1 }]
    "#;
    let disjunction = r#"
        [[criterion]]
        name = "a castable lantern by turn 5"
        any_of = [
          { require = [{ turn = 5, query = 'name:"Lantern of Insight"', min = 1 }] },
          { require = [{ turn = 4, query = 'name:"Trinket Mage"', min = 1 }] },
          { require = [{ turn = 3, query = "name:\"Urza's Saga\"", min = 1 }] },
        ]
    "#;
    assert_eq!(parse(routes).queries(), parse(disjunction).queries());
    // And the grouping the engine walks is the same width, group for group.
    let (separate, together) = (parse(routes), parse(disjunction));
    assert_eq!(
        grouping_for(&separate, 4).group_sizes().len(),
        grouping_for(&together, 4).group_sizes().len()
    );
}

// --- Zones -----------------------------------------------------------------

#[test]
fn an_unsaid_zone_is_the_hand_and_saying_so_changes_nothing() {
    // The compatibility promise, asserted rather than assumed: every criteria
    // file written before zones existed asked about the hand, so the two
    // spellings below have to be the same question down to the last digit.
    let silent = run_exact(
        r#"
        [[criterion]]
        name = "an arm in the opener"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
        6,
    );
    let spelled = run_exact(
        r#"
        [[criterion]]
        name = "an arm in the opener"
        require = [{ turn = 0, query = 'cat:"arm"', zone = "hand", min = 1 }]
        "#,
        6,
    );
    assert_eq!(percent(&silent, 0), percent(&spelled, 0));
    assert_eq!(
        parse(
            r#"[[criterion]]
                 name = "x"
                 require = [{ turn = 0, query = "t:land", min = 1 }]"#
        )
        .zones(),
        [Zone::Hand],
        "a file that names no zone still asks about one, and it is the hand"
    );
}

#[test]
fn the_battlefield_is_a_zone_this_file_hands_on_for_checking() {
    // Whether it is answerable is a fact about the cards rather than about the
    // file — a land gets there on a land drop and a spell has to be cast — so
    // this crate parses it, records that the question was asked, and hands the
    // query to whoever holds the card data.
    let criteria = parse(
        r#"
        [[criterion]]
        name = "a land in play by turn 3"
        require = [{ turn = 3, query = "t:land", zone = "battlefield", min = 2 }]
        "#,
    );
    assert_eq!(criteria.zones(), [Zone::Battlefield]);
    assert_eq!(
        criteria.battlefield_queries(),
        [("t:land", "a land in play by turn 3")]
    );
    assert_eq!(criteria.mana_question(), Some("a land in play by turn 3"));
    // And it is not the half that needs to know what a land *makes*.
    assert_eq!(criteria.casts(), None);
}

#[test]
fn an_unknown_zone_is_refused_and_lists_the_ones_that_work() {
    // Same discipline as an unknown key or an unsupported query term: a closed
    // set is named, because a typo that fell through to a default would report
    // a hand count under an exile question.
    let err = refuse(
        r#"
        [[criterion]]
        name = "loam in the yrad"
        require = [{ turn = 5, query = "t:land", zone = "yrad", min = 1 }]
        "#,
    );
    assert!(
        matches!(err, ErrorKind::BadZone { zone: ZoneError::Unknown { ref name }, .. } if name == "yrad"),
        "{err}"
    );
    let msg = err.to_string();
    for accepted in ["hand", "graveyard", "library", "battlefield"] {
        assert!(msg.contains(accepted), "lists {accepted}: {msg}");
    }
}

#[test]
fn the_whole_zone_set_is_known_before_anything_runs() {
    // Discovered from the file, exactly like the query set, and for the same
    // reason: a run has to be able to say what it was asked before it answers,
    // and a file that never says `graveyard` must not pay to track one.
    let criteria = parse(
        r#"
        [[criterion]]
        name = "loam in the yard"
        require = [
          { turn = 5, query = 'name:"Loam"', zone = "graveyard", min = 1 },
          { turn = 5, query = "t:land", min = 3 },
        ]

        [[expect]]
        name = "lands left in the deck"
        turn = 5
        query = "t:land"
        zone = "library"
        "#,
    );
    assert_eq!(
        criteria.zones(),
        [Zone::Graveyard, Zone::Hand, Zone::Library],
        "every zone, deduplicated, in first-mention order"
    );
    assert_eq!(
        criteria.zone_asked_by(Zone::Graveyard),
        Some("loam in the yard")
    );
    assert_eq!(
        criteria.zone_asked_by(Zone::Library),
        Some("lands left in the deck")
    );
}

#[test]
fn the_graveyard_is_askable_and_correctly_empty() {
    // Honest while half-built. Nothing routes a card to the graveyard yet, so
    // the answer is zero — and it is zero because it was computed, not because
    // the question was dropped. The CLI is what has to say which of those the
    // reader is looking at.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "loam in the yard by turn 5"
        require = [{ turn = 5, query = 'cat:"arm"', zone = "graveyard", min = 1 }]

        [[criterion]]
        name = "an empty yard"
        require = [{ turn = 5, query = 'cat:"arm"', zone = "graveyard", max = 0 }]
        "#,
        40,
    );
    assert_eq!(percent(&out, 0), 0.0, "no path ever puts a card there");
    // The complement collects every path's probability rather than none of it,
    // so it lands on 100% within the enumeration's summation error.
    assert!(
        (percent(&out, 1) - 100.0).abs() < 1e-9,
        "{}",
        percent(&out, 1)
    );
}

#[test]
fn the_library_is_what_the_hand_is_not() {
    // `library` is free rather than tracked: it is the deck's matching cards
    // minus the ones this path has drawn. So a bound on one is exactly the
    // complement of a bound on the other, and if it is not, the subtraction is
    // reading the wrong total.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "at most two drawn"
        require = [{ turn = 3, query = 'cat:"arm"', max = 2 }]

        [[criterion]]
        name = "at least six left"
        require = [{ turn = 3, query = 'cat:"arm"', zone = "library", min = 6 }]
        "#,
        8,
    );
    assert_eq!(percent(&out, 0), percent(&out, 1));
    // And not vacuous in either direction, which is what a broken complement
    // would look like.
    assert!(percent(&out, 0) > 0.0 && percent(&out, 0) < 100.0);
}

// --- Files that would otherwise have answered ------------------------------

#[test]
fn a_file_that_asks_nothing_is_refused() {
    assert!(matches!(refuse(""), ErrorKind::AsksNothing));
    assert!(matches!(
        refuse("# only a comment\n"),
        ErrorKind::AsksNothing
    ));
}

#[test]
fn a_criterion_with_no_clauses_is_refused_by_name() {
    // An empty conjunction holds on every hand, so this would report a
    // confident 100% of any deck ever written.
    let err = refuse(
        r#"
        [[criterion]]
        name = "nothing at all"
        at_least = 0.5
        "#,
    );
    assert!(matches!(&err, ErrorKind::NoClauses { name } if name == "nothing at all"));
    let msg = err.to_string();
    assert!(msg.contains("nothing at all"), "{msg}");
    // Both keys are named, because "no clauses" stopped being the only way to
    // write a criterion that asks nothing the day `any_of` existed.
    assert!(msg.contains("require") && msg.contains("any_of"), "{msg}");

    // And the same for a `require` that is written out but empty.
    assert!(matches!(
        refuse("[[criterion]]\nname = \"x\"\nrequire = []\n"),
        ErrorKind::NoClauses { .. }
    ));
}

#[test]
fn an_empty_any_of_is_refused_by_name() {
    // The other direction of the same mistake: an empty conjunction holds on
    // every hand, an empty disjunction on none. Refused even beside a `require`
    // that would otherwise carry the criterion, because a disjunction of no
    // routes is not a thing anybody meant to ask.
    let err = refuse(
        r#"
        [[criterion]]
        name = "no route at all"
        any_of = []
        "#,
    );
    assert!(matches!(&err, ErrorKind::EmptyAnyOf { name } if name == "no route at all"));
    assert!(err.to_string().contains("no route at all"));

    assert!(matches!(
        refuse(
            r#"
            [[criterion]]
            name = "a precondition and no route"
            require = [{ turn = 0, query = "t:land", min = 2 }]
            any_of = []
            "#
        ),
        ErrorKind::EmptyAnyOf { .. }
    ));
}

#[test]
fn a_branch_with_no_clauses_is_refused_by_name() {
    // A branch that asks nothing holds on every hand, so the whole criterion
    // would report 100% whatever the other branches say — and the report would
    // show a route nobody has that always works.
    let err = refuse(
        r#"
        [[criterion]]
        name = "two routes, one of them empty"
        any_of = [
          { require = [{ turn = 3, query = "t:land", min = 3 }] },
          { require = [] },
        ]
        "#,
    );
    assert!(
        matches!(&err, ErrorKind::EmptyBranch { name, position: 2 } if name == "two routes, one of them empty"),
        "{err}"
    );
    let msg = err.to_string();
    assert!(msg.contains("two routes, one of them empty"), "{msg}");
    assert!(msg.contains("branch 2"), "{msg}");

    // A [[criterion.any_of]] section with nothing in it at all is the same
    // mistake written the other way.
    assert!(matches!(
        refuse("[[criterion]]\nname = \"x\"\n[[criterion.any_of]]\n"),
        ErrorKind::EmptyBranch { position: 1, .. }
    ));
}

#[test]
fn a_clause_inside_a_branch_is_checked_like_any_other() {
    // Validation does not get thinner one level down. A branch clause with no
    // bounds, a bad turn or an unmodelled zone is refused exactly as a
    // `require` clause is, and the message says which branch it was in.
    let err = refuse(
        r#"
        [[criterion]]
        name = "routes"
        any_of = [
          { require = [{ turn = 3, query = "t:land", min = 1 }] },
          { require = [{ turn = 4, query = 'cat:"Ramp"' }] },
        ]
        "#,
    );
    assert!(matches!(err, ErrorKind::NoBounds { .. }), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("any_of branch 2, clause 1"), "{msg}");

    let err = refuse(
        r#"
        [[criterion]]
        name = "routes"
        any_of = [
          { require = [{ turn = 3, query = "t:land", zone = "exile", min = 1 }] },
        ]
        "#,
    );
    assert!(
        matches!(
            err,
            ErrorKind::BadZone {
                zone: ZoneError::Unknown { .. },
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn a_clause_with_neither_min_nor_max_is_refused_by_name() {
    // Read as a tautology it holds on every hand. The only honest reading of a
    // clause that names a turn and a query and asks nothing of them is that
    // somebody meant to write a bound.
    let err = refuse(
        r#"
        [[criterion]]
        name = "lands in opener"
        require = [{ turn = 0, query = "t:land" }]
        "#,
    );
    assert!(matches!(err, ErrorKind::NoBounds { .. }), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("lands in opener"), "{msg}");
    assert!(msg.contains("clause 1"), "{msg}");
    assert!(msg.contains("t:land"), "{msg}");
}

#[test]
fn a_range_no_hand_can_satisfy_is_refused_rather_than_answered_zero() {
    let err = refuse(
        r#"
        [[criterion]]
        name = "backwards"
        require = [{ turn = 0, query = "t:land", min = 5, max = 2 }]
        "#,
    );
    assert!(matches!(err, ErrorKind::EmptyRange { min: 5, max: 2, .. }));
}

#[test]
fn an_unknown_key_is_refused_and_the_message_says_what_the_format_has() {
    // The JavaScript spelling of the threshold. Dropping it quietly would turn
    // an assertion into an informational number that cannot fail.
    let err = refuse(
        r#"
        [[criterion]]
        name = "keepable opener"
        atLeast = 0.7
        require = [{ turn = 0, query = "t:land", min = 2 }]
        "#,
    );
    let msg = err.to_string();
    assert!(msg.contains("atLeast"), "names the key: {msg}");
    assert!(msg.contains("at_least"), "lists the real ones: {msg}");

    // The schema line has to list `zone` now that clauses take one, because
    // this message is the only place a reader is told what the format has.
    assert!(msg.contains("zone"), "lists zone as a real key: {msg}");
}

#[test]
fn a_turn_that_is_not_a_turn_is_refused() {
    let negative = refuse(
        r#"
        [[criterion]]
        name = "before the game"
        require = [{ turn = -1, query = "t:land", min = 1 }]
        "#,
    );
    assert!(
        matches!(negative, ErrorKind::BadTurn { turn: -1, .. }),
        "{negative}"
    );

    // Past the ceiling, which exists because the run horizon becomes one
    // checkpoint per turn and a typo should not ask for a billion of them.
    let far = refuse(&format!(
        r#"
        [[criterion]]
        name = "long game"
        require = [{{ turn = {}, query = "t:land", min = 1 }}]
        "#,
        MAX_TURN + 1
    ));
    assert!(matches!(far, ErrorKind::BadTurn { .. }), "{far}");
}

#[test]
fn a_threshold_outside_zero_to_one_is_refused() {
    // `at_least = 70` is the obvious mistake, and it is one no hand can meet,
    // so left alone it would fail every deck for a reason nothing stated.
    let err = refuse(
        r#"
        [[criterion]]
        name = "percent, surely"
        at_least = 70
        require = [{ turn = 0, query = "t:land", min = 2 }]
        "#,
    );
    assert!(matches!(err, ErrorKind::BadThreshold { .. }), "{err}");
    assert!(err.to_string().contains("0.70"), "{err}");

    // The same mistake at the other end, named by the key it was made under.
    let err = refuse(
        r#"
        [[criterion]]
        name = "flooding, in percent"
        at_most = 15
        require = [{ turn = 0, query = "t:land", min = 6 }]
        "#,
    );
    assert!(
        matches!(err, ErrorKind::BadThreshold { key: "at_most", .. }),
        "{err}"
    );
    assert!(err.to_string().contains("at_most = 15"), "{err}");
}

#[test]
fn a_criterion_can_be_bounded_from_above_or_from_both_sides() {
    let criteria = parse(
        r#"
        [[criterion]]
        name = "flooded opener"
        at_most = 0.15
        require = [{ turn = 0, query = "t:land", min = 6 }]

        [[criterion]]
        name = "two lands on turn two"
        at_least = 0.40
        at_most = 0.60
        require = [{ turn = 2, query = "t:land", min = 2 }]

        [[criterion]]
        name = "exactly this often"
        at_least = 0.5
        at_most = 0.5
        require = [{ turn = 0, query = "t:land", min = 1 }]
        "#,
    );
    let c = criteria.criteria();
    assert_eq!((c[0].at_least, c[0].at_most), (None, Some(0.15)));
    assert_eq!((c[1].at_least, c[1].at_most), (Some(0.40), Some(0.60)));
    // A range one point wide is odd but satisfiable, so it is not refused.
    assert_eq!((c[2].at_least, c[2].at_most), (Some(0.5), Some(0.5)));
}

#[test]
fn a_range_no_probability_fits_in_is_refused() {
    // Written backwards, it fails every deck, and the report would blame the
    // deck for it.
    let err = refuse(
        r#"
        [[criterion]]
        name = "backwards"
        at_least = 0.60
        at_most = 0.40
        require = [{ turn = 0, query = "t:land", min = 2 }]
        "#,
    );
    assert!(matches!(err, ErrorKind::EmptyThreshold { .. }), "{err}");
    let msg = err.to_string();
    assert!(msg.contains("at_least = 0.6"), "{msg}");
    assert!(msg.contains("at_most = 0.4"), "{msg}");
}

#[test]
fn a_question_with_no_name_is_refused() {
    let err = refuse(
        r#"
        [[criterion]]
        at_least = 0.5
        require = [{ turn = 0, query = "t:land", min = 2 }]
        "#,
    );
    assert!(
        matches!(err, ErrorKind::Unnamed { position: 1, .. }),
        "{err}"
    );
    let err = refuse("[[expect]]\nturn = 0\nquery = \"t:land\"\n");
    assert!(matches!(err, ErrorKind::Unnamed { .. }), "{err}");
}

#[test]
fn a_clause_missing_its_turn_or_its_query_is_refused_by_name() {
    let err = refuse(
        r#"
        [[criterion]]
        name = "which turn?"
        require = [{ query = "t:land", min = 1 }]
        "#,
    );
    assert!(
        matches!(err, ErrorKind::Missing { key: "turn", .. }),
        "{err}"
    );
    assert!(err.to_string().contains("which turn?"), "{err}");

    let err = refuse("[[expect]]\nname = \"count what?\"\nturn = 0\n");
    assert!(
        matches!(err, ErrorKind::Missing { key: "query", .. }),
        "{err}"
    );
    assert!(err.to_string().contains("count what?"), "{err}");
}

#[test]
fn the_file_names_itself_in_every_refusal() {
    // A CI run testing six decks against six criteria files reports one line,
    // and "criterion has no clauses" on its own does not say which file to open.
    let err = Criteria::parse(
        "[[criterion]]\nname = \"x\"\n",
        "decks/goblins.criteria.toml",
    )
    .expect_err("should be refused");
    assert!(
        err.to_string().starts_with("decks/goblins.criteria.toml: "),
        "{err}"
    );
}

// --- Effects --------------------------------------------------------------

#[test]
fn an_effect_is_read_off_the_same_file_as_the_questions() {
    // `[[effect]]` lives in the criteria file because it is part of the
    // question: the same surveil land wants the Loam in the yard for one deck
    // and in hand for another, and only the file knows which deck it is.
    let criteria = parse(
        r#"
        [[effect]]
        match = "t:land otag:surveil"
        look = 1
        on = "landdrop"
        to_graveyard = 'name:"Life from the Loam"'

        [[criterion]]
        name = "loam in the yard"
        require = [{ turn = 5, query = 'cat:"arm"', zone = "graveyard", min = 1 }]
        "#,
    );
    let effects = criteria.effects().entries();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].matches, "t:land otag:surveil");
    assert_eq!(effects[0].look, 1);
    assert_eq!(effects[0].trigger, Trigger::LandDrop);
    assert_eq!(
        effects[0].to_graveyard,
        Some(Destination::Matching("name:\"Life from the Loam\"".into()))
    );
    // The effect's queries are not the file's queries. They are not questions,
    // so a query breakdown that listed them would be reporting on something
    // nobody asked about.
    assert_eq!(criteria.queries(), &["cat:\"arm\""]);
}

#[test]
fn a_look_with_no_destination_is_the_default() {
    // A refusal to guess rather than a missing feature: nobody has said where
    // the looked-at card should go, and every answer to that is somebody's
    // question rather than the card's property.
    let criteria = parse(
        r#"
        [[effect]]
        match = "t:land otag:surveil"
        look = 1
        on = "landdrop"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert_eq!(criteria.effects().entries()[0].to_graveyard, None);
}

#[test]
fn a_star_routes_everything_which_is_what_mill_is() {
    let criteria = parse(
        r#"
        [[effect]]
        match = 't:land'
        look = 2
        on = "landdrop"
        to_graveyard = "*"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert_eq!(
        criteria.effects().entries()[0].to_graveyard,
        Some(Destination::Everything)
    );
}

#[test]
fn a_look_on_a_cast_is_refused_and_a_trigger_nobody_fires_is_refused_by_name() {
    // `on = "cast"` fires: the budget knows which spells a turn paid for. What
    // it may do when it fires is the restriction — a fetch is a subtraction
    // from the library and a look is a replacement draw, which is the one that
    // costs an enumeration checkpoint a turn.
    let with = |on: &str| {
        refuse(&format!(
            r#"
            [[effect]]
            match = 'name:"Opt"'
            look = 1
            on = "{on}"

            [[criterion]]
            name = "anything"
            require = [{{ turn = 0, query = 'cat:"arm"', min = 1 }}]
            "#
        ))
    };
    let mana = with("cast");
    assert!(matches!(&mana, ErrorKind::BadTrigger { .. }), "{mana:?}");
    assert!(
        mana.to_string().contains("issues/57"),
        "should say what it waits on: {mana}"
    );
    let unknown = with("upkeep");
    assert!(
        unknown.to_string().contains("landdrop"),
        "should list what it takes: {unknown}"
    );
    assert!(
        unknown.to_string().contains("cast"),
        "and cast is now one of them: {unknown}"
    );
}

#[test]
fn a_tutor_declares_what_it_fetches_and_where_it_puts_it() {
    let criteria = parse(
        r#"
        [[effect]]
        match = 'name:"Trinket Mage"'
        on = "cast"
        fetch = ['name:"Lantern of Insight"', 'otag:tutor']
        to = "hand"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    let entry = &criteria.effects().entries()[0];
    // A fetch turns over nothing, so it examines nothing, so the schedule it
    // needs is the one that was already there.
    assert_eq!(entry.look, 0);
    let fetch = entry.fetch.as_ref().expect("declared");
    assert_eq!(fetch.prefer.len(), 2, "a priority, read in order");
    assert_eq!(fetch.to, gauntlet_criteria::Fetched::Hand);
}

#[test]
fn a_source_declares_how_much_mana_it_adds() {
    // ADR-0018: a non-land card is a mana source when the line casts it and an
    // effect matching it says how much it adds. It neither looks nor fetches,
    // so `adds` alone is enough of an effect to be one.
    let criteria = parse(
        r#"
        [[effect]]
        match = 'name:"Sol Ring"'
        on = "cast"
        adds = 2

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    let entry = &criteria.effects().entries()[0];
    assert_eq!(entry.adds, Some(2));
    assert_eq!(entry.look, 0, "a source turns over nothing");
    assert_eq!(entry.fetch, None);
    assert_eq!(entry.trigger, Trigger::Cast);
}

#[test]
fn a_source_that_adds_nothing_or_arrives_on_a_land_drop_is_refused() {
    let with = |on: &str, adds: i64| {
        refuse(&format!(
            r#"
            [[effect]]
            match = 'name:"Sol Ring"'
            on = "{on}"
            adds = {adds}

            [[criterion]]
            name = "anything"
            require = [{{ turn = 0, query = 'cat:"arm"', min = 1 }}]
            "#
        ))
    };
    // Fellwar Stone with no opponent adds nothing, and the way to say so is
    // to declare no `adds`, not `adds = 0`.
    for adds in [0, -1] {
        let bad = with("cast", adds);
        assert!(matches!(bad, ErrorKind::BadAdds { .. }), "{bad}");
    }
    // A source is a card the line cast; a land's mana is the land drop's.
    let bad = with("landdrop", 1);
    assert!(matches!(bad, ErrorKind::AddsOnLandDrop { .. }), "{bad}");
    assert!(bad.to_string().contains("on = \"cast\""), "{bad}");
}

#[test]
fn half_a_tutor_is_refused_either_way_round() {
    // A priority with nowhere to put what it finds and a destination with
    // nothing arriving at it are each half a declaration, and half a
    // declaration is where a default nobody stated gets invented.
    let nowhere = refuse(
        r#"
        [[effect]]
        match = 'name:"Trinket Mage"'
        on = "cast"
        fetch = ['name:"Lantern of Insight"']

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        matches!(&nowhere, ErrorKind::Missing { key: "to", .. }),
        "{nowhere:?}"
    );
    let nothing = refuse(
        r#"
        [[effect]]
        match = 't:land otag:surveil'
        look = 1
        on = "landdrop"
        to = "hand"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        matches!(&nothing, ErrorKind::ToWithoutFetch { .. }),
        "{nothing:?}"
    );
    // And it says what `to` is not, because beside a routing key it reads like
    // it means something it does not.
    assert!(
        nothing.to_string().contains("to_graveyard"),
        "should name the key that does route: {nothing}"
    );
}

#[test]
fn a_fetch_destination_this_engine_cannot_model_is_refused_by_name() {
    let bad = refuse(
        r#"
        [[effect]]
        match = 'name:"Entomb"'
        on = "cast"
        fetch = ['name:"Life from the Loam"']
        to = "graveyard"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        matches!(&bad, ErrorKind::BadFetchDestination { .. }),
        "{bad:?}"
    );
    assert!(
        bad.to_string().contains("hand, battlefield"),
        "should list what it takes: {bad}"
    );
}

#[test]
fn a_land_arriving_off_a_spell_is_refused_by_name() {
    // Rampant Growth. What a land put down by a spell taps for on the turn it
    // arrives is a fact about the spell, not about the land, and no tag this
    // index carries separates Rampant Growth from Nature's Lore.
    let bad = refuse(
        r#"
        [[effect]]
        match = 'name:"Rampant Growth"'
        on = "cast"
        fetch = ['t:land t:basic']
        to = "battlefield"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        matches!(&bad, ErrorKind::FetchOntoTheBattlefieldFromASpell { .. }),
        "{bad:?}"
    );
    assert!(
        bad.to_string().contains("fetchland"),
        "should name the shape that does work: {bad}"
    );
}

#[test]
fn an_effect_that_neither_looks_nor_fetches_is_refused() {
    // It would cost a checkpoint a turn to compute a value it cannot change.
    let nothing = refuse(
        r#"
        [[effect]]
        match = 't:land'
        on = "landdrop"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        matches!(&nothing, ErrorKind::Missing { key: "look", .. }),
        "{nothing:?}"
    );
    assert!(
        nothing.to_string().contains("fetch"),
        "should name the other thing an effect can do: {nothing}"
    );
}

#[test]
fn a_tutor_priority_that_repeats_itself_is_refused() {
    // The same rule the other three lists are under: an entry the earlier one
    // already took can never decide anything.
    let repeated = refuse(
        r#"
        [[effect]]
        match = 'name:"Trinket Mage"'
        on = "cast"
        fetch = ['name:"Lantern of Insight"', 'name:"Lantern of Insight"']
        to = "hand"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        matches!(&repeated, ErrorKind::RepeatedPreference { .. }),
        "{repeated:?}"
    );
}

#[test]
fn an_effect_that_examines_nothing_is_refused() {
    // `look = 0` is a model of a card doing nothing, which is not a model of
    // any card; and a look deep enough to redraw the library is a typo rather
    // than a question.
    let with = |look: i64| {
        refuse(&format!(
            r#"
            [[effect]]
            match = 't:land'
            look = {look}
            on = "landdrop"

            [[criterion]]
            name = "anything"
            require = [{{ turn = 0, query = 'cat:"arm"', min = 1 }}]
            "#
        ))
    };
    assert!(matches!(with(0), ErrorKind::BadLook { .. }));
    assert!(matches!(with(-1), ErrorKind::BadLook { .. }));
    assert!(matches!(with(400), ErrorKind::BadLook { .. }));
}

#[test]
fn an_effect_missing_a_key_says_which_one() {
    let without_match = refuse(
        r#"
        [[effect]]
        look = 1
        on = "landdrop"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        without_match.to_string().contains("`match`"),
        "{without_match}"
    );
    let without_look = refuse(
        r#"
        [[effect]]
        match = 't:land'
        on = "landdrop"

        [[criterion]]
        name = "anything"
        require = [{ turn = 0, query = 'cat:"arm"', min = 1 }]
        "#,
    );
    assert!(
        without_look.to_string().contains("`look`"),
        "{without_look}"
    );
}

#[test]
fn the_standard_library_is_a_criteria_file_like_any_other() {
    // Not special machinery: the same parser, the same tables, the same
    // validation. A brewer with a card nobody thought of writes the same thing
    // this file writes.
    let std = EffectLibrary::parse(STANDARD_LIBRARY, STANDARD_LIBRARY_ORIGIN)
        .expect("the shipped library must parse");
    assert!(!std.is_empty(), "a library with nothing in it is not one");
    for entry in std.entries() {
        // A look is only modelled on the land drop. What fires on a cast is a
        // source the line cast (ADR-0018): it declares what it adds and turns
        // over nothing.
        match entry.trigger {
            Trigger::LandDrop => assert_eq!(entry.adds, None, "{entry:?}"),
            Trigger::Cast => assert!(
                entry.adds.is_some() && entry.look == 0,
                "a cast entry here only declares a source: {entry:?}"
            ),
        }
        // And never a fetch: which card a tutor finds is the question asked.
        assert_eq!(entry.fetch, None, "{entry:?}");
        // The library says what a card looks at, never where the cards go. A
        // destination here would be this tool answering a question nobody asked
        // it, and it would be wrong for half the decks that play the card.
        assert_eq!(
            entry.to_graveyard, None,
            "the standard library must not route: {entry:?}"
        );
    }
}

#[test]
fn a_file_of_nothing_but_effects_asks_nothing_and_that_is_fine() {
    // The standard library asks no questions, and a file of pure effects is not
    // a file that forgot to. Read as a criteria file it is still refused, so the
    // leniency is in the entry point rather than in the format.
    let source = r#"
        [[effect]]
        match = 't:land'
        look = 1
        on = "landdrop"
    "#;
    assert!(EffectLibrary::parse(source, "effects.toml").is_ok());
    assert!(matches!(refuse(source), ErrorKind::AsksNothing));
}

#[test]
fn loading_order_is_the_whole_of_last_wins() {
    // The prelude model: the standard library first, the user's file after. The
    // resolution of an overlap is "the last one declared", so the order these
    // arrive in is the only thing that decides it.
    let first = EffectLibrary::parse(
        r#"
        [[effect]]
        match = 't:land'
        look = 1
        on = "landdrop"
        "#,
        "first.toml",
    )
    .unwrap();
    let second = EffectLibrary::parse(
        r#"
        [[effect]]
        match = 't:land'
        look = 2
        on = "landdrop"
        "#,
        "second.toml",
    )
    .unwrap();
    let combined = first.followed_by(second);
    assert_eq!(combined.entries().len(), 2, "both are kept, neither merges");
    assert_eq!(combined.entries()[0].origin, "first.toml");
    assert_eq!(combined.entries()[1].origin, "second.toml");
}

// --- The land-drop priority (#54) -----------------------------------------

const ONE_CLAUSE: &str = r#"
[[criterion]]
name = "a land by turn 1"
require = [{ turn = 1, query = "t:land", min = 1 }]
"#;

#[test]
fn a_land_drop_priority_is_read_in_the_order_it_was_written() {
    // Order is the whole content of the mechanism, so it is asserted rather
    // than assumed: a list read back sorted, deduplicated or reversed would be
    // a different policy answering under the same name.
    let criteria = parse(&format!(
        "[land_drop]\nprefer = ['otag:surveil', 't:land -otag:tapland']\n{ONE_CLAUSE}"
    ));
    assert_eq!(
        criteria.land_drop(),
        ["otag:surveil", "t:land -otag:tapland"]
    );
    // And a file that declares none says so by being empty rather than by
    // having a default filled in for it. Nobody has decided which land they
    // would play, which is a state this tool reports rather than resolves.
    assert!(parse(ONE_CLAUSE).land_drop().is_empty());
}

#[test]
fn a_priority_that_arbitrates_nothing_is_refused_by_name() {
    // Both of these would appear in the report as a declared policy while
    // settling no drop at all: the run would say a number came from a list that
    // never decided anything.
    assert!(matches!(
        refuse(&format!("[land_drop]\nprefer = []\n{ONE_CLAUSE}")),
        ErrorKind::NoPreference { .. }
    ));
    assert!(matches!(
        refuse(&format!("[land_drop]\n{ONE_CLAUSE}")),
        ErrorKind::NoPreference { .. }
    ));
    let repeated = refuse(&format!(
        "[land_drop]\nprefer = ['t:land', 'otag:surveil', 't:land']\n{ONE_CLAUSE}"
    ));
    assert!(
        matches!(
            &repeated,
            ErrorKind::RepeatedPreference {
                query,
                first,
                position,
                ..
            }
                if query == "t:land" && *first == 1 && *position == 3
        ),
        "{repeated}"
    );
}

/// One `[[effect]]` table with `extra` keys spliced in, and one criterion so the
/// file asks something.
fn saga_with(extra: &str) -> String {
    format!(
        r#"
        [[effect]]
        match = "name:\"Urza's Saga\""
        {extra}

        [[criterion]]
        name = "anything"
        require = [{{ turn = 0, query = 'cat:"arm"', min = 1 }}]
        "#
    )
}

#[test]
fn a_saga_is_a_land_drop_that_fetches_two_turns_later() {
    let criteria = Criteria::parse(
        &saga_with(
            r#"on = "landdrop"
            after = 2
            sacrifice = true
            fetch = ['name:"Lantern of Insight"']
            to = "battlefield""#,
        ),
        "saga.toml",
    )
    .unwrap();
    let entry = &criteria.effects().entries()[0];
    assert_eq!(
        entry.delay,
        Some(Delay {
            turns: 2,
            sacrifice: true
        })
    );
    assert_eq!(entry.look, 0);
    // And an effect that does not wait carries no delay at all, rather than a
    // delay of zero turns.
    let now = Criteria::parse(
        &saga_with(
            r#"on = "landdrop"
            fetch = ['t:land']
            to = "battlefield""#,
        ),
        "fetchland.toml",
    )
    .unwrap();
    assert_eq!(now.effects().entries()[0].delay, None);
}

#[test]
fn what_cannot_wait_is_refused_by_what_it_would_have_needed() {
    for (keys, why) in [
        // A delayed look turns over cards on a turn the schedule cannot know.
        (
            r#"on = "landdrop"
            after = 2
            look = 1
            to_graveyard = "*""#,
            "look",
        ),
        // A cast has nothing in play to wait with.
        (
            r#"on = "cast"
            after = 2
            fetch = ['name:"Lantern of Insight"']
            to = "hand""#,
            "cast",
        ),
    ] {
        let bad = refuse(&saga_with(keys));
        assert!(
            matches!(&bad, ErrorKind::UnmodelledDelay { .. }),
            "{keys}: {bad:?}"
        );
        assert!(bad.to_string().contains(why), "{keys}: {bad}");
    }
}

#[test]
fn a_wait_is_a_number_of_turns_and_a_sacrifice_needs_one() {
    for after in ["0", "-1", "101"] {
        let bad = refuse(&saga_with(&format!(
            r#"on = "landdrop"
            after = {after}
            fetch = ['name:"Lantern of Insight"']
            to = "battlefield""#
        )));
        assert!(
            matches!(&bad, ErrorKind::BadAfter { .. }),
            "{after}: {bad:?}"
        );
    }
    // A land sacrificed the moment it is played for another is a fetchland,
    // and a fetchland is already written without `sacrifice`.
    let bad = refuse(&saga_with(
        r#"on = "landdrop"
        sacrifice = true
        fetch = ['t:land']
        to = "battlefield""#,
    ));
    assert!(
        matches!(&bad, ErrorKind::SacrificeWithoutDelay { .. }),
        "{bad:?}"
    );
    assert!(bad.to_string().contains("fetchland"), "{bad}");
}

/// Issue #52: an error the TOML reader raises names the line it is about and
/// shows it. The criteria-level refusals always named the criterion; these
/// named nothing, and a criteria file runs to hundreds of lines.
#[test]
fn a_malformed_file_names_the_line_and_shows_it() {
    let message = |source: &str| {
        Criteria::parse(source, "test.criteria.toml")
            .expect_err("should refuse")
            .to_string()
    };
    // A syntax error, and the mistake that causes most of them: an apostrophe
    // in a card name ends a single-quoted string.
    let syntax = message(
        "[[criterion]]\nname = \"a\"\n\
         require = [{ turn = 1, query = 'name:\"Artificer's Intuition\"', min = 1 }]\n",
    );
    assert!(syntax.contains("line 3:"), "{syntax}");
    assert!(syntax.contains("3 | require"), "shows the line: {syntax}");
    assert!(syntax.contains("triple quotes"), "names the fix: {syntax}");

    let criteria = |second: &str| {
        format!(
            "[[criterion]]\nname = \"a\"\nrequire = [{{ turn = 1, query = \"t:land\", min = 1 }}]\n\n\
             [[criterion]]\nname = \"b\"\n{second}\n"
        )
    };
    // An unknown key in the second criterion, on line 7 — found by counting
    // headers, because the reader's own offset for it points elsewhere.
    let unknown = message(&criteria(
        "atLeast = 0.5\nrequire = [{ turn = 1, query = \"t:land\", min = 1 }]",
    ));
    assert!(
        unknown.contains("line 7: unknown field `atLeast`"),
        "{unknown}"
    );
    // One inside an inline clause, where no line starts with the key.
    let inline = message(&criteria(
        "require = [{ turn = 1, query = \"t:land\", zon = \"hand\", min = 1 }]",
    ));
    assert!(inline.contains("line 7: unknown field `zon`"), "{inline}");
    assert!(
        !inline.contains("triple quotes"),
        "no hint where it does not apply"
    );
    // And a value of the wrong type.
    let typed = message(&criteria(
        "at_least = \"high\"\nrequire = [{ turn = 1, query = \"t:land\", min = 1 }]",
    ));
    assert!(typed.contains("line 7:"), "{typed}");
}

// --- [mulligan] (#7) ---------------------------------------------------------

#[test]
fn a_mulligan_is_read_as_written() {
    let criteria = parse(&format!(
        r#"
        [mulligan]
        keep = [
          {{ query = "t:land", min = 2, max = 5 }},
          {{ query = 'name:"Life from the Loam"', min = 1 }},
          {{ query = "mv>=5", max = 2 }},
        ]
        bottom = ["t:land", "mv>=5"]
        down_to = 5
        {ONE_CLAUSE}"#
    ));
    let mulligan = criteria.mulligan().expect("declared");
    assert_eq!(mulligan.down_to, 5);
    assert_eq!(mulligan.bottom, vec!["t:land", "mv>=5"]);
    let keep: Vec<(&str, u32, Option<u32>)> = mulligan
        .keep
        .iter()
        .map(|k| (k.query.as_str(), k.min, k.max))
        .collect();
    assert_eq!(
        keep,
        vec![
            ("t:land", 2, Some(5)),
            ("name:\"Life from the Loam\"", 1, None),
            ("mv>=5", 0, Some(2)),
        ]
    );
    // And a file that declares none keeps every seven.
    assert!(parse(ONE_CLAUSE).mulligan().is_none());
}

#[test]
fn the_keep_rule_may_be_written_as_tables() {
    let criteria = parse(&format!(
        r#"
        [mulligan]
        bottom = ["t:land"]
        down_to = 6

        [[mulligan.keep]]
        query = "t:land"
        min = 2
        {ONE_CLAUSE}"#
    ));
    assert_eq!(criteria.mulligan().expect("declared").keep.len(), 1);
}

#[test]
fn every_part_of_a_mulligan_is_required() {
    // Each of these would be the tool choosing how the pilot plays: no rule
    // keeps every seven, no list puts back cards nobody chose, and no floor
    // lets a rule no hand passes mulligan into nothing.
    assert!(matches!(
        refuse(&format!(
            "[mulligan]\nbottom = ['t:land']\ndown_to = 5\n{ONE_CLAUSE}"
        )),
        ErrorKind::KeepsEverything
    ));
    let no_bottom = refuse(&format!(
        "[mulligan]\nkeep = [{{ query = 't:land', min = 2 }}]\ndown_to = 5\n{ONE_CLAUSE}"
    ));
    assert!(
        matches!(&no_bottom, ErrorKind::Missing { key: "bottom", .. }),
        "{no_bottom}"
    );
    let no_floor = refuse(&format!(
        "[mulligan]\nkeep = [{{ query = 't:land', min = 2 }}]\nbottom = ['t:land']\n{ONE_CLAUSE}"
    ));
    assert!(
        matches!(&no_floor, ErrorKind::Missing { key: "down_to", .. }),
        "{no_floor}"
    );
}

#[test]
fn a_mulligan_that_cannot_mean_anything_is_refused() {
    for down_to in [0, 7, 8, -1] {
        let refused = refuse(&format!(
            "[mulligan]\nkeep = [{{ query = 't:land', min = 2 }}]\nbottom = ['t:land']\n\
             down_to = {down_to}\n{ONE_CLAUSE}"
        ));
        assert!(
            matches!(refused, ErrorKind::BadDownTo { .. }),
            "down_to = {down_to}: {refused}"
        );
    }
    let unbounded = refuse(&format!(
        "[mulligan]\nkeep = [{{ query = 't:land' }}]\nbottom = ['t:land']\ndown_to = 5\n\
         {ONE_CLAUSE}"
    ));
    assert!(
        matches!(unbounded, ErrorKind::NoBounds { .. }),
        "{unbounded}"
    );
    let empty = refuse(&format!(
        "[mulligan]\nkeep = [{{ query = 't:land', min = 5, max = 2 }}]\nbottom = ['t:land']\n\
         down_to = 5\n{ONE_CLAUSE}"
    ));
    assert!(matches!(empty, ErrorKind::EmptyRange { .. }), "{empty}");
    let repeated = refuse(&format!(
        "[mulligan]\nkeep = [{{ query = 't:land', min = 2 }}]\nbottom = ['t:land', 't:land']\n\
         down_to = 5\n{ONE_CLAUSE}"
    ));
    assert!(
        matches!(
            &repeated,
            ErrorKind::RepeatedPreference { key: "bottom", .. }
        ),
        "{repeated}"
    );
    assert!(
        repeated.to_string().contains("`bottom` entry 2"),
        "{repeated}"
    );
}

#[test]
fn a_keep_clause_is_about_the_hand_and_says_no_turn() {
    // A keep decision is made about the opener, before the first turn, so a
    // `turn` or a `zone` in it would be a question it cannot ask — refused by
    // the schema rather than ignored.
    for extra in ["turn = 0", "zone = 'hand'"] {
        let refused = refuse(&format!(
            "[mulligan]\nkeep = [{{ query = 't:land', min = 2, {extra} }}]\n\
             bottom = ['t:land']\ndown_to = 5\n{ONE_CLAUSE}"
        ));
        assert!(
            matches!(refused, ErrorKind::Malformed(_)),
            "{extra}: {refused}"
        );
    }
}

// --- optimise (#63) ------------------------------------------------------------

const TWO_CRITERIA: &str = r#"
[[criterion]]
name = "a land by turn 1"
require = [{ turn = 1, query = "t:land", min = 1 }]

[[criterion]]
name = "two lands by turn 2"
require = [{ turn = 2, query = "t:land", min = 2 }]

[[expect]]
name = "lands by turn 2"
turn = 2
query = "t:land"
"#;

#[test]
fn an_objective_is_read_in_the_order_the_file_declares_its_criteria() {
    let criteria = parse(&format!(
        "[mulligan]\noptimise = {{ \"two lands by turn 2\" = 3, \"a land by turn 1\" = 1.5 }}\n\
         down_to = 5\n{TWO_CRITERIA}"
    ));
    let mulligan = criteria.mulligan().expect("declared");
    assert!(
        !mulligan.declares_a_rule(),
        "an objective alone is not a rule"
    );
    let objective: Vec<(usize, &str, f64)> = mulligan
        .optimise
        .iter()
        .map(|w| (w.criterion, w.name.as_str(), w.weight))
        .collect();
    assert_eq!(
        objective,
        vec![
            (0, "a land by turn 1", 1.5),
            (1, "two lands by turn 2", 3.0)
        ]
    );
    assert!(mulligan.keep.is_empty() && mulligan.bottom.is_empty());
}

#[test]
fn an_objective_beside_a_declared_rule_is_both() {
    let criteria = parse(&format!(
        "[mulligan]\nkeep = [{{ query = 't:land', min = 2 }}]\nbottom = ['t:land']\n\
         optimise = {{ \"a land by turn 1\" = 1 }}\ndown_to = 5\n{TWO_CRITERIA}"
    ));
    let mulligan = criteria.mulligan().expect("declared");
    assert!(mulligan.declares_a_rule());
    assert_eq!(mulligan.optimise.len(), 1);
}

#[test]
fn an_objective_that_cannot_mean_anything_is_refused() {
    let refused = |optimise: &str| {
        refuse(&format!(
            "[mulligan]\noptimise = {optimise}\ndown_to = 5\n{TWO_CRITERIA}"
        ))
    };
    assert!(matches!(refused("{}"), ErrorKind::EmptyObjective));
    let unknown = refused("{ \"three lands\" = 1 }");
    assert!(
        matches!(&unknown, ErrorKind::UnknownObjective { name, .. } if name == "three lands"),
        "{unknown}"
    );
    assert!(
        unknown.to_string().contains("\"two lands by turn 2\""),
        "it lists what it would have taken: {unknown}"
    );
    let expectation = refused("{ \"lands by turn 2\" = 1 }");
    assert!(
        expectation.to_string().contains("is an [[expect]]"),
        "{expectation}"
    );
    for weight in ["0", "-1", "nan"] {
        let bad = refused(&format!("{{ \"a land by turn 1\" = {weight} }}"));
        assert!(
            matches!(bad, ErrorKind::BadWeight { .. }),
            "{weight}: {bad}"
        );
    }
    // Without a keep rule, a bottoming list has nothing to put back from.
    let orphan = refuse(&format!(
        "[mulligan]\nbottom = ['t:land']\noptimise = {{ \"a land by turn 1\" = 1 }}\n\
         down_to = 5\n{TWO_CRITERIA}"
    ));
    assert!(matches!(orphan, ErrorKind::BottomWithoutKeep), "{orphan}");
    // And a floor is still required: the induction needs a last depth.
    let floorless = refuse(&format!(
        "[mulligan]\noptimise = {{ \"a land by turn 1\" = 1 }}\n{TWO_CRITERIA}"
    ));
    assert!(
        matches!(&floorless, ErrorKind::Missing { key: "down_to", .. }),
        "{floorless}"
    );
}

#[test]
fn a_weight_on_a_name_two_criteria_share_is_refused() {
    let doubled = refuse(
        r#"
        [mulligan]
        optimise = { "twin" = 1 }
        down_to = 5

        [[criterion]]
        name = "twin"
        require = [{ turn = 1, query = "t:land", min = 1 }]

        [[criterion]]
        name = "twin"
        require = [{ turn = 2, query = "t:land", min = 2 }]
        "#,
    );
    assert!(
        doubled.to_string().contains("names two criteria"),
        "{doubled}"
    );
}

// --- Threads ---------------------------------------------------------------

#[test]
fn how_many_threads_walked_it_never_reaches_a_digit() {
    // A criteria file forks, so its continuations are walked on as many
    // threads as there are. Each is one thread's whole walk and nothing is
    // summed across threads, so one thread and eight have to agree bit for
    // bit, not closely.
    use gauntlet_criteria::{Answering, Conditionals, Table};
    let criteria = parse(
        r#"
        [[criterion]]
        name = "two by turn 3"
        require = [{ turn = 3, query = "t:land", min = 2 }]

        [[criterion]]
        name = "one on 1 and three on 4"
        require = [
          { turn = 1, query = "t:land", min = 1 },
          { turn = 4, query = "t:land", min = 3 },
        ]

        [[expect]]
        name = "lands by turn 4"
        turn = 4
        query = "t:land"
        "#,
    );
    let grouping = grouping_for(&criteria, 20);
    let schedule = schedule(&criteria);
    let answering = Answering::all(criteria.plan());
    let table = |threads: usize| {
        let mut ev = criteria.clone();
        let mut conditionals =
            Conditionals::new(&grouping, &schedule, &answering, &mut ev, Table::default())
                .unwrap()
                .with_threads(threads);
        conditionals.fill(2).unwrap();
        conditionals.into_table()
    };
    let (one, eight) = (table(1), table(8));
    assert_eq!(one.len(), eight.len());
    assert!(one.len() > 20, "enough pairs to share out: {}", one.len());
    let mut openers = Vec::new();
    chip_stats::for_each_composition(grouping.group_sizes(), 7, |h, _| openers.push(h.to_vec()));
    for first in &openers {
        for depth in 0..=2 {
            let mut backs = Vec::new();
            chip_stats::for_each_composition(first, depth, |b, _| backs.push(b.to_vec()));
            for back in &backs {
                let (a, b) = (
                    one.get(first, back).unwrap(),
                    eight.get(first, back).unwrap(),
                );
                let bits = |c: &gauntlet_criteria::Continuation| -> Vec<u64> {
                    c.held
                        .iter()
                        .chain(c.counted.iter().flatten())
                        .map(|x| x.to_bits())
                        .collect()
                };
                assert_eq!(bits(a), bits(b), "{first:?} back {back:?}");
            }
        }
    }
}

#[test]
fn a_range_of_one_value_is_exactly_that_many() {
    // `min = max` is a question, not an empty range: exactly two.
    let out = run_exact(
        r#"
        [[criterion]]
        name = "exactly two"
        require = [{ turn = 0, query = 'cat:"arm"', min = 2, max = 2 }]
        "#,
        20,
    );
    let want = chip_stats::pmf(99, 20, 7, 2) * 100.0;
    assert!(
        (percent(&out, 0) - want).abs() < 1e-9,
        "{} vs {want}",
        percent(&out, 0)
    );
}

#[test]
fn a_clause_asking_two_questions_is_refused_rather_than_answering_one() {
    let err = refuse(
        r#"
        [[criterion]]
        name = "cast and count"
        require = [{ turn = 3, query = "t:land", min = 1, cast = 'name:"Opt"' }]
        "#,
    );
    assert!(matches!(err, ErrorKind::TwoQuestions { .. }), "{err}");
}

#[test]
fn a_file_says_what_it_casts_and_which_questions_count_castings() {
    let criteria = parse(
        r#"
        [casting]
        prefer = ['name:"Opt"', 't:artifact']

        [[criterion]]
        name = "a land by turn 1"
        require = [{ turn = 1, query = "t:land", min = 1 }]

        [[expect]]
        name = "Opts cast by turn 2"
        turn = 2
        cast = 'name:"Opt"'
        "#,
    );
    assert_eq!(criteria.casting(), ["name:\"Opt\"", "t:artifact"]);
    assert_eq!(criteria.expectations().len(), 1);
    assert_eq!(criteria.expectations()[0].name, "Opts cast by turn 2");
    // Only the expectation counts castings, so it is the one named — which is
    // what a file with no `[casting]` would be refused against.
    assert_eq!(criteria.counts_castings(), Some("Opts cast by turn 2"));
    assert_eq!(criteria.casts(), Some("Opts cast by turn 2"));

    let silent = parse(
        r#"
        [[criterion]]
        name = "a land by turn 1"
        require = [{ turn = 1, query = "t:land", min = 1 }]
        "#,
    );
    assert!(silent.casting().is_empty());
    assert_eq!(silent.counts_castings(), None);
}

#[test]
fn battlefield_queries_come_from_expectations_too_once_each() {
    let criteria = parse(
        r#"
        [[criterion]]
        name = "lands in play"
        require = [{ turn = 3, query = "t:land", zone = "battlefield", min = 2 }]

        [[criterion]]
        name = "lands and artifacts in play"
        require = [
          { turn = 4, query = "t:land", zone = "battlefield", min = 3 },
          { turn = 4, query = "t:artifact", zone = "battlefield", min = 1 },
        ]

        [[expect]]
        name = "lands in play, counted"
        turn = 3
        query = "t:land"
        zone = "battlefield"

        [[expect]]
        name = "creatures in play"
        turn = 3
        query = "t:creature"
        zone = "battlefield"

        [[expect]]
        name = "artifacts in hand"
        turn = 3
        query = "t:artifact"
        "#,
    );
    assert_eq!(
        criteria.battlefield_queries(),
        [
            ("t:land", "lands in play"),
            ("t:artifact", "lands and artifacts in play"),
            ("t:creature", "creatures in play"),
        ]
    );
}

#[test]
fn what_each_question_reads_is_what_it_names() {
    // The reads size the enumeration each class of question runs on, so a
    // question that reads less than it asks is answered on a grouping too
    // coarse to tell its cards apart.
    let criteria = parse(
        r#"
        [[criterion]]
        name = "a land, then a creature in play"
        require = [
          { turn = 2, query = "t:land", min = 1 },
          { turn = 4, query = "t:creature", zone = "battlefield", min = 1 },
        ]

        [[criterion]]
        name = "castable on curve"
        require = [
          { turn = 3, can_cast = "{1}{U}" },
          { turn = 5, can_cast = "{B}" },
        ]

        [[expect]]
        name = "creatures seen"
        turn = 1
        query = "t:creature"
        "#,
    );
    assert_eq!(criteria.queries(), ["t:land", "t:creature"]);
    let reads = criteria.reads();

    let counting = &reads.criteria[0];
    assert_eq!(counting.queries(), 0b11);
    assert_eq!(counting.turns(), [2, 4]);
    assert!(counting.battlefield());
    assert_eq!(counting.demands(), None);

    let casting = &reads.criteria[1];
    assert_eq!(casting.queries(), 0);
    assert_eq!(casting.turns(), [3, 5]);
    assert!(!casting.battlefield());
    assert_eq!(
        casting.demands(),
        Some(gauntlet_criteria::Palette::from_letters(["UB"]))
    );

    let seen = &reads.expectations[0];
    assert_eq!(seen.queries(), 0b10);
    assert_eq!(seen.turns(), [1]);
    assert!(!seen.battlefield());
}
