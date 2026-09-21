//! End-to-end tests driving the real binary against a fixture deck.
//!
//! The fixture index is checked in so this needs no network and no card cache:
//! CI must be able to prove the numbers without a sync having run.
//!
//! The deck is deliberately ordinary — 36 lands, 10 one-mana accelerants, a
//! three-mana commander — because if the tool cannot get an obvious deck right,
//! nothing it says about a complicated one is worth reading.

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The card filed under `key`, still unparsed — the index is one card per line,
/// each behind the key it is filed under.
fn entry<'a>(body: &'a str, key: &str) -> &'a str {
    body.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('\t'))
        .unwrap_or_else(|| panic!("no entry for {key:?}"))
}

fn run(criteria: &str) -> std::process::Output {
    run_deck("simple-ramp.txt", criteria)
}

fn run_deck(deck: &str, criteria: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture(deck))
        .arg(fixture(criteria))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .output()
        .expect("binary should run")
}

fn percent(json: &serde_json::Value, name: &str) -> f64 {
    json["criteria"]
        .as_array()
        .expect("criteria array")
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no criterion named {name}"))["percent"]
        .as_f64()
        .expect("percent is a number")
}

#[test]
fn the_ramp_deck_reports_known_numbers() {
    let out = run("simple-ramp.criteria.toml");
    assert!(
        out.status.success(),
        "should exit 0 when all assertions pass"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout should be JSON");

    // The library is the deck minus the commander: 99, not 100. This is the
    // classic off-by-one, so it gets asserted.
    assert_eq!(json["library_size"], 99);
    assert_eq!(json["commanders"][0], "Marwyn, the Nurturer");
    assert_eq!(json["ok"], true);
    assert_eq!(json["failed"], 0);

    // Verified independently against a closed-form hypergeometric.
    assert!((percent(&json, "keepable opener (2-5 lands)") - 78.97).abs() < 0.01);
    assert!((percent(&json, "turn-1 accelerant") - 51.04).abs() < 0.01);
    assert!((percent(&json, "commander on turn 2") - 44.29).abs() < 0.01);
}

#[test]
fn requiring_a_second_land_by_turn_two_is_strictly_harder() {
    // The nested-prefix check. "commander on turn 2" is "turn-1 accelerant"
    // plus one more requirement, so it cannot come out higher.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(percent(&json, "commander on turn 2") < percent(&json, "turn-1 accelerant"));
}

#[test]
fn three_routes_to_one_outcome_report_their_union() {
    // #49, end to end. The three routes in this fixture overlap badly enough
    // that adding them comes to 145%, which is the number a reader would have
    // had to compute by hand from three separate runs before a criterion could
    // say "or". The engine answers the union in the same walk.
    let out = run("any-of.criteria.toml");
    assert!(out.status.success(), "should exit 0: {:?}", out.status);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    let union = percent(&json, "a route to four mana by turn 3");
    let routes = [
        percent(&json, "route: four lands"),
        percent(&json, "route: an accelerant and three lands"),
        percent(&json, "route: two lands and a ramp spell"),
    ];
    let sum: f64 = routes.iter().sum();
    assert!(sum > 100.0, "the routes overlap enough to matter: {sum}");
    assert!(union <= 100.0, "a probability, not a total: {union}");
    assert!(union < sum, "{union} vs the sum {sum}");
    let widest = routes.iter().cloned().fold(f64::MIN, f64::max);
    assert!(union > widest, "{union} vs the widest route {widest}");
    assert!((union - 82.93).abs() < 0.01, "{union}");

    // `require` and `any_of` on one criterion is the conjunction of the two, so
    // adding a precondition to the same three routes can only narrow them.
    let gated = percent(&json, "a keepable opener with a route to four mana");
    assert!(gated < union, "{gated} vs {union}");

    // #47's question, answered in the report rather than in a comment: a
    // disjunction adds no query beyond the union of what its branches name, so
    // the grouping the engine enumerates over is the same width it would be
    // without one.
    let queries: Vec<&str> = json["queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["query"].as_str().unwrap())
        .collect();
    assert_eq!(
        queries,
        ["t:land", "cat:\"Ramp - One Mana\"", "cat:\"Ramp\""]
    );
}

#[test]
fn a_disjunction_the_two_engines_have_to_agree_about() {
    // The third level the engines are held to each other at, now with a
    // criterion whose answer is a union. A sampler that ignored `any_of` would
    // land on one branch or on nothing, and either is far more than five
    // standard errors from 82.93%.
    let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture("any-of.criteria.toml"))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .arg("--simulate")
        .arg("--trials")
        .arg("200000")
        .arg("--seed")
        .arg("7")
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let exact: serde_json::Value =
        serde_json::from_slice(&run("any-of.criteria.toml").stdout).unwrap();
    for name in [
        "a route to four mana by turn 3",
        "a keepable opener with a route to four mana",
    ] {
        let (got, want) = (percent(&sampled, name), percent(&exact, name));
        // 200k trials puts one standard error near 0.1 percentage points.
        assert!((got - want).abs() < 0.5, "{name}: sampled {got} vs {want}");
    }
}

#[test]
fn the_verdict_goes_to_stderr() {
    // stdout is JSON only, so a caller piping it through jq cannot lose the
    // verdict. That has burned this project's predecessor.
    let out = run("simple-ramp.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("PASS: 3 of 3"), "stderr was: {stderr}");
    serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("stdout is pure JSON");
}

#[test]
fn a_missed_threshold_fails_the_run() {
    let out = run("impossible.criteria.toml");
    assert!(
        !out.status.success(),
        "should exit non-zero when an assertion misses"
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert_eq!(json["failed"], 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("FAIL"), "stderr was: {stderr}");
}

#[test]
fn an_unparseable_query_names_itself_and_the_question_that_asked_for_it() {
    let out = run("badquery.criteria.toml");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    // A query that cannot be parsed must say so, never quietly match nothing.
    assert!(stderr.contains("artist"), "stderr was: {stderr}");
    // And which criterion wanted it, which is only knowable because the file
    // hands over its whole query set before anything is grouped.
    assert!(stderr.contains("unsupported"), "stderr was: {stderr}");
}

#[test]
fn a_query_matching_no_cards_is_called_out() {
    // The defining failure mode: a misspelled category parses fine, matches
    // nothing, and yields a confident 0%. It cannot be a parse error, so it has
    // to be visible in the output instead.
    let out = run("typo.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("matched no cards"),
        "should warn about the empty query, stderr was: {stderr}"
    );
    assert!(stderr.contains("Rmap"), "should name it: {stderr}");

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let q = json["queries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["query"].as_str().unwrap().contains("Rmap"))
        .expect("query should be reported");
    assert_eq!(q["cards"], 0);
}

#[test]
fn a_zone_nothing_routes_a_card_into_is_called_out() {
    // The same failure as the empty query, arriving by a different door and
    // harder to spot: the query matches 36 lands, the criterion is well formed,
    // and the answer is still 0.00% for a reason that has nothing to do with
    // the deck. No effect this run loaded routes a card to the graveyard, so
    // every count in it is zero by construction, and a percentage cannot tell
    // the reader that. The pair of this test is
    // `routing_a_card_to_the_graveyard_switches_the_warning_off`, which is the
    // same fixture deck with an effect that does.
    let out = run("graveyard.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("nothing routes a card to the graveyard"),
        "should warn about the empty graveyard, stderr was: {stderr}"
    );
    assert!(
        stderr.contains("graveyard"),
        "should name the zone: {stderr}"
    );
    assert!(
        stderr.contains("a land in the yard by turn 5"),
        "should name the question that asked: {stderr}"
    );
    assert!(
        stderr.contains("to_graveyard"),
        "should say what would route a card there: {stderr}"
    );
    // The number is still reported. Refusing to print it would hide that the
    // question was asked at all, and the warning is what stops it reading as a
    // measurement.
    assert!(
        stderr.contains("0.00%"),
        "the answer is still an answer: {stderr}"
    );

    // And the same fact machine-readably, alongside the query breakdown it is
    // the sibling of. The query it names matched real cards, so the reader
    // cannot mistake this for the other kind of zero.
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let zones = json["zones"].as_array().expect("zones array");
    let yard = zones
        .iter()
        .find(|z| z["zone"] == "graveyard")
        .expect("the graveyard should be reported");
    assert_eq!(yard["reachable"], false);
    assert_eq!(yard["asked_by"], "a land in the yard by turn 5");
    assert_eq!(json["queries"][0]["cards"], 36);
}

#[test]
fn a_file_that_names_no_zone_is_told_nothing_about_zones() {
    // Discovered from the file: a file that never says `graveyard` never hears
    // about one, and the hand it meant without saying so is reachable and so
    // silent. A warning on every run is a warning nobody reads.
    let out = run("simple-ramp.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("routes a card to the"),
        "no zone note is owed here: {stderr}"
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let zones = json["zones"].as_array().expect("zones array");
    assert_eq!(zones.len(), 1, "one zone, the implied hand: {zones:?}");
    assert_eq!(zones[0]["zone"], "hand");
    assert_eq!(zones[0]["reachable"], true);
}

#[test]
fn the_query_breakdown_reports_real_match_counts() {
    // 36 Forests and 10 one-mana accelerants, which is checkable by eye against
    // the fixture decklist.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let find = |needle: &str| -> u64 {
        json["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["query"].as_str().unwrap().contains(needle))
            .unwrap_or_else(|| panic!("no query containing {needle}"))["cards"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(find("t:land"), 36);
    assert_eq!(find("Ramp - One Mana"), 10);
}

#[test]
fn sampling_agrees_with_the_exact_engine_end_to_end() {
    // The exact engine is the oracle for the sampled one. If these drift apart,
    // one of them is wrong and the tool cannot say which.
    let exact = run("simple-ramp.criteria.toml");
    let exact: serde_json::Value = serde_json::from_slice(&exact.stdout).unwrap();

    let sampled = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture("simple-ramp.criteria.toml"))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .args(["--simulate", "--trials", "50000", "--seed", "1"])
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&sampled.stdout).unwrap();

    assert_eq!(exact["method"], "exact");
    assert_eq!(sampled["method"], "sampled");
    assert_eq!(sampled["trials"], 50000);

    for c in sampled["criteria"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let got = c["percent"].as_f64().unwrap();
        let se = c["standard_error"].as_f64().unwrap() * 100.0;
        let want = percent(&exact, name);
        assert!(
            (got - want).abs() < 4.0 * se,
            "{name}: sampled {got} vs exact {want}, {:.2} SE away",
            (got - want).abs() / se
        );
    }
}

#[test]
fn both_engines_read_the_same_zones() {
    // The outermost of the three levels, for zones specifically. `pe_sim` is
    // generic over `Evaluator` and never names a zone, which is either why it
    // cannot disagree with the exact engine or why it would ignore zones
    // silently — and the two look identical until a clause's answer actually
    // moves with the zone it names.
    let go = |args: &[&str]| -> serde_json::Value {
        let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .arg("test")
            .arg(fixture("simple-ramp.txt"))
            .arg(fixture("zones.criteria.toml"))
            .arg("--index")
            .arg(fixture("index.jsonl"))
            .args(args)
            .output()
            .expect("binary should run");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        // Both engines owe the reader the same warning about the same zone.
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("nothing routes a card to the graveyard"),
            "the note is about the file, not about which engine read it"
        );
        serde_json::from_slice(&out.stdout).expect("stdout is JSON")
    };
    let exact = go(&[]);
    let sampled = go(&["--simulate", "--trials", "50000", "--seed", "3"]);

    for c in sampled["criteria"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let got = c["percent"].as_f64().unwrap();
        let se = c["standard_error"].as_f64().unwrap() * 100.0;
        let want = percent(&exact, name);
        // Not-greater-than: the graveyard clause is zero in both engines and so
        // has a standard error of exactly zero, which is agreement rather than
        // a failure to agree.
        assert!(
            (got - want).abs() <= 4.0 * se,
            "{name}: sampled {got} vs exact {want}"
        );
    }

    // The hand clause and the library clause are complements of one another
    // across the same 36 lands, so if the subtraction read the wrong total the
    // two would not sum to 100 in either engine.
    for json in [&exact, &sampled] {
        let hand = percent(json, "two lands in hand by turn 3");
        let library = percent(json, "at most three lands drawn by turn 3");
        assert!(hand > 0.0 && hand < 100.0, "not vacuous: {hand}");
        assert!(library > 0.0 && library < 100.0, "not vacuous: {library}");
        assert_eq!(percent(json, "a land in the yard by turn 3"), 0.0);
    }

    // And the mean of what is left in the deck, which is the same 36 lands seen
    // from the other side: 36 minus whatever turn 3 has drawn.
    let mean = |json: &serde_json::Value| -> f64 {
        json["expectations"][0]["mean"].as_f64().expect("a mean")
    };
    assert!((mean(&exact) - mean(&sampled)).abs() < 0.1);
    assert!(
        (mean(&exact) - (36.0 - 9.0 * 36.0 / 99.0)).abs() < 1e-3,
        "exact mean was {}",
        mean(&exact)
    );
}

#[test]
fn a_sampled_run_is_reproducible() {
    let go = |seed: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .arg("test")
            .arg(fixture("simple-ramp.txt"))
            .arg(fixture("simple-ramp.criteria.toml"))
            .arg("--index")
            .arg(fixture("index.jsonl"))
            .args(["--simulate", "--trials", "5000", "--seed", seed])
            .output()
            .expect("binary should run");
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        percent(&json, "turn-1 accelerant")
    };
    assert_eq!(go("7"), go("7"), "same seed must deal the same hands");
    assert_ne!(go("7"), go("8"), "different seeds should differ");
}

#[test]
fn only_sampled_runs_carry_error_bars() {
    // An exact answer has no standard error, and claiming one would be a lie.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for c in json["criteria"].as_array().unwrap() {
        assert!(
            c.get("standard_error").is_none(),
            "exact runs must not report SE"
        );
    }
    assert!(json.get("trials").is_none());
    // No seed dealt this either, and reporting one would imply a run that could
    // have come out differently.
    assert!(json.get("seed").is_none());
}

#[test]
fn both_spellings_of_a_criteria_file_answer_the_same() {
    // TOML says an inline table and an expanded [[criterion.require]] table are
    // the same document. A generator will write one and a person will write the
    // other, and the day those two answer differently is the day a saved file
    // loaded back into a builder quietly changes a deck's numbers.
    let inline = run("two-lands.criteria.toml");
    let expanded = run("two-lands-tables.criteria.toml");
    assert!(inline.status.success(), "the inline form should pass");
    assert!(expanded.status.success(), "the expanded form should pass");

    let inline: serde_json::Value = serde_json::from_slice(&inline.stdout).unwrap();
    let expanded: serde_json::Value = serde_json::from_slice(&expanded.stdout).unwrap();

    let got = percent(&inline, "two lands by turn 2");
    assert!(got > 1.0, "collapsed to {got}%");
    assert_eq!(got, percent(&expanded, "two lands by turn 2"));
    // Not only the headline: the whole answer, queries and all.
    assert_eq!(inline["criteria"], expanded["criteria"]);
    assert_eq!(inline["queries"], expanded["queries"]);
}

#[test]
fn an_answer_does_not_depend_on_an_unrelated_criterion() {
    // The run horizon is the deepest turn any question in the file names, so an
    // informational criterion reaching turn 5 stretches it for everyone. The
    // original symptom was that deleting such a neighbour silently changed
    // another criterion's answer.
    let alone = run("two-lands.criteria.toml");
    let alone: serde_json::Value = serde_json::from_slice(&alone.stdout).unwrap();
    let together = run("two-lands-plus-deep.criteria.toml");
    let together: serde_json::Value = serde_json::from_slice(&together.stdout).unwrap();

    assert_eq!(
        percent(&alone, "two lands by turn 2"),
        percent(&together, "two lands by turn 2"),
        "a criterion's answer must not depend on its neighbours"
    );
}

#[test]
fn an_empty_library_is_refused_rather_than_hanging() {
    // Every line is a commander or outside the deck. This used to spin forever
    // in release and panic on an underflow in debug.
    let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("no-library.txt"))
        .arg(fixture("two-lands.criteria.toml"))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .output()
        .expect("binary should run");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("library is empty"), "stderr was: {stderr}");
}

