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

use gauntlet_criteria::{CastingPolicy, Cost, Demand, Palette};

use crate::library::{is_land, Library};
use crate::prepare::Unprepared;
use crate::refusal::{self, QuerySite, Refusal};

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
    /// The same for [`Library::commanders`]: what each commander costs where
    /// the line names it. A named commander is cast from the command zone —
    /// always there, never drawn, and paid for out of the same pool as every
    /// other spell in the line.
    pub commanders: Vec<Option<Demand>>,
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
pub fn resolve(
    prefer: &[String],
    deck: &Library,
    asked: &[String],
    file: &str,
) -> Result<Resolved, Unprepared> {
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
    let mut commanders: Vec<Option<Demand>> = vec![None; deck.commanders.len()];
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
            let cost = price(&entry.card.name, &entry.card.mana_cost, query, file)?;
            demands = demands.union(cost.demands());
            costs[position] = Some(cost.demand());
        }
        // The command zone, on the same terms: first entry wins, a land is
        // played rather than cast, and a cost this engine cannot pay is
        // refused by name.
        for position in deck.commanders_matching(query)? {
            let entry = &deck.commanders[position];
            if is_land(&entry.card) {
                named_lands.push(entry.card.name.clone());
                continue;
            }
            castable += 1;
            if commanders[position].is_some() {
                continue;
            }
            let cost = price(&entry.card.name, &entry.card.mana_cost, query, file)?;
            demands = demands.union(cost.demands());
            commanders[position] = Some(cost.demand());
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
        commanders,
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
fn price(name: &str, mana_cost: &str, query: &str, file: &str) -> Result<Cost, Refusal> {
    if mana_cost.trim().is_empty() {
        return Err(Refusal::NoPrintedCost {
            file: file.to_string(),
            query: query.to_string(),
            card: name.to_string(),
        });
    }
    Cost::parse(mana_cost).map_err(|error| Refusal::UnpayableCost {
        file: file.to_string(),
        query: query.to_string(),
        card: name.to_string(),
        error,
    })
}

/// Parse every preference before anything is grouped, so a query this index
/// cannot answer is refused against the table that wrote it.
///
/// The same seam the criteria file's own queries and the land drop's go
/// through, and for the same reason: an `otag:` this index never fetched
/// matches nothing, which here would silently empty a tier rather than report
/// a gap.
pub fn check(prefer: &[String], deck: &Library, file: &str) -> Result<(), Refusal> {
    for query in prefer {
        refusal::check_query(file, QuerySite::Casting, query, deck)?;
    }
    Ok(())
}
