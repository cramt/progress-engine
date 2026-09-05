//! Reducing Scryfall's bulk data to the index, tested against real records.
//!
//! The fixture is fifteen lines lifted verbatim out of the oracle-cards bulk
//! file, unknown fields and all. Hand-written approximations would test this
//! crate against a second opinion about what Scryfall publishes, which is the
//! mistake that produced the bug in the first place — so these are the actual
//! bytes, including the three token records that shadow a real card.

use pe_scryfall::bulk::{strip_reminder_text, Anomaly, BulkCard};
use pe_scryfall::index::{Index, IndexFile};

const SAMPLE: &str = include_str!("fixtures/bulk-sample.jsonl");

fn sample() -> Vec<BulkCard> {
    SAMPLE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| facet_json::from_str(line).expect("real bulk record should parse"))
        .collect()
}

fn built() -> Index {
    Index::build(sample(), Some("2026-09-04".into())).0
}

#[test]
fn a_real_bulk_record_parses_with_its_sixty_unknown_fields_intact() {
    let records = sample();
    assert_eq!(records.len(), 15);
    let sol_ring = records
        .iter()
        .find(|c| c.name == "Sol Ring")
        .expect("fixture has Sol Ring");
    assert_eq!(sol_ring.cmc, Some(1.0));
    assert_eq!(
        sol_ring.produced_mana.as_deref(),
        Some(&["C".to_string()][..])
    );
}

/// Issue #38, as a regression test.
///
/// Scryfall prints tokens that share a name with a real card. Keyed by name
/// they collide, and whichever was written last won — which is how an index
/// came to report Llanowar Elves as a mana value 0 token that is not legal in
/// Commander. Filtering on `layout` removes the collision at its source.
#[test]
fn a_token_never_shadows_the_card_it_is_named_after() {
    let index = built();
    for (name, cmc, type_line) in [
        ("Llanowar Elves", 1.0, "Creature — Elf Druid"),
        ("Mutavault", 0.0, "Land"),
        ("Tarmogoyf", 2.0, "Creature — Lhurgoyf"),
    ] {
        let card = index.get(name).expect("the real card should be indexed");
        assert_eq!(card.cmc, cmc, "{name} should have the card's mana value");
        assert_eq!(card.type_line, type_line, "{name}");
        assert!(
            !card.type_line.contains("Token"),
            "{name} resolved to the token"
        );
    }
    assert_eq!(
        index.get("Llanowar Elves").unwrap().legalities.commander(),
        pe_scryfall::legality::CommanderLegality::Legal,
        "the token is not legal in Commander and the card is"
    );
}

#[test]
fn every_record_is_accounted_for_as_either_kept_or_skipped() {
    let (_, report) = Index::build(sample(), None);
    let skipped: usize = report.skipped.iter().map(|(_, n)| n).sum();
    assert_eq!(report.read, 15);
    assert_eq!(report.kept + skipped, report.read);
    assert_eq!(skipped, 3, "three token records");
}

/// The whole point of the fixture being real: if these hold on the actual
/// bytes, the flattening below is not resting on an assumption about them.
#[test]
fn real_records_produce_no_anomalies() {
    let (_, report) = Index::build(sample(), None);
    assert_eq!(report.anomalies, Vec::<Anomaly>::new());
}

#[test]
fn a_single_faced_card_still_has_exactly_one_face() {
    let index = built();
    let sol_ring = index.get("Sol Ring").unwrap();
    assert_eq!(sol_ring.faces.len(), 1);
    assert_eq!(sol_ring.faces[0].name, "Sol Ring");
    assert_eq!(sol_ring.faces[0].mana_cost, "{1}");
}

/// The back of a transforming card has no mana cost, so Scryfall states its
/// colour with an indicator instead of a `colors` array. Reading only `colors`
/// would make Insectile Aberration a colourless blue creature.
#[test]
fn a_transforming_card_keeps_both_faces_and_the_back_faces_colour() {
    let index = built();
    let delver = index
        .get("Delver of Secrets // Insectile Aberration")
        .expect("indexed under its full name");
    assert_eq!(delver.faces.len(), 2);
    assert_eq!(delver.faces[0].power.as_deref(), Some("1"));
    assert_eq!(delver.faces[1].power.as_deref(), Some("3"));
    assert_eq!(delver.faces[1].mana_cost, "");
    assert_eq!(
        delver.faces[1].colors,
        vec!["U".to_string()],
        "the back face is blue by its colour indicator"
    );
    assert_eq!(delver.colors, vec!["U".to_string()]);
}

