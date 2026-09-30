//! Archidekt's text, in and out, pinned to what Archidekt itself does with it:
//! every row of docs/research/archidekt-import-shapes.md's tables.

use std::collections::{BTreeSet, HashMap};

use chip_decklist::deck::{
    export_archidekt, Card, CardRef, CategoryType, Deck, ExportError, ImportError, Printing,
};

use CategoryType::*;

/// The first card of `text` read as Archidekt reads it, and the deck around it.
fn import(text: &str) -> (Card, Deck) {
    let imported = Deck::read_archidekt(text);
    assert_eq!(imported.unreadable, vec![], "{text}");
    (imported.deck.cards[0].clone(), imported.deck)
}

fn categories(card: &Card) -> Vec<&str> {
    card.categories.iter().map(String::as_str).collect()
}

// --- Shape by shape: what the import makes of each Archidekt line ----------

#[test]
fn commander_with_top_is_a_commander() {
    let (kenrith, deck) = import("1x Kenrith, the Returned King [Commander{top}]\n4x Island");
    assert_eq!(kenrith.place, Commander);
    assert!(kenrith.in_deck(), "the commander is one of the 100");
    assert_eq!(categories(&kenrith), ["Commander"]);
    assert_eq!(deck.category("Commander").unwrap().kind, Some(Commander));
}

#[test]
fn commander_with_top_keeps_its_other_categories() {
    let (kenrith, deck) = import("1x Kenrith, the Returned King [Commander{top},Ramp]");
    assert_eq!(kenrith.place, Commander);
    assert_eq!(categories(&kenrith), ["Commander", "Ramp"]);
    assert_eq!(deck.category("Ramp").unwrap().kind, None);
}

#[test]
fn commander_without_top_is_a_plain_group() {
    let (kenrith, deck) = import("1x Kenrith, the Returned King [Commander]");
    assert_eq!(kenrith.place, InDeck);
    assert_eq!(deck.category("Commander").unwrap().kind, None);
}

#[test]
fn commander_not_first_is_not_a_commander() {
    let (kenrith, deck) = import("1x Kenrith, the Returned King [Ramp,Commander{top}]");
    assert_eq!(kenrith.place, InDeck);
    assert_eq!(categories(&kenrith), ["Ramp", "Commander"]);
    assert_eq!(
        deck.category("Commander").unwrap().kind,
        None,
        "no card has it first, so it places nothing"
    );
}

#[test]
fn top_on_any_category_makes_a_commander() {
    let (bolt, _) = import("1x Lightning Bolt [Burn{top}]");
    assert_eq!(bolt.place, Commander);
}

#[test]
fn companion_needs_nodeck() {
    let (lurrus, _) = import("1x Lurrus of the Dream-Den [Companion{noDeck}]\n4x Island");
    assert_eq!(lurrus.place, Companion);
    assert!(!lurrus.in_deck());
    let (lurrus, _) = import("1x Lurrus of the Dream-Den [Companion]");
    assert_eq!(lurrus.place, InDeck, "an ordinary category in Archidekt");
}

#[test]
fn sideboard_is_a_board() {
    let (counterspell, _) = import("1x Counterspell [Sideboard]");
    assert_eq!(counterspell.place, Sideboard);
}

#[test]
fn a_sideboard_under_its_own_name_is_out_of_the_deck_but_no_sideboard() {
    let (negate, _) = import("1x Negate [Learnboard{noDeck}]");
    assert_eq!(negate.place, NotInDeck);
}

#[test]
fn maybeboard_is_a_board_as_typed_and_as_archidekt_exports_it() {
    let (brainstorm, _) = import("1x Brainstorm [Maybeboard]");
    assert_eq!(brainstorm.place, Maybeboard);
    let (brainstorm, _) = import("1x Brainstorm [Maybeboard{noDeck}{noPrice}]");
    assert_eq!(brainstorm.place, Maybeboard);
}

