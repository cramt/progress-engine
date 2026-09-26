//! Every refusal a run makes before a hand is enumerated, as data.
//!
//! A **refusal** is the tool declining to answer, by name: the query, key or
//! zone it could not model, and what would let it. It is deliberate — the
//! alternative is a confident wrong number — and so it is not an error, and
//! the type says so: [`Refusal`] is one variant per refusal, carrying what it
//! names as fields, and a genuine failure (a file that will not read, a bug)
//! never becomes one.
//!
//! The text a reader sees is [`Refusal`]'s `Display`, and exactly one place
//! decides how it is introduced: [`Refusal::file`] and [`Refusal::question`]
//! say which file and which question the refusal is named against, and the
//! body after them is the variant's own. A test can match on the variant
//! rather than hunt for a substring; a reader gets the same words either way.

use std::fmt;

use chip_scryfall::index::TagGap;
use chip_scryfall::ParseError;
use gauntlet_criteria::{CostError, RunError};
use gauntlet_toml::EvalError;

use crate::library::{self, Library};

/// Where in the criteria file a query was written: which is how a refusal of
/// that query names it.
///
/// The criteria file's own questions are named by the criterion that asked;
/// a declared priority's entries by the table and the position that wrote
/// them. The query itself is carried beside the site, by the refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuerySite {
    /// A criterion's or an expectation's clause, named by the question that
    /// asked it — or "this file" where none can be named.
    Question(String),
    /// An entry in `[land_drop] prefer`.
    LandDrop,
    /// An entry in `[casting] prefer`.
    Casting,
    /// A query the `[mulligan]` table names.
    Mulligan(MulliganAt),
}

/// Which of a mulligan's queries, counted from one as a reader counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MulliganAt {
    /// The n-th keep clause.
    Keep(usize),
    /// The n-th `bottom` entry.
    Bottom(usize),
}

/// Why this index cannot price a cost.
///
/// Both halves are silent failures of the same shape as an unfetched oracle
/// tag: an index with no `produces` reports every land as making nothing and
/// answers 0.00%, and an index with no tapland tag reports every land as
/// untapped and answers a number the deck cannot reach. Neither looks like a
/// gap in the data from the outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriceGap {
    /// The index predates the `produces` field.
    StaleIndex,
    /// The index does not carry these tapland tags; `carried` is what it
    /// does carry.
    NoTaplandTags {
        missing: Vec<String>,
        carried: Vec<String>,
    },
}

impl PriceGap {
    /// Why `library`'s index cannot price a cost, or `None` when it can.
    pub fn of(library: &Library) -> Option<PriceGap> {
        if library.index_is_stale {
            return Some(PriceGap::StaleIndex);
        }
        let missing: Vec<String> = [library::TAPLAND, library::CONDITIONAL_TAPLAND]
            .into_iter()
            .filter(|tag| !library.index_tags.contains(tag))
            .map(str::to_string)
            .collect();
        if missing.is_empty() {
            return None;
        }
        Some(PriceGap::NoTaplandTags {
            missing,
            carried: carried(library),
        })
    }
}

/// Which `otag:` terms a query names that this index cannot answer, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Untagged {
    /// The index carries tags, and these are not among them; `carried` is
    /// what it does carry, which is the list a misspelling is checked against.
    NotCarried {
        tags: Vec<String>,
        carried: Vec<String>,
    },
    /// The index carries no tags at all, so every `otag:` term is
    /// unanswerable — whatever the spelling.
    NoneFetched { tags: Vec<String> },
}

impl Untagged {
    fn of(gap: TagGap, library: &Library) -> Untagged {
        match gap {
            TagGap::NotCarried(tags) => Untagged::NotCarried {
                tags,
                carried: carried(library),
            },
            TagGap::NoneFetched(tags) => Untagged::NoneFetched { tags },
        }
    }
}

