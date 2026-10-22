use std::num::NonZeroU32;

use proptest::prelude::*;

use super::*;
use crate::deck::Finish;

const BASE: &str = r#"name = "Lantern"
format = "commander"

cards = [
  { name = "Sol Ring", in = ["Ramp"] },
  { name = "Mind Stone", in = ["Ramp"] },
  { name = "Lantern of Insight" },
  { name = "Island", qty = 10 },
]

[categories]
Ramp = {}
Sideboard = { type = "sideboard" }
"#;

fn clean(m: Result<Merged, MergeError>) -> (String, Vec<String>) {
    match m {
        Ok(Merged::Clean { text, taken }) => (text, taken),
        other => panic!("not clean: {other:?}"),
    }
}

fn clashes(m: Result<Merged, MergeError>) -> Vec<Clash> {
    match m {
        Ok(Merged::Clashed(c)) => c,
        other => panic!("no clash: {other:?}"),
    }
}

fn open(_: usize) -> Option<Side> {
    None
}

fn card<'d>(deck: &'d Deck, name: &str) -> &'d Card {
    deck.cards
        .iter()
        .find(|c| matches!(&c.card, CardRef::Name(n) if n == name))
        .unwrap_or_else(|| panic!("no {name}"))
}

fn qty(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).unwrap()
}

#[test]
fn edits_to_adjacent_lines_merge() {
    // A line merge stops here: both changed the stretch between two lines
    // they kept.
    let ours = BASE.replace(
        r#"{ name = "Sol Ring", in"#,
        r#"{ name = "Sol Ring", qty = 2, in"#,
    );
    let theirs = BASE.replace(
        r#"{ name = "Mind Stone", in = ["Ramp"] }"#,
        r#"{ name = "Mind Stone" }"#,
    );
    let (text, taken) = clean(merge(BASE, &ours, &theirs, open));
    println!("{text}");
    let d = Deck::parse(&text).unwrap();
    assert_eq!(card(&d, "Sol Ring").qty, qty(2));
    assert!(card(&d, "Mind Stone").categories.is_empty());
    assert_eq!(taken, ["Mind Stone: ramp → uncategorized"]);
}

