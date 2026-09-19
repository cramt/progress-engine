//! Reading Scryfall's bulk card data, and reducing it to the index.
//!
//! Scryfall publishes a card object with about sixty fields; this crate reads
//! seventeen of them. The reduction happens here, once, at sync time, rather
//! than at query time — so the shape a query sees is decided in one place that
//! can be tested against real bulk records.
//!
//! Two invariants of the bulk data are load-bearing, and both are *checked*
//! rather than assumed. Silence about a violated assumption is how the index
//! became wrong the last time; see [`Anomaly`].

use std::collections::BTreeMap;

use facet::Facet;

use crate::index::{Card, Face};

/// One card object as Scryfall's bulk data writes it.
///
/// Every field is optional, because a field this crate reads is a field
/// Scryfall may one day stop printing, and a sync that dies on the whole file
/// over one absent key is worse than one that reports what it could not find.
/// `name` is the exception: a record with no name cannot be keyed, so it is not
/// a card as far as this crate is concerned.
#[derive(Facet, Debug, Clone)]
pub struct BulkCard {
    pub name: String,
    #[facet(default)]
    pub oracle_id: Option<String>,
    #[facet(default)]
    pub lang: Option<String>,
    #[facet(default)]
    pub layout: Option<String>,
    #[facet(default)]
    pub type_line: Option<String>,
    #[facet(default)]
    pub oracle_text: Option<String>,
    #[facet(default)]
    pub mana_cost: Option<String>,
    #[facet(default)]
    pub cmc: Option<f64>,
    #[facet(default)]
    pub colors: Option<Vec<String>>,
    #[facet(default)]
    pub color_indicator: Option<Vec<String>>,
    #[facet(default)]
    pub color_identity: Option<Vec<String>>,
    #[facet(default)]
    pub produced_mana: Option<Vec<String>>,
    #[facet(default)]
    pub keywords: Option<Vec<String>>,
    #[facet(default)]
    pub power: Option<String>,
    #[facet(default)]
    pub toughness: Option<String>,
    #[facet(default)]
    pub loyalty: Option<String>,
    #[facet(default)]
    pub defense: Option<String>,
    #[facet(default)]
    pub rarity: Option<String>,
    #[facet(default)]
    pub set: Option<String>,
    #[facet(default)]
    pub legalities: BTreeMap<String, String>,
    #[facet(default)]
    pub game_changer: Option<bool>,
    #[facet(default)]
    pub reserved: Option<bool>,
    #[facet(default)]
    pub card_faces: Option<Vec<BulkFace>>,
}

/// One face of a multi-faced card, as Scryfall writes it.
#[derive(Facet, Debug, Clone)]
pub struct BulkFace {
    #[facet(default)]
    pub name: Option<String>,
    #[facet(default)]
    pub type_line: Option<String>,
    #[facet(default)]
    pub oracle_text: Option<String>,
    #[facet(default)]
    pub mana_cost: Option<String>,
    #[facet(default)]
    pub colors: Option<Vec<String>>,
    #[facet(default)]
    pub color_indicator: Option<Vec<String>>,
    #[facet(default)]
    pub power: Option<String>,
    #[facet(default)]
    pub toughness: Option<String>,
    #[facet(default)]
    pub loyalty: Option<String>,
    #[facet(default)]
    pub defense: Option<String>,
}

/// Why a bulk record did not become a card in the index.
///
/// Named rather than counted, because "we dropped 3,300 records" is not a
/// statement anybody can check. `sync` prints these by reason so that a number
/// that moves between two syncs can be attributed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Skipped {
    /// A token, emblem or art card. Not a card you can put in a deck, and the
    /// direct cause of the 41 real cards an earlier index reported as tokens:
    /// they share a name with one, and whichever was written last won the key.
    NotACard,
    /// Not English. Scryfall keys Oracle text by language and this index is
    /// keyed by English name, so a Japanese record would collide with the
    /// English one it duplicates.
    NotEnglish,
}

impl Skipped {
    pub fn as_str(self) -> &'static str {
        match self {
            Skipped::NotACard => "not a card (token, emblem or art card)",
            Skipped::NotEnglish => "not English",
        }
    }
}

/// Layouts that are printed like cards but are never cards you can play.
///
/// The discriminator is the layout and nothing else. `set_type` is not it —
/// tokens ship in `memorabilia`, `promo`, `masters` and `box` sets, and the
/// `emblem` layout ships in sets typed `token`. Nor is the words "Token" in the
/// type line: fifty token records do not carry it, and `Token Land` still
/// contains "Land", which is why `t:land` accidentally survived the bug and
/// `mv>=5` did not.
const NOT_CARD_LAYOUTS: [&str; 4] = ["token", "double_faced_token", "emblem", "art_series"];