#[test]
fn attractions_need_nodeck() {
    let (stand, _) = import("1x Balloon Stand [Attractions{noDeck}]");
    assert_eq!(stand.place, Attractions);
    let (stand, _) = import("1x Balloon Stand [Attractions]");
    assert_eq!(stand.place, InDeck);
}

#[test]
fn sticker_sheets_need_nodeck() {
    let (sheet, _) = import("1x Ancestral Hot Dog Minotaur [Sticker Sheet{noDeck}]");
    assert_eq!(sheet.place, StickerSheet);
    let (sheet, _) = import("1x Ancestral Hot Dog Minotaur [Sticker Sheet]");
    assert_eq!(sheet.place, InDeck);
}

#[test]
fn several_untyped_categories_are_all_kept() {
    let (ring, _) = import("1x Sol Ring [Ramp,Artifacts]");
    assert_eq!(ring.place, InDeck);
    assert_eq!(categories(&ring), ["Ramp", "Artifacts"]);
}

#[test]
fn typed_first_places_the_card_and_keeps_the_rest() {
    let (path, _) = import("1x Path to Exile [Sideboard,Removal]");
    assert_eq!(path.place, Sideboard);
    assert_eq!(categories(&path), ["Sideboard", "Removal"]);
}

#[test]
fn a_board_named_second_does_not_move_the_card() {
    let (wrath, deck) = import("1x Wrath of God [Removal,Sideboard]");
    assert_eq!(wrath.place, InDeck);
    assert_eq!(categories(&wrath), ["Removal", "Sideboard"]);
    assert_eq!(deck.category("Sideboard").unwrap().kind, None);
    let (draw, _) = import("1x Brainstorm [Draw,Maybeboard]");
    assert!(draw.in_deck());
    let (lurrus, _) = import("1x Lurrus of the Dream-Den [Recursion,Companion{noDeck}]");
    assert!(lurrus.in_deck());
}

#[test]
fn only_the_exact_board_names_are_boards() {
    for line in ["1x Negate [sideboard]", "1x Negate [Sideboard Lessons]"] {
        let (negate, _) = import(line);
        assert_eq!(negate.place, InDeck, "{line}");
    }
}

#[test]
fn an_untyped_category_is_in_the_deck() {
    let (swords, _) = import("1x Swords to Plowshares [Removal]");
    assert_eq!(swords.place, InDeck);
    assert_eq!(categories(&swords), ["Removal"]);
}

#[test]
fn no_categories_is_in_the_deck_with_none() {
    // Archidekt would add one of its own choosing ([Removal]); nothing here can
    // know which, so the card comes in with none.
    let (swords, _) = import("1x Swords to Plowshares");
    assert_eq!(swords.place, InDeck);
    assert!(swords.categories.is_empty());
}

#[test]
fn quantity_is_one_card_of_that_many() {
    let (island, deck) = import("4x Island [Lands]");
    assert_eq!(island.qty.get(), 4);
    assert_eq!(deck.cards.len(), 1);
}

#[test]
fn a_heading_is_the_first_category_of_the_lines_below_it() {
    let (counterspell, _) = import("# Sideboard\n1x Counterspell");
    assert_eq!(counterspell.place, Sideboard);
    let (kenrith, _) = import("# Commander\n1x Kenrith, the Returned King");
    assert_eq!(
        kenrith.place, Commander,
        "# Commander sets Premier by itself"
    );
    let (ring, _) = import("# Ramp\n1x Sol Ring [Artifacts]");
    assert_eq!(categories(&ring), ["Ramp", "Artifacts"]);
}

/// Archidekt text with a set and number names the printing; without them, the
/// name. Never both (ADR-0020).
#[test]
fn each_card_is_named_once() {
    let deck =
        Deck::read_archidekt("1x Rashmi and Ragavan (moc) 94 *F* [Commander{top}]\n1x Sol Ring\n")
            .deck;
    assert_eq!(
        deck.cards[0].card,
        CardRef::Printing(Printing {
            set: "moc".into(),
            num: "94".into()
        })
    );
    assert_eq!(deck.cards[0].finish, chip_decklist::deck::Finish::Foil);
    assert_eq!(deck.cards[1].card, CardRef::Name("Sol Ring".into()));
}

