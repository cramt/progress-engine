//! The card index: what a card is, as this tool stores it.
//!
//! Built by `gauntlet sync` from Scryfall's bulk data (see
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
use std::io::Write;
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
pub const SCHEMA: u32 = 2;

/// What the index is called on disk.
pub const FILE_NAME: &str = "index.jsonl";

/// What separates a card's key from the card on its line.
///
/// A tab, because no card name contains one, and because it means a key can be
/// found without parsing the card behind it — which is the whole point of the
/// format. It also leaves the file greppable: `grep '^sol ring<TAB>'` is a
/// working lookup.
const SEPARATOR: char = '\t';

#[derive(Debug, Error)]
pub enum IndexError {
    #[error(
        "no Scryfall index at {0}.\n\
         Build one with: gauntlet sync"
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
    #[error(
        "the index at {0} is empty: it has no header line.\n\
         Rebuild it with: gauntlet sync"
    )]
    NoHeader(PathBuf),
    /// The header's card count and the number of card lines disagree.
    ///
    /// Worth its own variant because it is the failure a line-per-card format
    /// adds: a JSON document cut in half stops parsing, whereas a truncated
    /// list of lines still reads as a shorter list. A missing card is a lookup
    /// that fails or, worse, a legality check with nothing to say — so the file
    /// states how many cards it should have and this is what checks it.
    #[error(
        "the index at {path} is truncated: its header claims {expected} cards \
         but {found} lines follow.\n\
         Rebuild it with: gauntlet sync"
    )]
    Truncated {
        path: PathBuf,
        expected: usize,
        found: usize,
    },
    /// A card line with no key in front of it.
    #[error("the index at {path} has an entry with no name key on line {line}")]
    UnkeyedEntry { path: PathBuf, line: usize },
    /// The key a card is filed under is not the key its name produces, so a
    /// lookup by that name would find the wrong card — or the right card under
    /// a name nobody can ask for. Only checked on cards actually read, which is
    /// where it would do harm.
    #[error(
        "the index at {path} files {name:?} under {key:?}, which is not the key \
         that name produces.\n\
         Rebuild it with: gauntlet sync"
    )]
    KeyMismatch {
        path: PathBuf,
        key: String,
        name: String,
    },
    #[error("serialising the index: {0}")]
    Serialize(String),
}

/// One face of a card.
///
/// Every card has at least one, including the single-faced ones — so nothing
/// downstream has to branch on how many there are.
///
/// A face carries the *typed* data a query reads per face — its cost, its
/// power and toughness — and not its oracle text. The joined text at card level
/// is what `o:` and `fo:` search, and storing it again per face was six
/// megabytes answering a question no query can currently ask. Scryfall's own shape makes
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
    /// Colours, with a colourless back face's colour indicator already folded
    /// in — see `bulk::BulkCard::face_colors`.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub colors: Vec<String>,
    /// Kept as printed rather than parsed, because `*`, `1+*`, `∞` and `.5` are
    /// all real values and none of them is a number. Reading one as a number is
    /// [`crate::numeric`]'s job, and it is allowed to answer *not a number*.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub power: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub toughness: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub loyalty: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub defense: Option<String>,
}