/// Something the bulk data did that this module was not expecting.
///
/// An anomaly is not fatal — one strange record must not cost you the other
/// 38,625 — but it is never silent either. `sync` prints them and, past a
/// threshold, refuses to install the result, because a bulk format that has
/// moved under us produces an index that is wrong in ways no query can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anomaly {
    /// Scryfall's data puts oracle text either on the card or on its faces,
    /// never both and never neither. Checked rather than assumed, because the
    /// whole face-flattening below rests on it.
    OracleTextAndFaces {
        name: String,
    },
    NeitherOracleTextNorFaces {
        name: String,
    },
    /// A record with no type line. Every one of the 38,626 records has one
    /// today, and a card without one answers `t:` wrongly rather than loudly.
    NoTypeLine {
        name: String,
    },
    /// Two cards that are genuinely different but share a lowercased name.
    /// The index is keyed by name, so one of them is unreachable — and a
    /// decklist naming it gets the other one's mana value.
    NameCollision {
        name: String,
        kept: String,
    },
}

impl std::fmt::Display for Anomaly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Anomaly::OracleTextAndFaces { name } => {
                write!(f, "{name}: has both oracle_text and card_faces")
            }
            Anomaly::NeitherOracleTextNorFaces { name } => {
                write!(f, "{name}: has neither oracle_text nor card_faces")
            }
            Anomaly::NoTypeLine { name } => write!(f, "{name}: has no type line"),
            Anomaly::NameCollision { name, kept } => write!(
                f,
                "{name:?} names more than one card; kept the printing from {kept}"
            ),
        }
    }
}

impl BulkCard {
    /// Whether this record is a card you could put in a deck.
    fn skipped(&self) -> Option<Skipped> {
        let layout = self.layout.as_deref().unwrap_or("");
        if NOT_CARD_LAYOUTS.contains(&layout) {
            return Some(Skipped::NotACard);
        }
        // Absent reads as English: `lang` is present on every record today, and
        // treating silence as "foreign" would drop the whole file if it ever
        // stopped being.
        match self.lang.as_deref() {
            Some(l) if l != "en" => Some(Skipped::NotEnglish),
            _ => None,
        }
    }

    /// Reduce this record to the index's shape, collecting anything surprising.
    ///
    /// A record that is not a card comes back as the reason it was left out,
    /// so the caller counts it rather than having to ask a second time and
    /// risk the two answers drifting apart.
    pub fn to_card(&self, anomalies: &mut Vec<Anomaly>) -> Result<Card, Skipped> {
        if let Some(reason) = self.skipped() {
            return Err(reason);
        }

        let has_text = self.oracle_text.is_some();
        let has_faces = self.card_faces.as_ref().is_some_and(|f| !f.is_empty());
        match (has_text, has_faces) {
            (true, true) => anomalies.push(Anomaly::OracleTextAndFaces {
                name: self.name.clone(),
            }),
            (false, false) => anomalies.push(Anomaly::NeitherOracleTextNorFaces {
                name: self.name.clone(),
            }),
            _ => {}
        }
        if self.type_line.is_none() {
            anomalies.push(Anomaly::NoTypeLine {
                name: self.name.clone(),
            });
        }

        let faces = self.faces();
        // The joined text is what `o:` and `fo:` search. Faces are joined with a
        // newline rather than Scryfall's " // " so that a phrase search cannot
        // match across the seam between two faces, which is text no card has.
        let full_oracle = self
            .face_texts()
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let oracle = strip_reminder_text(&full_oracle);
        let any_number = says_a_deck_can_have_any_number(&full_oracle);

        Ok(Card {
            name: self.name.clone(),
            oracle_id: self.oracle_id.clone(),
            layout: self.layout.clone().unwrap_or_default(),
            type_line: self.type_line.clone().unwrap_or_default(),
            cmc: self.cmc.unwrap_or(0.0),
            ci: self.color_identity.clone().unwrap_or_default(),
            colors: self.whole_card_colors(&faces),
            mana_cost: self.whole_card_mana_cost(&faces),
            produces: self.produced_mana.clone().unwrap_or_default(),
            keywords: self.keywords.clone().unwrap_or_default(),
            rarity: self.rarity.clone().unwrap_or_default(),
            set: self.set.clone().unwrap_or_default(),
            legalities: crate::legality::Legalities::from_words(&self.legalities),
            // Written only when it differs, which is 28% of cards: reminder
            // text is the difference between `o:` and `fo:`, and storing the
            // same string twice for the other 72% is three megabytes to say
            // nothing.
            full_oracle: (full_oracle != oracle).then_some(full_oracle),
            oracle,
            any_number: Some(any_number),
            game_changer: self.game_changer,
            reserved: self.reserved,
            faces,
            commander_legal: Default::default(),
            // Bulk data carries no tags. Membership is Scryfall's answer to a
            // search, so it is attached after this, by whoever did the asking.
            tags: Vec::new(),
        })
    }

    /// This card's faces, as one uniform list.
    ///
    /// A single-faced card gets exactly one face, so nothing downstream has to
    /// branch on how many there are. That is the point: Scryfall's own shape
    /// makes "power" mean the card's power on one layout and nothing at all on
    /// another, and every reader of it would need the same three-case match.
    fn faces(&self) -> Vec<Face> {
        match self.card_faces.as_ref().filter(|f| !f.is_empty()) {
            Some(faces) => faces.iter().map(|f| self.face(f)).collect(),
            None => vec![Face {
                name: self.name.clone(),
                type_line: self.type_line.clone().unwrap_or_default(),
                mana_cost: self.mana_cost.clone().unwrap_or_default(),
                colors: self.face_colors(self.colors.as_ref(), self.color_indicator.as_ref()),
                power: self.power.clone(),
                toughness: self.toughness.clone(),
                loyalty: self.loyalty.clone(),
                defense: self.defense.clone(),
            }],
        }
    }

