//! Preparing a run, in-process: the refusals that are made before a hand is
//! enumerated, and the notes said on the way, without spawning the binary.
//!
//! `cli.rs` proves the same refusals end to end, through stderr and the exit
//! status. These read them at the interface that makes them, so a refusal is
//! matched by the variant that names what could not be modelled, a refusal
//! can be told apart from a failure, and a note from the outcome it sits
//! beside.

use std::path::PathBuf;

use gauntlet_cli::prepare::{prepare, Preparation, Unprepared};
use gauntlet_cli::refusal::{QuerySite, Refusal};
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

/// The refusal that stopped a run, which must be a refusal and not a failure.
fn refusal(preparation: Preparation) -> Refusal {
    match preparation.run {
        Ok(_) => panic!("should have been refused"),
        Err(Unprepared::Refused(refusal)) => *refusal,
        Err(Unprepared::Failed(e)) => panic!("failed rather than refused: {e:#}"),
    }
}

#[test]
fn an_unparseable_query_is_refused_naming_the_question_that_asked() {
    let refused = refusal(prepared(
        "simple-ramp.txt",
        "badquery.criteria.toml",
        "index.jsonl",
    ));
    let Refusal::UnsupportedQuery {
        ref site,
        ref query,
        ..
    } = refused
    else {
        panic!("{refused:?}")
    };
    assert_eq!(site, &QuerySite::Question("unsupported".to_string()));
    assert!(query.contains("artist"), "{query}");
    assert!(
        refused.to_string().contains("unknown search key"),
        "{refused}"
    );
}

#[test]
fn a_keyword_the_index_does_not_list_is_refused() {
    let refused = refusal(prepared(
        "loam.txt",
        "kw-typo.criteria.toml",
        "loam-index.jsonl",
    ));
    let Refusal::UnknownKeyword {
        ref site,
        ref keywords,
        ..
    } = refused
    else {
        panic!("{refused:?}")
    };
    assert_eq!(site, &QuerySite::Question("a typoed keyword".to_string()));
    assert_eq!(keywords, &["flyign"]);
    let text = refused.to_string();
    assert!(
        text.starts_with("kw-typo.criteria.toml: a typoed keyword: in query "),
        "names the file, then the criterion: {text}"
    );
    assert!(text.contains("gauntlet sync"), "{text}");
}

#[test]
fn counting_castings_with_no_priority_is_refused_against_the_file() {
    let refused = refusal(prepared(
        "hand-1.txt",
        "cast-no-priority.criteria.toml",
        "budget-index.jsonl",
    ));
    let Refusal::CastingWithoutPriority { ref asked_by, .. } = refused else {
        panic!("{refused:?}")
    };
    assert_eq!(asked_by, "one Opt cast by turn 1");
    let text = refused.to_string();
    assert!(
        text.starts_with("cast-no-priority.criteria.toml: one Opt cast by turn 1: "),
        "names the file, then the question: {text}"
    );
    assert!(text.contains("[casting]"), "names the remedy: {text}");
}

#[test]
fn a_land_drop_tutor_with_no_land_drop_priority_is_refused() {
    let refused = refusal(prepared(
        "hand-fetchland.txt",
        "fetch-no-land-drop.criteria.toml",
        "tutor-index.jsonl",
    ));
    let Refusal::FetchWithoutLandDrop { ref effect, .. } = refused else {
        panic!("{refused:?}")
    };
    assert!(effect.contains("otag:fetchland"), "{effect}");
    let text = refused.to_string();
    assert!(
        text.starts_with("fetch-no-land-drop.criteria.toml: "),
        "{text}"
    );
    assert!(text.contains("[land_drop]"), "names the remedy: {text}");
}

#[test]
fn a_cast_tutor_with_no_casting_priority_is_refused() {
    let refused = refusal(prepared(
        "hand-tutor.txt",
        "fetch-no-casting.criteria.toml",
        "tutor-index.jsonl",
    ));
    assert!(
        matches!(refused, Refusal::FetchWithoutCasting { .. }),
        "{refused:?}"
    );
}

#[test]
fn a_battlefield_question_about_a_spell_names_the_spell() {
    let refused = refusal(prepared(
        "hand-saga.txt",
        "saga-off.criteria.toml",
        "tutor-index.jsonl",
    ));
    let Refusal::BattlefieldNonLand { ref spells, .. } = refused else {
        panic!("{refused:?}")
    };
    assert_eq!(spells, &["Lantern of Insight"]);
}

#[test]
fn a_mulligan_query_this_index_cannot_answer_is_refused_by_its_clause() {
    let refused = refusal(prepared(
        "simple-ramp.txt",
        "mulligan-unfetched-tag.criteria.toml",
        "index.jsonl",
    ));
    let Refusal::TagGap { ref site, .. } = refused else {
        panic!("{refused:?}")
    };
    assert!(matches!(site, QuerySite::Mulligan(_)), "{site:?}");
    assert!(
        refused
            .to_string()
            .starts_with("mulligan-unfetched-tag.criteria.toml: [mulligan]: keep clause "),
        "{refused}"
    );
}

#[test]
fn a_deck_with_no_library_is_refused_before_anything_is_walked() {
    let refused = refusal(prepared(
        "no-library.txt",
        "two-lands.criteria.toml",
        "index.jsonl",
    ));
    assert!(matches!(refused, Refusal::Infeasible { .. }), "{refused:?}");
    assert!(
        refused
            .to_string()
            .starts_with("two-lands.criteria.toml: the library is empty"),
        "{refused}"
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