/// One card, as the index stores it.
#[derive(Debug, Clone, Default, Facet)]
pub struct Card {
    pub name: String,
    /// Scryfall's identity for the card, kept so a later index can be keyed by
    /// something that is actually unique. Names are not: forty of them name more
    /// than one card.
    #[facet(default)]
    pub oracle_id: Option<String>,
    #[facet(default)]
    pub layout: String,
    /// Colour identity letters, e.g. `["W","U"]`. What `id:` reads.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub ci: Vec<String>,
    /// The card's own colours, which are **not** its identity. Kor Haven's
    /// identity is white; its colour is nothing at all. Conflating the two is
    /// the classic quiet error, so they are separate fields read by separate
    /// keys: `c:` here, `id:` above.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
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
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub full_oracle: Option<String>,
    /// Keyword abilities, keyword actions and ability words as Scryfall prints
    /// them, e.g. `["Hexproof from", "Hexproof"]`. Read through `kw:` rather
    /// than the oracle text: `o:flying` also matches "creatures with flying
    /// can't block".
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub keywords: Vec<String>,
    /// The mana this card can actually make, as Scryfall's `produced_mana`.
    /// The answer to the README's opening bug: Kor Haven's `{W}` is in an
    /// activation cost, so it produces `["C"]` and no amount of oracle-text
    /// regex has to be trusted about it.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub produces: Vec<String>,
    #[facet(default)]
    pub rarity: String,
    #[facet(default)]
    pub set: String,
    /// Every face, including the only one on a single-faced card.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
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
    #[facet(default, skip_serializing_if = LegalityWord::is_empty)]
    pub commander_legal: LegalityWord,
    /// Whether a deck may contain any number of copies. `None` when the index
    /// never said — see [`Card::may_appear_any_number_of_times`].
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub any_number: Option<bool>,
    /// Scryfall's Commander "game changer" flag, for bracket questions.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub game_changer: Option<bool>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub reserved: Option<bool>,
    /// The oracle tags this card is a member of, as Scryfall's search answered
    /// at sync time.
    ///
    /// Empty means *this card is in none of the tags this index carries*, which
    /// is only meaningful alongside [`Header::tags`] saying which those were.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub tags: Vec<String>,
}

/// Every card, in memory, as `sync` builds it before writing.
///
/// The read side is [`IndexFile`], which does not build one of these: a run
/// wants a hundred cards and this is thirty-five thousand.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub cards: HashMap<String, Card>,
    /// Which shape this index is being written in; see [`SCHEMA`] and
    /// [`Header::schema`].
    pub schema: Option<u32>,
    /// When the bulk data this was built from was published; see
    /// [`Header::updated_at`].
    pub updated_at: Option<String>,
    /// Which oracle tags were fetched into this index; see [`Header::tags`].
    ///
    /// Held here rather than derived from the cards, because a tag that turned
    /// out to have no members in this pool is still a tag the index carries —
    /// and deriving the list from the cards would silently drop it, turning
    /// *asked, nobody matched* into *never asked*.
    pub tags: Vec<String>,
    /// When those memberships were fetched; see [`Header::tags_fetched_at`].
    pub tags_fetched_at: Option<String>,
}

/// What an index file says about itself, on its first line.
///
/// Everything here is about the file as a whole rather than about any card, so
/// a run that only needs to know *when this was built* or *whether it predates
/// the fields my queries read* reads one line instead of thirty-five thousand.
#[derive(Debug, Clone, Default, Facet)]
pub struct Header {
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
    /// Kept as text rather than a parsed timestamp: parsing would reject a
    /// whole index over one unfamiliar format, and nothing here does arithmetic
    /// on it.
    #[facet(default)]
    pub updated_at: Option<String>,
    /// How many card lines should follow. `None` from a hand-written fixture
    /// that never counted itself, which is a gap rather than a claim of zero.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub cards: Option<usize>,
    /// Every keyword the whole card pool carries — see
    /// [`IndexFile::keyword_vocabulary`].
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub keywords: Vec<String>,
    /// Every oracle tag this index carries, as the vocabulary `otag:` checks a
    /// typo against — see [`IndexFile::tag_vocabulary`].
    ///
    /// Unlike keywords, this is not derived from the cards. Tag membership is
    /// Scryfall's answer to a search rather than a field on a card, so an index
    /// carries the tags it was told to fetch and no others. Listing them is
    /// what lets a query tell *this index does not carry that tag* apart from
    /// *no card has it* — the same empty result, and very different facts.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub tags: Vec<String>,
    /// When the tag memberships were fetched.
    ///
    /// Separate from [`Self::updated_at`], which is when the bulk data was
    /// published. The two move independently — tags come from the search API at
    /// sync time and the bulk file carries its own release date — so one date
    /// standing for both would misdate whichever it was not.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub tags_fetched_at: Option<String>,
}

/// An index on disk, read a card at a time.
///
/// The file is held as text and the lines are located, but only the cards
/// actually asked for are parsed. That is the difference between a run costing
/// what Magic costs and a run costing what your deck costs: a hundred cards
/// parsed instead of thirty-five thousand.
///
/// Locating a line is cheap because the key is written in front of it, so
/// finding Sol Ring never involves deciding what the JSON after the tab means.
pub struct IndexFile {
    path: PathBuf,
    header: Header,
    text: String,
    /// Key to the byte range of that card's JSON within `text`.
    entries: HashMap<String, (usize, usize)>,
}

