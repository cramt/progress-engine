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