/// The oracle tags `library`'s index carries, sorted, as a refusal lists them.
pub fn carried(library: &Library) -> Vec<String> {
    library
        .index_tags
        .carried()
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// One refusal, naming what it could not model.
///
/// `file` is how the run names the criteria file it was reading. Every variant
/// carries it and every refusal's text starts with it: a caller running
/// several criteria files needs to know which one it was before it needs to
/// know which criterion.
#[derive(Debug)]
pub enum Refusal {
    /// A query in a syntax this tool does not read. Refused rather than
    /// guessed at: a term it half-understood would match the wrong cards.
    UnsupportedQuery {
        file: String,
        site: QuerySite,
        query: String,
        /// Boxed only to keep every refusal small enough to return by value.
        error: Box<ParseError>,
    },
    /// A query naming an oracle tag this index cannot answer.
    ///
    /// Refused rather than answered, for the reason the whole tool exists: a
    /// count of a tag nobody fetched is zero by construction, and it reads
    /// exactly like a deck that plays no such card. The two halves of the
    /// message are the two facts a reader needs — what this index knows, and
    /// which command changes it.
    TagGap {
        file: String,
        site: QuerySite,
        query: String,
        gap: Untagged,
    },
    /// A query naming a keyword no card in this index has.
    ///
    /// The index's keyword list is the whole card pool's, not this deck's, so
    /// a keyword missing from it is missing from Magic as this index knows
    /// Magic — which leaves exactly two readings, a typo or a set newer than
    /// the file, and one command tells them apart. No list of what it does
    /// carry, unlike the tag refusal: that one names six tags, this one would
    /// name two thousand keywords.
    UnknownKeyword {
        file: String,
        site: QuerySite,
        query: String,
        keywords: Vec<String>,
    },
    /// A `[casting]` entry naming a card with no printed mana cost.
    ///
    /// A card with no printed cost is refused rather than treated as free.
    /// The cards that have none are lands, which are played, and things like
    /// Ancestral Vision that are cast some other way — and a free spell in a
    /// budget is not a rounding error, it is a spell cast every turn forever.
    NoPrintedCost {
        file: String,
        query: String,
        card: String,
    },
    /// A `[casting]` entry naming a card whose cost this engine cannot pay.
    UnpayableCost {
        file: String,
        query: String,
        card: String,
        error: CostError,
    },
    /// Counting castings with no casting priority declared.
    ///
    /// One Island, one Opt and one Preordain: which do you cast? The mana
    /// pays for one of them, the answer differs by which, and nothing in a
    /// decklist says. A tool that picked — the cheapest, the first one listed,
    /// the one the criterion happened to ask about — would be reporting a line
    /// nobody chose, and the percentage would look exactly like a measured
    /// one. It names the remedy rather than choosing a default.
    CastingWithoutPriority { file: String, asked_by: String },
    /// A land-drop tutor with no declared land-drop priority.
    ///
    /// A fetchland goes and gets its land *on* the drop and leaves the
    /// battlefield doing it, so what is standing there at the end of the turn
    /// is a fact about which land you played. With no priority declared the
    /// walk plays the deepest-looking land in hand, which is a rule adopted
    /// when nothing else could tell two drops apart and is not one anybody
    /// chose — and a run that fetched off it would report a thinned library
    /// nobody asked for.
    FetchWithoutLandDrop { file: String, effect: String },
    /// A cast tutor with no declared casting priority.
    ///
    /// A spell this run does not cast is a spell that never resolved, so it
    /// never fetched either, and a run that fired the tutor anyway would be
    /// putting a card in your hand off a spell nobody paid for.
    FetchWithoutCasting { file: String, effect: String },
    /// A delayed fetch onto the battlefield that can find a land.
    ///
    /// A fetchland may only find lands because a land drop is the one way onto
    /// the battlefield the walk models; a Saga's chapter is the other way, and
    /// what it may not find is a land, because a land arriving off an ability
    /// puts mana in the pool on a turn nothing says whether it entered tapped.
    DelayedFetchFindsLand {
        file: String,
        effect: String,
        query: String,
        lands: u32,
    },
    /// A battlefield fetch naming something that is not a land.
    ///
    /// The same refusal `zone = "battlefield"` is under, at the same seam: a
    /// land arrives on a land drop, which this engine models, and everything
    /// else has to be cast, which — once it is on the battlefield rather than
    /// merely paid for — it does not.
    FetchNonLandToBattlefield {
        file: String,
        effect: String,
        query: String,
        spells: Vec<String>,
    },
    /// A battlefield question about something that is not a land.
    ///
    /// The one approximation this would not be is "drawn", and it is wrong in
    /// the direction that flatters the deck: an opening hand with one Island
    /// and a three-drop has the three-drop in hand on turn 0 and on the
    /// battlefield on no turn at all. A land is different in kind rather than
    /// in degree — it arrives on a land drop, which is free, capped at one a
    /// turn, and something this engine walks — so the zone opens for lands and
    /// stays shut for everything else.
    BattlefieldNonLand {
        file: String,
        asked_by: String,
        query: String,
        /// What the query matches that is not a land.
        spells: Vec<String>,
        /// Of those, the cards whose land is a back face reached by
        /// transforming — the case that reads as a contradiction.
        back_faces: Vec<String>,
    },
    /// A mana question beside a live land-drop effect, with no land-drop
    /// priority declared.
    ///
    /// Both are answers to *which land did you play this turn*, and with
    /// nothing declared they are different answers. Running both would be two
    /// policies over one resource, which is the failure VISION.md is written
    /// against. **What it does not do is pick one**: the refusal names the
    /// declaration that settles it — `[land_drop]` — rather than
    /// choosing a default and mentioning it in a note nobody reads.
    ManaBesideLandDropEffect { file: String, asked_by: String },
    /// A mana question against an index that cannot price a cost.
    IndexCannotPriceMana {
        file: String,
        asked_by: String,
        gap: PriceGap,
    },
    /// A mana question beside a fetched land.
    ///
    /// `otag:fetchland` holds Scalding Tarn, which puts its Island down
    /// untapped, and Terramorphic Expanse, which does not — and the difference
    /// is a property of the card that did the fetching rather than of the land
    /// it found. What the fetch *does* say exactly is what left the library,
    /// so the thinning is answerable and the mana is not.
    ManaBesideAFetchedLand {
        file: String,
        asked_by: String,
        effect: String,
    },
    /// An objective weighing a question too wide to enumerate.
    ///
    /// A strategy is chosen from each criterion's chance given every opener,
    /// and over estimates it would keep the openers that were lucky in the
    /// sample.
    ObjectiveTooWide {
        file: String,
        /// The weighed criteria in the refused class, by name.
        weighed: Vec<String>,
        paths: u128,
        groups: usize,
    },
    /// An objective that would walk more paths than an optimiser may.
    ObjectiveOverBudget {
        file: String,
        /// The weighed criteria in the class that crossed the budget.
        weighed: Vec<String>,
        width: u128,
    },
    /// A run that cannot be dealt at all: an empty library, a hand bigger
    /// than the deck, a library that runs out.
    ///
    /// Refused about the run rather than about one of its classes. Narrowing
    /// asks a smaller question than the file did, so a class about turn 2
    /// would happily answer against a library the file's own horizon could
    /// never be dealt from — turning a refusal into a number by changing the
    /// question.
    Infeasible {
        file: String,
        reason: RunError<EvalError>,
    },
}

impl Refusal {
    /// The criteria file this refusal is named against.
    pub fn file(&self) -> &str {
        match self {
            Refusal::UnsupportedQuery { file, .. }
            | Refusal::TagGap { file, .. }
            | Refusal::UnknownKeyword { file, .. }
            | Refusal::NoPrintedCost { file, .. }
            | Refusal::UnpayableCost { file, .. }
            | Refusal::CastingWithoutPriority { file, .. }
            | Refusal::FetchWithoutLandDrop { file, .. }
            | Refusal::FetchWithoutCasting { file, .. }
            | Refusal::DelayedFetchFindsLand { file, .. }
            | Refusal::FetchNonLandToBattlefield { file, .. }
            | Refusal::BattlefieldNonLand { file, .. }
            | Refusal::ManaBesideLandDropEffect { file, .. }
            | Refusal::IndexCannotPriceMana { file, .. }
            | Refusal::ManaBesideAFetchedLand { file, .. }
            | Refusal::ObjectiveTooWide { file, .. }
            | Refusal::ObjectiveOverBudget { file, .. }
            | Refusal::Infeasible { file, .. } => file,
        }
    }

    /// The question — the criterion, the table entry, the effect — this
    /// refusal is named against, where its text names one apart from its body.
    pub fn question(&self) -> Option<String> {
        match self {
            Refusal::UnsupportedQuery { site, query, .. }
            | Refusal::TagGap { site, query, .. }
            | Refusal::UnknownKeyword { site, query, .. } => Some(match site {
                QuerySite::Question(asked_by) => format!("{asked_by}: in query {query:?}"),
                QuerySite::LandDrop => format!("[land_drop]: in `prefer` entry {query:?}"),
                QuerySite::Casting => format!("[casting]: in `prefer` entry {query:?}"),
                QuerySite::Mulligan(at) => format!("[mulligan]: {at}, query {query:?}"),
            }),
            Refusal::NoPrintedCost { .. } | Refusal::UnpayableCost { .. } => {
                Some("[casting]".to_string())
            }
            Refusal::CastingWithoutPriority { asked_by, .. }
            | Refusal::BattlefieldNonLand { asked_by, .. }
            | Refusal::ManaBesideLandDropEffect { asked_by, .. }
            | Refusal::IndexCannotPriceMana { asked_by, .. }
            | Refusal::ManaBesideAFetchedLand { asked_by, .. } => Some(asked_by.clone()),
            Refusal::DelayedFetchFindsLand { effect, .. }
            | Refusal::FetchNonLandToBattlefield { effect, .. } => {
                Some(format!("effect {effect:?}"))
            }
            Refusal::ObjectiveTooWide { .. } | Refusal::ObjectiveOverBudget { .. } => {
                Some("[mulligan]".to_string())
            }
            // These name their effect inside the sentence rather than before
            // it, and an infeasible run is about the whole file.
            Refusal::FetchWithoutLandDrop { .. }
            | Refusal::FetchWithoutCasting { .. }
            | Refusal::Infeasible { .. } => None,
        }
    }

    /// Everything after the file and the question: what could not be modelled
    /// and what would let it be.
    fn body(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::UnsupportedQuery { error, .. } => write!(f, "{error}"),
            Refusal::TagGap { gap, .. } => {
                let (Untagged::NotCarried { tags, .. } | Untagged::NoneFetched { tags }) = gap;
                let named: Vec<String> = tags.iter().map(|t| format!("otag:{t}")).collect();
                let named = named.join(", ");
                match gap {
                    Untagged::NotCarried { carried, .. } => write!(
                        f,
                        "this index does not carry {named}, so counting it would be zero by \
                         construction\n      rather than by measurement. This index carries: \
                         {}.\n      \
                         Check the spelling; a tag outside that list has to be added to \
                         chip-scryfall's\n      standard tags and fetched by `gauntlet sync`.",
                        carried.join(", ")
                    ),
                    Untagged::NoneFetched { .. } => write!(
                        f,
                        "this index carries no oracle tags at all, so {named} would match \
                         nothing here\n      whether or not this deck plays such a card.\n      \
                         `sync --from` builds an index like this one: tags come from Scryfall's \
                         search API,\n      not from the bulk file. Fetch them with: gauntlet sync"
                    ),
                }
            }
            Refusal::UnknownKeyword { keywords, .. } => {
                let named: Vec<String> = keywords.iter().map(|k| format!("kw:{k}")).collect();
                write!(
                    f,
                    "no card in this index has {}, so counting it would be zero by \
                     construction\n      rather than by measurement — which reads exactly like \
                     a deck that plays none. The\n      index lists every keyword the whole card \
                     pool carries, so this is a misspelling\n      unless it is newer than the \
                     index. Check the spelling; rebuild with: gauntlet sync",
                    named.join(", ")
                )
            }
            Refusal::NoPrintedCost { query, card, .. } => write!(
                f,
                "`prefer` entry {query:?} names {card}, which has no printed mana cost, so there \
                 is no way to work out what casting it would spend.\n      \
                 A card with no cost is not a free spell — it is one that gets onto the \
                 battlefield some other way, and this engine does not model that route."
            ),
            Refusal::UnpayableCost {
                query, card, error, ..
            } => write!(
                f,
                "`prefer` entry {query:?} names {card}, whose cost this engine cannot pay: \
                 {error}\n      \
                 A budget spends the pool, so a cost read too cheaply does not only get that \
                 spell wrong — it leaves mana the rest of the line then spends."
            ),
            Refusal::CastingWithoutPriority { .. } => f.write_str(
                "counting the spells you cast means knowing which ones you would cast, and this \
                 file declares\n      no priority. One Island, one Opt and one Preordain is \
                 one spell cast and two left in hand,\n      and which one it was is a decision \
                 this tool will not make for you.\n      Declare it, highest priority \
                 first:\n\n      \
                 [casting]\n      prefer = ['name:\"Opt\"', 'name:\"Preordain\"']\n\n      \
                 The list is read in order and the first entry the pool can still pay for is \
                 cast. A spell\n      the list does not name is not cast at all — the list is \
                 the line you are asking about,\n      not a preference over your whole deck.",
            ),
            Refusal::FetchWithoutLandDrop { effect, .. } => write!(
                f,
                "effect {effect:?} fetches on a land drop, and this file declares no priority \
                 over the drop.\n      \
                 A fetchland goes and gets its land on the turn it is played and is not there \
                 afterwards,\n      so which land you played decides both what you fetched \
                 and what is standing there.\n      \
                 Declare it, highest priority first:\n\n      \
                 [land_drop]\n      prefer = ['otag:fetchland', 't:land']\n\n      \
                 The list is read in order, the first entry a land in hand matches wins, and \
                 any land the list\n      does not name is played last."
            ),
            Refusal::FetchWithoutCasting { effect, .. } => write!(
                f,
                "effect {effect:?} fetches when it is cast, and this file declares no casting \
                 priority.\n      \
                 A spell this run does not cast is one that never resolved, so it never went \
                 and got anything.\n      \
                 Declare the line, highest priority first:\n\n      \
                 [casting]\n      prefer = ['name:\"Trinket Mage\"', 'name:\"Lantern of \
                 Insight\"']\n\n      \
                 The list is read in order and the first entry the pool can still pay for is \
                 cast. A spell\n      the list does not name is not cast at all."
            ),
            Refusal::DelayedFetchFindsLand { query, lands, .. } => write!(
                f,
                "`fetch = {query:?}` with `after` and `to = \"battlefield\"` matches {lands} \
                 land{} in this deck.\n      \
                 A delayed effect puts its card beside the land that waited for it, and a land \
                 arriving that way\n      is not a land drop: whether it enters tapped is a \
                 fact about the card that fetched it,\n      which no tag carries. Urza's \
                 Saga's third chapter finds an artifact; narrow the query to\n      what it can \
                 actually find, such as `-t:land`.",
                if *lands == 1 { "" } else { "s" }
            ),
            Refusal::FetchNonLandToBattlefield { query, spells, .. } => write!(
                f,
                "`fetch = {query:?}` with `to = \"battlefield\"` names {} this engine cannot put \
                 there: {}.\n      \
                 A land arrives on a land drop, which is free and capped at one a turn, so the \
                 walk knows\n      where it is. Anything else has to be cast, and where a \
                 spell goes after it resolves is\n      not modelled at all.",
                if spells.len() == 1 { "a card" } else { "cards" },
                spells.join(", ")
            ),
            Refusal::BattlefieldNonLand {
                query,
                spells,
                back_faces,
                ..
            } => {
                write!(
                    f,
                    "`zone = \"battlefield\"` in query {query:?} is only answerable for lands, \
                     and this query matches {}\n      that {}: {}.\n      \
                     A land arrives on a land drop, which is free and one a turn, so this engine \
                     knows when it\n      got there. Everything else has to be cast, and which \
                     spells you cast when you cannot cast\n      them all is the budget half of \
                     the mana model \
                     (https://github.com/cramt/progress-engine/issues/10).\n      \
                     Narrow the query to lands, or ask about `hand` and know that is what you \
                     asked.",
                    spells.len(),
                    if spells.len() == 1 {
                        "is not"
                    } else {
                        "are not"
                    },
                    spells.join(", ")
                )?;
                // The case that reads as a contradiction: a card with "Land"
                // on it, refused for not being a land. Its land is a back face
                // it transforms into, which no land drop plays.
                if !back_faces.is_empty() {
                    write!(
                        f,
                        "\n      {} {} a land only on a back face reached by transforming, not \
                         by a land drop\n      \
                         (https://github.com/cramt/progress-engine/issues/61). `t:land` \
                         matches it because Scryfall\n      reads every face; write `t:land \
                         -is:transform` for the lands you can play.",
                        back_faces.join(", "),
                        if back_faces.len() == 1 { "is" } else { "are" },
                    )?;
                }
                Ok(())
            }
            Refusal::ManaBesideLandDropEffect { .. } => f.write_str(
                "a mana question and a live land-drop effect are both answers to which land you \
                 played this turn,\n      and this file declares no priority between them. \
                 With none declared the effect plays the\n      deepest-looking land in hand \
                 and the mana question assumes whichever land pays, which are\n      two \
                 answers to one drop — so this is refused rather than arbitrated.\n      \
                 Declare the priority and both read the same drop:\n\n      \
                 [land_drop]\n      prefer = ['otag:surveil', 't:land -otag:tapland']\n\n      \
                 The list is read in order, the first entry a land in hand matches wins, and \
                 any land the list\n      does not name is played last.",
            ),
            Refusal::IndexCannotPriceMana { gap, .. } => match gap {
                PriceGap::StaleIndex => f.write_str(
                    "this index was built before it recorded what a land produces, so every \
                     land in it makes\n      no mana and every cost would be unpayable. \
                     Rebuild it with: gauntlet sync",
                ),
                PriceGap::NoTaplandTags { missing, carried } => write!(
                    f,
                    "this index does not carry {}, so it cannot say which lands enter \
                     tapped.\n      \
                     A land that enters tapped makes no mana the turn it arrives, which is the \
                     difference between\n      two lands and two mana — and without the tag \
                     every land here would read as untapped, which\n      is the optimistic \
                     answer rather than the measured one. This index carries: {}.\n      \
                     Fetch them with: gauntlet sync   (--from cannot: they come from the \
                     search API)",
                    missing
                        .iter()
                        .map(|t| format!("otag:{t}"))
                        .collect::<Vec<_>>()
                        .join(" or "),
                    carried.join(", ")
                ),
            },
            Refusal::ManaBesideAFetchedLand { effect, .. } => write!(
                f,
                "effect {effect:?} puts a land onto the battlefield out of the library, and what \
                 that land\n      taps for on the turn it arrives is not modelled. A Scalding \
                 Tarn fetches untapped and a\n      Terramorphic Expanse fetches tapped; \
                 `otag:fetchland` holds both and nothing on the land\n      it found tells them \
                 apart, so a mana answer here would be optimistic or pessimistic with\n      \
                 nothing saying which.\n      \
                 What the fetch does say exactly is what left the library. Ask this file's \
                 thinning question\n      without a `can_cast`, a `cast` or a `[casting]` \
                 table, and ask the mana in a file of its own."
            ),
            Refusal::ObjectiveTooWide {
                weighed,
                paths,
                groups,
                ..
            } => write!(
                f,
                "`optimise` weighs {}, which {} too wide to enumerate: {paths} compositions \
                 across {groups} groups.\n\
                 A strategy is chosen from each criterion's chance given every opener, and over \
                 estimates it would keep the openers that were lucky in the sample and report a \
                 score that is too high. Weigh a question this run answers exactly, or narrow \
                 this one.",
                quoted(weighed),
                if weighed.len() == 1 { "is" } else { "are" },
            ),
            Refusal::ObjectiveOverBudget { weighed, width, .. } => write!(
                f,
                "`optimise` would walk {width} paths to price every opener and every way of \
                 putting cards back, over the {} an optimiser is allowed.\n\
                 The last criterion it reached was {}. Fewer weighed criteria, a nearer turn or \
                 a higher `down_to` all shrink it.",
                gauntlet_criteria::MAX_OPTIMISE_PATHS,
                quoted(weighed),
            ),
            Refusal::Infeasible { reason, .. } => write!(f, "{reason}"),
        }
    }
}

