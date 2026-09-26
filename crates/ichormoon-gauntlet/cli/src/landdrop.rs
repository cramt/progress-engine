//! Working out which land *this* deck plays, from the priority the file
//! declared.
//!
//! The file names queries in the order it would play them and this resolves
//! them against real cards: which grouping bit each one is, whether it picks
//! out any land in this deck at all, and whether it also picks out things that
//! are not lands and therefore cannot be a land drop.
//!
//! One query is added that the file did not write: `t:land`, as the last tier.
//! A priority list is a preference and not a whitelist — a land nobody ranked
//! is still a land, and a drop you decline is a drop you never get back — so
//! the engine needs to be told what a land is in order to play the ones the
//! list is silent about. It costs a grouping bit and, in any run that prices
//! mana, no group at all: lands are already their own groups there, because
//! the gate tells a Plains from an Island.

use anyhow::Result;
use gauntlet_criteria::LandDropPolicy;

use crate::library::Library;
use crate::refusal::{self, QuerySite, Refusal};

/// The query that decides what a land is, for the tier the file did not write.
pub const ANY_LAND: &str = "t:land";

/// A declared priority, resolved against one deck.
pub struct Resolved {
    pub policy: LandDropPolicy,
    /// Extra queries the grouping needs beyond the criteria file's own: the
    /// preferences, and the catch-all. Appended after the file's queries, so
    /// the bits a clause already holds do not move.
    pub queries: Vec<String>,
    /// What the file wrote, for the report that has to say the run used it.
    pub prefer: Vec<String>,
    /// Preferences picking out no land in this deck. They decide nothing here,
    /// which is a fact about the deck rather than an error in the file — the
    /// same treatment a criteria query matching nothing gets.
    pub unmatched: Vec<String>,
    /// Preferences that also pick out cards you cannot play as a land drop,
    /// and which of them. Ignored by the policy, and said out loud because a
    /// pilot writing `otag:surveil` meant the land.
    pub non_lands: Vec<(String, Vec<String>)>,
}

/// Resolve `prefer` against `deck`. `asked` is the criteria file's own query
/// list, which already owns the low grouping bits.
pub fn resolve(prefer: &[String], deck: &Library, asked: &[String]) -> Result<Resolved> {
    let mut queries: Vec<String> = Vec::new();
    // A preference repeating one of the file's own queries reuses its bit:
    // same query, same cards, and a second bit for one question would split
    // every group in the deck along a line that means nothing.
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

    let mut tiers = Vec::with_capacity(prefer.len());
    let mut unmatched = Vec::new();
    let mut non_lands = Vec::new();
    for query in prefer {
        tiers.push(bit_of(query, &mut queries));
        if deck.lands_matching(query)? == 0 {
            unmatched.push(query.clone());
        }
        let spells = deck.non_lands_matching(query)?;
        if !spells.is_empty() {
            non_lands.push((query.clone(), spells));
        }
    }
    let any_land = bit_of(ANY_LAND, &mut queries);

    Ok(Resolved {
        policy: LandDropPolicy::new(tiers, any_land),
        queries,
        prefer: prefer.to_vec(),
        unmatched,
        non_lands,
    })
}

/// Parse every preference before anything is grouped, so a query this index
/// cannot answer is refused against the table that wrote it.
///
/// The same seam the criteria file's own queries go through, and for the same
/// reason: an `otag:` this index never fetched matches nothing, which here
/// would silently demote a whole tier rather than report a gap.
pub fn check(prefer: &[String], deck: &Library, file: &str) -> Result<(), Refusal> {
    for query in prefer {
        refusal::check_query(file, QuerySite::LandDrop, query, deck)?;
    }
    Ok(())
}