#[test]
fn their_quantity_and_our_printing_pin_on_one_card_both_land() {
    // The editor pinned every card by name when the fork was opened
    // (ADR-0027); the deck it came from then changed a quantity.
    let ours = BASE
        .replace(
            r#"{ name = "Sol Ring", in = ["Ramp"] },"#,
            r#"{ printing = "sld/2417", in = ["Ramp"] },  # Sol Ring"#,
        )
        .replace(
            r#"{ name = "Island", qty = 10 },"#,
            r#"{ printing = "unf/236", qty = 10 },  # Island"#,
        );
    let theirs = BASE.replace(r#"qty = 10"#, r#"qty = 9"#).replace(
        r#"{ name = "Sol Ring", in = ["Ramp"] }"#,
        r#"{ name = "Sol Ring", finish = "foil", in = ["Ramp"] }"#,
    );
    let (text, taken) = clean(merge(BASE, &ours, &theirs, open));
    println!("{text}");
    assert!(
        text.contains(r#"{ printing = "sld/2417", finish = "foil", in = ["Ramp"] },  # Sol Ring"#)
    );
    assert!(text.contains(r#"{ printing = "unf/236", qty = 9 },  # Island"#));
    assert_eq!(taken, ["Island: 10 → 9", "Sol Ring: nonfoil → foil"]);
}

#[test]
fn the_same_change_on_both_sides_is_one_change() {
    let both = BASE.replace("qty = 10", "qty = 8");
    let ours = format!("# my notes\n{both}");
    let (text, taken) = clean(merge(BASE, &ours, &both, open));
    assert_eq!(text, ours);
    assert!(taken.is_empty());
}

#[test]
fn quantity_against_quantity_clashes_and_a_pick_settles_it() {
    let ours = BASE.replace("qty = 10", "qty = 8");
    let theirs = BASE.replace("qty = 10", "qty = 12");
    let c = clashes(merge(BASE, &ours, &theirs, open));
    assert_eq!(
        c,
        [Clash::Card {
            card: "Island".into(),
            clash: CardClash::Quantity {
                ours: qty(8),
                theirs: qty(12)
            }
        }]
    );
    let said = c[0].said();
    assert_eq!(
        (
            said.subject.as_str(),
            said.attribute,
            said.ours.as_str(),
            said.theirs.as_str()
        ),
        ("Island", "quantity", "8", "12")
    );
    let (text, _) = clean(merge(BASE, &ours, &theirs, |_| Some(Side::Theirs)));
    assert!(text.contains("qty = 12"));
    let (text, _) = clean(merge(BASE, &ours, &theirs, |_| Some(Side::Ours)));
    assert_eq!(text, ours);
}

#[test]
fn categories_against_categories_clash() {
    let ours = BASE.replace(
        r#"{ name = "Mind Stone", in = ["Ramp"] }"#,
        r#"{ name = "Mind Stone", in = ["Sideboard"] }"#,
    );
    let theirs = BASE.replace(
        r#"{ name = "Mind Stone", in = ["Ramp"] }"#,
        r#"{ name = "Mind Stone" }"#,
    );
    let c = clashes(merge(BASE, &ours, &theirs, open));
    assert!(matches!(
        &c[..],
        [Clash::Card { card, clash: CardClash::Categories { .. } }] if card == "Mind Stone"
    ));
}

#[test]
fn a_card_one_side_removed_and_the_other_changed_clashes_both_ways() {
    let removed = BASE.replace("  { name = \"Lantern of Insight\" },\n", "");
    let changed = BASE.replace(
        r#"{ name = "Lantern of Insight" }"#,
        r#"{ name = "Lantern of Insight", qty = 2 }"#,
    );
    // Theirs removed it, ours changed it.
    let c = clashes(merge(BASE, &changed, &removed, open));
    assert!(matches!(
        &c[..],
        [Clash::Card {
            clash: CardClash::Removed {
                by: Side::Theirs,
                ..
            },
            ..
        }]
    ));
    let said = c[0].said();
    assert_eq!(
        (said.ours.as_str(), said.theirs.as_str()),
        ("2 × any printing, uncategorized", "removed")
    );
    let (text, _) = clean(merge(BASE, &changed, &removed, |_| Some(Side::Theirs)));
    assert!(!text.contains("Lantern of"));
    // Ours removed it, theirs changed it; taking theirs brings it back.
    let c = clashes(merge(BASE, &removed, &changed, open));
    assert!(matches!(
        &c[..],
        [Clash::Card {
            clash: CardClash::Removed { by: Side::Ours, .. },
            ..
        }]
    ));
    let (text, _) = clean(merge(BASE, &removed, &changed, |_| Some(Side::Theirs)));
    assert_eq!(
        card(&Deck::parse(&text).unwrap(), "Lantern of Insight").qty,
        qty(2)
    );
    // Removed on one side and untouched on the other: it goes.
    let ours = BASE.replace("qty = 10", "qty = 9");
    let (text, taken) = clean(merge(BASE, &ours, &removed, open));
    assert!(!text.contains("Lantern of") && text.contains("qty = 9"));
    assert_eq!(taken, ["-1 Lantern of Insight"]);
}

#[test]
fn the_decks_own_fields_merge_one_by_one() {
    let ours = BASE.replace(
        "format = \"commander\"",
        "format = \"commander\"\ndescription = \"\"\"\nmine\"\"\"",
    );
    let theirs = BASE.replace("name = \"Lantern\"", "name = \"Lantern Control\"");
    let (text, taken) = clean(merge(BASE, &ours, &theirs, open));
    let d = Deck::parse(&text).unwrap();
    assert_eq!(d.name.as_deref(), Some("Lantern Control"));
    assert_eq!(d.description.as_deref(), Some("mine"));
    assert_eq!(taken, [r#"name: "Lantern" → "Lantern Control""#]);
    // Both renamed: a clash on the deck's name.
    let ours = BASE.replace("name = \"Lantern\"", "name = \"Mine\"");
    let c = clashes(merge(BASE, &ours, &theirs, open));
    assert_eq!(
        c,
        [Clash::Field {
            field: Field::Name,
            ours: Some("Mine".into()),
            theirs: Some("Lantern Control".into())
        }]
    );
}

#[test]
fn a_category_theirs_declares_comes_with_the_card_they_move_into_it() {
    let ours = BASE.replace("qty = 10", "qty = 9");
    let theirs = BASE
        .replace(
            r#"{ name = "Mind Stone", in = ["Ramp"] }"#,
            r#"{ name = "Mind Stone", in = ["Maybe"] }"#,
        )
        .replace(
            "Ramp = {}\n",
            "Maybe = { type = \"maybeboard\" }\nRamp = {}\n",
        );
    let (text, _) = clean(merge(BASE, &ours, &theirs, open));
    let d = Deck::parse(&text).unwrap();
    assert_eq!(card(&d, "Mind Stone").place, CategoryType::Maybeboard);
    assert_eq!(card(&d, "Island").qty, qty(9));
    // They stop declaring Ramp, but a card we added is in it: it stays.
    let theirs = BASE
        .replace(r#", in = ["Ramp"]"#, "")
        .replace("Ramp = {}\n", "");
    let ours = BASE.replace(
        "]\n\n[categories]",
        "  { name = \"Arcane Signet\", in = [\"Ramp\"] },\n]\n\n[categories]",
    );
    let (text, _) = clean(merge(BASE, &ours, &theirs, open));
    let d = Deck::parse(&text).unwrap();
    assert!(d.category("Ramp").is_some());
    assert!(card(&d, "Mind Stone").categories.is_empty());
}

#[test]
fn a_card_both_added_is_one_card_and_clashes_only_where_the_adds_differ() {
    let add = |line: &str| {
        BASE.replace(
            "]\n\n[categories]",
            &format!("  {line},\n]\n\n[categories]"),
        )
    };
    let ours = add(r#"{ name = "Arcane Signet" }"#);
    let same = add(r#"{ name = "Arcane Signet" }"#);
    let (text, taken) = clean(merge(BASE, &ours, &format!("{same}\n"), open));
    assert_eq!(text.matches("Arcane Signet").count(), 1, "{text}");
    assert!(taken.is_empty());
    let more = add(r#"{ name = "Arcane Signet", qty = 2 }"#);
    let c = clashes(merge(BASE, &ours, &more, open));
    assert!(matches!(
        &c[..],
        [Clash::Card {
            clash: CardClash::Quantity { .. },
            ..
        }]
    ));
    // Each adding another card keeps both, theirs where theirs has it.
    let other = add(r#"{ name = "Fellwar Stone" }"#);
    let (text, _) = clean(merge(BASE, &ours, &other, open));
    assert!(text.contains("Arcane Signet") && text.contains("Fellwar Stone"));
}

#[test]
fn ours_comments_spacing_and_order_survive() {
    let ours = r#"# Lantern, my way
name = "Lantern"
format = "commander"

cards = [
  # the lock
  { name = "Lantern of Insight" },
  { name = "Island", qty = 10 },   # snow next time

  # rocks
  { name = "Mind Stone", in = ["Ramp"] },
  { name = "Sol Ring", in = ["Ramp"] },
]

[categories]
Ramp = {}   # mana
Sideboard = { type = "sideboard" }
"#;
    let theirs = BASE.replace("qty = 10", "qty = 11").replace(
        "]\n\n[categories]",
        "  { printing = \"woc/145\" },  # Arcane Signet\n]\n\n[categories]",
    );
    let (text, _) = clean(merge(BASE, ours, &theirs, open));
    println!("{text}");
    // Theirs has Arcane Signet below Island, so it goes below ours' Island,
    // in its group and above the next one's comment.
    let expected = ours.replace("qty = 10", "qty = 11").replace(
        "# snow next time\n",
        "# snow next time\n  { printing = \"woc/145\" },  # Arcane Signet\n",
    );
    assert_eq!(text, expected);
}

#[test]
fn every_clash_is_listed_in_one_order_and_picked_by_its_index() {
    let ours = BASE
        .replace("qty = 10", "qty = 8")
        .replace("name = \"Lantern\"", "name = \"A\"");
    let theirs = BASE
        .replace("qty = 10", "qty = 12")
        .replace("name = \"Lantern\"", "name = \"B\"");
    let c = clashes(merge(BASE, &ours, &theirs, open));
    assert_eq!(c.len(), 2);
    assert!(matches!(c[0], Clash::Card { .. }) && matches!(c[1], Clash::Field { .. }));
    // One picked and one open: still clashed, with both listed.
    let half = clashes(merge(BASE, &ours, &theirs, |i| {
        (i == 0).then_some(Side::Theirs)
    }));
    assert_eq!(half, c);
    let (text, _) = clean(merge(BASE, &ours, &theirs, |i| {
        Some(if i == 0 { Side::Theirs } else { Side::Ours })
    }));
    let d = Deck::parse(&text).unwrap();
    assert_eq!(
        (d.name.as_deref(), card(&d, "Island").qty),
        (Some("A"), qty(12))
    );
}

// Generated decks: a small pool of cards, each by name or by a printing
// commented with its name, as the writer leaves it.

const POOL: [&str; 6] = [
    "Sol Ring",
    "Mind Stone",
    "Island",
    "Arcane Signet",
    "Fellwar Stone",
    "Lantern of Insight",
];

#[derive(Debug, Clone)]
struct Gen {
    cards: Vec<(usize, u32, bool, bool, u8)>,
    name: Option<String>,
}

fn gen_card() -> impl Strategy<Value = (usize, u32, bool, bool, u8)> {
    (0..POOL.len(), 1u32..4, any::<bool>(), any::<bool>(), 0u8..3)
}

fn gen_deck() -> impl Strategy<Value = Gen> {
    (
        proptest::collection::vec(gen_card(), 0..6),
        proptest::option::of("[A-C]"),
    )
        .prop_map(|(cards, name)| Gen { cards, name })
}

fn printing(i: usize) -> Printing {
    Printing {
        set: "tst".into(),
        num: i.to_string(),
    }
}

fn deck_of(g: &Gen) -> Deck {
    let cats = ["Ramp", "Side"];
    Deck {
        name: g.name.clone(),
        format: None,
        cover: None,
        description: None,
        categories: vec![
            crate::deck::Category {
                name: "Ramp".into(),
                kind: None,
            },
            crate::deck::Category {
                name: "Side".into(),
                kind: Some(CategoryType::Sideboard),
            },
        ],
        cards: g
            .cards
            .iter()
            .map(|&(i, q, by_printing, foil, cat)| Card {
                card: if by_printing {
                    CardRef::Printing(printing(i))
                } else {
                    CardRef::Name(POOL[i].into())
                },
                qty: qty(q),
                finish: if foil { Finish::Foil } else { Finish::Nonfoil },
                categories: cats
                    .iter()
                    .take(usize::from(cat))
                    .skip(usize::from(cat == 2))
                    .map(|s| (*s).to_string())
                    .collect(),
                place: CategoryType::InDeck,
            })
            .collect(),
    }
}

fn text_of(g: &Gen) -> String {
    deck_of(g).to_toml(|p| p.num.parse::<usize>().ok().map(|i| POOL[i].to_string()))
}

/// The same deck in other words: a comment on top and one inside the list.
fn reworded(text: &str) -> String {
    format!(
        "# kept as mine\n{}",
        text.replacen("cards = [\n", "cards = [\n  # the list\n", 1)
    )
}

fn semantically(a: &str, b: &str) -> Vec<String> {
    let (x, y, names) = crate::diff::parse_pair(a, b).unwrap();
    Diff::new(&x, &y, |p| names.get(p).cloned())
        .changes
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

proptest! {
    #[test]
    fn with_ours_unchanged_the_result_is_theirs(base in gen_deck(), theirs in gen_deck()) {
        let base = text_of(&base);
        let ours = reworded(&base);
        let theirs = text_of(&theirs);
        let (text, _) = clean(merge(&base, &ours, &theirs, open));
        prop_assert!(Deck::parse(&text).is_ok());
        prop_assert_eq!(semantically(&theirs, &text), Vec::<String>::new());
        prop_assert!(text.contains("# kept as mine\n") && text.contains("# the list\n"));
    }

    #[test]
    fn with_theirs_unchanged_the_result_is_ours_text(base in gen_deck(), ours in gen_deck()) {
        let base = text_of(&base);
        let ours = reworded(&text_of(&ours));
        let theirs = reworded(&base);
        let (text, _) = clean(merge(&base, &ours, &theirs, open));
        prop_assert_eq!(text, ours);
    }

    #[test]
    fn any_merge_settled_by_either_side_reads(
        base in gen_deck(),
        ours in gen_deck(),
        theirs in gen_deck(),
        side in any::<bool>(),
    ) {
        let (base, ours, theirs) = (text_of(&base), text_of(&ours), text_of(&theirs));
        let side = if side { Side::Theirs } else { Side::Ours };
        let (text, _) = clean(merge(&base, &ours, &theirs, |_| Some(side)));
        prop_assert!(Deck::parse(&text).is_ok());
        // Unsettled, the same clashes, or none.
        match merge(&base, &ours, &theirs, open).unwrap() {
            Merged::Clean { text: t, .. } => prop_assert_eq!(t, text),
            Merged::Clashed(c) => prop_assert!(!c.is_empty()),
        }
    }
}