#[test]
fn asking_for_help_succeeds_and_goes_to_stdout() {
    for args in [vec!["--help"], vec!["test", "--help"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .args(&args)
            .output()
            .expect("binary should run");
        assert!(out.status.success(), "{args:?} should exit 0");
        assert!(
            !out.stdout.is_empty(),
            "{args:?} should print help to stdout"
        );
        assert!(out.stderr.is_empty(), "{args:?} should leave stderr clean");
    }
}

#[test]
fn a_usage_error_fails_and_goes_to_stderr() {
    // figue renders a missing argument as a help request on stdout with exit 0.
    // Both halves of that would break callers here: other tools shell out to
    // this binary and read the exit code, and stdout carries JSON they pipe
    // through jq.
    for args in [
        vec![],
        vec!["test"],
        vec!["bogus"],
        vec!["test", "a", "b", "--nope"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .args(&args)
            .output()
            .expect("binary should run");
        assert!(!out.status.success(), "{args:?} should exit non-zero");
        assert!(
            out.stdout.is_empty(),
            "{args:?} must not put usage text on stdout"
        );
        assert!(
            !out.stderr.is_empty(),
            "{args:?} should explain itself on stderr"
        );
    }
}

#[test]
fn cards_that_live_outside_the_library_are_not_counted_in_it() {
    // Stickers, attractions, planes and the rest are shuffled into a deck of
    // their own or into none at all. Counting them inflates library_size and so
    // moves every probability in the report.
    let out = run_deck("outside-the-library.txt", "two-lands.criteria.toml");
    assert!(out.status.success());

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    // 40 Forests, two planeswalkers, and the two real cards that apply
    // stickers. Eleven cards across nine listings left.
    assert_eq!(json["library_size"], 44);

    let excluded = json["excluded"].as_array().expect("excluded array");
    assert_eq!(excluded.len(), 9);
    let by_name = |name: &str| {
        excluded
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("{name} should be reported as excluded"))
    };
    for (name, card_type) in [
        ("Ancestral Hot Dog Minotaur", "Stickers"),
        ("Bumper Cars", "Attraction"),
        ("Academy at Tolaria West", "Plane"),
        ("Chaotic Aether", "Phenomenon"),
        ("All in Good Time", "Scheme"),
        ("Akroma, Angel of Wrath Avatar", "Vanguard"),
        ("Backup Plan", "Conspiracy"),
        ("Tomb of Annihilation", "Dungeon"),
        ("Ajani Steadfast Emblem", "Emblem"),
    ] {
        assert_eq!(by_name(name)["card_type"], card_type);
    }
    assert_eq!(by_name("Bumper Cars")["qty"], 3, "quantities are kept");
}

#[test]
fn a_planeswalker_is_not_a_plane_and_a_sticker_payoff_is_not_a_sticker() {
    // Both halves of the trap this fix had to avoid. "Plane" is a substring of
    // every planeswalker's type line, and "Sticker Package" is the category the
    // real cards that apply stickers live in -- matching it once reported five
    // of them as companions.
    let out = run_deck("outside-the-library.txt", "two-lands.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    for name in [
        "Vivien Reid",
        "Nissa, Who Shakes the World",
        "Park Bleater",
        "Ticketomaton",
    ] {
        assert!(
            !json["excluded"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["name"] == name),
            "{name} belongs in the library"
        );
    }
}

#[test]
fn an_exclusion_says_how_many_left_and_why() {
    // A silent exclusion is the same failure as a query that matches nothing: a
    // confident number nobody can question. Whoever watches their list shrink
    // gets the count, the names and the type that did it.
    let out = run_deck("outside-the-library.txt", "two-lands.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("11 cards never in the library"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("3x Bumper Cars (Attraction)"),
        "stderr was: {stderr}"
    );
    serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("stdout is pure JSON");
}

#[test]
fn a_run_says_what_produced_it() {
    // A percentage on its own cannot explain why it differs from yesterday's:
    // the deck, the criteria, the index and the tool all move it and all four
    // look identical in the output. So the run names its inputs.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let p = &json["provenance"];

    assert_eq!(p["tool_version"], env!("CARGO_PKG_VERSION"));
    for field in ["deck_sha256", "criteria_sha256"] {
        let hash = p[field].as_str().unwrap_or_else(|| panic!("no {field}"));
        assert_eq!(hash.len(), 64, "{field} should be a sha256: {hash}");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{field}");
    }
}

#[test]
fn the_same_inputs_hash_the_same_way() {
    // Worth nothing as an identifier if it moves on its own, which would make
    // every comparison report a change nobody made.
    let first = run("simple-ramp.criteria.toml");
    let second = run("simple-ramp.criteria.toml");
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let second: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(first["provenance"], second["provenance"]);
}

#[test]
fn editing_the_criteria_moves_only_the_criteria_hash() {
    // The whole point of hashing the two files separately: "your criteria
    // changed" and "your deck changed" are different diagnoses, and a single
    // hash over both could not tell them apart.
    let a = run("simple-ramp.criteria.toml");
    let b = run("two-lands-tables.criteria.toml");
    let a: serde_json::Value = serde_json::from_slice(&a.stdout).unwrap();
    let b: serde_json::Value = serde_json::from_slice(&b.stdout).unwrap();

    assert_ne!(
        a["provenance"]["criteria_sha256"], b["provenance"]["criteria_sha256"],
        "a different criteria file must hash differently"
    );
    assert_eq!(
        a["provenance"]["deck_sha256"], b["provenance"]["deck_sha256"],
        "the deck did not change, so its hash must not"
    );
}

#[test]
fn an_index_with_no_date_reports_the_date_as_unknown() {
    // The fixture index is a hand-written subset with no `updated_at`, as any
    // index built before the field existed is. Null says nobody knows; omitting
    // the key would read as a tool that forgot to look, and filling it in with
    // today would be evidence for a claim nothing supports.
    let out = run("simple-ramp.criteria.toml");
    assert!(out.status.success(), "a dateless index must still run");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let date = json["provenance"]
        .get("index_updated_at")
        .expect("the date must be reported even when it is unknown");
    assert!(date.is_null(), "expected null, got {date}");
}

#[test]
fn a_sampled_run_reports_the_seed_that_dealt_it() {
    // Trials without a seed does not identify the hands, so quoting a sampled
    // figure from it cannot be checked by anyone who wants to repeat the run.
    let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture("simple-ramp.criteria.toml"))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .args(["--simulate", "--trials", "5000", "--seed", "9"])
        .output()
        .expect("binary should run");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["seed"], 9);
}

// --- Expectations ---------------------------------------------------------

/// The expectation named `name`, as the report emitted it.
fn expectation<'a>(json: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    json["expectations"]
        .as_array()
        .expect("every report carries an expectations array")
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("no expectation named {name}"))
}

#[test]
fn an_expectation_reports_the_closed_form_mean_end_to_end() {
    // 36 lands in a 99-card library, opening seven. The mean of a hypergeometric
    // is draws * successes / population, which is 2.545454..., and it is the
    // constant this project's predecessor put in its shuffler acceptance test.
    // Nothing between the decklist and this number goes near that arithmetic.
    let out = run("simple-ramp.criteria.toml");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    let lands = expectation(&json, "lands in opener");
    assert_eq!(lands["mean"], 2.5455);

    // And the distribution behind it, which is the half a mean cannot show:
    // 3.7% of opening hands have no land at all.
    let d: Vec<f64> = lands["distribution"]
        .as_array()
        .expect("a distribution array")
        .iter()
        .map(|p| p.as_f64().unwrap())
        .collect();
    assert_eq!(d.len(), 8, "seven cards drawn, so values 0 through 7");
    assert!(
        (d.iter().sum::<f64>() - 1.0).abs() < 1e-5,
        "summed to {:?}",
        d.iter().sum::<f64>()
    );
    assert!((d[0] - 0.037165).abs() < 1e-6, "P(no lands) was {}", d[0]);
    // The mean the report prints is the mean of the buckets it prints beside it.
    let from_buckets: f64 = d.iter().enumerate().map(|(k, p)| k as f64 * p).sum();
    assert!((from_buckets - 2.5455).abs() < 1e-4, "{from_buckets}");
}