/// A name ending in a parenthetical is the whole name, not a name and a set.
#[test]
fn a_name_ending_in_a_parenthetical_is_named_whole() {
    let (erase, _) = import("1x Erase (Not the Urza's Legacy One) [Removal]");
    assert_eq!(
        erase.card,
        CardRef::Name("Erase (Not the Urza's Legacy One)".into())
    );
}

/// Archidekt's export with collector numbers off: a set is no printing, so
/// the card is named by name and the set is handed on for Scryfall to pin.
#[test]
fn a_set_without_a_number_is_named_by_name_and_handed_on() {
    let text = "1x Sol Ring (c21) 263\n1x Doomskar (khm) [Board Wipe,Cast From Exile]";
    let imported = Deck::read_archidekt(text);
    assert_eq!(imported.unreadable, vec![]);
    assert_eq!(
        imported.deck.cards[1].card,
        CardRef::Name("Doomskar".into())
    );
    assert_eq!(
        imported.set_only,
        vec![chip_decklist::deck::SetOnly {
            index: 1,
            line: 2,
            text: "1x Doomskar (khm) [Board Wipe,Cast From Exile]".into(),
            name: "Doomskar".into(),
            set: "khm".into(),
        }]
    );
}

#[test]
fn an_etched_line_imports_as_etched() {
    let (ring, _) = import("1x Sol Ring (c21) 263 *E* [Ramp]");
    assert_eq!(ring.finish, chip_decklist::deck::Finish::Etched);
    assert_eq!(
        ring.card,
        CardRef::Printing(Printing {
            set: "c21".into(),
            num: "263".into()
        })
    );
}

// --- What cannot be read is said, never dropped ----------------------------

#[test]
fn a_marker_the_import_cannot_read_is_named_and_the_rest_imports() {
    let imported = Deck::read_archidekt("1x Sol Ring (c21) 263 *X* [Ramp]\n1x Arcane Signet\n");
    assert_eq!(imported.unreadable.len(), 1, "{:?}", imported.unreadable);
    assert_eq!(imported.unreadable[0].line, 1);
    assert!(
        imported.unreadable[0].reason.contains("*X*"),
        "{}",
        imported.unreadable[0].reason
    );
    assert_eq!(imported.deck.cards.len(), 1);
}

#[test]
fn every_unreadable_line_is_named_with_its_reason_and_the_rest_imports() {
    let imported = Deck::read_archidekt(
        "// a comment\n1x Sol Ring [Ramp]\nSol Ring without a count\n0x Island\n\n1x Arcane Signet\n",
    );
    let lines: Vec<(usize, &str)> = imported
        .unreadable
        .iter()
        .map(|u| (u.line, u.text.as_str()))
        .collect();
    assert_eq!(lines, [(3, "Sol Ring without a count"), (4, "0x Island")]);
    assert!(imported.unreadable[0].reason.contains("not a card"));
    assert!(imported.unreadable[1].reason.contains("0"));
    assert_eq!(imported.deck.cards.len(), 2);
}

/// A category is typed by the cards it comes first on. A card that names it
/// later, where Archidekt ignores it, would be moved by it in a deck.toml, so
/// the card comes in without it, and the import says so.
#[test]
fn a_typed_category_listed_second_is_left_off_that_card_and_reported() {
    let imported = Deck::read_archidekt(
        "1x Kenrith, the Returned King [Commander{top}]\n1x Sol Ring [Ramp,Commander{top}]\n",
    );
    let ring = &imported.deck.cards[1];
    assert_eq!(ring.place, InDeck, "Archidekt shows it under Ramp");
    assert_eq!(categories(ring), ["Ramp"]);
    assert_eq!(imported.unreadable.len(), 1);
    assert_eq!(imported.unreadable[0].line, 2);
    assert!(
        imported.unreadable[0].reason.contains("\"Commander\""),
        "{}",
        imported.unreadable[0].reason
    );
    assert!(matches!(
        Deck::from_archidekt(
            "1x Kenrith, the Returned King [Commander{top}]\n1x Sol Ring [Ramp,Commander{top}]\n"
        ),
        Err(ImportError::Unreadable(_))
    ));
}

