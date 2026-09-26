//! Preparing a run, in-process: the refusals that are made before a hand is
//! enumerated, and the notes said on the way, without spawning the binary.
//!
//! `cli.rs` proves the same refusals end to end, through stderr and the exit
//! status. These read them at the interface that makes them, so a refusal can
//! be asserted on exactly — wording, origin and all — and a note can be told
//! apart from the outcome it sits beside.

use std::path::PathBuf;

use gauntlet_cli::prepare::{prepare, Preparation};
use gauntlet_cli::Library;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Prepare `criteria` against `deck` on the play, with the criteria file named
/// by its bare file name so a refusal's origin is predictable.
fn prepared(deck: &str, criteria: &str, index: &str) -> Preparation {
    let library = Library::load(&fixture(deck), Some(&fixture(index))).expect("fixture deck loads");
    let source = std::fs::read_to_string(fixture(criteria)).expect("fixture criteria reads");
    let mut parsed =
        gauntlet_toml::Criteria::parse(&source, criteria).expect("fixture criteria parses");
    prepare(&library, &mut parsed, criteria, false)
}

fn refusal(preparation: Preparation) -> String {
    match preparation.run {
        Ok(_) => panic!("should have been refused"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn an_unparseable_query_is_refused_naming_the_question_that_asked() {
    let refused = refusal(prepared(
        "simple-ramp.txt",
        "badquery.criteria.toml",
        "index.jsonl",
    ));
    assert!(refused.contains("artist"), "{refused}");
    assert!(refused.contains("unsupported"), "{refused}");
}

#[test]
fn a_keyword_the_index_does_not_list_is_refused() {
    let refused = refusal(prepared(
        "loam.txt",
        "kw-typo.criteria.toml",
        "loam-index.jsonl",
    ));
    assert!(
        refused.starts_with("a typoed keyword: in query "),
        "names the criterion first: {refused}"
    );
    assert!(refused.contains("kw:flyign"), "{refused}");
    assert!(refused.contains("gauntlet sync"), "{refused}");
}

#[test]
fn counting_castings_with_no_priority_is_refused_against_the_file() {
    let refused = refusal(prepared(
        "hand-1.txt",
        "cast-no-priority.criteria.toml",
        "budget-index.jsonl",
    ));
    assert!(
        refused.starts_with("cast-no-priority.criteria.toml: one Opt cast by turn 1: "),
        "names the file, then the question: {refused}"
    );
    assert!(refused.contains("[casting]"), "names the remedy: {refused}");
}

#[test]
fn a_land_drop_tutor_with_no_land_drop_priority_is_refused() {
    let refused = refusal(prepared(
        "hand-fetchland.txt",
        "fetch-no-land-drop.criteria.toml",
        "tutor-index.jsonl",
    ));
    assert!(
        refused.starts_with("fetch-no-land-drop.criteria.toml: "),
        "{refused}"
    );
    assert!(
        refused.contains("[land_drop]") && refused.contains("otag:fetchland"),
        "names the remedy and the effect: {refused}"
    );
}

#[test]
fn a_hand_written_effect_matching_nothing_is_a_note_and_not_a_refusal() {
    let Preparation { notes, run } = prepared(
        "simple-ramp.txt",
        "unmatched-effect.criteria.toml",
        "index.jsonl",
    );
    let run = run.expect("a note does not stop the run");
    assert!(
        run.classes() > 0,
        "the prepared run has questions to answer"
    );
    assert!(
        notes
            .iter()
            .any(|n| n
                == "note: effect \"name:\\\"Undercity Sewers\\\"\" matched no cards in this deck"),
        "notes were: {notes:?}"
    );
}

#[test]
fn an_effect_library_blind_to_a_tagless_index_is_a_note_and_not_a_refusal() {
    // Nobody asked for the standard library's entries, so a tagless index
    // does not refuse the run over them. It moves numbers, though, so it is
    // said, and said as data the caller prints rather than on stderr here.
    let Preparation { notes, run } = prepared(
        "simple-ramp.txt",
        "simple-ramp.criteria.toml",
        "index.jsonl",
    );
    assert!(run.is_ok(), "{:?}", run.err());
    assert_eq!(notes.len(), 1, "notes were: {notes:?}");
    assert!(
        notes[0]
            .starts_with("note: this index carries no oracle tags, so 2 effect library entries"),
        "{}",
        notes[0]
    );
}
