//! Working out what the effect library says about *this* deck.
//!
//! The library is keyed on queries, so before anything can run, two questions
//! have to be answered against real card data: which cards does each entry
//! match, and — where several match the same card — which entry wins.
//!
//! The answer to the second is **last-wins, per card**. It is settled here,
//! once, rather than inside the enumeration, because the enumeration visits
//! millions of paths and an overlap re-decided on each of them is a second
//! opinion waiting to differ from the first. What the engine receives is
//! already resolved: one group bit per effect, meaning *this card's effect is
//! this one*, and the bits are disjoint because a card has one effect.

use anyhow::{Context, Result};
use chip_scryfall::index::TagGap;
use chip_scryfall::Query;
use gauntlet_criteria::{Effect, Fetch, Route};
use gauntlet_toml::{Destination, EffectEntry, EffectLibrary, STANDARD_LIBRARY_ORIGIN};

use crate::library::{Library, Marked};

/// An effect that matched at least one card in this deck, and which cards.
///
/// The condition attached to last-wins: *assume the library is true* is a
/// reasonable stance, and *assume it is true and give no way to see what it
/// did* is not, because the first useful question about a surprising number is
/// what the tool thought the cards do.
pub struct Applied {
    pub matches: String,
    pub look: u32,
    pub on: &'static str,
    pub to_graveyard: Option<String>,
    /// The declared tutor priority, as written, and where it puts what it
    /// finds. A run that fetched has to say what it fetched: a number that
    /// hinged on a declared policy and did not name it is the bug this project
    /// exists to prevent.
    pub fetch: Option<(Vec<String>, &'static str)>,
    /// Which of the fetch's preferences pick out no card in this deck, so a
    /// tier that decides nothing is a fact about the deck rather than silence.
    pub fetch_misses: Vec<String>,
    pub origin: String,
    /// The cards this effect actually got, after the overlap was resolved. A
    /// card matched by a later entry is not here — it is under that entry.
    pub cards: Vec<String>,
    pub copies: u32,
    /// Whether this one can move a number, which is a stricter thing than
    /// matching cards: it also has to route somewhere a card can reach.
    pub live: bool,
}

/// The effect library, resolved against one deck.
pub struct Resolved {
    pub applied: Vec<Applied>,
    /// Live effects only, in declaration order, with their queries resolved to
    /// grouping bits.
    pub effects: Vec<Effect>,
    /// Extra queries the grouping needs, beyond the ones the criteria file
    /// named: the routing destinations. Appended after the file's own, so the
    /// indices a clause already holds do not move.
    pub queries: Vec<String>,
    /// One per live effect: which library cards it owns.
    pub marked: Vec<Marked>,
    /// Hand-written entries whose `match` picked out nothing.
    ///
    /// Only hand-written ones. A standard library entry that matches nothing is
    /// the ordinary case — most decks have no surveil lands — and warning about
    /// it on every run would be noise about a question the user did not ask.
    /// An entry *they* wrote matching nothing is the same failure as a criteria
    /// query matching nothing, and gets the same treatment.
    pub unmatched: Vec<String>,
    /// Entries that matched nothing because the *index* cannot answer them.
    ///
    /// Standard library ones too, which is the whole point. Silence was granted
    /// to an entry whose deck has no surveil land — a fact about the deck, and
    /// the ordinary case. An entry matching nothing because this index carries
    /// no oracle tags is a fact about the index, it applies to every deck
    /// equally, and it silently switches the effect library off: the standard
    /// library is keyed entirely on `otag:`, so a tagless index reports no
    /// effects and narrates none of it. That is a different fact and it is said
    /// out loud.
    pub tag_blind: Vec<Blind>,
}

/// An effect the index cannot answer, and why it cannot.
pub struct Blind {
    pub matches: String,
    pub gap: TagGap,
}

/// Resolve `library` against `deck`.
///
/// `asked` is the criteria file's own query list, which already owns the low
/// group bits. A routing destination that repeats one of those reuses its bit
/// rather than claiming a second — same query, same cards, and two bits for one
/// question would split every group in the deck along a line that means
/// nothing.
pub fn resolve(library: &EffectLibrary, deck: &Library, asked: &[String]) -> Result<Resolved> {
    let matchers = library
        .entries()
        .iter()
        .map(|e| parse(&e.matches, e, "match"))
        .collect::<Result<Vec<_>>>()?;

    // Last-wins, decided per card: every entry is tried and the last one to
    // match keeps it.
    let owner: Vec<Option<usize>> = deck
        .entries
        .iter()
        .map(|card| {
            let view = card.card.view(&card.categories);
            matchers.iter().rposition(|q| q.matches(&view))
        })
        .collect();

    let mut applied = Vec::new();
    let mut unmatched = Vec::new();
    let mut tag_blind = Vec::new();
    let mut live: Vec<usize> = Vec::new();
    for (i, entry) in library.entries().iter().enumerate() {
        let mine: Vec<usize> = owner
            .iter()
            .enumerate()
            .filter(|(_, o)| **o == Some(i))
            .map(|(c, _)| c)
            .collect();
        // Matched by *some* entry but lost the card to a later one: reported as
        // a miss would be a lie, so only an entry nothing at all matched counts.
        let matched_anything = matchers[i].matches_any(deck);
        if !matched_anything {
            // Asked in this order because the two reasons are not equally
            // informative: an entry that cannot be evaluated at all did not
            // fail to find cards, it never looked.
            match matchers[i].tag_gap(&deck.index_tags) {
                Some(gap) => tag_blind.push(Blind {
                    matches: entry.matches.clone(),
                    gap,
                }),
                None if entry.origin != STANDARD_LIBRARY_ORIGIN => {
                    unmatched.push(entry.matches.clone())
                }
                None => {}
            }
        }
        if mine.is_empty() {
            continue;
        }
        // An effect is live when it can move a number, which needs a
        // destination *and* a destination some card in this deck can reach. A
        // route naming a card the deck does not play sends nothing anywhere,
        // and paying a checkpoint a turn to discover that is paying to compute
        // a zero.
        let routes = match &entry.to_graveyard {
            None => false,
            Some(Destination::Everything) => true,
            Some(Destination::Matching(q)) => parse(q, entry, "to_graveyard")?.matches_any(deck),
        };
        // A tutor with nothing to find moves no number either, and the tiers
        // that found nothing are worth saying out loud: a priority naming a
        // card this deck does not play is the same failure as a criteria query
        // matching nothing.
        let mut fetch_misses = Vec::new();
        if let Some(fetch) = &entry.fetch {
            for query in &fetch.prefer {
                if !parse(query, entry, "fetch")?.matches_any(deck) {
                    fetch_misses.push(query.clone());
                }
            }
        }
        let fetches = entry
            .fetch
            .as_ref()
            .is_some_and(|f| f.prefer.len() > fetch_misses.len());
        let reachable = routes || fetches;
        if reachable {
            live.push(i);
        }
        applied.push(Applied {
            matches: entry.matches.clone(),
            look: entry.look,
            on: entry.trigger.as_str(),
            to_graveyard: entry.to_graveyard.as_ref().map(|d| match d {
                Destination::Everything => gauntlet_toml::EVERYTHING.to_string(),
                Destination::Matching(q) => q.clone(),
            }),
            fetch: entry
                .fetch
                .as_ref()
                .map(|f| (f.prefer.clone(), gauntlet_toml::fetched_name(f.to))),
            fetch_misses,
            origin: entry.origin.clone(),
            cards: mine
                .iter()
                .map(|&c| deck.entries[c].card.name.clone())
                .collect(),
            copies: mine.iter().map(|&c| deck.entries[c].qty).sum(),
            live: reachable,
        });
    }

    // Destination queries first, so a live effect's group bit can be computed
    // once the whole set is known.
    let mut queries: Vec<String> = Vec::new();
    let bit_of = |q: &String, extra: &[String]| -> Option<usize> {
        asked
            .iter()
            .position(|a| a == q)
            .or_else(|| extra.iter().position(|a| a == q).map(|i| asked.len() + i))
    };
    for &i in &live {
        if let Some(Destination::Matching(q)) = &library.entries()[i].to_graveyard {
            if bit_of(q, &queries).is_none() {
                queries.push(q.clone());
            }
        }
        // And a tutor's priority, on the same terms: the engine picks a group
        // by which of these queries it matches, so each one is a bit.
        if let Some(fetch) = &library.entries()[i].fetch {
            for q in &fetch.prefer {
                if bit_of(q, &queries).is_none() {
                    queries.push(q.clone());
                }
            }
        }
    }
    let first_mark = asked.len() + queries.len();

    let mut marked = Vec::with_capacity(live.len());
    let mut effects = Vec::with_capacity(live.len());
    for (slot, &i) in live.iter().enumerate() {
        let entry = &library.entries()[i];
        marked.push(Marked {
            label: format!("<effect {}>", entry.matches),
            members: owner.iter().map(|o| *o == Some(i)).collect(),
        });
        effects.push(Effect {
            matched_by: first_mark + slot,
            look: entry.look,
            trigger: entry.trigger,
            route: match &entry.to_graveyard {
                None => Route::Nowhere,
                Some(Destination::Everything) => Route::Everything,
                Some(Destination::Matching(q)) => {
                    Route::Matching(bit_of(q, &queries).expect("just collected"))
                }
            },
            fetch: entry.fetch.as_ref().map(|f| Fetch {
                prefer: f
                    .prefer
                    .iter()
                    .map(|q| bit_of(q, &queries).expect("just collected"))
                    .collect(),
                to: f.to,
            }),
        });
    }

    Ok(Resolved {
        applied,
        effects,
        queries,
        marked,
        unmatched,
        tag_blind,
    })
}

/// Parse one of an effect's queries, named against the entry that wrote it.
///
/// Context rather than a bare error: an effect's queries never appear in the
/// criteria file's own query list, so nothing else in the run knows this string
/// exists to blame it.
fn parse(query: &str, entry: &EffectEntry, key: &str) -> Result<Query> {
    chip_scryfall::parse(query)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| {
            format!(
                "{}: effect {:?}: in `{key} = {query:?}`",
                entry.origin, entry.matches
            )
        })
}

trait MatchesAny {
    fn matches_any(&self, deck: &Library) -> bool;
}

impl MatchesAny for Query {
    fn matches_any(&self, deck: &Library) -> bool {
        deck.entries
            .iter()
            .any(|e| self.matches(&e.card.view(&e.categories)))
    }
}
