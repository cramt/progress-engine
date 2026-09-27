//! Edits change what they say and nothing else in the file.

use chip_decklist::deck::CategoryType;
use chip_decklist::edit::{declare_category, set_categories, EditError};

const DECK: &str = r#"name = "Izzet Lessons"

# The lessons live in the sideboard.
cards = [
  { printing = "tla/46", in = ["learnboard", "tempo"] },  # Boomerang Basics
  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration
  { name = "Island", qty = 8 },
]

[categories]
learnboard = { type = "sideboard" }
tempo = {}
self-bounce = {}
"#;

fn changed_lines(a: &str, b: &str) -> Vec<(String, String)> {
    assert_eq!(a.lines().count(), b.lines().count(), "{b}");
    a.lines()
        .zip(b.lines())
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect()
}

#[test]
fn recategorising_a_card_is_a_one_line_diff_that_keeps_its_comment() {
    let edited = set_categories(DECK, 1, &["tempo".into(), "self-bounce".into()]).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        vec![(
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#.into(),
            r#"  { printing = "msc/183", qty = 4, in = ["tempo", "self-bounce"] },  # Expressive Iteration"#.into(),
        )]
    );
}

#[test]
fn a_card_without_categories_gains_an_in_key() {
    let edited = set_categories(DECK, 2, &["tempo".into()]).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited)[0].1,
        r#"  { name = "Island", qty = 8, in = ["tempo"] },"#
    );
}

#[test]
fn an_edit_the_format_refuses_is_refused() {
    let err = set_categories(DECK, 0, &["nonsense".into()]).unwrap_err();
    assert!(matches!(err, EditError::Invalid(_)), "{err}");
    assert_eq!(
        set_categories(DECK, 9, &[]).unwrap_err(),
        EditError::NoCard(9)
    );
}

#[test]
fn declaring_a_category_appends_it_and_touches_nothing_else() {
    let edited = declare_category(DECK, "Maybe", Some(CategoryType::Maybeboard)).unwrap();
    assert!(edited.starts_with(DECK), "{edited}");
    assert!(
        edited.ends_with("Maybe = { type = \"maybeboard\" }\n"),
        "{edited}"
    );
    assert_eq!(declare_category(DECK, "tempo", None).unwrap(), DECK);
    assert!(matches!(
        declare_category(DECK, "tempo", Some(CategoryType::Sideboard)).unwrap_err(),
        EditError::DeclaredDifferently { .. }
    ));
}
