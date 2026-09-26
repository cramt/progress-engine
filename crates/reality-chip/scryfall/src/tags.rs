//! Which oracle tags an index carries by default.
//!
//! Oracle tags are Scryfall Tagger's answer to *what does this card do*, and
//! they are not in the bulk data. They are fetched from the search API at sync
//! time, which is why there has to be a list at all: there are thousands of
//! them and each one is a request.
//!
//! The list below is not "the useful tags". It is the tags this tool's own
//! features read, which is a smaller and more defensible thing — every entry
//! names what needs it, so an entry nothing needs can be deleted rather than
//! argued about.

/// A tag the index fetches, and the reason it does.
pub struct StandardTag {
    pub name: &'static str,
    /// What in this tool reads it. Kept beside the tag so that a list of
    /// plausible-looking Magic vocabulary cannot accumulate here unexamined.
    pub because: &'static str,
}

/// The tags `sync` fetches unless told otherwise.
///
/// Deliberately short. Each of these costs a paginated search at sync time and
/// a few bytes on every card that matches, and the honest default is the set
/// the tool cannot answer its own questions without.
pub const STANDARD_TAGS: &[StandardTag] = &[
    StandardTag {
        name: "tapland",
        because: "castability: a land that enters tapped makes no mana the turn it lands, \
                  which is the difference between two lands and two mana",
    },
    StandardTag {
        name: "conditional-tapland",
        because: "castability again, and the half `tapland` is not: Hallowed Fountain enters \
                  tapped unless you pay 2 life, which is a decision rather than a property. \
                  Scryfall keeps the two apart — no shockland is in `tapland` — so the gate \
                  can state which way it read a choice instead of folding it into a fact",
    },
    StandardTag {
        name: "surveil",
        because: "filtering: how many cards deep a turn actually sees",
    },
    StandardTag {
        name: "scry",
        because: "filtering, as surveil but leaving the card on top rather than binning it",
    },
    StandardTag {
        name: "mill",
        because: "the graveyard as a destination — a Loam deck's library-to-yard route",
    },
    StandardTag {
        name: "tutor",
        because: "selection over the whole library rather than over a looked-at set",
    },
    StandardTag {
        name: "ramp",
        because: "the mana curve questions this tool was built to answer",
    },
    StandardTag {
        name: "fetchland",
        because: "deck thinning: a fetchland removes a land from the library rather than \
                  looking at one, so it is the case that makes the library a population that \
                  shrinks. `is:fetchland` is the ten-card allied/enemy cycle; this tag is the \
                  54 cards that actually do it, Prismatic Vista and Terramorphic Expanse \
                  included, and no query over card text separates them",
    },
    StandardTag {
        name: "mana-rock",
        because: "mana sources: the standard effect library declares how much a rock the line \
                  cast adds (`adds = n`), and a card's text says `{T}: Add` on a great many \
                  things that are not a rock. The tag is Scryfall's answer to which are",
    },
    StandardTag {
        name: "mana-dork",
        because: "mana sources, as `mana-rock` but for creatures, which the rules make wait a \
                  turn before they tap for anything",
    },
];

/// Just the names, for the fetcher and for the index header.
pub fn standard_tag_names() -> Vec<String> {
    STANDARD_TAGS.iter().map(|t| t.name.to_string()).collect()
}
