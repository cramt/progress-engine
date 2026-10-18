//! Where one deck's cards come from: the collection's copies each line takes,
//! what its deck place holds besides, and who has what is short.

use chip_decklist::collection::{move_some, Collection};
use chip_decklist::deck::{CardRef, Deck, Finish, Printing};
use chip_decklist::identity::Names;
use chip_decklist::supply::{supply, Supply, Whereabouts};

const PATH: &str = "decks/lantern.deck.toml";

fn run(deck: &str, mine: &str, friends: &[(&str, &str)]) -> Supply {
    let d = Deck::parse(deck).unwrap();
    let m = Collection::parse(mine).unwrap();
    let friends: Vec<(String, Collection, String)> = friends
        .iter()
        .map(|(who, text)| {
            (
                who.to_string(),
                Collection::parse(text).unwrap(),
                text.to_string(),
            )
        })
        .collect();
    supply(PATH, (&d, deck), (&m, mine), &friends, &Names::new())
}

fn printing(set: &str, num: &str) -> CardRef {
    CardRef::Printing(Printing {
        set: set.into(),
        num: num.into(),
    })
}

/// Each line's copies as `(collection line, qty, exact)`.
fn taken(s: &Supply) -> Vec<Vec<(usize, u32, bool)>> {
    s.lines
        .iter()
        .map(|l| l.copies.iter().map(|c| (c.line, c.qty, c.exact)).collect())
        .collect()
}

const MINE: &str = r#"cards = [
  { printing = "cmm/400", at = "Bulk" },  # Sol Ring
  { printing = "sld/2417", at = "Loam" },  # Sol Ring
  { printing = "sld/2417", at = "Lantern" },  # Sol Ring
  { name = "Mind Stone", finish = "foil", at = "Lantern" },
  { name = "Mind Stone" },
  { name = "Brainstorm", at = "Lantern" },
]

[places]
Bulk = {}
Lantern = { deck = "decks/lantern.deck.toml" }
Loam = { deck = "decks/loam.deck.toml" }
"#;

#[test]
fn a_line_takes_the_copy_it_names_from_the_deck_first() {
    let deck = r#"cards = [
  { printing = "sld/2417" },  # Sol Ring
  { name = "Mind Stone" },
]
"#;
    let s = run(deck, MINE, &[]);
    assert_eq!(s.place.as_deref(), Some("Lantern"));
    // The sleeved Sol Ring beats the one in Loam; the nonfoil Mind Stone,
    // loose, beats the foil already sleeved, which the list does not name.
    assert_eq!(taken(&s), [vec![(2, 1, true)], vec![(4, 1, true)]]);
    assert_eq!(s.lines[0].copies[0].whereabouts, Whereabouts::ThisDeck);
    assert_eq!(s.lines[1].copies[0].whereabouts, Whereabouts::Loose);
    let extra: Vec<_> = s.extra.iter().map(|e| (e.line, e.listed)).collect();
    assert_eq!(extra, [(3, true), (5, false)], "{:?}", s.extra);
    assert!(s.short.is_empty());
}

#[test]
fn another_printing_stands_in_only_once_the_named_ones_are_gone() {
    let deck = r#"cards = [
  { printing = "sld/2417", qty = 4 },  # Sol Ring
]
"#;
    let s = run(deck, MINE, &[]);
    // Both SLD copies, the sleeved one first, then the CMM one; a fourth
    // there is none of.
    assert_eq!(taken(&s), [vec![(2, 1, true), (1, 1, true), (0, 1, false)]]);
    assert_eq!(s.lines[0].copies[1].whereabouts, Whereabouts::OtherDeck);
    assert_eq!(s.short.len(), 1);
    assert_eq!(
        (s.short[0].name.as_str(), s.short[0].short),
        ("Sol Ring", 1)
    );
}

#[test]
fn one_lines_stand_in_never_takes_anothers_own_copy() {
    // The foil line goes first in the deck, but the nonfoil copy is the
    // second line's own; the foil line gets it only if nothing else is left.
    let deck = r#"cards = [
  { name = "Mind Stone", finish = "etched" },
  { name = "Mind Stone" },
]
"#;
    let mine = r#"cards = [
  { name = "Mind Stone" },
]
"#;
    let s = run(deck, mine, &[]);
    assert_eq!(taken(&s), [vec![], vec![(0, 1, true)]]);
    assert_eq!(s.short[0].short, 1);
}

