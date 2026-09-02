//! Joining a decklist to card data.
//!
//! This is where the two halves meet. `pe-decklist` knows what a decklist is and
//! nothing about cards; `pe-scryfall` knows what a card is and nothing about
//! decklists. Neither depends on the other — the join lives here.

use std::path::Path;

use anyhow::{bail, Context, Result};
use pe_criteria::{Grouping, GroupingError};
use pe_scryfall::index::{Card, Index};

pub struct Entry {
    pub card: Card,
    pub categories: Vec<String>,
    pub qty: u32,
}

pub struct Library {
    pub entries: Vec<Entry>,
    pub commanders: Vec<String>,
}

impl Library {
    pub fn load(deck: &Path, index_path: Option<&Path>) -> Result<Self> {
        let text = std::fs::read_to_string(deck)
            .with_context(|| format!("reading decklist {}", deck.display()))?;
        let parsed = pe_decklist::parse(&text)?;

        let path = index_path
            .map(Path::to_path_buf)
            .unwrap_or_else(Index::default_path);
        let index = Index::load(&path)?;

        // Strict about unknown cards, unlike the legality checker which merely
        // reports them: you cannot compute a land count for a card you cannot
        // look up, so a typo here would silently skew every probability.
        let unknown: Vec<&str> = parsed
            .iter()
            .filter(|e| index.get(&e.name).is_none())
            .map(|e| e.name.as_str())
            .collect();
        if !unknown.is_empty() {
            bail!(
                "unknown card(s): {} — run `scryfall check` first",
                unknown.join(", ")
            );
        }

        let commanders = parsed
            .iter()
            .filter(|e| e.is_commander())
            .map(|e| e.name.clone())
            .collect();

        // The library is what you draw from: the deck minus anything outside it
        // (companions, sideboards) and minus the commanders, which start in the
        // command zone. Getting this wrong is the classic 99-versus-100 error.
        let entries = parsed
            .iter()
            .filter(|e| !e.is_outside() && !e.is_commander())
            .map(|e| Entry {
                card: index.get(&e.name).expect("checked above").clone(),
                categories: e.categories.iter().map(|c| c.name.clone()).collect(),
                qty: e.qty.get(),
            })
            .collect();

        Ok(Library {
            entries,
            commanders,
        })
    }

    pub fn size(&self) -> u32 {
        self.entries.iter().map(|e| e.qty).sum()
    }

    /// Group the library by which of `queries` each card matches.
    pub fn grouping_for(&self, queries: &[String]) -> Result<Grouping> {
        let parsed: Vec<_> = queries
            .iter()
            .map(|q| pe_scryfall::parse(q).with_context(|| format!("in query {q:?}")))
            .collect::<Result<_>>()?;

        let cards = self.entries.iter().map(|e| {
            let view = e.card.view(&e.categories);
            let mut mask = 0u64;
            for (i, q) in parsed.iter().enumerate() {
                if q.matches(&view) {
                    mask |= 1u64 << i;
                }
            }
            (mask, e.qty)
        });

        Grouping::build(queries.to_vec(), cards).map_err(|e: GroupingError| anyhow::anyhow!(e))
    }

    /// How many library cards match a query, for the per-query breakdown.
    pub fn matching(&self, query: &str) -> Result<u32> {
        let q = pe_scryfall::parse(query).with_context(|| format!("in query {query:?}"))?;
        Ok(self
            .entries
            .iter()
            .filter(|e| q.matches(&e.card.view(&e.categories)))
            .map(|e| e.qty)
            .sum())
    }
}
