//! The card index: what a card is, as this tool stores it.
//!
//! Built by `progress-engine sync` from Scryfall's bulk data (see
//! [`crate::bulk`]) and read by everything else. The reduction from Scryfall's
//! sixty-odd fields to the seventeen here happens once, at sync time, so that
//! the shape a query sees is decided in one place rather than re-derived by
//! every reader.
//!
//! Every field but the name defaults. That is not laziness: the index is a
//! cache, older copies of it predate half these fields, and the test fixtures
//! are hand-written subsets that carry two or three. A reader that treats an
//! absent field as a fact about the card is the confidently wrong number this
//! project exists to prevent, so absence has to be representable everywhere.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use facet::Facet;
use thiserror::Error;

use crate::bulk::{Anomaly, BulkCard, Skipped};
use crate::legality::{self, CommanderLegality, CommanderRoute, Legalities, LegalityWord};
use crate::{CardView, Colors, OutsideLibrary};

/// The index shape this build of the tool writes.
///
/// Bumped when a field is added that a query depends on, so a run against an
/// older index can say *rebuild it* rather than quietly answering from data
/// that was never there. An index with no schema at all predates the field and
/// is treated as schema 0 — which is the truth about every index the external
/// `scryfall sync` shell tool ever wrote.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error(
        "no Scryfall index at {0}.\n\
         Build one with: progress-engine sync"
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

/// One face of a card.
///
/// Every card has at least one, including the single-faced ones — so nothing
/// downstream has to branch on how many there are. Scryfall's own shape makes
/// `power` mean the card's power on one layout and nothing at all on another,
/// and flattening that three-case match into one uniform list here is what lets
/// `pow>=3` be written once and be right about Delver of Secrets.
#[derive(Debug, Clone, Default, Facet)]
pub struct Face {
    #[facet(default)]
    pub name: String,
    #[facet(default)]
    pub type_line: String,
    #[facet(default)]
    pub mana_cost: String,
    /// Verbatim, reminder text included. The card-level [`Card::oracle`] is
    /// what `o:` searches; this is here so a face-level question can be asked
    /// later without another sync.
    #[facet(default)]
    pub full_oracle: String,
    /// Colours, with a colourless back face's colour indicator already folded
    /// in — see `bulk::BulkCard::face_colors`.
    #[facet(default)]
    pub colors: Vec<String>,
    /// Kept as printed rather than parsed, because `*`, `1+*`, `∞` and `.5` are
    /// all real values and none of them is a number. Reading one as a number is
    /// [`crate::numeric`]'s job, and it is allowed to answer *not a number*.
    #[facet(default)]
    pub power: Option<String>,
    #[facet(default)]
    pub toughness: Option<String>,
    #[facet(default)]
    pub loyalty: Option<String>,
    #[facet(default)]
    pub defense: Option<String>,
}