#[test]
fn a_split_card_joins_the_costs_and_the_text_of_both_halves() {
    let index = built();
    let wear = index.get("Wear // Tear").unwrap();
    assert_eq!(wear.mana_cost, "{1}{R} // {W}");
    assert!(wear.oracle.contains("Destroy target artifact"));
    assert!(wear.oracle.contains("Destroy target enchantment"));
    assert_eq!(wear.faces.len(), 2);
}

/// Scryfall's `o:` does not search reminder text and its `fo:` does. Without
/// the split, `o:flying` is satisfied by "(This creature can't be blocked
/// except by creatures with flying.)" — the README's opening complaint with a
/// different card in it.
#[test]
fn reminder_text_is_out_of_the_oracle_and_in_the_full_oracle() {
    let index = built();
    let angel = index.get("Serra Angel").unwrap();
    assert!(angel.oracle.contains("Vigilance"));
    assert!(
        !angel.oracle.contains("Attacking doesn't cause"),
        "reminder text should be out of `oracle`: {:?}",
        angel.oracle
    );
    assert!(
        angel.full_oracle().contains("Attacking doesn't cause"),
        "and in `full_oracle`"
    );
}

#[test]
fn a_card_with_no_reminder_text_does_not_store_its_oracle_twice() {
    let index = built();
    let sol_ring = index.get("Sol Ring").unwrap();
    assert_eq!(sol_ring.full_oracle, None);
    assert_eq!(sol_ring.full_oracle(), sol_ring.oracle);
}

#[test]
fn stripping_reminder_text_survives_nesting_and_refuses_to_guess_at_stray_brackets() {
    assert_eq!(strip_reminder_text("Flying (It flies.)"), "Flying");
    assert_eq!(strip_reminder_text("A (b (c) d) e"), "A e");
    // Unbalanced: returned whole rather than truncated. `o:` matching some
    // reminder text is a far smaller error than `o:` losing the rules text.
    let stray = "Morph (You may cast this face down.";
    assert_eq!(strip_reminder_text(stray), stray);
    let stray = "Turn it face up.)";
    assert_eq!(strip_reminder_text(stray), stray);
}

/// Scryfall has no field for the ten cards that lift the singleton rule, so it
/// is read off their own text — the one derivation in the module, and safe
/// because the sentence appears in exactly one form across all 38,626 records.
#[test]
fn the_singleton_exemption_is_read_off_the_card_that_grants_it() {
    let index = built();
    assert_eq!(index.get("Relentless Rats").unwrap().any_number, Some(true));
    assert_eq!(index.get("Sol Ring").unwrap().any_number, Some(false));
}

/// The bug the README opens with. Kor Haven's `{W}` is in an activation cost,
/// so any oracle-text search for a white source finds it; `produced_mana` says
/// it makes colourless and nothing else.
#[test]
fn produced_mana_is_what_a_card_makes_not_what_its_text_mentions() {
    let index = built();
    let kor_haven = index.get("Kor Haven").unwrap();
    assert!(
        kor_haven.oracle.contains("{W}"),
        "the trap is still in the text"
    );
    assert_eq!(kor_haven.produces, vec!["C".to_string()]);

    let birds = index.get("Birds of Paradise").unwrap();
    let mut produces = birds.produces.clone();
    produces.sort();
    assert_eq!(produces, ["B", "G", "R", "U", "W"]);
}

#[test]
fn the_index_records_the_shape_it_was_written_in() {
    let index = built();
    assert_eq!(index.schema(), pe_scryfall::index::SCHEMA);
    assert!(!index.is_stale());
    assert_eq!(index.updated_at.as_deref(), Some("2026-09-04"));

    // An index from before the field is schema 0, and says so rather than
    // claiming to carry fields nobody wrote.
    let old = IndexFile::parse(std::path::Path::new("<memory>"), "{}\n".into()).unwrap();
    assert_eq!(old.schema(), 0);
    assert!(old.is_stale());
}