#[test]
fn expectations_do_not_disturb_the_criteria_contract() {
    // The `criteria` array is a contract several things already read by that
    // name and that shape, including the provenance comparison. Expectations
    // arrive alongside it rather than inside it.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    assert_eq!(json["criteria"].as_array().unwrap().len(), 4);
    assert!((percent(&json, "keepable opener (2-5 lands)") - 78.97).abs() < 0.01);
    for c in json["criteria"].as_array().unwrap() {
        assert!(c.get("mean").is_none(), "a criterion has no mean");
    }
    // An expectation cannot fail, so it cannot move the verdict either.
    assert_eq!(json["asserted"], 3);
    assert_eq!(json["failed"], 0);
    assert_eq!(json["ok"], true);
    for e in json["expectations"].as_array().unwrap() {
        assert!(
            e.get("at_least").is_none(),
            "expectations have no threshold"
        );
        assert!(e.get("pass").is_none(), "and so no verdict");
    }
}

#[test]
fn the_human_report_shows_the_mean_and_the_shape() {
    // "2.55 lands on average" hides whether you are flooding or screwing, so the
    // histogram is printed under it rather than left in the JSON.
    let out = run("simple-ramp.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("lands in opener"), "stderr was: {stderr}");
    assert!(stderr.contains("mean 2.55"), "stderr was: {stderr}");
    assert!(stderr.contains("0: 3.7%"), "stderr was: {stderr}");
    assert!(stderr.contains("2: 29.7%"), "stderr was: {stderr}");
    // And none of it lands on the stdout a caller pipes through jq.
    serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("stdout is pure JSON");
}

#[test]
fn sampled_expectations_agree_with_the_exact_engine_end_to_end() {
    // The third level at which the two engines are held to each other, now for
    // the second kind of answer. A sampled mean without its error bar is how a
    // 2.54 and a 2.55 get mistaken for a disagreement, so the report carries one.
    let exact = run("simple-ramp.criteria.toml");
    let exact: serde_json::Value = serde_json::from_slice(&exact.stdout).unwrap();

    let sampled = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture("simple-ramp.criteria.toml"))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .args(["--simulate", "--trials", "50000", "--seed", "1"])
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&sampled.stdout).unwrap();

    for e in sampled["expectations"].as_array().unwrap() {
        let name = e["name"].as_str().unwrap();
        let got = e["mean"].as_f64().unwrap();
        let se = e["standard_error"]
            .as_f64()
            .expect("a sampled mean carries one");
        let want = expectation(&exact, name)["mean"].as_f64().unwrap();
        assert!(
            (got - want).abs() < 4.0 * se,
            "{name}: sampled {got} vs exact {want}, {:.2} SE away",
            (got - want).abs() / se
        );

        // Bucket for bucket too: a mean can be right while the shape is wrong.
        let want_d = expectation(&exact, name)["distribution"]
            .as_array()
            .unwrap();
        let got_d = e["distribution"].as_array().unwrap();
        for (k, w) in want_d.iter().enumerate() {
            let w = w.as_f64().unwrap();
            let g = got_d.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
            assert!(
                (g - w).abs() < 4.0 * (g * (1.0 - g) / 50_000.0).sqrt() + 3.0 / 50_000.0,
                "{name}, P(exactly {k}): sampled {g} vs exact {w}"
            );
        }
    }
}

#[test]
fn only_sampled_expectations_carry_error_bars() {
    // An exact distribution has no sampling error, and quoting one would be a
    // lie about how the number was produced.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for e in json["expectations"].as_array().unwrap() {
        assert!(
            e.get("standard_error").is_none(),
            "exact runs must not report SE"
        );
    }
}

/// A file that would have answered rather than complained.
///
/// Each of these is a criteria file that parses as TOML and asks a question
/// with no honest answer. Left alone they produce a plausible percentage, which
/// is the failure this whole tool is about, so each one has to refuse by name:
/// the file it was in, the question that was wrong, and what was wrong with it.
#[test]
fn a_criteria_file_that_asks_nothing_refuses_by_name() {
    for (file, expected) in [
        // An empty conjunction holds on every hand, so this would report 100%.
        (
            "no-clauses.criteria.toml",
            ["nothing at all", "require"].as_slice(),
        ),
        // A clause naming a turn and a query and asking nothing of them.
        (
            "no-bounds.criteria.toml",
            ["lands in opener", "min", "max"].as_slice(),
        ),
        // The JavaScript spelling of the threshold. Silently dropping it would
        // turn an assertion into a number that cannot fail.
        (
            "unknown-key.criteria.toml",
            ["atLeast", "at_least"].as_slice(),
        ),
        // A battlefield question about something that has to be cast.
        // Approximating it with "drawn" would report a hand count under a
        // battlefield question. Lands are answerable; this is not.
        (
            "battlefield-spell.criteria.toml",
            ["battlefield", "cast", "issues/10", "Llanowar Elves"].as_slice(),
        ),
        // Two questions in one clause, which would have to answer one of them
        // silently.
        (
            "cast-and-count.criteria.toml",
            ["query", "can_cast", "are different questions"].as_slice(),
        ),
        // A symbol the gate cannot pay. Read as zero, an X-spell is castable on
        // turn one.
        ("bad-cost.criteria.toml", ["{X}", "an X spell"].as_slice()),
    ] {
        let out = run(file);
        assert!(!out.status.success(), "{file} should fail the run");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(file), "{file} must name itself: {stderr}");
        for needle in expected {
            assert!(
                stderr.contains(needle),
                "{file} should mention {needle:?}: {stderr}"
            );
        }
    }
}

