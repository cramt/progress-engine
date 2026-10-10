//! What another player's collection holds of the wanted list.

use chip_decklist::collection::Collection;
use chip_decklist::deck::{CardRef, Deck, Finish, Printing};
use chip_decklist::identity::Names;
use chip_decklist::trade::{offers, Offer, Pull};
use chip_decklist::wanted::Wanted;

fn trade(wanted: &str, mine: &str, decks: &[(&str, &str)], theirs: &str) -> Vec<Offer> {
    let w = Wanted::parse(wanted).unwrap();
    let m = Collection::parse(mine).unwrap();
    let t = Collection::parse(theirs).unwrap();
    let decks: Vec<(String, Deck, String)> = decks
        .iter()
        .map(|(p, text)| (p.to_string(), Deck::parse(text).unwrap(), text.to_string()))
        .collect();
    offers(
        (&w, wanted),
        (&m, mine),
        &decks,
        (&t, theirs),
        &Names::new(),
    )
}

fn pull(place: Option<&str>, in_deck: bool, card: CardRef, finish: Finish, qty: u32) -> Pull {
    Pull {
        place: place.map(str::to_string),
        in_deck,
        card,
        finish,
        qty,
    }
}

fn name(n: &str) -> CardRef {
    CardRef::Name(n.to_string())
}

const THEIRS: &str = r#"cards = [
  { name = "Sol Ring", qty = 2, at = "Lantern" },
  { name = "Sol Ring", at = "Bulk" },
  { printing = "ltr/451", finish = "foil", at = "Trade binder" },  # The One Ring
  { name = "Mind Stone" },
]

[places]
Bulk = {}
"Trade binder" = {}
Lantern = { deck = "decks/lantern.deck.toml" }
"#;

#[test]
fn a_hand_want_is_answered_by_any_printing_they_hold() {
    let got = trade(r#"cards = [{ name = "The One Ring" }]"#, "", &[], THEIRS);
    assert_eq!(
        got,
        vec![Offer {
            name: "The One Ring".into(),
            short: 1,
            pulls: vec![pull(
                Some("Trade binder"),
                false,
                CardRef::Printing(Printing {
                    set: "ltr".into(),
                    num: "451".into()
                }),
                Finish::Foil,
                1
            )],
        }]
    );
}

#[test]
fn a_hand_want_already_owned_asks_for_nothing() {
    let got = trade(
        r#"cards = [{ name = "Mind Stone", qty = 2 }]"#,
        r#"cards = [{ name = "Mind Stone", qty = 2 }]"#,
        &[],
        THEIRS,
    );
    assert_eq!(got, vec![]);
}

#[test]
fn copies_outside_their_decks_go_first_and_no_more_than_wanted() {
    let got = trade(
        r#"cards = [{ name = "Sol Ring", qty = 2 }]"#,
        "",
        &[],
        THEIRS,
    );
    assert_eq!(
        got[0].pulls,
        vec![
            pull(Some("Bulk"), false, name("Sol Ring"), Finish::Nonfoil, 1),
            pull(Some("Lantern"), true, name("Sol Ring"), Finish::Nonfoil, 1),
        ]
    );
}

#[test]
fn what_the_decks_lack_adds_to_what_is_wanted_by_hand() {
    let deck = "cards = [\n  { name = \"Sol Ring\" },\n  { name = \"Island\", qty = 30 },\n]\n";
    let got = trade(
        r#"cards = [{ name = "Sol Ring" }]"#,
        "",
        &[("decks/a.deck.toml", deck)],
        THEIRS,
    );
    assert_eq!(got.len(), 1, "basics are never short: {got:?}");
    assert_eq!(got[0].short, 2);
    assert_eq!(got[0].pulls.iter().map(|p| p.qty).sum::<u32>(), 2);
}

#[test]
fn a_card_they_lack_is_not_offered() {
    let got = trade(r#"cards = [{ name = "Mana Crypt" }]"#, "", &[], THEIRS);
    assert_eq!(got, vec![]);
}
