//! A `.deck.toml` read into the cards a game starts with: each card's
//! Oracle facts from the card index, the commander apart from the library.

use chip_decklist::deck::{CardRef, Deck, DeckError};
use chip_scryfall::index::{Card, IndexError, IndexFile};
use thiserror::Error;

use crate::lower;
use crate::model::{pt, Color, Pt};

/// One card as the index knows it: what `lower::card` reads. Its
/// characteristics are the card's in a zone other than the stack and the
/// battlefield: a two-faced card's front face's (CR 712.8a), an adventurer
/// card's normal ones (CR 715.4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CardText {
    /// The card's name, a two-faced card's both faces' ("Delver of Secrets
    /// // Insectile Aberration").
    pub name: String,
    pub type_line: String,
    /// As printed, `{1}{G}{G}`; empty for a card with none (CR 202.1b).
    pub mana_cost: String,
    pub oracle: Oracle,
    pub mana_value: u32,
    pub colors: Vec<Color>,
    pub pt: Option<Pt>,
    /// Its colour identity (CR 903.4), which a commander gives its deck.
    pub identity: Vec<Color>,
}

/// A card's rules text, as far as the index keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Oracle {
    OneFace(String),
    /// A card with two faces the engine plays (CR 712, 715): the front's
    /// name and text, the card's own characteristics being the front's,
    /// and the other face whole.
    TwoFaces {
        layout: TwoFaced,
        front_name: String,
        front: String,
        back: Box<CardText>,
    },
    /// More than one face, of a layout the engine doesn't play yet (split,
    /// flip, prepare …), or from an index that keeps only their text
    /// joined: Scryfall's layout and the joined text.
    Faces {
        layout: String,
        joined: String,
    },
}

/// The kinds of two-faced card the engine plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TwoFaced {
    /// A modal double-faced card: either face may be cast or played, and
    /// the face chosen is up on the stack and the battlefield (CR 712.3,
    /// 712.11b, 712.12).
    Modal,
    /// An adventurer card: its back is the Adventure, whose alternative
    /// characteristics it has only on the stack cast as one (CR 715.2,
    /// 715.3b).
    Adventure,
}

/// Which face of a two-faced card (`Oracle::TwoFaces`): an adventurer
/// card's front is its normal characteristics and its back the Adventure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Front,
    Back,
}

impl CardText {
    /// The card as `lower::card` reads it, or why it can't: a card with
    /// two faces is read a face at a time (`face`).
    pub fn as_card(&self) -> Result<lower::Card<'_>, String> {
        let text = match &self.oracle {
            Oracle::OneFace(text) => text,
            Oracle::TwoFaces { .. } => {
                return Err(format!("{} has two faces, read one at a time", self.name))
            }
            Oracle::Faces { layout, .. } => {
                return Err(format!(
                    "{} is a {layout} card, a layout whose faces the engine doesn't read yet",
                    self.name
                ))
            }
        };
        Ok(lower::Card {
            name: &self.name,
            type_line: &self.type_line,
            oracle_text: text,
            mana_value: self.mana_value,
            colors: &self.colors,
            pt: self.pt,
        })
    }

    /// A card of two faces, each a card of one face: `name` the card's,
    /// its characteristics outside the stack and battlefield the front's.
    /// `None` if either face has more than one itself.
    pub fn two_faced(
        name: String,
        layout: TwoFaced,
        front: CardText,
        back: CardText,
    ) -> Option<CardText> {
        let (Oracle::OneFace(text), Oracle::OneFace(_)) = (&front.oracle, &back.oracle) else {
            return None;
        };
        Some(CardText {
            name,
            oracle: Oracle::TwoFaces {
                layout,
                front_name: front.name.clone(),
                front: text.clone(),
                back: Box::new(back),
            },
            ..front
        })
    }

    /// One face of a two-faced card as a card of its own, the face's name
    /// and characteristics; `None` for any other card.
    pub fn face(&self, side: Side) -> Option<CardText> {
        let Oracle::TwoFaces {
            front_name,
            front,
            back,
            ..
        } = &self.oracle
        else {
            return None;
        };
        Some(match side {
            Side::Front => CardText {
                name: front_name.clone(),
                oracle: Oracle::OneFace(front.clone()),
                ..self.clone()
            },
            Side::Back => (**back).clone(),
        })
    }

    /// The layout of a card with two faces the engine plays.
    pub fn layout(&self) -> Option<TwoFaced> {
        match &self.oracle {
            Oracle::TwoFaces { layout, .. } => Some(*layout),
            _ => None,
        }
    }

    /// Which face of it a log's name for it names: the back's name names
    /// the back, and any other name the front (a log prints the face it
    /// shows).
    pub fn side_named(&self, name: &str) -> Side {
        match &self.oracle {
            Oracle::TwoFaces { back, .. } if back.name == name => Side::Back,
            _ => Side::Front,
        }
    }

    /// Whether a log's name names this card: its own, or a face's, as a
    /// log prints the face it shows.
    pub fn is_called(&self, name: &str) -> bool {
        self.name == name
            || match &self.oracle {
                Oracle::TwoFaces {
                    front_name, back, ..
                } => front_name == name || back.name == name,
                _ => false,
            }
    }

    /// Its text, each face's in turn.
    pub fn rules_text(&self) -> String {
        match &self.oracle {
            Oracle::OneFace(t) => t.clone(),
            Oracle::TwoFaces { front, back, .. } => format!("{front}\n{}", back.rules_text()),
            Oracle::Faces { joined, .. } => joined.clone(),
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
            mana_cost: String::new(),
            oracle: Oracle::OneFace(format!("({{T}}: Add {{{symbol}}}.)")),
            mana_value: 0,
            colors: Vec::new(),
            pt: None,
            identity: Vec::new(),
        }
    }
}