/// `sync` builds the index this tool reads, from a bulk file rather than from
/// the network — so CI proves the whole path without depending on Scryfall
/// being up, which would make a test's answer a fact about reachability.
#[test]
fn sync_builds_an_index_from_a_bulk_file() {
    let dir = std::env::temp_dir().join(format!("pe-sync-{}", std::process::id()));
    let index = dir.join("index.jsonl");
    let _ = std::fs::remove_dir_all(&dir);

    let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("sync")
        .arg("--index")
        .arg(&index)
        .arg("--from")
        .arg(fixture("bulk-sample.jsonl"))
        .output()
        .expect("binary should run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "sync should succeed: {stderr}");

    // Every record is accounted for, by name rather than as a bare total.
    assert!(stderr.contains("read 15 records"), "{stderr}");
    assert!(stderr.contains("skipped 3"), "{stderr}");
    assert!(stderr.contains("kept 12 cards"), "{stderr}");

    let text = std::fs::read_to_string(&index).expect("index written");
    let (header, body) = text.split_once('\n').expect("a header line and a body");

    // Issue #38 end to end: the token must not have won the name.
    let elves: serde_json::Value =
        serde_json::from_str(entry(body, "llanowar elves")).expect("the entry is JSON");
    assert_eq!(elves["cmc"], 1.0, "the card, not the mana value 0 token");
    assert_eq!(elves["type_line"], "Creature — Elf Druid");
    assert_eq!(elves["produces"][0], "G");

    // The header says which shape the file was written in, so a later run can
    // tell a fresh index from one that predates the fields it reads — and how
    // many cards should follow, so a truncated file is caught rather than read
    // as a smaller card pool.
    let header: serde_json::Value = serde_json::from_str(header).expect("header is JSON");
    // Against the constant rather than a literal: this asserts that sync writes
    // the shape this build reads, which is the thing that matters. A literal
    // here only ever asserts that nobody bumped the number.
    assert_eq!(header["schema"], pe_scryfall::index::SCHEMA);
    assert_eq!(header["cards"], 12);
    // Built with --from, which reads a bulk file and so cannot fetch tags.
    // The header must not claim any: an index that looked tagged but was not
    // would answer otag: with a confident nothing.
    assert!(header.get("tags").is_none(), "a --from sync claims no tags");
    // And it says so at the time, with what it costs. The documented offline
    // route quietly building an index that switches the effect library off is
    // half of #50.
    assert!(stderr.contains("skipping oracle tags"), "{stderr}");
    assert!(stderr.contains("This index will carry none"), "{stderr}");
    assert!(stderr.contains("effect library"), "{stderr}");
    assert_eq!(body.lines().count(), 12);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A second sync over the same path leaves a readable index rather than a
/// half-written one — the write goes to a neighbour and is renamed over.
#[test]
fn sync_replaces_an_existing_index_without_leaving_debris() {
    let dir = std::env::temp_dir().join(format!("pe-resync-{}", std::process::id()));
    let index = dir.join("index.jsonl");
    let _ = std::fs::remove_dir_all(&dir);

    for _ in 0..2 {
        let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .arg("sync")
            .arg("--index")
            .arg(&index)
            .arg("--from")
            .arg(fixture("bulk-sample.jsonl"))
            .output()
            .expect("binary should run");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(index.exists());
    assert!(
        !dir.join("index.jsonl.partial").exists(),
        "the temporary file should have been renamed away"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An index built before a field existed answers every query that reads it with
/// nothing, which reads exactly like a deck that has none of that thing. That
/// is this project's defining failure mode aimed at its own cache, so the run
/// says which one you might be looking at.
#[test]
fn an_index_that_predates_the_fields_says_so() {
    let out = run("simple-ramp.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("built before some of the fields"),
        "the checked-in fixture index has no schema, so it is stale: {stderr}"
    );
    assert!(stderr.contains("progress-engine sync"), "{stderr}");
    // And it still answers, because a stale index is not a reason to refuse.
    assert!(out.status.success(), "{stderr}");
}

/// The whole path, from Scryfall's bytes to a probability: build an index out
/// of real bulk records, then answer questions that only the fields that index
/// carries can answer. Everything above this runs against a checked-in index,
/// so without this the two halves are only ever tested apart.
#[test]
fn sync_then_test_answers_questions_the_old_index_could_not() {
    let dir = std::env::temp_dir().join(format!("pe-e2e-{}", std::process::id()));
    let index = dir.join("index.jsonl");
    let _ = std::fs::remove_dir_all(&dir);

    let sync = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("sync")
        .arg("--index")
        .arg(&index)
        .arg("--from")
        .arg(fixture("simple-ramp.bulk.jsonl"))
        .output()
        .expect("binary should run");
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );

    let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture("produces.criteria.toml"))
        .arg("--index")
        .arg(&index)
        .output()
        .expect("binary should run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");

    // A freshly built index is not stale, so the warning must be absent.
    assert!(
        !stderr.contains("built before some of the fields"),
        "an index this tool just built should not be called stale: {stderr}"
    );
    // And no query matched nothing, which is the failure these keys exist to
    // avoid rather than to cause.
    assert!(
        !stderr.contains("matched no cards"),
        "the new keys should match real cards: {stderr}"
    );

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let counted = |query: &str| -> u64 {
        json["queries"]
            .as_array()
            .expect("queries array")
            .iter()
            .find(|q| q["query"] == query)
            .unwrap_or_else(|| panic!("no query {query}"))["cards"]
            .as_u64()
            .expect("a count")
    };

    // 36 lands plus the accelerants; the exact figure is the deck's, and what
    // matters is that it is neither zero nor everything.
    let green_sources = counted("produces:g");
    assert!(
        (40..=60).contains(&green_sources),
        "produces:g found {green_sources} of a 99 card deck"
    );
    assert!(percent(&json, "green source in opener") > 90.0);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A query error has to arrive with its cause attached. The message that names
/// the offending term and lists what would have been accepted is the entire
/// value of refusing rather than matching nothing — and it travels through a
/// generic error whose Display shows only its outermost layer, so it is easy to
/// lose and impossible to notice from inside the parser's own tests.
#[test]
fn a_refused_query_says_what_it_would_have_accepted() {
    let out = run("bad-query.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(stderr.contains("is:tapland"), "names the query: {stderr}");
    assert!(stderr.contains("tapland"), "names the term: {stderr}");
    assert!(
        stderr.contains("phyrexian"),
        "lists the properties it does support: {stderr}"
    );
}

// --- The effect library ---------------------------------------------------

fn run_loam(criteria: &str) -> std::process::Output {
    run_with("loam.txt", criteria, "loam-index.jsonl")
}

fn run_with(deck: &str, criteria: &str, index: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture(deck))
        .arg(fixture(criteria))
        .arg("--index")
        .arg(fixture(index))
        .output()
        .expect("binary should run")
}

#[test]
fn the_three_ways_an_otag_question_comes_back_empty_are_three_different_answers() {
    // #50. All three produce no cards, and only one of them is a fact about the
    // deck. Asserted together because the point is that they are told apart:
    // any two of these collapsing into one message is the confident zero this
    // tool exists to prevent, and it is only visible side by side.

    // One: the index carries tags, and not this one. Refused by name, with what
    // it does carry, because the fix is a spelling or a sync rather than a
    // different deck.
    let unfetched = run_with(
        "loam.txt",
        "unfetched-tag.criteria.toml",
        "loam-index.jsonl",
    );
    let stderr = String::from_utf8_lossy(&unfetched.stderr);
    assert!(!unfetched.status.success(), "{stderr}");
    assert!(stderr.contains("does not carry otag:mill"), "{stderr}");
    assert!(
        stderr.contains("scry, surveil, tapland"),
        "it says what it does carry: {stderr}"
    );
    assert!(
        unfetched.stdout.is_empty(),
        "and produces no JSON that could be read as an answer"
    );

    // Two: the index carries no tags at all, which no spelling fixes and which
    // the header states plainly. The criterion that asked is named, and so is
    // the command that fixes it.
    let tagless = run("otag.criteria.toml");
    let stderr = String::from_utf8_lossy(&tagless.stderr);
    assert!(!tagless.status.success(), "{stderr}");
    assert!(stderr.contains("carries no oracle tags at all"), "{stderr}");
    assert!(stderr.contains("surveil lands in the opener"), "{stderr}");
    assert!(stderr.contains("progress-engine sync"), "{stderr}");
    assert!(tagless.stdout.is_empty());

    // Three: the tag is carried, and no card in this deck is in it. The only
    // one of the three that is an answer, and it gets the note every query
    // matching nothing gets.
    let empty = run_with("loam.txt", "empty-tag.criteria.toml", "loam-index.jsonl");
    let stderr = String::from_utf8_lossy(&empty.stderr);
    assert!(empty.status.success(), "{stderr}");
    assert!(
        stderr.contains("note: query \"otag:scry\" matched no cards in this deck"),
        "{stderr}"
    );
    let json: serde_json::Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert_eq!(json["queries"][0]["cards"], 0);
}

#[test]
fn a_keyword_no_card_in_the_index_has_is_refused_rather_than_counted() {
    // #53. The precedent the otag refusal above was built on, and it had no
    // caller: `kw:flyign` parsed, matched nothing, and reported 0.00% with the
    // same note a real keyword nobody plays would get. Same three cases, same
    // seam, one difference — an index silent about keywords refuses nothing,
    // because keywords are derived from the cards rather than fetched.

    // One: the index lists its keywords, and this is not one of them. Refused
    // by name, before anything is enumerated, with no JSON to read as an answer.
    let typo = run_with("loam.txt", "kw-typo.criteria.toml", "loam-index.jsonl");
    let stderr = String::from_utf8_lossy(&typo.stderr);
    assert!(!typo.status.success(), "{stderr}");
    assert!(stderr.contains("kw:flyign"), "names the keyword: {stderr}");
    assert!(
        stderr.contains("a typoed keyword"),
        "and the criterion that asked: {stderr}"
    );
    assert!(stderr.contains("progress-engine sync"), "{stderr}");
    assert!(
        typo.stdout.is_empty(),
        "and produces no JSON that could be read as an answer"
    );

    // Two: the keyword is real and in the pool. Answered, with no note.
    let real = run_with("loam.txt", "kw-real.criteria.toml", "loam-index.jsonl");
    let stderr = String::from_utf8_lossy(&real.stderr);
    assert!(real.status.success(), "{stderr}");
    assert!(!stderr.contains("kw:dredge"), "not refused: {stderr}");
    let json: serde_json::Value = serde_json::from_slice(&real.stdout).unwrap();
    assert_eq!(json["queries"][0]["cards"], 4);

    // Three: the index never wrote a keyword list down, so it knows nothing
    // about keywords and says nothing about this one. Refusing here would fail
    // a correct query over a gap in the file rather than a gap in the deck.
    let unlisted = run("kw-unlisted.criteria.toml");
    let stderr = String::from_utf8_lossy(&unlisted.stderr);
    assert!(unlisted.status.success(), "{stderr}");
    assert!(!stderr.contains("kw:flying"), "not refused: {stderr}");
    let json: serde_json::Value = serde_json::from_slice(&unlisted.stdout).unwrap();
    assert_eq!(
        json["queries"][0]["cards"], 1,
        "and the keyword still matches: Birds of Paradise"
    );
}

#[test]
fn the_autoloading_library_does_not_move_the_known_numbers() {
    // The regression anchor for the whole feature. The standard library loads
    // on every run without anybody asking, so the first thing it has to prove
    // is that it changed nothing: same four percentages, same two means.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!((percent(&json, "keepable opener (2-5 lands)") - 78.97).abs() < 0.01);
    assert!((percent(&json, "turn-1 accelerant") - 51.04).abs() < 0.01);
    assert!((percent(&json, "commander on turn 2") - 44.29).abs() < 0.01);
    assert!((percent(&json, "any ramp by turn 3") - 94.39).abs() < 0.01);
    let means: Vec<f64> = json["expectations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["mean"].as_f64().unwrap())
        .collect();
    assert_eq!(means, vec![2.5455, 2.3636]);

    // And nothing applied, so nothing is claimed. An entry that matched no card
    // said nothing about this deck and does not get to appear as though it did.
    assert_eq!(json["effects"].as_array().unwrap().len(), 0);
}

#[test]
fn a_standard_library_entry_matching_nothing_is_silent_when_the_index_could_have_answered() {
    // The contrast with `a_query_matching_no_cards_is_called_out`. A query in
    // the user's file matching nothing is a typo worth shouting about; a
    // standard library entry matching nothing is most decks, and a note about
    // it on every run is noise about a question nobody asked.
    //
    // This index carries scry, and this deck has no scry land -- so the silence
    // is granted over a question that was genuinely asked and genuinely
    // answered, which is the only case it was ever meant to cover (#50).
    let out = run_with(
        "surveil-tiny.txt",
        "two-lands.criteria.toml",
        "loam-index.jsonl",
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("otag:scry"),
        "the library should not narrate itself: {stderr}"
    );
    assert!(!stderr.contains("carries no oracle tags"), "{stderr}");
    // And the run is live rather than inert: the surveil entry in the same
    // library did match, so the silence above is about that entry and not about
    // an effect library that never ran.
    assert!(stderr.contains("otag:surveil"), "{stderr}");
}

#[test]
fn a_standard_library_entry_that_a_tagless_index_silenced_says_so() {
    // The other half of #50, and the reason the silence above had to become
    // conditional. The standard library is keyed entirely on `otag:`, so
    // against an index carrying no tags it matches nothing, applies nothing and
    // reports `"effects": []` -- a whole autoloading feature switched off, by a
    // fact about the index rather than about the deck, saying nothing.
    let out = run("simple-ramp.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("this index carries no oracle tags"),
        "{stderr}"
    );
    assert!(stderr.contains("\"t:land otag:surveil\""), "{stderr}");
    assert!(stderr.contains("\"t:land otag:scry\""), "{stderr}");
    assert!(stderr.contains("progress-engine sync"), "{stderr}");
    // It is a note and not a refusal: nobody asked for these entries, and the
    // questions this file does ask are answerable without them.
    assert!(out.status.success(), "{stderr}");
}

#[test]
fn a_hand_written_effect_matching_nothing_is_called_out() {
    // And the other side of that contrast: an effect the user wrote themselves
    // that picks out no card is the same confident-zero failure as a query that
    // does, so it gets the same note.
    let out = run("unmatched-effect.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("effect \"name:\\\"Undercity Sewers\\\"\" matched no cards"),
        "stderr was: {stderr}"
    );
}

#[test]
fn routing_is_exact_on_a_deck_small_enough_to_check_by_hand() {
    // Ten cards, one surveil land, one Loam, turn 2 on the play. Worked out on
    // paper from the shuffled positions:
    //
    //   binned  = P(Sewers in 1-7, Loam at 8) + P(Sewers at 8, Loam at 9)
    //           = (1/10)(7/9) + (1/10)(1/9) = 8/90
    //   in hand = P(Loam in 1-8) - P(Loam at 8, Sewers in 1-7)
    //           = 8/10 - 7/90 = 65/90
    //   library = the rest, 17/90
    //
    // The second binning term is the one a naive model misses: the Sewers drawn
    // on turn 2 is played on turn 2, and it surveils the card behind it.
    let out = run_with(
        "surveil-tiny.txt",
        "surveil-tiny.criteria.toml",
        "loam-index.jsonl",
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let binned = percent(&json, "loam binned by turn 2");
    let hand = percent(&json, "loam in hand by turn 2");
    let library = percent(&json, "loam still in the library by turn 2");
    assert!((binned - 800.0 / 90.0).abs() < 0.01, "binned was {binned}");
    assert!((hand - 6500.0 / 90.0).abs() < 0.01, "in hand was {hand}");
    assert!(
        (library - 1700.0 / 90.0).abs() < 0.01,
        "library was {library}"
    );
    // One Loam, three zones, so they partition it. A routing bug that dropped a
    // card or counted it twice would break this without breaking any one of the
    // three above on its own.
    assert!(
        (binned + hand + library - 100.0).abs() < 1e-6,
        "{binned} + {hand} + {library}"
    );
}

#[test]
fn a_surveil_land_puts_loam_in_the_yard() {
    // HANDS.md hand 9, the Life from the Loam north star: the surveil fires on
    // the land drop, the Loam on top is binned, and the graveyard is a zone the
    // criterion can ask about. This number was 0.00% by construction until
    // routing existed.
    let out = run_loam("loam-yard.criteria.toml");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let yard = percent(&json, "loam in the yard by turn 5");
    assert!(yard > 0.0, "the yard should be reachable now: {yard}");
    assert!(yard < 100.0, "and it is not a certainty either: {yard}");
}

#[test]
fn routing_a_card_to_the_graveyard_switches_the_warning_off() {
    // The pair of `a_zone_nothing_routes_a_card_into_is_called_out`. A warning
    // that keeps firing once the thing it warns about is fixed is a permanent
    // false alarm, and one that never fires is a lie waiting to happen -- so
    // both directions are asserted, against the same zone in the same run.
    let routed = run_loam("loam-yard.criteria.toml");
    let stderr = String::from_utf8_lossy(&routed.stderr);
    assert!(
        !stderr.contains("nothing routes a card to the graveyard"),
        "an effect routes one there now: {stderr}"
    );
    let json: serde_json::Value = serde_json::from_slice(&routed.stdout).unwrap();
    let yard = json["zones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|z| z["zone"] == "graveyard")
        .expect("the graveyard is asked about");
    assert_eq!(yard["reachable"], true);

    // The same deck and the same card with no destination declared: the look
    // still happens and every card it sees stays on top, so the graveyard is
    // unreachable again and says so.
    let unrouted = run_loam("loam-hand.criteria.toml");
    let stderr = String::from_utf8_lossy(&unrouted.stderr);
    assert!(
        stderr.contains("nothing routes a card to the graveyard"),
        "nothing routes one here: {stderr}"
    );
    let json: serde_json::Value = serde_json::from_slice(&unrouted.stdout).unwrap();
    let yard = json["zones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|z| z["zone"] == "graveyard")
        .expect("the graveyard is asked about");
    assert_eq!(yard["reachable"], false);
}

#[test]
fn the_same_card_routed_the_other_way_keeps_loam_in_hand() {
    // HANDS.md hand 10. Same deck, same surveil land, opposite routing, and the
    // answer moves -- which is the whole argument for routing being part of the
    // question rather than part of the card.
    //
    // The unrouted run is also the control: with every looked-at card left on
    // top, nothing has changed about which cards arrive when, so it has to
    // reproduce the closed form exactly. Eleven cards seen of sixty, four Loam.
    let kept: serde_json::Value =
        serde_json::from_slice(&run_loam("loam-hand.criteria.toml").stdout).unwrap();
    let binned: serde_json::Value =
        serde_json::from_slice(&run_loam("loam-yard.criteria.toml").stdout).unwrap();

    let closed = (1.0 - pe_stats::pmf(60, 4, 11, 0)) * 100.0;
    let in_hand = percent(&kept, "loam in hand by turn 5");
    assert!(
        (in_hand - closed).abs() < 0.01,
        "a look that routes nothing must move nothing: {in_hand} vs {closed}"
    );
    assert!(
        percent(&binned, "loam in hand by turn 5") < in_hand,
        "a Loam in the yard is a Loam not in hand"
    );
    assert_eq!(percent(&kept, "loam in the yard by turn 5"), 0.0);
}

#[test]
fn both_engines_apply_effects_identically() {
    // The third place the two engines are held to each other, now for routing.
    // `pe_sim` is generic over `Evaluator` and never mentions an effect, which
    // is either why it cannot disagree or why it would ignore effects silently
    // -- and only a clause whose answer *moves* with the feature tells those
    // apart. The graveyard clause here is 4.9%, not 0%, so a sampler that
    // ignored routing would miss by every point of it.
    let exact: serde_json::Value =
        serde_json::from_slice(&run_loam("loam-yard.criteria.toml").stdout).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("loam.txt"))
        .arg(fixture("loam-yard.criteria.toml"))
        .arg("--index")
        .arg(fixture("loam-index.jsonl"))
        .args(["--simulate", "--trials", "200000", "--seed", "5"])
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    for c in sampled["criteria"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let got = c["percent"].as_f64().unwrap();
        let se = c["standard_error"].as_f64().unwrap() * 100.0;
        let want = percent(&exact, name);
        assert!(se > 0.0, "{name} is vacuous in the sampler");
        assert!(
            (got - want).abs() < 4.0 * se,
            "{name}: sampled {got} vs exact {want}, {:.2} SE away",
            (got - want).abs() / se
        );
    }
    // And the distribution of how many got binned, bucket for bucket.
    let want = exact["expectations"][0]["mean"].as_f64().unwrap();
    let got = sampled["expectations"][0]["mean"].as_f64().unwrap();
    let se = sampled["expectations"][0]["standard_error"]
        .as_f64()
        .unwrap();
    assert!(want > 0.0, "a mean of zero would prove nothing");
    assert!(
        (got - want).abs() < 4.0 * se,
        "sampled {got} vs exact {want}"
    );
}

#[test]
fn the_run_says_which_effect_applied_to_which_card() {
    // The condition attached to last-wins. Assuming the library is true is a
    // reasonable stance; assuming it is true with no way to see what it did is
    // not, because the first question about a surprising number is what the
    // tool thought the cards do.
    let out = run_loam("loam-yard.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let effects = json["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{effects:?}");
    assert_eq!(effects[0]["match"], "t:land otag:surveil");
    assert_eq!(effects[0]["look"], 1);
    assert_eq!(effects[0]["on"], "landdrop");
    assert_eq!(effects[0]["cards"][0], "Undercity Sewers");
    assert_eq!(effects[0]["copies"], 4);
    assert_eq!(effects[0]["live"], true);
    // And in front of a human, because the library autoloads: nobody asked for
    // this effect, so the run mentions it unprompted.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Undercity Sewers") && stderr.contains("look 1"),
        "stderr was: {stderr}"
    );
}

#[test]
fn a_users_effect_overrides_the_standard_library_for_the_cards_it_names() {
    // Last-wins, per card. The standard library declares `t:land otag:surveil`
    // with no destination; the file declares the same query with one. Both
    // match Undercity Sewers, one entry reports it, and the destination is the
    // later one -- so overriding the library needs no override syntax to exist.
    let out = run_loam("loam-yard.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let effects = json["effects"].as_array().unwrap();
    assert_eq!(
        effects.len(),
        1,
        "stacked instead of overriding: {effects:?}"
    );
    assert_eq!(effects[0]["to_graveyard"], "name:\"Life from the Loam\"");
    assert!(
        effects[0]["source"].as_str().unwrap().contains("loam-yard"),
        "the later declaration wins: {effects:?}"
    );
    // And it looks one card, not two. Stacking would be a confidently wrong
    // number of exactly the shape this project exists to prevent.
    assert_eq!(effects[0]["look"], 1);
}

#[test]
fn a_look_on_a_cast_is_refused_by_name() {
    // `on = "cast"` now fires — the budget knows which spells a turn paid for
    // — and the refusal has narrowed to the half that is still true. A *look*
    // on a cast is a replacement draw, which costs an enumeration checkpoint a
    // turn; a *fetch* on a cast is a subtraction, and that is what ships. So
    // the refusal names the draw, the issue that measures it, and the key that
    // does work.
    let out = run("cast-effect.criteria.toml");
    assert!(!out.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("\"cast\""), "should name it: {stderr}");
    assert!(
        stderr.contains("replacement draw"),
        "should say what it is actually missing: {stderr}"
    );
    assert!(
        stderr.contains("issues/57"),
        "should say what it is waiting for: {stderr}"
    );
    assert!(
        stderr.contains("fetch"),
        "should name the half that does work: {stderr}"
    );
}

#[test]
fn the_effect_library_is_named_in_the_provenance() {
    // An input that moves the numbers and that nobody edited. Without it, a
    // percentage that changed because the shipped library changed is
    // indistinguishable from one that changed because the deck did.
    let out = run("simple-ramp.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let hash = json["provenance"]["effect_library_sha256"]
        .as_str()
        .expect("the effect library is provenance");
    assert_eq!(hash.len(), 64, "a sha256, like the other two: {hash}");
    // A different deck, the same library: the hash is about the library.
    let other: serde_json::Value =
        serde_json::from_slice(&run_loam("loam-hand.criteria.toml").stdout).unwrap();
    assert_eq!(other["provenance"]["effect_library_sha256"], hash);
    assert_ne!(
        other["provenance"]["deck_sha256"],
        json["provenance"]["deck_sha256"]
    );
}

/// The `test` subcommand against the standard fixture deck, with extra flags.
fn run_flags(criteria: &str, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture(criteria))
        .arg("--index")
        .arg(fixture("index.jsonl"))
        .args(args)
        .output()
        .expect("binary should run")
}

#[test]
fn a_question_too_wide_to_enumerate_is_answered_by_sampling() {
    // #48. A criterion correlating turn 2 with turn 6 across seven queries is a
    // hundred million compositions against a ceiling of five million, and until
    // now that was the end of the run. A block editor cannot pass a flag and
    // cannot act on advice to ask something smaller, so a refusal there is a
    // dead end rather than a lesson.
    //
    // This test is as much about the warning as about the number. An estimate
    // that reads like an exact answer is the failure this repository is named
    // against, and the fallback is only acceptable because it is impossible to
    // miss.
    let out = run_flags("too-wide.criteria.toml", &[]);
    assert!(
        out.status.success(),
        "the question gets an answer: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");

    // #31: the file is several enumerations, so one question going over the
    // ceiling no longer takes the others with it.
    assert_eq!(json["method"], "mixed");
    assert_eq!(json["sampled_because"], "too_wide");
    assert_eq!(json["too_wide"]["paths"], 103_169_430u64);
    assert_eq!(json["too_wide"]["groups"], 12);
    assert_eq!(json["too_wide"]["ceiling"], 5_000_000u64);
    assert_eq!(json["trials"], 200_000);
    assert_eq!(
        json["seed"], 0,
        "a fallback is reproducible or it is a rumour"
    );

    // The estimated number says how uncertain it is, or the JSON is claiming an
    // exactness the run did not have — and the enumerated ones must not be
    // given an error bar they never had.
    for c in json["criteria"].as_array().unwrap() {
        let sampled = c["method"] == "sampled";
        assert_eq!(
            c["standard_error"].as_f64().is_some(),
            sampled,
            "{} is {} and its error bar disagrees",
            c["name"],
            c["method"]
        );
    }
    for e in json["expectations"].as_array().unwrap() {
        assert_eq!(
            e["standard_error"].as_f64().is_some(),
            e["method"] == "sampled",
            "{}",
            e["name"]
        );
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ESTIMATE"),
        "the warning has to be unmissable: {stderr}"
    );
    assert!(
        stderr.contains("too wide to enumerate exactly"),
        "it says what happened: {stderr}"
    );
    assert!(
        stderr.contains("103169430") && stderr.contains("12 groups"),
        "it says how wide the question was: {stderr}"
    );
    assert!(
        stderr.contains("5000000"),
        "and how wide it was allowed to be: {stderr}"
    );
    assert!(
        stderr.contains("200000 hands"),
        "and what it did instead: {stderr}"
    );
    assert!(
        stderr.contains("--exact"),
        "and how to get the refusal back: {stderr}"
    );
    // The warning is above the numbers rather than under them, because a reader
    // who skims stops at the first percentage.
    let banner = stderr.find("ESTIMATE").expect("the banner");
    let first_verdict = stderr.find('%').expect("a percentage");
    assert!(banner < first_verdict, "the warning comes first: {stderr}");
    // And the numbers themselves carry the error bar, so a line copied out of
    // the middle of the report cannot lose it.
    assert!(
        stderr.contains("% ± "),
        "every sampled percentage is quoted with its error: {stderr}"
    );
}

#[test]
fn exact_refuses_a_question_too_wide_rather_than_estimating_it() {
    // The other half of #48, and the reason the fallback is safe to have: the
    // cross-engine agreement tests need an oracle that either enumerates or
    // says nothing. An oracle that quietly became an estimate would be checking
    // the sampler against itself.
    let out = run_flags("too-wide.criteria.toml", &["--exact"]);
    assert!(!out.status.success(), "--exact refuses rather than answers");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("too wide to answer exactly"),
        "the refusal is unchanged: {stderr}"
    );
    assert!(
        stderr.contains("103169430 compositions across 12 groups"),
        "and still names the width: {stderr}"
    );
    assert!(
        stderr.contains("t:creature"),
        "and still names the queries that made it: {stderr}"
    );
    assert!(
        !stderr.contains("ESTIMATE"),
        "nothing was estimated: {stderr}"
    );
    assert!(
        out.stdout.is_empty(),
        "no report, because there is no answer"
    );
}

#[test]
fn a_question_that_fits_is_answered_exactly_either_way() {
    // The regression this feature is most likely to cause is a fallback that
    // fires when it should not, and the symptom would be a number that moved by
    // a tenth of a percent and a `method` nobody looked at.
    let fallback_allowed = run_flags("simple-ramp.criteria.toml", &[]);
    let exact_only = run_flags("simple-ramp.criteria.toml", &["--exact"]);
    assert!(fallback_allowed.status.success());
    assert!(exact_only.status.success());
    assert_eq!(
        fallback_allowed.stdout, exact_only.stdout,
        "--exact changes nothing about a question that fits"
    );

    let json: serde_json::Value = serde_json::from_slice(&fallback_allowed.stdout).unwrap();
    assert_eq!(json["method"], "exact");
    assert!(json.get("sampled_because").is_none());
    assert!(json.get("too_wide").is_none());
    assert!((percent(&json, "keepable opener (2-5 lands)") - 78.97).abs() < 0.01);

    let stderr = String::from_utf8_lossy(&fallback_allowed.stderr);
    assert!(
        !stderr.contains("ESTIMATE"),
        "nothing to warn about: {stderr}"
    );
    assert!(
        !stderr.contains('±'),
        "an exact answer has no error bar: {stderr}"
    );
}

#[test]
fn asking_for_both_engines_at_once_is_a_usage_error() {
    // --simulate and --exact are answers to the same question. A run that
    // honoured both would have to pick one silently, which is the one thing
    // this feature exists to avoid.
    let out = run_flags("simple-ramp.criteria.toml", &["--simulate", "--exact"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--simulate and --exact"),
        "it names both: {stderr}"
    );
}

#[test]
fn a_threshold_inside_the_error_bar_is_flagged_rather_than_rounded() {
    // 78.97% exactly, against a threshold of 79%. No number of hands short of
    // the whole enumeration can tell those apart, so the PASS or FAIL printed
    // here is a fact about the seed. The verdict is still computed — a
    // comparison that sometimes declines to answer is harder to build on than
    // one that is always reproducible — but it does not get to stand there
    // unqualified.
    let out = run_flags(
        "inconclusive.criteria.toml",
        &["--simulate", "--trials", "5000", "--seed", "1"],
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let c = &json["criteria"][0];
    assert_eq!(c["inconclusive"], true);
    let (percent, se) = (
        c["percent"].as_f64().unwrap(),
        c["standard_error"].as_f64().unwrap() * 100.0,
    );
    assert!(
        (percent - 79.0).abs() <= 2.0 * se,
        "the fixture is only interesting while this holds: {percent} ± {se}"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("inside the error bar"),
        "the close call is called out: {stderr}"
    );
    assert!(
        stderr.contains("is this seed's answer"),
        "and says what that means: {stderr}"
    );

    // A verdict nowhere near its threshold is not worth a note, and a note on
    // every line is a note nobody reads.
    let clear = run_flags(
        "simple-ramp.criteria.toml",
        &["--simulate", "--trials", "5000", "--seed", "1"],
    );
    assert!(
        !String::from_utf8_lossy(&clear.stderr).contains("inside the error bar"),
        "no close calls here"
    );
    let clear: serde_json::Value = serde_json::from_slice(&clear.stdout).unwrap();
    for c in clear["criteria"].as_array().unwrap() {
        if c["at_least"].as_f64().is_some() {
            assert_eq!(
                c["inconclusive"], false,
                "{} is not a close call",
                c["name"]
            );
        }
    }
}

// --- The mana gate (#10) --------------------------------------------------
//
// The fixture libraries here are nine cards, which is not a deck. It is the
// only way to write a *hand* down: nine cards and a question about turn 3 means
// the whole library is in hand by then, so every deal answers the same way and
// the percentage is the hand rather than a distribution over hands.

fn run_mana(deck: &str, criteria: &str) -> std::process::Output {
    run_with(deck, criteria, "mana-index.jsonl")
}

#[test]
fn land_drops_are_use_it_or_lose_it() {
    // HANDS.md hand 4. Five Islands drawn by turn 3, three of them in play,
    // because three land drops have happened and no number of lands in hand
    // adds a fourth. The two clauses read the same query on the same turn and
    // differ by two cards, which is the hole every mana-relevant criterion in
    // this repo had before the battlefield was answerable.
    let out = run_mana("hand-4.txt", "hand-4.criteria.toml");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!((percent(&json, "five lands drawn by turn 3") - 100.0).abs() < 0.01);
    assert!((percent(&json, "three lands in play by turn 3") - 100.0).abs() < 0.01);
    assert!(
        percent(&json, "four lands in play by turn 3") == 0.0,
        "a fourth drop does not exist: {json}"
    );
    // And as an expectation, so the whole distribution is pinned rather than a
    // threshold on it: every deal puts exactly three lands in play.
    let mean = expectation(&json, "lands in play by turn 3")["mean"]
        .as_f64()
        .unwrap();
    assert!((mean - 3.0).abs() < 1e-9, "{mean}");
}

#[test]
fn the_same_two_counts_pay_on_one_hand_and_not_on_the_other() {
    // HANDS.md hands 6 and 7, which are one test. Both libraries hold a white
    // source and a blue source by turn 3, so the conjunction a user would have
    // to write without a castability primitive holds on both. One of them can
    // pay {W}{U} and the other cannot, because one Hallowed Fountain is both
    // counts and one mana.
    let six: serde_json::Value =
        serde_json::from_slice(&run_mana("hand-6.txt", "hands-6-and-7.criteria.toml").stdout)
            .unwrap();
    let seven: serde_json::Value =
        serde_json::from_slice(&run_mana("hand-7.txt", "hands-6-and-7.criteria.toml").stdout)
            .unwrap();

    for hand in [&six, &seven] {
        assert!(
            (percent(hand, "a white source and a blue source by turn 3") - 100.0).abs() < 0.01,
            "the naive counts hold on both: {hand}"
        );
    }
    assert_eq!(
        percent(&six, "{W}{U} payable on turn 3"),
        0.0,
        "one Fountain is one mana: {six}"
    );
    // Eight deals in nine. The ninth is the one where the Fountain is the card
    // left out of the opening hand: it then arrives on turn 3, has to be played
    // on turn 3, and enters tapped under the assumption below.
    assert!(
        (percent(&seven, "{W}{U} payable on turn 3") - 88.89).abs() < 0.01,
        "{seven}"
    );
    // Two pips need two lands whatever colour they are, and turn 1 has one drop.
    for hand in [&six, &seven] {
        assert_eq!(percent(hand, "{W}{U} payable on turn 1"), 0.0);
    }
    // Generic takes any land, so the turn-1 question is only about tapped-ness:
    // the Fountain pays nothing the turn it lands, and the Island pays whenever
    // it is in the opener, which is seven deals in nine.
    assert_eq!(percent(&six, "{1} payable on turn 1"), 0.0);
    assert!((percent(&seven, "{1} payable on turn 1") - 77.78).abs() < 0.01);
}

#[test]
fn a_land_that_chooses_whether_to_enter_tapped_says_which_way_it_was_read() {
    // HANDS.md hand 8. `otag:tapland` does not hold the shocklands —
    // `otag:conditional-tapland` does, and membership of it settles nothing on
    // its own, because "you may pay 2 life" is the pilot's decision. The run
    // makes it pessimistically and does not get to make it quietly: the
    // assumption moves numbers and nobody wrote it down.
    let out = run_mana("hand-7.txt", "hands-6-and-7.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Hallowed Fountain"),
        "names the card it assumed about: {stderr}"
    );
    assert!(
        stderr.contains("enter tapped"),
        "says which way it assumed: {stderr}"
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["assumed_tapped"],
        serde_json::json!(["Hallowed Fountain"])
    );

    // And not on a run that asked no mana question, whatever the deck holds: a
    // note that fires on every run is a note nobody reads.
    let quiet = run_mana("hand-7.txt", "drawn-only.criteria.toml");
    let quiet_err = String::from_utf8_lossy(&quiet.stderr);
    assert!(
        !quiet_err.contains("enter tapped"),
        "nothing was assumed here: {quiet_err}"
    );
    let quiet_json: serde_json::Value = serde_json::from_slice(&quiet.stdout).unwrap();
    assert!(quiet_json.get("assumed_tapped").is_none(), "{quiet_json}");
}

#[test]
fn an_index_that_cannot_say_what_a_land_makes_refuses_the_question() {
    // The same failure as an unfetched oracle tag, one field along: an index
    // with no `produces` reports every land as making nothing and answers a
    // confident 0.00%, and one with no tapland tag reports every land as
    // untapped and answers a number the deck cannot reach. Both are refused.
    let out = run_with(
        "simple-ramp.txt",
        "cast-three-mana.criteria.toml",
        "index.jsonl",
    );
    assert!(!out.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("progress-engine sync"),
        "says how to fix it: {stderr}"
    );
    assert!(
        stderr.contains("three mana by turn 3"),
        "names the question that asked: {stderr}"
    );
}

#[test]
fn a_mana_question_beside_a_live_effect_is_refused_with_the_remedy_named() {
    // Two answers to one land drop, and still refused rather than arbitrated —
    // but the refusal names what to write rather than telling you to ask the
    // two halves in separate files. Picking one silently is the failure; asking
    // is the remedy.
    let out = run_loam("mana-with-routing.criteria.toml");
    assert!(!out.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("which land you played"), "{stderr}");
    assert!(
        stderr.contains("[land_drop]") && stderr.contains("prefer"),
        "names the remedy: {stderr}"
    );
    assert!(
        stderr.contains("two mana by turn 2"),
        "names the question: {stderr}"
    );
}

fn run_hand_twelve(criteria: &str) -> std::process::Output {
    run_with("hand-12.txt", criteria, "lantern-index.jsonl")
}

#[test]
fn a_declared_priority_lets_one_file_hold_the_filtering_and_the_mana() {
    // HANDS.md hand 12, which is the thing the refusal above was costing. Two
    // files, one deck, one question each way: the only difference between them
    // is which land they play first, and on turn 1 that is a factor of two.
    //
    // Worked on paper. Twelve cards, seven in the opener, so a named card is
    // out of the opening hand with probability 5/12. Lantern castable on turn 1
    // needs an untapped land in play, which is the Island; under "surveil
    // first" that happens only when the Sewers was left out, so it is the
    // openers holding the Island and the Lantern and not the Sewers,
    // C(9,5)/C(12,7) = 126/792. Under "untapped first" the Sewers is irrelevant
    // and it is C(10,5)/C(12,7) = 252/792, exactly twice as many.
    let surveil_first = run_hand_twelve("hand-12-surveil-first.criteria.toml");
    let untapped_first = run_hand_twelve("hand-12-untapped-first.criteria.toml");
    assert!(surveil_first.status.success(), "should answer, not refuse");
    assert!(untapped_first.status.success(), "should answer, not refuse");
    let surveil: serde_json::Value = serde_json::from_slice(&surveil_first.stdout).unwrap();
    let untapped: serde_json::Value = serde_json::from_slice(&untapped_first.stdout).unwrap();

    let turn_one = "Lantern castable on turn 1";
    assert!((percent(&surveil, turn_one) - 100.0 * 126.0 / 792.0).abs() < 0.01);
    assert!((percent(&untapped, turn_one) - 100.0 * 252.0 / 792.0).abs() < 0.01);

    // And the routing half is live in both, on the same file, which is what
    // could not be written at all before: the surveil fires either way, one
    // turn apart, and by turn 2 it has looked either way.
    let binned = "a Bolt binned by turn 2";
    assert!((percent(&surveil, binned) - percent(&untapped, binned)).abs() < 0.01);
    assert!(percent(&surveil, binned) > 0.0);

    // The turn-2 answer reverses, which is the Lantern tradeoff rather than a
    // rounding artefact: playing the tapland first costs turn 1 and pays for
    // itself by turn 2, because the surveil dug a card deeper before the draw.
    let turn_two = "Lantern castable on turn 2";
    assert!(
        percent(&surveil, turn_two) > percent(&untapped, turn_two),
        "filtering first should be ahead by turn 2: {} vs {}",
        percent(&surveil, turn_two),
        percent(&untapped, turn_two)
    );
}

#[test]
fn a_run_that_resolved_a_land_drop_by_policy_says_which_policy() {
    // The non-negotiable half of #54. A number that depended on a declared
    // priority and did not name it is indistinguishable from one the tool
    // decided for you, which is the failure this whole project is written
    // against — so the list is on stderr and in the JSON, beside the
    // tapped-ness assumptions it is the sibling of.
    let out = run_hand_twelve("hand-12-surveil-first.criteria.toml");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("land drop"), "{stderr}");
    assert!(stderr.contains("otag:surveil"), "names the list: {stderr}");
    assert!(
        stderr.contains("then any other land"),
        "names the tier nobody wrote: {stderr}"
    );
    assert!(stderr.contains("Ties:"), "states the tie rule: {stderr}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["land_drop"]["prefer"],
        serde_json::json!(["otag:surveil"])
    );
    assert!(json["land_drop"]["tie_break"].is_string(), "{json}");

    // And a run that declared none does not grow the field: "nobody arbitrated
    // this" is a different fact from "the list was empty".
    let quiet: serde_json::Value =
        serde_json::from_slice(&run_loam("loam-yard.criteria.toml").stdout).unwrap();
    assert!(quiet.get("land_drop").is_none(), "{quiet}");
}

#[test]
fn both_engines_answer_a_declared_land_drop_the_same_way() {
    // The sampler walks the same `Board`, so it learned the policy by learning
    // nothing: a disagreement here is a disagreement about how a path is
    // produced rather than about which land was played.
    let exact: serde_json::Value =
        serde_json::from_slice(&run_hand_twelve("hand-12-surveil-first.criteria.toml").stdout)
            .unwrap();
    let sampled_out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("hand-12.txt"))
        .arg(fixture("hand-12-surveil-first.criteria.toml"))
        .arg("--index")
        .arg(fixture("lantern-index.jsonl"))
        .arg("--simulate")
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&sampled_out.stdout).unwrap();
    for name in [
        "Lantern castable on turn 1",
        "Lantern castable on turn 2",
        "a Bolt binned by turn 2",
    ] {
        let (a, b) = (percent(&exact, name), percent(&sampled, name));
        assert!((a - b).abs() < 0.5, "{name}: exact {a} vs sampled {b}");
    }
}

#[test]
fn a_preference_that_names_no_land_here_says_so_and_still_answers() {
    // The same treatment a criteria query matching nothing gets: a fact about
    // the deck rather than an error in the file. The tier below it does the
    // work, and the run does not pretend the list decided anything it did not.
    let out = run_hand_twelve("hand-12-idle-preference.criteria.toml");
    assert!(out.status.success(), "should answer");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("matches no land in this deck"), "{stderr}");
}

#[test]
fn lands_drawn_and_lands_played_agree_when_the_turn_allows_both() {
    // Worked rather than assumed, because it is the reason none of the numbers
    // in this file moved. Every land clause in the fixtures asks for N lands by
    // turn T with N <= T, and by turn T there have been T drops — so "two lands
    // drawn" and "two lands played" are the same set of hands. The two clauses
    // below are the same query on the same turn in different zones, and they
    // come back equal to the last digit.
    let out = run("commander-on-two.criteria.toml");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let drawn = percent(&json, "two lands drawn by turn 2");
    let played = percent(&json, "two lands in play by turn 2");
    assert_eq!(drawn, played, "{json}");
    assert!((drawn - 86.10).abs() < 0.01, "{drawn}");
}

#[test]
fn both_engines_answer_a_mana_question_the_same_way() {
    // The sampler walks the same `Board`, so a disagreement here is a
    // disagreement about how a path is produced rather than about what the
    // lands did. 200,000 hands of a nine-card library put the standard error
    // at 0.07 points.
    let exact: serde_json::Value =
        serde_json::from_slice(&run_mana("hand-7.txt", "hands-6-and-7.criteria.toml").stdout)
            .unwrap();
    let sampled_out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("hand-7.txt"))
        .arg(fixture("hands-6-and-7.criteria.toml"))
        .arg("--index")
        .arg(fixture("mana-index.jsonl"))
        .arg("--simulate")
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&sampled_out.stdout).unwrap();
    for name in [
        "{W}{U} payable on turn 3",
        "{W}{U} payable on turn 1",
        "{1} payable on turn 1",
    ] {
        let (a, b) = (percent(&exact, name), percent(&sampled, name));
        assert!((a - b).abs() < 0.5, "{name}: exact {a} vs sampled {b}");
    }
}

#[test]
fn an_easy_question_does_not_pay_for_a_hard_one_beside_it() {
    // #31, end to end and in the form the issue put it: refusing an easy
    // question because it shares a file with a hard one is not honest, and
    // neither is estimating it. The same two questions are asked twice — once
    // beside a criterion nothing can enumerate, once alone — and the numbers
    // have to be the same numbers, not close ones.
    let beside = run_flags("too-wide.criteria.toml", &[]);
    let alone = run_flags("too-wide-alone.criteria.toml", &[]);
    assert!(beside.status.success() && alone.status.success());
    let beside: serde_json::Value = serde_json::from_slice(&beside.stdout).unwrap();
    let alone: serde_json::Value = serde_json::from_slice(&alone.stdout).unwrap();

    assert_eq!(beside["method"], "mixed");
    assert_eq!(alone["method"], "exact", "nothing here is wide at all");

    let find = |json: &serde_json::Value, kind: &str, name: &str| -> serde_json::Value {
        json[kind]
            .as_array()
            .expect("an array")
            .iter()
            .find(|q| q["name"] == name)
            .unwrap_or_else(|| panic!("no {kind} named {name}"))
            .clone()
    };

    let shared = find(&beside, "criteria", "a land by turn 6");
    let solo = find(&alone, "criteria", "a land by turn 6");
    assert_eq!(shared["method"], "exact", "it was never the wide one");
    assert_eq!(shared["probability"], solo["probability"]);
    assert!(
        shared["standard_error"].is_null(),
        "an enumerated answer has no error to report"
    );

    let shared = find(&beside, "expectations", "lands by turn 6");
    let solo = find(&alone, "expectations", "lands by turn 6");
    assert_eq!(shared["method"], "exact");
    assert_eq!(shared["mean"], solo["mean"]);
    assert_eq!(shared["distribution"], solo["distribution"]);
}

#[test]
fn a_criterion_correlating_two_turns_is_not_narrowed_away() {
    // The narrowing collapses the draws between two turns nobody reads. A
    // criterion that reads both is the case where that would answer a
    // different question, so it keeps them — and the way to see that it did is
    // that the conjunction comes out strictly below either half, which a
    // collapsed enumeration could not produce.
    let exact = run_flags("cross-turn.criteria.toml", &[]);
    assert!(exact.status.success());
    let exact: serde_json::Value = serde_json::from_slice(&exact.stdout).unwrap();
    assert_eq!(exact["method"], "exact");

    let both = percent(&exact, "two lands on turn 1, three by turn 3");
    let late = percent(&exact, "three lands by turn 3");
    let early = percent(&exact, "two lands on turn 1");
    assert!(both < late, "{both} vs {late}");
    assert!(both < early, "{both} vs {early}");

    // And the independent check: the sampler walks real hands turn by turn and
    // has no notion of a narrowing at all, so it is the oracle for whether the
    // enumerated number is the right one.
    let sampled = run_flags(
        "cross-turn.criteria.toml",
        &["--simulate", "--trials", "200000", "--seed", "7"],
    );
    let sampled: serde_json::Value = serde_json::from_slice(&sampled.stdout).unwrap();
    for c in sampled["criteria"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let got = c["percent"].as_f64().unwrap();
        let se = c["standard_error"].as_f64().unwrap() * 100.0;
        let want = percent(&exact, name);
        assert!(
            (got - want).abs() < 4.0 * se,
            "{name}: sampled {got} vs enumerated {want}, {:.2} SE away",
            (got - want).abs() / se
        );
    }
}

/// A run's own account of how it enumerated, as the JSON carries it.
fn enumerations(json: &serde_json::Value) -> &Vec<serde_json::Value> {
    json["enumerations"]
        .as_array()
        .expect("every run reports how it enumerated")
}

#[test]
fn a_cost_is_enumerated_on_the_colours_it_demands() {
    // #55 end to end. Twelve cards in five mana profiles, asked whether
    // `{1}{U}` could have been paid: three of those profiles make no blue, so
    // they are one source to this question however differently they read on
    // the card. The run says which colours it kept and how wide that left it.
    let out = run_with(
        "manabase.txt",
        "blue-on-three.criteria.toml",
        "mana-index.jsonl",
    );
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["method"], "exact");

    let walked = enumerations(&json);
    assert_eq!(walked.len(), 1, "one question, one enumeration");
    assert_eq!(walked[0]["pips"], serde_json::json!(["{U}"]));
    assert_eq!(walked[0]["groups"], 5);
    assert_eq!(walked[0]["method"], "exact");
    assert_eq!(
        walked[0]["criteria"],
        serde_json::json!(["{1}{U} on turn 3"])
    );

    // The independent check, and the only one that is not this engine
    // agreeing with itself: the sampler deals real hands over the un-narrowed
    // manabase and knows nothing about any of this.
    let sampled = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("manabase.txt"))
        .arg(fixture("blue-on-three.criteria.toml"))
        .arg("--index")
        .arg(fixture("mana-index.jsonl"))
        .args(["--simulate", "--trials", "400000", "--seed", "3"])
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&sampled.stdout).unwrap();
    let name = "{1}{U} on turn 3";
    let (exact, estimate) = (percent(&json, name), percent(&sampled, name));
    let se = sampled["criteria"][0]["standard_error"].as_f64().unwrap() * 100.0;
    assert!(
        (exact - estimate).abs() < 4.0 * se,
        "narrowed {exact} vs sampled {estimate}, {:.2} SE away",
        (exact - estimate).abs() / se
    );
}

