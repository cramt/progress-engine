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
    Command::new(env!("CARGO_BIN_EXE_progress-engine"))
        .arg("test")
        .arg(fixture("simple-ramp.txt"))
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