#[test]
fn a_more_specific_place_outside_the_deck_is_kept() {
    // Both out of the deck; the deck.toml keeps the more specific.
    let imported = Deck::read_archidekt(
        "1x Counterspell [Sideboard]\n1x Lurrus of the Dream-Den [Companion{noDeck},Sideboard]\n",
    );
    assert_eq!(imported.unreadable, vec![]);
    assert_eq!(imported.deck.cards[1].place, Companion);
    assert_eq!(
        categories(&imported.deck.cards[1]),
        ["Companion", "Sideboard"]
    );
}

// --- The committed decks ----------------------------------------------------

fn repo(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// The name a `.deck.toml` writes beside each printing. The format never reads
/// it back; a test may, to stand in for the card data the browser has.
fn comment_names(toml: &str) -> HashMap<String, String> {
    toml.lines()
        .filter_map(|l| {
            let printing = l.split_once("printing = \"")?.1.split_once('"')?.0;
            let name = l.rsplit_once("},  # ")?.1;
            Some((printing.to_string(), name.to_string()))
        })
        .collect()
}

fn name(card: &CardRef, names: &HashMap<String, String>) -> String {
    match card {
        CardRef::Name(n) => n.clone(),
        CardRef::Printing(p) => names[&p.to_string()].clone(),
    }
}

/// `decks/lantern.deck.toml` was made from this Archidekt export; reading the
/// export as Archidekt does gives the same deck.
#[test]
fn lantern_as_archidekt_text_imports_as_lantern_deck_toml() {
    let toml = repo("decks/lantern.deck.toml");
    let names = comment_names(&toml);
    let want = Deck::parse(&toml).unwrap();
    let imported = Deck::read_archidekt(&fixture("lantern.txt"));
    assert_eq!(imported.unreadable, vec![]);
    let have = imported.deck;

    assert_eq!(have.categories, want.categories);
    assert_eq!(have.cards.len(), want.cards.len());
    let imported_names: HashMap<String, String> = imported
        .names
        .iter()
        .map(|(p, n)| (p.to_string(), n.clone()))
        .collect();
    for (h, w) in have.cards.iter().zip(&want.cards) {
        let wn = name(&w.card, &names);
        assert_eq!(name(&h.card, &imported_names), wn);
        assert_eq!(
            (h.qty, h.finish, &h.categories, h.place),
            (w.qty, w.finish, &w.categories, w.place),
            "{wn}"
        );
    }
}

// --- Shape by shape: what the export writes for each deck.toml shape --------

const TYPES: &str = r#"
[categories]
cmdr = { type = "commander" }
Ramp = {}
Artifacts = {}
Removal = {}
Lands = {}
recursion = {}
companion = { type = "companion" }
learnboard = { type = "sideboard" }
maybe = { type = "maybeboard" }
attractions = { type = "attractions" }
stickers = { type = "sticker-sheet" }
tokens = { type = "not-in-deck" }
"#;

/// The Archidekt line for one deck.toml card, among [`TYPES`].
fn export(card: &str) -> String {
    let text = format!("cards = [\n  {card},\n]\n{TYPES}");
    export_archidekt(&text, &HashMap::new()).unwrap()
}

/// Each deck.toml shape, the line the research doc says to write for it, and
/// where Archidekt (and so the import) puts that line.
const SHAPES: &[(&str, &str, CategoryType)] = &[
    (
        r#"{ name = "Kenrith, the Returned King", in = ["cmdr"] }"#,
        "1x Kenrith, the Returned King [Commander{top}]",
        Commander,
    ),
    (
        r#"{ name = "Kenrith, the Returned King", in = ["Ramp", "cmdr"] }"#,
        "1x Kenrith, the Returned King [Commander{top},Ramp]",
        Commander,
    ),
    (
        r#"{ name = "Lurrus of the Dream-Den", in = ["recursion", "companion"] }"#,
        "1x Lurrus of the Dream-Den [Companion{noDeck},recursion]",
        Companion,
    ),
    (
        r#"{ name = "Counterspell", in = ["learnboard"] }"#,
        "1x Counterspell [Sideboard]",
        Sideboard,
    ),
    (
        r#"{ name = "Brainstorm", in = ["maybe"] }"#,
        "1x Brainstorm [Maybeboard]",
        Maybeboard,
    ),
    (
        r#"{ name = "Balloon Stand", in = ["attractions"] }"#,
        "1x Balloon Stand [Attractions{noDeck}]",
        Attractions,
    ),
    (
        r#"{ name = "Ancestral Hot Dog Minotaur", in = ["stickers"] }"#,
        "1x Ancestral Hot Dog Minotaur [Sticker Sheet{noDeck}]",
        StickerSheet,
    ),
    (
        r#"{ name = "Treasure", in = ["tokens"] }"#,
        "1x Treasure [tokens{noDeck}]",
        NotInDeck,
    ),
    (
        r#"{ name = "Sol Ring", in = ["Ramp", "Artifacts"] }"#,
        "1x Sol Ring [Ramp,Artifacts]",
        InDeck,
    ),
    (
        r#"{ name = "Path to Exile", in = ["Removal", "learnboard"] }"#,
        "1x Path to Exile [Sideboard,Removal]",
        Sideboard,
    ),
    (
        r#"{ name = "Swords to Plowshares", in = ["Removal"] }"#,
        "1x Swords to Plowshares [Removal]",
        InDeck,
    ),
    (
        r#"{ name = "Swords to Plowshares" }"#,
        "1x Swords to Plowshares",
        InDeck,
    ),
    (
        r#"{ name = "Island", qty = 4, finish = "foil", in = ["Lands"] }"#,
        "4x Island [Lands]",
        InDeck,
    ),
];

#[test]
fn every_shape_exports_as_the_research_doc_says() {
    for (card, line, _) in SHAPES {
        assert_eq!(export(card), format!("{line}\n"), "{card}");
    }
}

/// The exported line is read back to the same place: typed first, `{top}` on
/// the commander only, the boards in exact case, `{noDeck}` on the rest.
#[test]
fn every_exported_shape_imports_back_to_the_same_place() {
    for (card, line, place) in SHAPES {
        let (back, _) = import(line);
        assert_eq!(back.place, *place, "{card} -> {line}");
    }
}

#[test]
fn a_printing_is_written_by_the_name_it_is_given() {
    let text = "cards = [\n  { printing = \"moc/94\", in = [\"C\"] },\n]\n[categories]\nC = { type = \"commander\" }\n";
    let names = HashMap::from([("moc/94".to_string(), "Rashmi and Ragavan".to_string())]);
    assert_eq!(
        export_archidekt(text, &names).unwrap(),
        "1x Rashmi and Ragavan [Commander{top}]\n"
    );
    let err = export_archidekt(text, &HashMap::new()).unwrap_err();
    assert!(
        matches!(err, ExportError::Unnamed(ref p) if p.len() == 1),
        "{err}"
    );
    assert!(err.to_string().contains("moc/94"), "{err}");
}

#[test]
fn a_category_archidekt_cannot_spell_is_refused() {
    let text = "cards = [{ name = \"Sol Ring\", in = [\"Ramp, fast\"] }]\n[categories]\n\"Ramp, fast\" = {}\n";
    assert!(matches!(
        export_archidekt(text, &HashMap::new()),
        Err(ExportError::Unwritable(_))
    ));
}

/// Archidekt reads a word as one category, so two categories written as the
/// same word come back as one, and a card in the untyped one would move.
#[test]
fn two_categories_written_as_the_same_word_are_refused() {
    let text = r#"cards = [
  { name = "Kenrith, the Returned King", in = ["boss"] },
  { name = "Sol Ring", in = ["Commander"] },
]
[categories]
boss = { type = "commander" }
Commander = {}
"#;
    // What the export would have written, and why it cannot be.
    let (sol_ring, _) = {
        let imported = Deck::read_archidekt(
            "1x Kenrith, the Returned King [Commander{top}]\n1x Sol Ring [Commander]\n",
        );
        (imported.deck.cards[1].clone(), imported.deck)
    };
    assert_eq!(
        sol_ring.place, Commander,
        "Sol Ring would come back a commander"
    );
    let err = export_archidekt(text, &HashMap::new()).unwrap_err();
    assert!(
        matches!(&err, ExportError::SameWord { word, .. } if word == "Commander"),
        "{err}"
    );
    assert!(err.to_string().contains("boss"), "{err}");

    // Two categories of the same type come back as one of that type, and no
    // card moves: that is the typed name Archidekt has no room for.
    let text = r#"cards = [
  { name = "Counterspell", in = ["learnboard"] },
  { name = "Negate", in = ["sb"] },
]
[categories]
learnboard = { type = "sideboard" }
sb = { type = "sideboard" }
"#;
    assert_eq!(
        export_archidekt(text, &HashMap::new()).unwrap(),
        "1x Counterspell [Sideboard]\n1x Negate [Sideboard]\n"
    );
}