#[test]
fn a_declared_priority_is_enumerated_on_the_whole_palette() {
    // The negative control at the boundary that decides it. The same cost
    // over the same manabase, with the land drop declared: the priority plays
    // the first land it is holding and ranks two lands this cost cannot tell
    // apart, so merging them would change which one was played. The run keeps
    // all six pips and pays for them.
    let out = run_with(
        "manabase.txt",
        "blue-on-three-ranked.criteria.toml",
        "mana-index.jsonl",
    );
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let walked = enumerations(&json);
    assert_eq!(
        walked[0]["pips"],
        serde_json::json!(["{W}", "{U}", "{B}", "{R}", "{G}", "{C}"]),
        "a priority reads the manabase for a reason no cost can state"
    );
    let ranked = walked[0]["groups"].as_u64().unwrap();

    let narrowed = run_with(
        "manabase.txt",
        "blue-on-three.criteria.toml",
        "mana-index.jsonl",
    );
    let narrowed: serde_json::Value = serde_json::from_slice(&narrowed.stdout).unwrap();
    assert!(
        ranked > enumerations(&narrowed)[0]["groups"].as_u64().unwrap(),
        "and it costs groups, which is the price of being right"
    );

    // Still the right answer for the question it is now asking, which is a
    // different question: the drops are the ones the file declared.
    let sampled = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("manabase.txt"))
        .arg(fixture("blue-on-three-ranked.criteria.toml"))
        .arg("--index")
        .arg(fixture("mana-index.jsonl"))
        .args(["--simulate", "--trials", "400000", "--seed", "3"])
        .output()
        .expect("binary should run");
    let sampled: serde_json::Value = serde_json::from_slice(&sampled.stdout).unwrap();
    let name = "{1}{U} on turn 3";
    let (exact, estimate) = (percent(&json, name), percent(&sampled, name));
    let se = sampled["criteria"][0]["standard_error"].as_f64().unwrap() * 100.0;
    assert!(
        (exact - estimate).abs() < 4.0 * se,
        "declared {exact} vs sampled {estimate}, {:.2} SE away",
        (exact - estimate).abs() / se
    );
}