/// One card, as the index stores it.
#[derive(Debug, Clone, Default, Facet)]
pub struct Card {
    pub name: String,
    /// Scryfall's identity for the card, kept so a later index can be keyed by
    /// something that is actually unique. Names are not: thirty-eight of them
    /// name more than one card.
    #[facet(default)]
    pub oracle_id: Option<String>,
    #[facet(default)]
    pub layout: String,
    /// Colour identity letters, e.g. `["W","U"]`. What `id:` reads.
    #[facet(default)]
    pub ci: Vec<String>,
    /// The card's own colours, which are **not** its identity. Kor Haven's
    /// identity is white; its colour is nothing at all. Conflating the two is
    /// the classic quiet error, so they are separate fields read by separate
    /// keys: `c:` here, `id:` above.
    #[facet(default)]
    pub colors: Vec<String>,
    #[facet(default)]
    pub type_line: String,
    #[facet(default)]
    pub mana_cost: String,
    #[facet(default)]
    pub cmc: f64,
    /// Oracle text of every face, joined, **with reminder text removed**. This
    /// is what `o:` searches, matching Scryfall, so `o:flying` is not satisfied
    /// by "(This creature can't be blocked except by creatures with flying.)".
    #[facet(default)]
    pub oracle: String,
    /// The same text with reminder text left in, which is what `fo:` searches.
    /// Written only when it differs from `oracle`, which is 28% of cards;
    /// `None` means the two are the same, not that the text is missing.
    #[facet(default)]
    pub full_oracle: Option<String>,
    /// Keyword abilities, keyword actions and ability words as Scryfall prints
    /// them, e.g. `["Hexproof from", "Hexproof"]`. Read through `kw:` rather
    /// than the oracle text: `o:flying` also matches "creatures with flying
    /// can't block".
    #[facet(default)]
    pub keywords: Vec<String>,
    /// The mana this card can actually make, as Scryfall's `produced_mana`.
    /// The answer to the README's opening bug: Kor Haven's `{W}` is in an
    /// activation cost, so it produces `["C"]` and no amount of oracle-text
    /// regex has to be trusted about it.
    #[facet(default)]
    pub produces: Vec<String>,
    #[facet(default)]
    pub rarity: String,
    #[facet(default)]
    pub set: String,
    /// Every face, including the only one on a single-faced card.
    #[facet(default)]
    pub faces: Vec<Face>,
    /// What every format says about this card. Read `f:`, `banned:` and the
    /// Commander check through it.
    #[facet(default)]
    pub legalities: Legalities,
    /// An older index's single Commander legality word.
    ///
    /// Read for compatibility and **never written**: `sync` writes
    /// [`Card::legalities`] instead. Kept so that upgrading the tool without
    /// re-syncing does not silently stop checking the banlist — which would be
    /// a check quietly becoming a no-op, indistinguishable in the output from a
    /// deck that is fine.
    #[facet(default)]
    pub commander_legal: LegalityWord,
    /// Whether a deck may contain any number of copies. `None` when the index
    /// never said — see [`Card::may_appear_any_number_of_times`].
    #[facet(default)]
    pub any_number: Option<bool>,
    /// Scryfall's Commander "game changer" flag, for bracket questions.
    #[facet(default)]
    pub game_changer: Option<bool>,
    #[facet(default)]
    pub reserved: Option<bool>,
}

#[derive(Debug, Clone, Default, Facet)]
pub struct Index {
    pub cards: HashMap<String, Card>,
    /// Which shape this index was written in; see [`SCHEMA`]. `None` is
    /// schema 0, an index from before the field existed.
    #[facet(default)]
    pub schema: Option<u32>,
    /// When this index was built, as the builder wrote it.
    ///
    /// `None` is an answer rather than a failure, for the same reason a missing
    /// legality word is: older copies and hand-written fixtures routinely lack
    /// it. A caller that reports it must say *unknown* rather than invent a
    /// date — a report claiming today's index when nobody knows which index ran
    /// is the confidently wrong number this project exists to prevent.
    ///
    /// Kept as text rather than a parsed timestamp: parsing would reject a 25MB
    /// index over one unfamiliar format, and nothing here does arithmetic on it.
    #[facet(default)]
    pub updated_at: Option<String>,
}

/// What a sync did, in terms anybody can check against the next one.
///
/// "Wrote 38,626 cards" is not a statement you can act on; "dropped 3,309 as
/// tokens" is, because it moves when Scryfall's data moves. Every record that
/// went in is accounted for by exactly one line of this.
#[derive(Debug, Clone, Default)]
pub struct BuildReport {
    pub read: usize,
    pub kept: usize,
    pub skipped: Vec<(Skipped, usize)>,
    pub anomalies: Vec<Anomaly>,
}

/// Normalise a card name to the index's key form: lowercase and trimmed.
///
/// Must match how the index was built, or every lookup misses.
pub fn keyname(name: &str) -> String {
    name.trim().to_lowercase()
}