/// Criteria names as a refusal lists them: each quoted, comma-separated.
fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("{n:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for MulliganAt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MulliganAt::Keep(n) => write!(f, "keep clause {n}"),
            MulliganAt::Bottom(n) => write!(f, "`bottom` entry {n}"),
        }
    }
}

/// `{file}: {question}: {body}`, the question where the refusal names one
/// apart from its body.
///
/// The one place a refusal's introduction is decided.
impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.file())?;
        if let Some(question) = self.question() {
            write!(f, "{question}: ")?;
        }
        self.body(f)
    }
}

/// A refusal is reported where an error would be, and so it is one to the
/// caller's `?` — but it has no source: it is the end of the chain, not a
/// wrapper around a failure.
impl std::error::Error for Refusal {}

/// Check one query the file wrote against what this index can answer, and
/// refuse it by `site` where it cannot be.
///
/// The one seam every declared query passes through — a criterion's, a
/// priority's, a mulligan's — before anything is grouped: an `otag:` this index
/// never fetched matches nothing, which would read as a deck that has none, or
/// silently empty a tier, or throw back every hand.
///
/// The keyword check is separate from the tag check because the two indexes
/// are authoritative about different things. An index carrying no tags is a
/// fact it asserts about itself; an index listing no keywords is a fact it
/// never recorded, so it refuses nothing — `unknown_keywords` already knows
/// that and returns nothing there rather than calling every keyword a typo.
pub fn check_query(
    file: &str,
    site: QuerySite,
    query: &str,
    library: &Library,
) -> Result<(), Refusal> {
    let parsed = match chip_scryfall::parse(query) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(Refusal::UnsupportedQuery {
                file: file.to_string(),
                site,
                query: query.to_string(),
                error: Box::new(error),
            })
        }
    };
    if let Some(gap) = parsed.tag_gap(&library.index_tags) {
        return Err(Refusal::TagGap {
            file: file.to_string(),
            site,
            query: query.to_string(),
            gap: Untagged::of(gap, library),
        });
    }
    let keywords = parsed.unknown_keywords(&library.index_keywords);
    if !keywords.is_empty() {
        return Err(Refusal::UnknownKeyword {
            file: file.to_string(),
            site,
            query: query.to_string(),
            keywords,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_refusal_is_named_by_where_the_query_was_written() {
        let refusal = |site| Refusal::UnknownKeyword {
            file: "f.toml".to_string(),
            site,
            query: "kw:flyign".to_string(),
            keywords: vec!["flyign".to_string()],
        };
        let starts = [
            (
                QuerySite::Question("a flier".to_string()),
                "f.toml: a flier: in query \"kw:flyign\": no card in this index has kw:flyign, ",
            ),
            (
                QuerySite::LandDrop,
                "f.toml: [land_drop]: in `prefer` entry \"kw:flyign\": no card",
            ),
            (
                QuerySite::Casting,
                "f.toml: [casting]: in `prefer` entry \"kw:flyign\": no card",
            ),
            (
                QuerySite::Mulligan(MulliganAt::Bottom(2)),
                "f.toml: [mulligan]: `bottom` entry 2, query \"kw:flyign\": no card",
            ),
        ];
        for (site, start) in starts {
            let text = refusal(site).to_string();
            assert!(text.starts_with(start), "{text}");
        }
    }

    #[test]
    fn a_refusal_names_the_file_first_and_the_question_where_it_has_one() {
        let casting = Refusal::CastingWithoutPriority {
            file: "f.toml".to_string(),
            asked_by: "an Opt by turn 1".to_string(),
        };
        assert_eq!(casting.file(), "f.toml");
        assert_eq!(casting.question().as_deref(), Some("an Opt by turn 1"));
        assert!(casting
            .to_string()
            .starts_with("f.toml: an Opt by turn 1: counting the spells you cast"));

        let tutor = Refusal::FetchWithoutCasting {
            file: "f.toml".to_string(),
            effect: "name:\"Trinket Mage\"".to_string(),
        };
        assert_eq!(tutor.question(), None);
        assert!(tutor
            .to_string()
            .starts_with("f.toml: effect \"name:\\\"Trinket Mage\\\"\" fetches when it is cast"));

        let saga = Refusal::DelayedFetchFindsLand {
            file: "f.toml".to_string(),
            effect: "name:\"Urza's Saga\"".to_string(),
            query: "t:artifact".to_string(),
            lands: 1,
        };
        assert!(saga.to_string().starts_with(
            "f.toml: effect \"name:\\\"Urza's Saga\\\"\": `fetch = \"t:artifact\"` with `after` \
             and `to = \"battlefield\"` matches 1 land in this deck."
        ));
    }

    #[test]
    fn a_tag_gap_lists_what_the_index_carries_only_where_it_carries_any() {
        let refusal = |gap| Refusal::TagGap {
            file: "f.toml".to_string(),
            site: QuerySite::Question("q".to_string()),
            query: "otag:mill".to_string(),
            gap,
        };
        let carried = refusal(Untagged::NotCarried {
            tags: vec!["mill".to_string()],
            carried: vec!["scry".to_string(), "surveil".to_string()],
        })
        .to_string();
        assert!(
            carried.contains("does not carry otag:mill")
                && carried.contains("carries: scry, surveil."),
            "{carried}"
        );
        let none = refusal(Untagged::NoneFetched {
            tags: vec!["mill".to_string()],
        })
        .to_string();
        assert!(
            none.contains("carries no oracle tags at all, so otag:mill"),
            "{none}"
        );
    }
}