#[test]
fn a_run_says_how_it_enumerated_question_by_question() {
    // The other half of #55, and the same argument as the provenance block:
    // since #31 a file is several enumerations and only the widest *refused*
    // one reached the output, so the group and composition counts this project
    // quotes about its own narrowings could not be reproduced from a run that
    // performed them.
    let out = run("simple-ramp.criteria.toml");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let walked = enumerations(&json);
    assert!(walked.len() > 1, "this file is several questions");

    // Every question in the file is answered by exactly one of them.
    let mut answered: Vec<&str> = Vec::new();
    for class in walked {
        assert!(class["groups"].as_u64().unwrap() >= 1);
        assert!(class["compositions"].as_f64().unwrap() >= 1.0);
        assert_eq!(class["method"], "exact");
        assert!(
            class["pips"].is_null(),
            "nothing here asks whether a cost could be paid"
        );
        for kind in ["criteria", "expectations"] {
            for name in class[kind].as_array().unwrap() {
                answered.push(name.as_str().unwrap());
            }
        }
    }
    let mut asked: Vec<&str> = json["criteria"]
        .as_array()
        .unwrap()
        .iter()
        .chain(json["expectations"].as_array().unwrap())
        .map(|q| q["name"].as_str().unwrap())
        .collect();
    asked.sort_unstable();
    answered.sort_unstable();
    assert_eq!(asked, answered, "every question, enumerated exactly once");

    // And the narrowing it reports is the one it performed: a class keeps the
    // queries it reads and no others.
    let opener = walked
        .iter()
        .find(|c| c["criteria"] == serde_json::json!(["keepable opener (2-5 lands)"]))
        .expect("the opener criterion has an enumeration of its own");
    assert_eq!(opener["queries"], serde_json::json!(["t:land"]));
    assert_eq!(opener["turns"], serde_json::json!([0]));
    assert_eq!(opener["reading"], "cumulative");
    assert_eq!(opener["groups"], 2, "lands and everything else");
}

