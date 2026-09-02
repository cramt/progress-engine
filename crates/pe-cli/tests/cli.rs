//! End-to-end tests driving the real binary against a fixture deck.
//!
//! The fixture index is checked in so this needs no network and no card cache:
//! CI must be able to prove the numbers without `scryfall sync` having run.
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

fn run(criteria: &str) -> std::process::Output {
    run_deck("simple-ramp.txt", criteria)
}

fn run_deck(deck: &str, criteria: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture(deck))
        .arg(fixture(criteria))
        .arg("--index")
        .arg(fixture("index.json"))
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
    let out = run("simple-ramp.criteria.js");
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
    let out = run("simple-ramp.criteria.js");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(percent(&json, "commander on turn 2") < percent(&json, "turn-1 accelerant"));
}

#[test]
fn the_verdict_goes_to_stderr() {
    // stdout is JSON only, so a caller piping it through jq cannot lose the
    // verdict. That has burned this project's predecessor.
    let out = run("simple-ramp.criteria.js");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("PASS: 3 of 3"), "stderr was: {stderr}");
    serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("stdout is pure JSON");
}

#[test]
fn a_missed_threshold_fails_the_run() {
    let out = run("impossible.criteria.js");
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
fn an_unparseable_query_names_itself() {
    let out = run("badquery.criteria.js");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    // A query that cannot be parsed must say so, never quietly match nothing.
    assert!(stderr.contains("power"), "stderr was: {stderr}");
}

#[test]
fn a_query_matching_no_cards_is_called_out() {
    // The defining failure mode: a misspelled category parses fine, matches
    // nothing, and yields a confident 0%. It cannot be a parse error, so it has
    // to be visible in the output instead.
    let out = run("typo.criteria.js");
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
fn the_query_breakdown_reports_real_match_counts() {
    // 36 Forests and 10 one-mana accelerants, which is checkable by eye against
    // the fixture decklist.
    let out = run("simple-ramp.criteria.js");
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
    let exact = run("simple-ramp.criteria.js");
    let exact: serde_json::Value = serde_json::from_slice(&exact.stdout).unwrap();

    let sampled = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
        .arg(fixture("simple-ramp.criteria.js"))
        .arg("--index")
        .arg(fixture("index.json"))
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
fn a_sampled_run_is_reproducible() {
    let go = |seed: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_progress-engine"))
            .arg("test")
            .arg(fixture("simple-ramp.txt"))
            .arg(fixture("simple-ramp.criteria.js"))
            .arg("--index")
            .arg(fixture("index.json"))
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
    let out = run("simple-ramp.criteria.js");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for c in json["criteria"].as_array().unwrap() {
        assert!(
            c.get("standard_error").is_none(),
            "exact runs must not report SE"
        );
    }
    assert!(json.get("trials").is_none());
}

#[test]
fn a_turn_behind_a_short_circuit_is_still_modelled() {
    // How deep into the game a file looks is discovered by running it, and `&&`
    // means an all-zero probe stops before the deepest turn. Getting this wrong
    // does not error: the unmodelled turn answers 0 for every composition and
    // the criterion reports a confident 0%, which is the failure this whole tool
    // exists to prevent.
    let hidden = run("short-circuit.criteria.js");
    let eager = run("eager.criteria.js");
    assert!(hidden.status.success(), "short-circuited form should pass");

    let hidden: serde_json::Value = serde_json::from_slice(&hidden.stdout).unwrap();
    let eager: serde_json::Value = serde_json::from_slice(&eager.stdout).unwrap();

    let got = percent(&hidden, "two lands by turn 2");
    assert!(got > 1.0, "collapsed to {got}%");
    assert_eq!(got, percent(&eager, "two lands by turn 2"));
}

#[test]
fn an_answer_does_not_depend_on_an_unrelated_criterion() {
    // The original symptom: an informational criterion that happened to reach a
    // later turn was dragging the run horizon out for everyone else, so deleting
    // it silently changed another criterion's answer.
    let alone = run("short-circuit.criteria.js");
    let alone: serde_json::Value = serde_json::from_slice(&alone.stdout).unwrap();
    let together = run("short-circuit-plus-deep.criteria.js");
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
        .arg(fixture("short-circuit.criteria.js"))
        .arg("--index")
        .arg(fixture("index.json"))
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
    let out = run_deck("outside-the-library.txt", "short-circuit.criteria.js");
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
    let out = run_deck("outside-the-library.txt", "short-circuit.criteria.js");
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
    let out = run_deck("outside-the-library.txt", "short-circuit.criteria.js");
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
