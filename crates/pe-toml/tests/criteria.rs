//! The criteria file as the two engines see it.
//!
//! The engine itself is tested with Rust closures in pe-criteria and the
//! sampler against closed-form hypergeometrics in pe-sim. What is under test
//! here is the layer between a file on disk and those engines: that a clause
//! reads the count it names at the turn it names, that the two TOML spellings
//! of the same document are the same document, that a file asking something
//! unanswerable refuses instead, and that the exact and sampled engines running
//! the same file still agree.

use pe_criteria::{Grouping, Outcomes, RunError, Zone, ZoneError};
use pe_toml::{Criteria, ErrorKind, MAX_TURN};

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
fn gaps(criteria: &Criteria) -> Vec<u32> {
    let mut gaps = vec![7];
    gaps.extend(std::iter::repeat_n(1, criteria.horizon() as usize));
    gaps
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
    let gaps = gaps(&criteria);
    let plan = criteria.plan();
    pe_criteria::run(&grouping, &gaps, plan, &mut criteria).expect("should run")
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
    let want = (1.0 - pe_stats::pmf(99, 6, 7, 0)) * 100.0;
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
    let gaps = gaps(&criteria);
    let plan = criteria.plan();
    let err = pe_criteria::run(&grouping, &gaps, plan, &mut criteria).expect_err("too wide");
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
    // Zones are in here deliberately. `pe_sim` is generic over `Evaluator` and
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
    let gaps = gaps(&criteria);
    let plan = criteria.plan();

    let exact = pe_criteria::run(&grouping, &gaps, plan, &mut criteria).expect("exact");
    let trials = 200_000;
    let sampled =
        pe_sim::simulate(&grouping, &gaps, trials, 7, plan, &mut criteria).expect("sampled");

    for (i, want) in exact.probabilities.iter().enumerate() {
        let got = sampled.proportions[i];
        let se = pe_sim::standard_error(got, trials);
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
        let se = pe_sim::mean_standard_error(got, trials);
        assert!(
            (got.mean() - want.mean()).abs() <= 5.0 * se,
            "expectation {i}: sampled mean {} vs exact {}",
            got.mean(),
            want.mean()
        );
    }
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
fn the_battlefield_is_refused_by_name_rather_than_approximated() {
    // The tempting approximation is "held it, so it is in play", which is wrong
    // in the direction that flatters the deck. Refusing is the feature.
    let err = refuse(
        r#"
        [[criterion]]
        name = "lantern in play by turn 5"
        require = [{ turn = 5, query = 'name:"Lantern of Insight"', zone = "battlefield", min = 1 }]
        "#,
    );
    assert!(
        matches!(
            err,
            ErrorKind::BadZone {
                zone: ZoneError::Battlefield,
                ..
            }
        ),
        "{err}"
    );
    let msg = err.to_string();
    assert!(msg.contains("battlefield"), "names the zone: {msg}");
    assert!(
        msg.contains("cast"),
        "says what it would need to know: {msg}"
    );
    assert!(msg.contains("issues/10"), "points at the mana model: {msg}");
    assert!(
        msg.contains("clause 1"),
        "names the clause that wrote it: {msg}"
    );
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
    for accepted in ["hand", "graveyard", "library"] {
        assert!(msg.contains(accepted), "lists {accepted}: {msg}");
    }
    assert!(
        !msg.contains("battlefield"),
        "must not offer a zone it also refuses: {msg}"
    );
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
    assert!(err.to_string().contains("nothing at all"));

    // And the same for a `require` that is written out but empty.
    assert!(matches!(
        refuse("[[criterion]]\nname = \"x\"\nrequire = []\n"),
        ErrorKind::NoClauses { .. }
    ));
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