#[test]
fn a_sampled_question_still_says_what_it_would_have_cost() {
    // A class that went over the ceiling is the one a caller most needs the
    // width of, and the one a run used to describe only in prose. It reports
    // the width it would have walked and `"sampled"` beside it, so the reason
    // it is an estimate is a number rather than an adjective.
    let out = run_flags("too-wide.criteria.toml", &[]);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["method"], "mixed");
    let walked = enumerations(&json);
    let refused: Vec<&serde_json::Value> =
        walked.iter().filter(|c| c["method"] == "sampled").collect();
    assert_eq!(refused.len(), 1, "one class went over, not the file");
    assert!(
        refused[0]["compositions"].as_f64().unwrap()
            > json["too_wide"]["ceiling"].as_f64().unwrap()
    );
    // The widest refused class is what the top-level `too_wide` describes, so
    // the two have to agree rather than be two measurements of one run.
    assert_eq!(refused[0]["compositions"], json["too_wide"]["paths"]);
    assert_eq!(refused[0]["groups"], json["too_wide"]["groups"]);
}

// --- The declared budget (#10) --------------------------------------------

fn run_budget(deck: &str) -> std::process::Output {
    run_with(deck, "six-opts.criteria.toml", "budget-index.jsonl")
}

#[test]
fn one_island_and_six_opts_casts_one_opt_end_to_end() {
    // HANDS.md hands 1, 2 and 3, which are one test rather than three: the same
    // file against three seven-card hands that differ by a single card, and the
    // claim is that the count of castings moves while the count of cards in
    // hand does not.
    //
    // Hand 1 plays the Island, taps it and casts one Opt. Hand 2 swaps the
    // Island for Undercity Sewers, which enters tapped, and casts none. Hand 3
    // holds seven Opts and no land, and does nothing at all. All three hold six
    // or more Opts in the opening hand, which is the number a model that fired
    // on the holding would have reported as castings.
    let hands = [
        ("hand-1.txt", 100.0, 1.0),
        ("hand-2.txt", 0.0, 0.0),
        ("hand-3.txt", 0.0, 0.0),
    ];
    for (deck, first, mean) in hands {
        let out = run_budget(deck);
        assert!(
            out.status.success(),
            "{deck} should answer: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(
            (percent(&json, "an Opt cast on turn 1") - first).abs() < 0.01,
            "{deck}: an Opt on turn 1 is {}",
            percent(&json, "an Opt cast on turn 1")
        );
        assert_eq!(
            percent(&json, "two Opts cast on turn 1"),
            0.0,
            "{deck}: one land is never two Opts"
        );
        assert_eq!(
            percent(&json, "six Opts in the opening hand"),
            100.0,
            "{deck}: the cards are all there, which is what holding them is not"
        );
        assert!(
            (expectation(&json, "Opts cast by turn 1")["mean"]
                .as_f64()
                .unwrap()
                - mean)
                .abs()
                < 1e-9,
            "{deck}: mean castings"
        );
    }
}

