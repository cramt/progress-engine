//! A `.deck.toml` read into the cards a game starts with: each card's
//! Oracle facts from the card index, the commander apart from the library.

use chip_decklist::deck::{CardRef, Deck, DeckError};
use chip_scryfall::index::{IndexError, IndexFile};
use thiserror::Error;

use crate::lower;
use crate::model::{pt, Color, Pt};

/// One card as the index knows it: what `lower::card` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardText {
    pub name: String,
    pub type_line: String,
    pub oracle: Oracle,
    pub mana_value: u32,
    pub colors: Vec<Color>,
    pub pt: Option<Pt>,
}

/// A card's rules text, as far as the index keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Oracle {
    OneFace(String),
    /// The index joins a multi-faced card's faces' text into one and keeps
    /// no face's own, so it can't be read as either face.
    JoinedFaces(String),
}

impl CardText {
    /// The card as `lower::card` reads it, or why it can't.
    pub fn as_card(&self) -> Result<lower::Card<'_>, String> {
        match &self.oracle {
            Oracle::OneFace(text) => Ok(lower::Card {
                name: &self.name,
                type_line: &self.type_line,
                oracle_text: text,
                mana_value: self.mana_value,
                colors: &self.colors,
                pt: self.pt,
            }),
            Oracle::JoinedFaces(_) => Err(format!(
                "{} has more than one face, and the card index keeps only their text joined",
                self.name
            )),
        }
    }

    /// A basic land with its reminder text, for decks built by hand.
    pub fn basic(name: &str) -> CardText {
        let symbol = match name {
            "Plains" => "W",
            "Island" => "U",
            "Swamp" => "B",
            "Mountain" => "R",
            "Forest" => "G",
            other => panic!("{other} is not a basic land type"),
        };
        CardText {
            name: name.to_string(),
            type_line: format!("Basic Land — {name}"),
            oracle: Oracle::OneFace(format!("({{T}}: Add {{{symbol}}}.)")),
            mana_value: 0,
            colors: Vec::new(),
            pt: None,
        }
    }
}

/// What a player brings: the commander (CR 903.3) and the rest, which
/// becomes the library (CR 903.6).
#[derive(Clone, Debug, Default)]
pub struct DeckCards {
    pub commander: Vec<CardText>,
    pub library: Vec<CardText>,
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error(transparent)]
    Deck(#[from] DeckError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error("{0} is not in the card index")]
    Unknown(String),
}

/// The deck file's cards counted in the deck, each looked up by name or
/// printing. A card outside the deck (sideboard, maybeboard) is left out.
pub fn read(text: &str, index: &IndexFile) -> Result<DeckCards, LoadError> {
    let deck = Deck::parse(text)?;
    let mut cards = DeckCards::default();
    for line in deck.cards.iter().filter(|c| c.in_deck()) {
        let name = match &line.card {
            CardRef::Name(n) => n.clone(),
            CardRef::Printing(p) => index
                .printing(&p.set, &p.num)?
                .ok_or_else(|| LoadError::Unknown(p.to_string()))?,
        };
        let card = index
            .get(&name)?
            .ok_or_else(|| LoadError::Unknown(name.clone()))?;
        let face = card.faces.first();
        let number = |v: &Option<String>| v.as_deref().and_then(|s| s.parse::<i32>().ok());
        let text = CardText {
            name: card.name.clone(),
            type_line: card.type_line.clone(),
            oracle: if card.faces.len() > 1 {
                Oracle::JoinedFaces(card.oracle.clone())
            } else {
                Oracle::OneFace(card.oracle.clone())
            },
            mana_value: card.cmc as u32,
            colors: card.colors.iter().filter_map(|c| color(c)).collect(),
            pt: face.and_then(|f| Some(pt(number(&f.power)?, number(&f.toughness)?))),
        };
        let into = if line.is_commander() {
            &mut cards.commander
        } else {
            &mut cards.library
        };
        for _ in 0..line.qty.get() {
            into.push(text.clone());
        }
    }
    Ok(cards)
}

fn color(letter: &str) -> Option<Color> {
    Some(match letter {
        "W" => Color::White,
        "U" => Color::Blue,
        "B" => Color::Black,
        "R" => Color::Red,
        "G" => Color::Green,
        _ => return None,
    })
}
