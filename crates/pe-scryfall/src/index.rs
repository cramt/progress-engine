//! Reading the cached Scryfall card index.
//!
//! The index is built and refreshed by the `scryfall sync` shell tool; this
//! crate only reads it. That split is deliberate — fetching bulk data, respecting
//! rate limits and caching are already solved there, and duplicating them here
//! would mean two things that could disagree about what a card is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use facet::Facet;
use thiserror::Error;

use crate::CardView;

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
}

#[derive(Debug, Clone, Facet)]
pub struct Index {
    pub cards: HashMap<String, Card>,
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
            color_identity: &self.ci,
            categories,
        }
    }
}
