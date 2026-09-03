//! Reading the cached Scryfall card index.
//!
//! The index is built and refreshed by the `scryfall sync` shell tool; this
//! crate only reads it. That split is deliberate — fetching bulk data, respecting
//! rate limits and caching are already solved there, and duplicating them here
//! would mean two things that could disagree about what a card is.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use facet::Facet;
use thiserror::Error;

use crate::legality::{self, CommanderLegality, CommanderRoute, LegalityWord};
use crate::{CardView, Colors, OutsideLibrary};

#[derive(Debug, Error)]
pub enum IndexError {
    #[error(
        "no Scryfall index at {0}.\n\
         Build one with: scryfall sync"
    )]
    Missing(PathBuf),
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Boxed because facet-json's error carries the source text and spans it
    /// needs for a pointed diagnostic, which is far larger than the ok variant.
    #[error("parsing {path}: {source}")]
    Json {
        path: PathBuf,
        source: Box<facet_json::DeserializeError>,
    },
}

/// One card, as the index stores it.
#[derive(Debug, Clone, Facet)]
pub struct Card {
    pub name: String,
    #[facet(default)]
    pub ci: Vec<String>,
    #[facet(default)]
    pub type_line: String,
    #[facet(default)]
    pub cmc: f64,
    /// Oracle text of every face joined, so a query sees the whole card.
    #[facet(default)]
    pub oracle: String,
    /// Keyword abilities, keyword actions and ability words as Scryfall prints
    /// them, e.g. `["Hexproof from", "Hexproof"]`. Read through `kw:` rather
    /// than the oracle text: `o:flying` also matches "creatures with flying
    /// can't block".
    ///
    /// An absent field reads as empty, because the fixtures are hand-written
    /// subsets of the index and routinely omit it. That errs in the direction
    /// this crate insists on — a `kw:` term over an index that never carried
    /// keywords matches nothing rather than everything — but it does mean a
    /// negated `-kw:flying` is reading silence as proof of absence.
    #[facet(default)]
    pub keywords: Vec<String>,
    /// Scryfall's Commander legality word. Read it through
    /// [`Card::commander_legality`] rather than comparing the text.
    #[facet(default)]
    pub commander_legal: LegalityWord,
    /// Scryfall's flag for the handful of cards that lift the singleton rule.
    /// `None` when the index never said — see
    /// [`Card::may_appear_any_number_of_times`].
    #[facet(default)]
    pub any_number: Option<bool>,
}

#[derive(Debug, Clone, Facet)]
pub struct Index {
    pub cards: HashMap<String, Card>,
    /// When `scryfall sync` last rebuilt this index, as it wrote it.
    ///
    /// `None` is an answer rather than a failure, for the same reason a missing
    /// legality word is: the index is a cache built by an external tool and the
    /// test fixtures are hand-written subsets of it, so the field is routinely
    /// absent. A caller that reports it must say *unknown* rather than invent a
    /// date — a report claiming today's index when nobody knows which index ran
    /// is the confidently wrong number this project exists to prevent.
    ///
    /// Kept as the text the index wrote rather than a parsed timestamp: parsing
    /// would reject a 25MB index over one unfamiliar format, and nothing here
    /// does arithmetic on it.
    #[facet(default)]
    pub updated_at: Option<String>,
}

/// Normalise a card name to the index's key form: lowercase and trimmed.
///
/// Must match how the index was built, or every lookup misses.
pub fn keyname(name: &str) -> String {
    name.trim().to_lowercase()
}

impl Index {
    /// Where `scryfall sync` puts the index.
    pub fn default_path() -> PathBuf {
        if let Ok(dir) = std::env::var("SCRYFALL_CACHE") {
            return PathBuf::from(dir).join("index.json");
        }
        let base = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
            });
        base.join("scryfall").join("index.json")
    }

    pub fn load(path: &Path) -> Result<Self, IndexError> {
        if !path.exists() {
            return Err(IndexError::Missing(path.to_path_buf()));
        }
        let text = std::fs::read_to_string(path).map_err(|source| IndexError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        facet_json::from_str(&text).map_err(|source| IndexError::Json {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
    }

    pub fn get(&self, name: &str) -> Option<&Card> {
        self.cards.get(&keyname(name))
    }

    /// Every keyword any card here carries, lowercased for comparison.
    ///
    /// Scryfall answers `kw:tramp` with *Unknown keyword "tramp"* rather than
    /// an empty result, and it is right to: a mistyped keyword that quietly
    /// matches zero cards is the confident 0% this project exists to prevent.
    /// The parser cannot make that check on its own — the set of real keywords
    /// grows with every set, so a list hard-coded next to the parser would be a
    /// second opinion about what a card is, and would start refusing real
    /// queries the day it fell behind. The index is the authority, so the check
    /// lives here; see [`crate::Query::unknown_keywords`].
    pub fn keyword_vocabulary(&self) -> KeywordVocabulary {
        KeywordVocabulary {
            known: self
                .cards
                .values()
                .flat_map(|c| c.keywords.iter())
                .map(|k| k.to_lowercase())
                .collect(),
        }
    }
}

/// The keywords an index knows about, for checking a `kw:` term against.
#[derive(Debug, Clone, Default)]
pub struct KeywordVocabulary {
    known: HashSet<String>,
}

impl KeywordVocabulary {
    pub fn contains(&self, keyword: &str) -> bool {
        self.known.contains(&keyword.to_lowercase())
    }

    /// Whether the index said anything at all about keywords. Not public:
    /// callers ask [`crate::Query::unknown_keywords`], which already knows an
    /// index silent about keywords proves nothing about any keyword.
    pub(crate) fn is_empty(&self) -> bool {
        self.known.is_empty()
    }
}

impl Card {
    /// Join this card with the categories a decklist gave it.
    ///
    /// The seam between card data and decklist data: categories arrive as a
    /// plain slice so this crate never has to know what a decklist is.
    pub fn view<'a>(&'a self, categories: &'a [String]) -> CardView<'a> {
        CardView {
            name: &self.name,
            type_line: &self.type_line,
            oracle: &self.oracle,
            cmc: self.cmc,
            keywords: &self.keywords,
            color_identity: &self.ci,
            categories,
        }
    }

    /// Which never-in-the-library type this card is, if it is one.
    pub fn outside_library(&self) -> Option<OutsideLibrary> {
        crate::outside_library(&self.type_line)
    }

    /// What the index says about this card's Commander legality.
    pub fn commander_legality(&self) -> CommanderLegality {
        self.commander_legal.commander()
    }

    /// Whether a Commander deck may contain this card, or `None` when the
    /// index never said.
    pub fn may_be_played_in_commander(&self) -> Option<bool> {
        self.commander_legality().permits_play()
    }

    /// Which route, if any, puts this card in the command zone.
    pub fn commander_route(&self) -> Option<CommanderRoute> {
        legality::commander_route(&self.name, &self.type_line, &self.oracle)
    }

    /// Whether a deck may contain any number of copies, or `None` when the
    /// index never said.
    ///
    /// The type line settles it for anything with the `Basic` supertype, so an
    /// index built before that field existed still answers for the eleven
    /// Forests in a green deck instead of shrugging at them.
    pub fn may_appear_any_number_of_times(&self) -> Option<bool> {
        if legality::has_basic_supertype(&self.type_line) {
            return Some(true);
        }
        self.any_number
    }

    /// Whether this card's colour identity fits inside the one a deck allows.
    pub fn identity_fits_within(&self, allowed: Colors) -> bool {
        legality::identity_fits_within(&self.ci, allowed)
    }
}