/// What a player brings: the commander (CR 903.3) and the rest, which
/// becomes the library (CR 903.6), of which `hidden` cards no one has
/// named yet (`game`'s "Hidden cards").
#[derive(Clone, Debug, Default)]
pub struct DeckCards {
    pub commander: Vec<CardText>,
    pub library: Vec<CardText>,
    pub hidden: usize,
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
        let text = card(&name, index)?;
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

/// One card by its name, as the index knows it.
pub fn card(name: &str, index: &IndexFile) -> Result<CardText, LoadError> {
    let card = index
        .get(name)?
        .ok_or_else(|| LoadError::Unknown(name.to_string()))?;
    Ok(text(&card))
}

/// A card of the index, or of an index built from Scryfall's bulk file.
pub fn text(card: &Card) -> CardText {
    let number = |v: &Option<String>| v.as_deref().and_then(|s| s.parse::<i32>().ok());
    let colors = |c: &[String]| c.iter().filter_map(|c| color(c)).collect::<Vec<_>>();
    let identity: Vec<Color> = colors(&card.ci);
    let pt_of = |f: &chip_scryfall::index::Face| Some(pt(number(&f.power)?, number(&f.toughness)?));
    let one = |oracle| CardText {
        name: card.name.clone(),
        type_line: card.type_line.clone(),
        mana_cost: card.mana_cost.clone(),
        oracle,
        mana_value: card.cmc as u32,
        colors: colors(&card.colors),
        pt: card.faces.first().and_then(pt_of),
        identity: identity.clone(),
    };
    let [front, back] = card.faces.as_slice() else {
        return match card.faces.len() {
            0 | 1 => one(Oracle::OneFace(card.oracle.clone())),
            _ => one(Oracle::Faces {
                layout: card.layout.clone(),
                joined: card.oracle.clone(),
            }),
        };
    };
    let layout = match card.layout.as_str() {
        "modal_dfc" => TwoFaced::Modal,
        "adventure" => TwoFaced::Adventure,
        _ => {
            return one(Oracle::Faces {
                layout: card.layout.clone(),
                joined: card.oracle.clone(),
            })
        }
    };
    // An index from before faces kept their text joins it, and either
    // face's can't be told from it.
    let (Some(front_text), Some(back_text)) = (&front.oracle, &back.oracle) else {
        return one(Oracle::Faces {
            layout: card.layout.clone(),
            joined: card.oracle.clone(),
        });
    };
    let face = |f: &chip_scryfall::index::Face, text: &String| CardText {
        name: f.name.clone(),
        type_line: f.type_line.clone(),
        mana_cost: f.mana_cost.clone(),
        oracle: Oracle::OneFace(text.clone()),
        // Each face of a modal card and an Adventure has its own mana
        // cost (CR 712.8f, 715.3b).
        mana_value: mana_value(&f.mana_cost),
        colors: colors(&f.colors),
        pt: pt_of(f),
        identity: identity.clone(),
    };
    CardText::two_faced(
        card.name.clone(),
        layout,
        face(front, front_text),
        face(back, back_text),
    )
    .expect("faces of one face each")
}

/// The mana value of a printed mana cost (CR 202.3): each symbol 1, a
/// number its amount, X 0 (CR 202.3e), a monocoloured hybrid symbol 2 (CR
/// 202.3f).
fn mana_value(cost: &str) -> u32 {
    cost.split('}')
        .filter_map(|s| s.strip_prefix('{'))
        .map(|s| match s.parse::<u32>() {
            Ok(n) => n,
            Err(_) if matches!(s, "X" | "Y" | "Z") => 0,
            Err(_) if s.starts_with("2/") => 2,
            Err(_) => 1,
        })
        .sum()
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