#[test]
fn the_budget_agrees_with_the_sampler() {
    // The oracle. A budget is a new thing for the walk to do on every path, so
    // it is a new way for the two engines to disagree — and they share the
    // board, so a disagreement here would be about how a path is produced
    // rather than about what happens along it.
    for deck in ["hand-1.txt", "hand-2.txt", "hand-3.txt"] {
        let exact: serde_json::Value = serde_json::from_slice(&run_budget(deck).stdout).unwrap();
        let sampled = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .arg("test")
            .arg(fixture(deck))
            .arg(fixture("six-opts.criteria.toml"))
            .arg("--index")
            .arg(fixture("budget-index.jsonl"))
            .arg("--simulate")
            .arg("--trials")
            .arg("20000")
            .output()
            .expect("binary should run");
        let sampled: serde_json::Value = serde_json::from_slice(&sampled.stdout).unwrap();
        for name in [
            "an Opt cast on turn 1",
            "two Opts cast on turn 1",
            "six Opts in the opening hand",
        ] {
            assert!(
                (percent(&exact, name) - percent(&sampled, name)).abs() < 1.0,
                "{deck}, {name}: {} exact against {} sampled",
                percent(&exact, name),
                percent(&sampled, name)
            );
        }
    }
}

#[test]
fn a_run_that_cast_by_policy_says_which_policy() {
    // The non-negotiable half, and the same one #54 has: a number that turned
    // on a declared priority and did not name it is indistinguishable from one
    // the tool decided for you. The budget's version carries one extra claim,
    // because its list is not a preference over the whole deck — a spell it
    // does not name is not cast, and no reader could work that out from the
    // percentage.
    let out = run_budget("hand-1.txt");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("spells cast here are decided"), "{stderr}");
    assert!(
        stderr.contains("name:\\\"Opt\\\""),
        "names the list: {stderr}"
    );
    assert!(
        stderr.contains("is not cast at all"),
        "states what silence means: {stderr}"
    );
    assert!(stderr.contains("Ties:"), "states the tie rule: {stderr}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["casting"]["prefer"],
        serde_json::json!(["name:\"Opt\""])
    );
    assert!(json["casting"]["tie_break"].is_string(), "{json}");

    // And a run that declared none has no field at all, which is the other
    // fact: it cast nothing, rather than casting by some default.
    let plain: serde_json::Value =
        serde_json::from_slice(&run("simple-ramp.criteria.toml").stdout).unwrap();
    assert!(plain.get("casting").is_none(), "{plain}");
}

#[test]
fn counting_castings_with_no_priority_is_refused_with_the_remedy_named() {
    // The budget's version of the land drop's refusal, over the other
    // resource. Which spell you cast out of one turn's mana is a decision, and
    // a tool that picked would report a line nobody chose.
    let out = run_with(
        "hand-1.txt",
        "cast-no-priority.criteria.toml",
        "budget-index.jsonl",
    );
    assert!(!out.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("[casting]"), "names the remedy: {stderr}");
    assert!(stderr.contains("prefer"), "{stderr}");
    assert!(
        stderr.contains("one Opt cast"),
        "names the question that asked: {stderr}"
    );
}

#[test]
fn the_enumerations_block_names_the_colours_the_deck_made_it_read() {
    // The width a budget costs, said out loud. The criteria file names no
    // colour at all — it asks how many Opts were cast — and the enumeration
    // still tells a blue source from every other land, because the *card data*
    // says Opt costs {U}. That is a grouping decision nothing in the file
    // states, which is exactly the kind this block exists to report.
    let json: serde_json::Value = serde_json::from_slice(&run_budget("hand-1.txt").stdout).unwrap();
    let enumerations = json["enumerations"].as_array().unwrap();
    assert!(!enumerations.is_empty());
    for class in enumerations {
        assert_eq!(class["pips"], serde_json::json!(["{U}"]), "{class}");
        assert_eq!(class["reading"], "per-turn", "a budget reads the turns");
        assert_eq!(class["method"], "exact");
    }
}

// --- tutors -----------------------------------------------------------------

fn run_tutor(deck: &str, criteria: &str) -> std::process::Output {
    run_with(deck, criteria, "tutor-index.jsonl")
}

#[test]
fn a_tutor_fetches_the_card_it_names_and_the_run_says_what_it_fetched() {
    // #18's cheap half, and **the pair is the test rather than either half of
    // it**. One deck, two criteria files differing only by an `[[effect]]`
    // block, three numbers: one that must not move, one that must, and one
    // that must move the other way.
    //
    // Twelve cards, four Islands, one Trinket Mage at `{2}{U}` and one Lantern
    // of Insight at `{1}`. Turn 4 on the play has seen ten of the twelve, so a
    // named card is still in the library on exactly 2/12 of deals — which is
    // the third row, and it is the row the fetch empties.
    let off: serde_json::Value =
        serde_json::from_slice(&run_tutor("hand-tutor.txt", "tutor-off.criteria.toml").stdout)
            .unwrap();
    let out = run_tutor("hand-tutor.txt", "tutor-on.criteria.toml");
    let on: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    // The tutor is cast just as often either way. It has to be: what it does
    // when it resolves cannot change whether the pool paid for it.
    let cast = "Trinket Mage cast by turn 4";
    assert_eq!(percent(&off, cast), 74.24);
    assert_eq!(
        percent(&on, cast),
        74.24,
        "casting it cannot depend on this"
    );
    // And the line it exists for does move, by more than a rounding error.
    let both = "Trinket Mage and a Lantern both cast by turn 4";
    assert_eq!(percent(&off, both), 59.09, "drawing both halves");
    assert_eq!(percent(&on, both), 71.82, "fetching the second one");
    // The other half of a fetch, and the half that needed the population to
    // stop being fixed: the card is gone from the library. 2/12 is 16.67%.
    let left = "Lantern of Insight still in the library on turn 4";
    assert_eq!(percent(&off, left), 16.67);
    assert_eq!(
        percent(&on, left),
        1.52,
        "all but the deals that cast nothing"
    );

    // And the run names the policy it used, because a number that hinged on a
    // declared priority and did not name it is the bug this project exists to
    // prevent.
    let effect = &on["effects"][0];
    assert_eq!(effect["fetch"][0], "name:\"Lantern of Insight\"");
    assert_eq!(effect["to"], "hand");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("fetches, to your hand"),
        "should say so on stderr too: {stderr}"
    );
    assert!(
        stderr.contains("Lantern of Insight"),
        "and name what it went and got: {stderr}"
    );
    // A file that declares no tutor has no `effects` entry at all, which is the
    // other fact: that run fetched nothing.
    assert_eq!(off["effects"].as_array().expect("array").len(), 0);
}

#[test]
fn a_fetchland_leaves_the_battlefield_and_thins_the_library() {
    // The same pair over the other trigger, and it is the claim the deck-
    // thinning argument turns on. Twelve cards: two Misty Rainforests, five
    // basics, five Bolts.
    //
    // Three rows again. The fetchland is **drawn** just as often either way —
    // it is a card, and declaring what it does cannot change when it turns up.
    // It is **on the battlefield** far less often, because it sacrificed
    // itself. And the basics left in the library fall, which is the thinning.
    let off: serde_json::Value =
        serde_json::from_slice(&run_tutor("hand-fetchland.txt", "fetch-off.criteria.toml").stdout)
            .unwrap();
    let on: serde_json::Value =
        serde_json::from_slice(&run_tutor("hand-fetchland.txt", "fetch-on.criteria.toml").stdout)
            .unwrap();

    let drawn = "a fetchland drawn by turn 3";
    assert_eq!(percent(&off, drawn), 95.45);
    assert_eq!(
        percent(&on, drawn),
        95.45,
        "a card is drawn when it is drawn"
    );
    // Not zero: with two fetchlands and five basics in twelve cards, the
    // basics sometimes run out, and a tutor that finds nothing fetches
    // nothing — so the fetchland stays where it is.
    let played = "a fetchland on the battlefield by turn 3";
    assert_eq!(percent(&off, played), 95.45);
    assert_eq!(
        percent(&on, played),
        18.48,
        "cracked unless there is nothing left"
    );
    // And a land drop is still a land drop: whatever is standing there on turn
    // 1, something is.
    let land = "a land in play on turn 1";
    assert_eq!(percent(&off, land), 100.0);
    assert_eq!(percent(&on, land), 100.0);

    let basics = "basics left in the library on turn 3";
    let mean = |j: &serde_json::Value| expectation(j, basics)["mean"].as_f64().unwrap();
    assert!((mean(&off) - 1.25).abs() < 1e-9, "was {}", mean(&off));
    assert!(
        mean(&on) < mean(&off) - 0.5,
        "the thinning, which is the whole argument: {} against {}",
        mean(&on),
        mean(&off)
    );
}

#[test]
fn a_tutor_that_nothing_would_fire_is_refused_by_name() {
    // Both triggers, and the same argument each way: a fetch happens at a
    // point in the game the run has to be able to name. A land-drop fetch
    // replaces the land that made the drop, so a run with no declared priority
    // cannot say what it fetched; a cast fetch fires when the declared line
    // casts the card, so with no line there is nothing to fire it.
    let drop = run_tutor("hand-fetchland.txt", "fetch-no-land-drop.criteria.toml");
    assert!(!drop.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&drop.stderr);
    assert!(
        stderr.contains("[land_drop]") && stderr.contains("otag:fetchland"),
        "names the remedy and the effect: {stderr}"
    );

    let cast = run_tutor("hand-tutor.txt", "fetch-no-casting.criteria.toml");
    assert!(!cast.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&cast.stderr);
    assert!(
        stderr.contains("[casting]") && stderr.contains("Trinket Mage"),
        "names the remedy and the effect: {stderr}"
    );
}

#[test]
fn a_fetched_land_is_counted_and_not_tapped_for() {
    // The line between the half of a fetchland this engine answers exactly and
    // the half it will not answer at all. What left the library is a
    // subtraction; what the land it found taps for on the turn it arrives is a
    // fact about the *spell* that fetched it — Scalding Tarn untapped,
    // Terramorphic Expanse tapped — and `otag:fetchland` holds both.
    let out = run_tutor("hand-fetchland.txt", "fetch-mana.criteria.toml");
    assert!(!out.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Terramorphic Expanse"),
        "names the card that makes it a real distinction: {stderr}"
    );
    // And the other refusal at the same seam: a battlefield fetch may only
    // name lands, because a spell has to be cast to get there.
    let spell = run_tutor("hand-fetchland.txt", "fetch-non-land.criteria.toml");
    assert!(!spell.status.success(), "should refuse");
    let stderr = String::from_utf8_lossy(&spell.stderr);
    assert!(
        stderr.contains("Lightning Bolt"),
        "names the card it cannot put there: {stderr}"
    );
}