/// Hand-written because the derived one would print the whole file: this holds
/// every byte of a 24MB index, and a panic message is not the place for it.
impl std::fmt::Debug for IndexFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexFile")
            .field("path", &self.path)
            .field("header", &self.header)
            .field("cards", &self.entries.len())
            .finish()
    }
}

impl IndexFile {
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        if !path.exists() {
            return Err(IndexError::Missing(path.to_path_buf()));
        }
        let text = std::fs::read_to_string(path).map_err(|source| IndexError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(path, text)
    }

    /// The same, for text already in hand. `path` is only ever named in errors.
    pub fn parse(path: &Path, text: String) -> Result<Self, IndexError> {
        let header_end = text
            .find('\n')
            .ok_or_else(|| IndexError::NoHeader(path.to_path_buf()))?;
        let header: Header =
            facet_json::from_str(text[..header_end].trim_end()).map_err(|source| {
                IndexError::Json {
                    path: path.to_path_buf(),
                    source: Box::new(source),
                }
            })?;

        let mut entries = HashMap::new();
        let mut found = 0usize;
        let mut offset = header_end + 1;
        let mut line_number = 1usize;
        while offset < text.len() {
            let rest = &text[offset..];
            let length = rest.find('\n').unwrap_or(rest.len());
            let line = &rest[..length];
            line_number += 1;
            if !line.trim().is_empty() {
                let separator = line.find(SEPARATOR).ok_or(IndexError::UnkeyedEntry {
                    path: path.to_path_buf(),
                    line: line_number,
                })?;
                found += 1;
                entries.insert(
                    line[..separator].to_string(),
                    (offset + separator + 1, offset + length),
                );
            }
            offset += length + 1;
        }

        if let Some(expected) = header.cards {
            if expected != found {
                return Err(IndexError::Truncated {
                    path: path.to_path_buf(),
                    expected,
                    found,
                });
            }
        }

        Ok(IndexFile {
            path: path.to_path_buf(),
            header,
            text,
            entries,
        })
    }

    /// The card filed under this name, parsed now.
    ///
    /// Parsing is deferred to here rather than done at open, so the cost of an
    /// index is the cost of the cards you name.
    pub fn get(&self, name: &str) -> Result<Option<Card>, IndexError> {
        let key = keyname(name);
        let Some(&(start, end)) = self.entries.get(&key) else {
            return Ok(None);
        };
        let card: Card =
            facet_json::from_str(&self.text[start..end]).map_err(|source| IndexError::Json {
                path: self.path.clone(),
                source: Box::new(source),
            })?;
        // The key is written beside the card rather than derived from it, which
        // is what makes a lookup cheap — and what lets the two disagree. They
        // cannot disagree unnoticed about a card anyone actually reads.
        if keyname(&card.name) != key {
            return Err(IndexError::KeyMismatch {
                path: self.path.clone(),
                key,
                name: card.name,
            });
        }
        Ok(Some(card))
    }

    /// Whether this index has an entry for a name, without parsing it.
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(&keyname(name))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn updated_at(&self) -> Option<&str> {
        self.header.updated_at.as_deref()
    }

    /// Which shape this index was written in. An index that never said is
    /// schema 0 rather than the current one — claiming otherwise would be a
    /// report vouching for fields nobody wrote.
    pub fn schema(&self) -> u32 {
        self.header.schema.unwrap_or(0)
    }

    /// Whether this index predates fields the current queries read.
    pub fn is_stale(&self) -> bool {
        self.schema() < SCHEMA
    }

    /// Every keyword the pool carries, lowercased for comparison.
    ///
    /// Scryfall answers `kw:tramp` with *Unknown keyword "tramp"* rather than
    /// an empty result, and it is right to: a mistyped keyword that quietly
    /// matches zero cards is the confident 0% this project exists to prevent.
    /// The parser cannot make that check on its own — the set of real keywords
    /// grows with every set, so a list hard-coded next to the parser would be a
    /// second opinion about what a card is, and would start refusing real
    /// queries the day it fell behind. The index is the authority, so the check
    /// lives here; see [`crate::Query::unknown_keywords`].
    ///
    /// Read from the header rather than from the cards, because the cards are
    /// not read. `sync` derives it from the cards it is writing, so the two
    /// describe the same pool by construction; an index whose header never
    /// mentioned keywords yields an empty vocabulary, which
    /// [`crate::Query::unknown_keywords`] already knows proves nothing about
    /// any keyword.
    pub fn keyword_vocabulary(&self) -> KeywordVocabulary {
        KeywordVocabulary {
            known: self
                .header
                .keywords
                .iter()
                .map(|k| k.to_lowercase())
                .collect(),
        }
    }