/// A label that happens to be spelled like one of Archidekt's boards goes
/// after the card's other labels, and is refused when it would come first.
#[test]
fn a_label_named_like_a_board_is_never_first() {
    let text = "cards = [\n  { name = \"Sol Ring\", in = [\"Sideboard\", \"Ramp\"] },\n]\n[categories]\nSideboard = {}\nRamp = {}\n";
    assert_eq!(
        export_archidekt(text, &HashMap::new()).unwrap(),
        "1x Sol Ring [Ramp,Sideboard]\n"
    );
    let text =
        "cards = [{ name = \"Sol Ring\", in = [\"Sideboard\"] }]\n[categories]\nSideboard = {}\n";
    assert!(matches!(
        export_archidekt(text, &HashMap::new()),
        Err(ExportError::Misread { .. })
    ));
}

// --- Round trip on the committed decks ---------------------------------------

/// Export then import gives the same deck, apart from what Archidekt's text
/// cannot carry: printings (a name comes back), finishes, and a typed
/// category's own name (Archidekt's word for the type comes back).
#[test]
fn lantern_and_loam_survive_a_trip_through_archidekt_text() {
    for path in ["decks/lantern.deck.toml", "decks/loam.deck.toml"] {
        let toml = repo(path);
        let names = comment_names(&toml);
        let deck = Deck::parse(&toml).unwrap();
        let text = export_archidekt(&toml, &names).unwrap();
        assert!(
            !text.contains('('),
            "{path}: names only, no set codes\n{text}"
        );

        let back = Deck::read_archidekt(&text);
        assert_eq!(back.unreadable, vec![], "{path}");
        let back = back.deck;
        assert_eq!(back.cards.len(), deck.cards.len(), "{path}");

        // The one typed category in each is named Commander already, so every
        // category comes back under its own name and type.
        assert_eq!(back.categories, deck.categories, "{path}");
        for (b, d) in back.cards.iter().zip(&deck.cards) {
            let n = name(&d.card, &names);
            assert_eq!(b.card, CardRef::Name(n.clone()), "{path}");
            assert_eq!(b.qty, d.qty, "{path}: {n}");
            assert_eq!(b.place, d.place, "{path}: {n}");
            let set = |c: &Card| c.categories.iter().cloned().collect::<BTreeSet<_>>();
            assert_eq!(set(b), set(d), "{path}: {n}");
        }
        let total = |d: &Deck| -> u32 {
            d.cards
                .iter()
                .filter(|c| c.in_deck())
                .map(|c| c.qty.get())
                .sum()
        };
        assert_eq!(total(&back), total(&deck), "{path}");
    }
}
