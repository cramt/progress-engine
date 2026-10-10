use std::collections::HashMap;

use chip_decklist::export::{
    export_cardmarket, export_cockatrice, export_tabletop_simulator, ExportError,
};

const DECK: &str = r#"cards = [
  { printing = "moc/94", in = ["Commander"] },
  { printing = "c21/263", in = ["Ramp"] },
  { name = "Forest", qty = 5, in = ["Land"] },
  { printing = "plst/LTR-123", in = ["Ramp"] },
  { printing = "sld/263a", in = ["Ramp"] },
  { name = "Forest", qty = 2, in = ["Sideboard"] },
  { name = "Fire // Ice", in = ["Maybe"] },
  { name = "Lurrus of the Dream-Den", in = ["Companion"] },
]
[categories]
Commander = { type = "commander" }
Sideboard = { type = "sideboard" }
Maybe = { type = "maybeboard" }
Companion = { type = "companion" }
Ramp = {}
Land = {}
"#;

fn names() -> HashMap<String, String> {
    [
        ("moc/94", "Rashmi and Ragavan"),
        ("c21/263", "Sol Ring"),
        ("plst/LTR-123", "Delver of Secrets // Insectile Aberration"),
        ("sld/263a", "Sol Ring"),
    ]
    .into_iter()
    .map(|(p, n)| (p.to_string(), n.to_string()))
    .collect()
}

#[test]
fn cockatrice_gets_its_printings_and_the_commander_in_the_sideboard() {
    // A number Cockatrice would misread, `LTR-123` or `263a`, goes by name;
    // the maybeboard is not part of the deck it plays.
    assert_eq!(
        export_cockatrice(DECK, &names()).unwrap(),
        "1 Sol Ring (C21) 263\n\
         5 Forest\n\
         1 Delver of Secrets // Insectile Aberration\n\
         1 Sol Ring\n\
         \n\
         SB: 1 Rashmi and Ragavan (MOC) 94\n\
         SB: 2 Forest\n\
         SB: 1 Lurrus of the Dream-Den\n"
    );
}

#[test]
fn cardmarket_gets_one_line_a_card_with_the_copies_summed() {
    assert_eq!(
        export_cardmarket(DECK, &names()).unwrap(),
        "1 Rashmi and Ragavan\n\
         2 Sol Ring\n\
         7 Forest\n\
         1 Delver of Secrets // Insectile Aberration\n\
         1 Lurrus of the Dream-Den\n"
    );
}

#[test]
fn tabletop_simulator_gets_every_printing_and_a_pile_for_the_commander() {
    // The companion sits with the sideboard: the importer's own `Companion`
    // heading would pile it with the commander.
    assert_eq!(
        export_tabletop_simulator(DECK, &names()).unwrap(),
        "Commander\n\
         1 Rashmi and Ragavan (MOC) 94\n\
         \n\
         Deck\n\
         1 Sol Ring (C21) 263\n\
         5 Forest\n\
         1 Delver of Secrets // Insectile Aberration (PLST) LTR-123\n\
         1 Sol Ring (SLD) 263a\n\
         \n\
         Sideboard\n\
         2 Forest\n\
         1 Lurrus of the Dream-Den\n"
    );
}

#[test]
fn tabletop_simulator_leaves_out_an_empty_section() {
    let deck = "cards = [{ name = \"Sol Ring\", in = [\"Ramp\"] }]\n[categories]\nRamp = {}\n";
    assert_eq!(
        export_tabletop_simulator(deck, &HashMap::new()).unwrap(),
        "Deck\n1 Sol Ring\n"
    );
}

#[test]
fn a_printing_with_no_name_is_refused_by_every_writer() {
    for export in [
        export_cockatrice,
        export_cardmarket,
        export_tabletop_simulator,
    ] {
        let err = export(DECK, &HashMap::new()).unwrap_err();
        assert!(
            matches!(err, ExportError::Unnamed(ref p) if p.len() == 4),
            "{err}"
        );
    }
}