    /// Which oracle tags this index carries, read off the header.
    ///
    /// The same argument as [`Self::keyword_vocabulary`], one step stronger.
    /// A keyword list hard-coded beside the parser would merely fall behind;
    /// a tag list would be wrong on arrival, because tag membership is not
    /// derivable from a card at all. Scryfall is asked, the answer is written
    /// down with the date it was given, and the header says which tags were
    /// asked about so that a query naming any other one can be refused instead
    /// of quietly matching nothing.
    pub fn tag_vocabulary(&self) -> TagVocabulary {
        TagVocabulary {
            known: self.header.tags.iter().map(|t| t.to_lowercase()).collect(),
        }
    }

    /// When this index's tag memberships were fetched, if it carries any.
    pub fn tags_fetched_at(&self) -> Option<&str> {
        self.header.tags_fetched_at.as_deref()
    }
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
    ///
    /// Named `.jsonl` rather than `.json` because it is one, and because the
    /// `index.json` the shell tool wrote is a different format that this build
    /// cannot read. Sitting beside it rather than on top of it means an
    /// upgrade asks for a sync instead of failing to parse a file it never
    /// wrote.
    pub fn default_path() -> PathBuf {
        if let Ok(dir) = std::env::var("SCRYFALL_CACHE") {
            return PathBuf::from(dir).join(FILE_NAME);
        }
        let base = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
            });
        base.join("scryfall").join(FILE_NAME)
    }

    /// This index as the file format: a header line, then one card per line.
    ///
    /// Sorted by key so that two syncs of the same bulk data produce the same
    /// bytes. A `HashMap` iterated in its own order would not, and an index
    /// whose bytes move for no reason is one whose hash cannot be quoted as
    /// provenance.
    pub fn to_lines(&self) -> Result<String, IndexError> {
        let header = Header {
            schema: self.schema,
            updated_at: self.updated_at.clone(),
            cards: Some(self.cards.len()),
            keywords: self.keyword_list(),
            tags: {
                let mut t = self.tags.clone();
                t.sort();
                t.dedup();
                t
            },
            tags_fetched_at: self.tags_fetched_at.clone(),
        };
        let mut out =
            facet_json::to_string(&header).map_err(|e| IndexError::Serialize(e.to_string()))?;
        out.push('\n');

        let mut keys: Vec<&String> = self.cards.keys().collect();
        keys.sort();
        for key in keys {
            let card = &self.cards[key];
            let json = facet_json::to_string(card)
                .map_err(|e| IndexError::Serialize(format!("{}: {e}", card.name)))?;
            out.push_str(key);
            out.push(SEPARATOR);
            out.push_str(&json);
            out.push('\n');
        }
        Ok(out)
    }

    /// Write to a neighbouring temporary file and rename over the target.
    ///
    /// A rename is atomic, so an interrupted sync leaves the previous index
    /// intact rather than a half-written one. The write lives here beside
    /// [`IndexFile::open`] because a format read in one crate and written in
    /// another is two opinions about one file.
    pub fn write_atomically(&self, path: &Path) -> Result<(), IndexError> {
        let text = self.to_lines()?;
        let io = |path: &Path| {
            let path = path.to_path_buf();
            move |source| IndexError::Io {
                path: path.clone(),
                source,
            }
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io(dir))?;
        }
        // `<name>.partial`, appended rather than replacing the extension, so
        // the debris of a failed sync is named after the file it was going to
        // become.
        let temp = {
            let mut name = path.as_os_str().to_owned();
            name.push(".partial");
            PathBuf::from(name)
        };
        {
            let mut file = std::fs::File::create(&temp).map_err(io(&temp))?;
            file.write_all(text.as_bytes()).map_err(io(&temp))?;
            file.sync_all().map_err(io(&temp))?;
        }
        std::fs::rename(&temp, path).map_err(io(path))
    }

    /// Every keyword any card here carries, as Scryfall prints them, sorted.
    ///
    /// Written into the header so that [`IndexFile`] can answer `kw:` questions
    /// about the whole card pool without reading the whole card pool. Derived
    /// at write time from the cards being written, so the two cannot describe
    /// different pools.
    fn keyword_list(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .cards
            .values()
            .flat_map(|c| c.keywords.iter().cloned())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        all.sort();
        all
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
                // Two genuinely different cards under one name — forty of
                // them, nearly all Un-set variants. One has to lose, so the winner
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
                // Built from bulk data alone, which says nothing about tags.
                // `sync` attaches them after this, so an index built straight
                // from a bulk file honestly carries none.
                tags: Vec::new(),
                tags_fetched_at: None,
            },
            report,
        )
    }

    /// Record which tags were fetched, and which cards are in them.
    ///
    /// `membership` is keyed by oracle id rather than by name, because tags are
    /// a property of the card rather than of a printing, and because forty
    /// names in Magic refer to more than one card. Cards the map does not
    /// mention are in none of these tags — which is a claim this index is
    /// entitled to make only because `tags` records that it asked.
    ///
    /// Returns how many cards were tagged, so `sync` can report it and a
    /// reader can tell "the fetch returned nothing" from "the fetch was never
    /// made". Silence here is the failure this whole file is careful about.
    pub fn attach_tags(
        &mut self,
        tags: Vec<String>,
        fetched_at: Option<String>,
        membership: &HashMap<String, Vec<String>>,
    ) -> usize {
        let mut tagged = 0;
        for card in self.cards.values_mut() {
            let Some(oracle_id) = card.oracle_id.as_deref() else {
                continue;
            };
            if let Some(found) = membership.get(oracle_id) {
                card.tags = found.clone();
                card.tags.sort();
                tagged += 1;
            }
        }
        self.tags = tags;
        self.tags_fetched_at = fetched_at;
        tagged
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

/// The oracle tags an index carries, for checking an `otag:` term against.
///
/// Separate type from [`KeywordVocabulary`] despite the identical shape,
/// because the two answer different questions and an index can be authoritative
/// about one while silent about the other. Sharing a type would let a caller
/// check a tag against the keyword list and get a confident wrong answer.
#[derive(Debug, Clone, Default)]
pub struct TagVocabulary {
    known: HashSet<String>,
}

impl TagVocabulary {
    pub fn contains(&self, tag: &str) -> bool {
        self.known.contains(&tag.to_lowercase())
    }

    /// Every tag this index carries, sorted.
    ///
    /// For a refusal that has to say what it *does* have: "no such tag" sends
    /// somebody hunting for a typo, and the list beside it settles in one line
    /// whether they made one.
    pub fn carried(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.known.iter().map(String::as_str).collect();
        out.sort_unstable();
        out
    }

    /// Whether the index carries any tags at all. Not public, because the
    /// answer callers need is the classified one from [`crate::Query::tag_gap`]
    /// — an index carrying nothing and an index carrying the wrong things are
    /// different facts with different fixes, and a bare predicate here would
    /// let a caller collapse them back into one.
    pub(crate) fn is_empty(&self) -> bool {
        self.known.is_empty()
    }
}

/// Why an index cannot answer an `otag:` term.
///
/// Two variants because the two have different causes and different fixes, and
/// a caller handed one list of names could not tell them apart. A tag missing
/// from an index that carries others is a typo or a tag nobody added to the
/// standard list; an index carrying no tags at all is what `--from` builds and
/// what a sync whose tag phase never ran leaves behind, and no spelling
/// correction fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagGap {
    /// The index carries tags, and these are not among them.
    NotCarried(Vec<String>),
    /// The index carries no tags at all, so every `otag:` term is unanswerable.
    NoneFetched(Vec<String>),
}

impl TagGap {
    /// The `otag:` terms this gap is about, in the order the query named them.
    pub fn tags(&self) -> &[String] {
        match self {
            TagGap::NotCarried(tags) | TagGap::NoneFetched(tags) => tags,
        }
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
            tags: &self.tags,
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
        match self.legalities.commander() {
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