#[test]
fn the_maybeboard_needs_no_copy_and_a_basic_says_it_is_one() {
    let deck = r#"cards = [
  { name = "Island", qty = 10 },
  { name = "The One Ring", in = ["Maybe"] },
]

[categories]
Maybe = { type = "maybeboard" }
"#;
    let s = run(deck, "", &[]);
    assert_eq!(s.lines.len(), 1);
    assert_eq!(s.place, None);
    assert_eq!(s.short.len(), 1);
    assert!(s.short[0].basic);
    assert_eq!(s.short[0].short, 10);
}

#[test]
fn a_friend_is_asked_only_for_what_the_deck_is_short_of() {
    let deck = r#"cards = [
  { name = "Sol Ring" },
  { name = "The One Ring", qty = 2 },
  { name = "Forest" },
]
"#;
    let mine = r#"cards = [{ name = "Sol Ring" }]"#;
    let alice = r#"cards = [
  { name = "Sol Ring" },
  { printing = "ltr/451", finish = "foil", at = "Binder" },  # The One Ring
  { name = "Forest", qty = 30 },
]

[places]
Binder = {}
"#;
    let bob = r#"cards = [{ name = "The One Ring", qty = 4 }]"#;
    let s = run(deck, mine, &[("alice", alice), ("bob", bob)]);
    let short: Vec<_> = s
        .short
        .iter()
        .map(|s| {
            let friends: Vec<_> = s
                .friends
                .iter()
                .map(|(who, pulls)| (who.as_str(), pulls.iter().map(|p| p.qty).sum::<u32>()))
                .collect();
            (s.name.as_str(), s.short, friends)
        })
        .collect();
    // Each friend offers what they have up to the shortfall; nobody is asked
    // for a basic.
    assert_eq!(
        short,
        [
            ("The One Ring", 2, vec![("alice", 1), ("bob", 2)]),
            ("Forest", 1, vec![]),
        ]
    );
    let pull = &s.short[0].friends[0].1[0];
    assert_eq!(pull.card, printing("ltr", "451"));
    assert_eq!(pull.finish, Finish::Foil);
    assert_eq!(pull.place.as_deref(), Some("Binder"));
}

#[test]
fn a_copy_logged_by_name_stands_in_for_a_named_printing() {
    let deck = r#"cards = [
  { printing = "cmm/400" },  # Sol Ring
]
"#;
    let mine = r#"cards = [{ name = "Sol Ring" }]"#;
    let s = run(deck, mine, &[]);
    assert_eq!(taken(&s), [vec![(0, 1, false)]]);
    assert!(s.short.is_empty());
}

#[test]
fn moving_parts_of_several_lines_is_one_edit() {
    let moved = move_some(MINE, &[(0, 1), (4, 1), (1, 1)], Some("Lantern")).unwrap();
    let c = Collection::parse(&moved).unwrap();
    let at: Vec<_> = c
        .cards
        .iter()
        .map(|o| {
            (
                o.card.to_string(),
                o.qty.get(),
                o.at.clone().unwrap_or_default(),
            )
        })
        .collect();
    // The SLD Sol Ring from Loam joins the one already sleeved; the CMM one
    // and the loose Mind Stone move in place, the latter beside the foil.
    assert_eq!(
        at,
        [
            ("cmm/400".into(), 1, "Lantern".into()),
            ("sld/2417".into(), 2, "Lantern".into()),
            ("Mind Stone".into(), 1, "Lantern".into()),
            ("Mind Stone".into(), 1, "Lantern".into()),
            ("Brainstorm".into(), 1, "Lantern".into()),
        ]
    );
    assert_eq!(c.cards[2].finish, Finish::Foil);
}

#[test]
fn what_is_left_to_print_is_the_need_less_what_the_collection_gave() {
    let deck = r#"cards = [
  { printing = "sld/2417", qty = 2 },  # Sol Ring
  { name = "Island", qty = 3 },
  { name = "Counterspell" },
]
"#;
    let s = run(deck, MINE, &[]);
    // Three Sol Rings are owned, so both lines of it are answered.
    let all = s.to_print(false);
    assert_eq!(all.lines, [(1, 3), (2, 1)]);
    assert_eq!((all.basics, all.owned), (0, 2));
    let no_basics = s.to_print(true);
    assert_eq!(no_basics.lines, [(2, 1)]);
    assert_eq!(no_basics.basics, 3);

    let whole = run(deck, "", &[]).to_print(false);
    assert_eq!(whole.lines, [(0, 2), (1, 3), (2, 1)]);
    assert_eq!(whole.owned, 0);
}