impl Index {
    /// Where the index lives.
    ///
    /// `$SCRYFALL_CACHE` first, for compatibility with the external shell tool
    /// this grew out of, then the XDG cache directory.
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

    /// Build an index from Scryfall bulk records.
    ///
    /// Takes an iterator so a 200MB file can be streamed a line at a time
    /// rather than held in memory twice over.
    pub fn build<I>(records: I, updated_at: Option<String>) -> (Self, BuildReport)
    where
        I: IntoIterator<Item = BulkCard>,
    {
        let mut cards: HashMap<String, Card> = HashMap::new();
        let mut report = BuildReport::default();
        let mut skipped: HashMap<Skipped, usize> = HashMap::new();

        for record in records {
            report.read += 1;
            let card = match record.to_card(&mut report.anomalies) {
                Ok(card) => card,
                Err(reason) => {
                    *skipped.entry(reason).or_default() += 1;
                    continue;
                }
            };
            let key = keyname(&card.name);
            match cards.get(&key) {
                // Two genuinely different cards under one name — thirty-eight
                // of them, all Un-set variants. One has to lose, so the winner
                // is chosen by a rule that gives the same answer every sync
                // rather than by whichever line came first: a name that
                // resolves differently between two syncs would move a
                // probability for no reason anybody could see.
                Some(existing) if existing.oracle_id <= card.oracle_id => {
                    report.anomalies.push(Anomaly::NameCollision {
                        name: card.name.clone(),
                        kept: existing.set.clone(),
                    });
                }
                Some(existing) => {
                    report.anomalies.push(Anomaly::NameCollision {
                        name: card.name.clone(),
                        kept: card.set.clone(),
                    });
                    let _ = existing;
                    cards.insert(key, card);
                }
                None => {
                    cards.insert(key, card);
                }
            }
        }

        report.kept = cards.len();
        report.skipped = {
            let mut v: Vec<_> = skipped.into_iter().collect();
            v.sort();
            v
        };
        (
            Index {
                cards,
                schema: Some(SCHEMA),
                updated_at,
            },
            report,
        )
    }

    pub fn get(&self, name: &str) -> Option<&Card> {
        self.cards.get(&keyname(name))
    }

    /// Which shape this index was written in. An index that never said is
    /// schema 0 rather than the current one — claiming otherwise would be a
    /// report vouching for fields nobody wrote.
    pub fn schema(&self) -> u32 {
        self.schema.unwrap_or(0)
    }

    /// Whether this index predates fields the current queries read.
    pub fn is_stale(&self) -> bool {
        self.schema() < SCHEMA
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
            full_oracle: self.full_oracle(),
            mana_cost: &self.mana_cost,
            cmc: self.cmc,
            keywords: &self.keywords,
            color_identity: &self.ci,
            colors: &self.colors,
            produces: &self.produces,
            rarity: &self.rarity,
            set: &self.set,
            layout: &self.layout,
            faces: &self.faces,
            legalities: &self.legalities,
            game_changer: self.game_changer,
            reserved: self.reserved,
            categories,
        }
    }

    /// Oracle text with reminder text left in, for `fo:`.
    ///
    /// Falls back to the stripped text, because `None` means the two are the
    /// same rather than that the text is missing.
    pub fn full_oracle(&self) -> &str {
        self.full_oracle.as_deref().unwrap_or(&self.oracle)
    }

    /// Which never-in-the-library type this card is, if it is one.
    pub fn outside_library(&self) -> Option<OutsideLibrary> {
        crate::outside_library(&self.type_line)
    }

    /// What the index says about this card's Commander legality.
    ///
    /// Prefers the format table this build writes, falling back to the single
    /// word an older index carried. The fallback cannot disagree with the
    /// table, because `sync` never writes both.
    pub fn commander_legality(&self) -> CommanderLegality {
        match self.legalities.commander.commander() {
            CommanderLegality::Unknown => self.commander_legal.commander(),
            known => known,
        }
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
