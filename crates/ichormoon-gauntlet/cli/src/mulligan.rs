//! Working out which openers *this* deck keeps, and what it puts back, from the
//! mulligan the file declared.
//!
//! The same job [`crate::landdrop`] does for the land drop: the file names
//! queries, and this resolves them against real cards — which grouping bit each
//! one is, and whether it picks out anything in this deck at all. Nothing is
//! added that the file did not write. A land-drop priority needs to be told
//! what a land is so it can play one nobody ranked; a bottoming list needs no
//! such query, because "every card the list did not name" is a set of groups
//! the board can compute for itself.

use anyhow::{anyhow, Result};
use gauntlet_criteria::{Keep, MulliganPolicy};
use gauntlet_toml::MulliganDecl;

use crate::library::Library;

/// A declared mulligan, resolved against one deck.
pub struct Resolved {
    pub policy: MulliganPolicy,
    /// Extra queries the grouping needs beyond what is already asked for,
    /// appended after it so no bit a clause already holds moves.
    pub queries: Vec<String>,
    /// Every query the policy reads, keep rule and bottoming list alike, as
    /// grouping bits. Every class keeps them, because they decide which hand
    /// was played at all.
    pub bits: u64,
    /// Queries in the keep rule or the bottoming list that pick out no card
    /// in this deck. A keep clause asking for at least one of them throws back
    /// every hand, which is the deck's answer rather than a mistake in the
    /// file — and it is said out loud.
    pub unmatched: Vec<String>,
}

/// Resolve `declared` against `deck`. `asked` is every query already given a
/// grouping bit, which the keep rule and the bottoming list reuse where they
/// repeat one.
pub fn resolve(declared: &MulliganDecl, deck: &Library, asked: &[String]) -> Result<Resolved> {
    let mut queries: Vec<String> = Vec::new();
    // The same rule the land drop's list follows: the same query is the same
    // cards, and a second bit for it would split every group along nothing.
    let bit_of = |query: &str, queries: &mut Vec<String>| -> usize {
        if let Some(i) = asked.iter().position(|a| a == query) {
            return i;
        }
        match queries.iter().position(|q| q == query) {
            Some(i) => asked.len() + i,
            None => {
                queries.push(query.to_string());
                asked.len() + queries.len() - 1
            }
        }
    };
    let mut unmatched = Vec::new();
    let mut note = |query: &str| -> Result<()> {
        if deck.matching(query)? == 0 && !unmatched.iter().any(|u| u == query) {
            unmatched.push(query.to_string());
        }
        Ok(())
    };

    let mut keep = Vec::with_capacity(declared.keep.len());
    for clause in &declared.keep {
        keep.push(Keep {
            query: bit_of(&clause.query, &mut queries),
            min: clause.min,
            max: clause.max,
        });
        note(&clause.query)?;
    }
    let mut bottom = Vec::with_capacity(declared.bottom.len());
    for query in &declared.bottom {
        bottom.push(bit_of(query, &mut queries));
        note(query)?;
    }
    let bits = keep
        .iter()
        .map(|k| k.query)
        .chain(bottom.iter().copied())
        .fold(0u64, |bits, q| bits | 1u64 << q);

    Ok(Resolved {
        policy: MulliganPolicy::new(keep, bottom, declared.down_to),
        queries,
        bits,
        unmatched,
    })
}

/// Parse every query the mulligan names before anything is grouped, so one this
/// index cannot answer is refused against the clause that wrote it.
///
/// The same seam the criteria file's own queries go through. An `otag:` this
/// index never fetched matches nothing, which in a keep rule would throw back
/// every hand — a mulligan that reads as brutal where it is only blind.
pub fn check(declared: &MulliganDecl, deck: &Library) -> Result<()> {
    let named = declared
        .keep
        .iter()
        .enumerate()
        .map(|(i, k)| (format!("keep clause {}", i + 1), k.query.as_str()))
        .chain(
            declared
                .bottom
                .iter()
                .enumerate()
                .map(|(i, q)| (format!("`bottom` entry {}", i + 1), q.as_str())),
        );
    for (at, query) in named {
        let parsed = chip_scryfall::parse(query)
            .map_err(|e| anyhow!("[mulligan]: {at}, query {query:?}: {e}"))?;
        if let Some(gap) = parsed.tag_gap(&deck.index_tags) {
            anyhow::bail!(
                "[mulligan]: {at}, query {query:?}: {}",
                crate::report::tag_gap_refusal(&gap, deck)
            );
        }
        let unknown = parsed.unknown_keywords(&deck.index_keywords);
        if !unknown.is_empty() {
            anyhow::bail!(
                "[mulligan]: {at}, query {query:?}: {}",
                crate::report::unknown_keyword_refusal(&unknown)
            );
        }
    }
    Ok(())
}

/// A keep clause as the report prints it: `2-5 of "t:land"`.
pub fn describe(clause: &gauntlet_toml::KeepDecl) -> String {
    let range = match (clause.min, clause.max) {
        (min, None) => format!("at least {min}"),
        (0, Some(max)) => format!("at most {max}"),
        (min, Some(max)) if min == max => format!("exactly {min}"),
        (min, Some(max)) => format!("{min} to {max}"),
    };
    format!("{range} of {:?}", clause.query)
}
