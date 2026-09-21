//! Working out which spells *this* deck casts, from the priority the file
//! declared, and what each of them costs.
//!
//! The file names queries in the order it would cast them and this resolves
//! them against real cards: which grouping bit each one is, what the cards it
//! picks out cost, and which of them cannot be cast at all.
//!
//! **No catch-all is added, and that is the one place this differs from
//! [`crate::landdrop`].** A land nobody ranked is still played, because a drop
//! you decline is a drop you never get back and because "any other land" costs
//! one query bit. "Any other spell" is not one bit: every spell in the deck
//! would have to carry its own cost into the grouping, which splits a
//! Commander library forty ways along a line nobody asked about. So the list
//! is the **line** — the spells this question is about — and a spell it does
//! not name is one this run declines to cast. That is the same reading the
//! gate already takes of the land drop, and every run that uses one says so.
//!
//! What is refused here is a cost this engine cannot pay. A priority naming
//! Bala Ged Recovery, or any `{X}` spell, would otherwise be cast for some
//! amount nobody chose — and a spell cast too cheaply spends mana the rest of
//! the line then does not have, so the error is not confined to that card.

use anyhow::{anyhow, Result};
use gauntlet_criteria::{CastingPolicy, Cost, Demand, Palette};

use crate::library::{is_land, Library};

/// A declared casting priority, resolved against one deck.
pub struct Resolved {
    pub policy: CastingPolicy,
    /// Extra queries the grouping needs beyond what the file already asked
    /// for. Appended after the criteria file's own and the land drop's, so the
    /// bits a clause already holds do not move.
    pub queries: Vec<String>,
    /// What the file wrote, for the report that has to say the run used it.
    pub prefer: Vec<String>,
    /// What each library entry costs this run, `None` for every card the
    /// priority does not name. One entry per [`Library::entries`] position,
    /// which is the only thing it is meaningful beside.
    pub costs: Vec<Option<Demand>>,
    /// The pip kinds every cost in this line demands, joined.
    ///
    /// The half of the palette narrowing only the *deck* can state. A file
    /// asking `can_cast = "{1}{U}"` names its own colour; a file asking how
    /// many Opts it cast names none, and the blue is in the card data.
    pub demands: Palette,
    /// Preferences picking out no castable card in this deck. They decide
    /// nothing here, which is a fact about the deck rather than an error in
    /// the file — the same treatment a criteria query matching nothing gets.
    pub unmatched: Vec<String>,
    /// Preferences that also pick out lands, and which of them. Ignored by the
    /// policy and said out loud, because a land is played rather than cast and
    /// a pilot writing `t:land otag:ramp` meant something.
    pub lands: Vec<(String, Vec<String>)>,
}

/// Resolve `prefer` against `deck`. `asked` is the query list built so far,
/// which already owns the low grouping bits.
pub fn resolve(prefer: &[String], deck: &Library, asked: &[String]) -> Result<Resolved> {
    let mut queries: Vec<String> = Vec::new();
    // A preference repeating a query the run already holds reuses its bit:
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
    let mut costs: Vec<Option<Demand>> = vec![None; deck.entries.len()];
    let mut demands = Palette::EMPTY;
    let mut unmatched = Vec::new();
    let mut lands = Vec::new();
    for query in prefer {
        tiers.push(bit_of(query, &mut queries));
        let mut castable = 0;
        let mut named_lands = Vec::new();
        for position in deck.positions_matching(query)? {
            let entry = &deck.entries[position];
            if is_land(&entry.card) {
                named_lands.push(entry.card.name.clone());
                continue;
            }
            castable += 1;
            // First entry wins, so a card two tiers both name is cast under
            // the higher one — which is what reading a priority in order
            // means, and it is the same rule the land drop uses.
            if costs[position].is_some() {
                continue;
            }
            let cost = price(&entry.card.name, &entry.card.mana_cost, query)?;
            demands = demands.union(cost.demands());
            costs[position] = Some(cost.demand());
        }
        if castable == 0 {
            unmatched.push(query.clone());
        }
        named_lands.sort_unstable();
        named_lands.dedup();
        if !named_lands.is_empty() {
            lands.push((query.clone(), named_lands));
        }
    }

    Ok(Resolved {
        policy: CastingPolicy::new(tiers),
        queries,
        prefer: prefer.to_vec(),
        costs,
        demands,
        unmatched,
        lands,
    })
}

/// What one named card costs, or a refusal saying why it cannot be cast.
///
/// A card with no printed cost is refused rather than treated as free. The
/// cards that have none are lands, which are played, and things like Ancestral
/// Vision that are cast some other way — and a free spell in a budget is not a
/// rounding error, it is a spell cast every turn forever.
fn price(name: &str, mana_cost: &str, query: &str) -> Result<Cost> {
    if mana_cost.trim().is_empty() {
        anyhow::bail!(
            "[casting]: `prefer` entry {query:?} names {name}, which has no printed mana cost, \
             so there is no way to work out what casting it would spend.\n      \
             A card with no cost is not a free spell — it is one that gets onto the battlefield \
             some other way, and this engine does not model that route."
        );
    }
    Cost::parse(mana_cost).map_err(|e| {
        anyhow!(
            "[casting]: `prefer` entry {query:?} names {name}, whose cost this engine cannot \
             pay: {e}\n      \
             A budget spends the pool, so a cost read too cheaply does not only get that spell \
             wrong — it leaves mana the rest of the line then spends."
        )
    })
}

/// Parse every preference before anything is grouped, so a query this index
/// cannot answer is refused against the table that wrote it.
///
/// The same seam the criteria file's own queries and the land drop's go
/// through, and for the same reason: an `otag:` this index never fetched
/// matches nothing, which here would silently empty a tier rather than report
/// a gap.
pub fn check(prefer: &[String], deck: &Library) -> Result<()> {
    for query in prefer {
        let parsed = chip_scryfall::parse(query)
            .map_err(|e| anyhow!("[casting]: in `prefer` entry {query:?}: {e}"))?;
        if let Some(gap) = parsed.tag_gap(&deck.index_tags) {
            anyhow::bail!(
                "[casting]: in `prefer` entry {query:?}: {}",
                crate::report::tag_gap_refusal(&gap, deck)
            );
        }
        let unknown = parsed.unknown_keywords(&deck.index_keywords);
        if !unknown.is_empty() {
            anyhow::bail!(
                "[casting]: in `prefer` entry {query:?}: {}",
                crate::report::unknown_keyword_refusal(&unknown)
            );
        }
    }
    Ok(())
}