    /// The oracle text of each face, in printed order.
    ///
    /// Separate from [`Self::faces`] because the index keeps the text joined at
    /// card level and the typed values per face; the two are collected here
    /// from the same place so they cannot come from different readings of the
    /// same record.
    fn face_texts(&self) -> Vec<String> {
        match self.card_faces.as_ref().filter(|f| !f.is_empty()) {
            Some(faces) => faces
                .iter()
                .map(|f| f.oracle_text.clone().unwrap_or_default())
                .collect(),
            None => vec![self.oracle_text.clone().unwrap_or_default()],
        }
    }

    fn face(&self, f: &BulkFace) -> Face {
        Face {
            name: f.name.clone().unwrap_or_default(),
            type_line: f.type_line.clone().unwrap_or_default(),
            // A back face with no cost prints as an empty string rather than
            // being absent, and that is the honest reading: it has a cost, and
            // the cost is nothing.
            mana_cost: f.mana_cost.clone().unwrap_or_default(),
            colors: self.face_colors(f.colors.as_ref(), f.color_indicator.as_ref()),
            power: f.power.clone(),
            toughness: f.toughness.clone(),
            loyalty: f.loyalty.clone(),
            defense: f.defense.clone(),
        }
    }

    /// A face's colours, falling back to its colour indicator.
    ///
    /// The back of a transforming card has no mana cost, so Scryfall states its
    /// colour with an indicator instead. Reading only `colors` would call
    /// Insectile Aberration colourless, and a colourless blue creature is the
    /// quiet miscategorisation this crate exists to prevent.
    fn face_colors(
        &self,
        colors: Option<&Vec<String>>,
        indicator: Option<&Vec<String>>,
    ) -> Vec<String> {
        match colors {
            Some(c) if !c.is_empty() => c.clone(),
            _ => indicator.cloned().unwrap_or_default(),
        }
    }

    /// The colours of the whole card, which is the union of its faces'.
    ///
    /// Scryfall states it at the top level for the layouts whose faces share
    /// one object, and omits it for the layouts whose faces are separate. Where
    /// it is stated it is already the union, so the two agree.
    fn whole_card_colors(&self, faces: &[Face]) -> Vec<String> {
        if let Some(c) = self.colors.as_ref().filter(|c| !c.is_empty()) {
            return c.clone();
        }
        let mut out: Vec<String> = Vec::new();
        for f in faces {
            for c in &f.colors {
                if !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        out
    }

    /// The whole card's mana cost, joined as Scryfall joins it.
    fn whole_card_mana_cost(&self, faces: &[Face]) -> String {
        if let Some(m) = self.mana_cost.as_ref().filter(|m| !m.is_empty()) {
            return m.clone();
        }
        faces
            .iter()
            .map(|f| f.mana_cost.as_str())
            .collect::<Vec<_>>()
            .join(" // ")
            .trim_matches(|c: char| c == ' ' || c == '/')
            .to_string()
    }
}

/// The ten cards that lift the singleton rule, read off their own text.
///
/// Scryfall has no field for this, and it is the one derivation in this module
/// that reads oracle text. It is safe to make because the rule is printed in
/// exactly one sentence — "A deck can have any number of cards named ..." —
/// which was surveyed across all 38,626 records and appears in that form and no
/// other. The *bounded* rule ("up to seven") is deliberately not derived; see
/// the README.
fn says_a_deck_can_have_any_number(oracle: &str) -> bool {
    const PHRASE: &str = "a deck can have any number of cards named";
    oracle.to_lowercase().contains(PHRASE)
}

/// Oracle text with its reminder text removed, which is what `o:` searches.
///
/// Scryfall's bulk data prints reminder text inline — "Vigilance (Attacking
/// doesn't cause this creature to tap.)" — and Scryfall's own `o:` does not
/// search it, while `fo:` does. Without this, `o:flying` matches every card
/// whose *reminder* text mentions flying, which is the false positive the
/// README opens with wearing different clothes.
///
/// Parentheses nest in five cards, so this counts depth rather than scanning
/// for the next `)`. Where they do not balance — a handful of reminder cards
/// split their text across two records — the text is returned **unchanged**
/// rather than truncated at the stray bracket: `o:` matching some reminder text
/// is a far smaller error than `o:` silently losing a card's actual rules.
pub fn strip_reminder_text(text: &str) -> String {
    if !text.contains('(') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => return text.to_string(), // closed one that never opened
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    if depth != 0 {
        return text.to_string(); // opened one that never closed
    }
    // Removing a bracketed span leaves the spaces that surrounded it back to
    // back, and "Flying  Vigilance" would not match a search for the phrase.
    out.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
