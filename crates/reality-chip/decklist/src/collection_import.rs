//! Another app's collection export, read into rows the collection can take
//! (ADR-0029).
//!
//! `docs/research/collection-import-formats.md` is the spec: which app writes
//! which header, which of its columns name a printing the way Scryfall does,
//! and how each writes a finish. What this module makes of it:
//!
//! - A file is recognised by its header, never by column position, because
//!   ManaBox and Archidekt move and add columns.
//! - A row names its card by the best key the file has. A Scryfall ID is
//!   Scryfall's, so it is trusted. A set and collector number is only a
//!   claim: Deckbox, TCGplayer and Dragon Shield write codes of their own, so
//!   [`resolve`] pins it only when Scryfall's card there has the row's name.
//! - What the collection has no key for (condition, language, price) is
//!   counted per column and said, not dropped without a word.
//! - A text list, `4 Sol Ring (CMM) 400 *F*`, is read as Archidekt text is.

use std::collections::HashMap;
use std::num::NonZeroU32;

use crate::archidekt::Unreadable;
use crate::collection::Incoming;
use crate::deck::{CardRef, Finish, Printing};
use crate::identity::same_name;

/// Whose export a file is, by its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    ManaBox,
    Moxfield,
    Archidekt,
    Deckbox,
    DragonShield,
    Sorted,
    Tcgplayer,
    Helvault,
    MtgGoldfish,
    TopDecked,
    /// A CSV none of the above wrote, read by the commonest column names.
    Csv,
    /// One card a line, as Archidekt and Moxfield take text.
    Text,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::ManaBox => "ManaBox",
            Source::Moxfield => "Moxfield",
            Source::Archidekt => "Archidekt",
            Source::Deckbox => "Deckbox",
            Source::DragonShield => "Dragon Shield",
            Source::Sorted => "Sorted",
            Source::Tcgplayer => "TCGplayer",
            Source::Helvault => "Helvault",
            Source::MtgGoldfish => "MTGGoldfish",
            Source::TopDecked => "TopDecked",
            Source::Csv => "CSV",
            Source::Text => "text list",
        }
    }
}

/// How a row named its card, before Scryfall has been asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Named {
    /// Scryfall's id for the printing; the row's name, where it gave one.
    Id { id: String, name: Option<String> },
    /// A set and collector number, which the row's name must match on
    /// Scryfall before it is taken as the printing.
    Printing { printing: Printing, name: String },
    /// A name and nothing that says which printing.
    Name(String),
}

impl Named {
    fn name(&self) -> Option<&str> {
        match self {
            Named::Id { name, .. } => name.as_deref(),
            Named::Printing { name, .. } | Named::Name(name) => Some(name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 1-based, counting every line of the file.
    pub line: usize,
    pub card: Named,
    pub qty: NonZeroU32,
    pub finish: Finish,
    /// The binder or folder the export put it in, where it says.
    pub place: Option<String>,
}

/// A column the collection keeps nothing of, and how many rows gave it a
/// value: what the import leaves behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    pub column: String,
    pub rows: usize,
}

/// Rows read on purpose as no owned card, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub rows: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub source: Source,
    pub rows: Vec<Row>,
    pub unreadable: Vec<Unreadable>,
    pub dropped: Vec<Dropped>,
    pub skipped: Vec<Skipped>,
}

/// Which column, by header, says what. Each list is tried in order and the
/// first the file has is the one read.
struct Profile {
    source: Source,
    /// Headers that together say the file is this app's.
    marks: &'static [&'static str],
    qty: &'static [&'static str],
    name: &'static [&'static str],
    id: &'static [&'static str],
    /// Only where the app writes Scryfall's set codes and numbers, or close
    /// enough that a name check catches the rest.
    set: &'static [&'static str],
    num: &'static [&'static str],
    finish: &'static [&'static str],
    place: &'static [&'static str],
    /// Columns that say nothing the collection would keep: another key for
    /// what the row already names, or the app's own bookkeeping.
    ignore: &'static [&'static str],
}

const PROFILES: &[Profile] = &[
    Profile {
        source: Source::ManaBox,
        marks: &["ManaBox ID"],
        qty: &["Quantity"],
        name: &["Name"],
        id: &["Scryfall ID"],
        set: &["Set code"],
        num: &["Collector number"],
        finish: &["Foil"],
        place: &["Binder Name"],
        ignore: &[
            "Binder Type",
            "Set name",
            "Rarity",
            "ManaBox ID",
            "Added",
            "Misprint",
            "Altered",
            "Signed",
            "Proxy",
            "Purchase price currency",
        ],
    },
    Profile {
        source: Source::Moxfield,
        marks: &["Tradelist Count", "Collector Number"],
        qty: &["Count"],
        name: &["Name"],
        id: &[],
        set: &["Edition"],
        num: &["Collector Number"],
        finish: &["Foil"],
        place: &[],
        ignore: &["Tradelist Count", "Last Modified", "Alter", "Proxy"],
    },
    Profile {
        source: Source::Deckbox,
        marks: &["Tradelist Count", "Card Number"],
        qty: &["Count"],
        name: &["Name"],
        id: &["Scryfall ID"],
        // Deckbox's edition codes and card numbers are its own (`ex_127`,
        // `DDC-49` written `49`), so only its Scryfall ID names a printing.
        set: &[],
        num: &[],
        finish: &["Foil"],
        place: &[],
        ignore: &[
            "Tradelist Count",
            "Edition",
            "Edition Code",
            "Card Number",
            "Printing Id",
            "Printing Note",
            "Rarity",
            "TcgPlayer ID",
            "Signed",
            "Artist Proof",
            "Altered Art",
            "Misprint",
            "Promo",
            "Textless",
        ],
    },
    Profile {
        source: Source::Archidekt,
        marks: &["Edition Code"],
        qty: &["Quantity"],
        name: &["Name"],
        id: &["Scryfall ID"],
        set: &["Edition Code"],
        num: &["Collector Number"],
        finish: &["Finish"],
        place: &[],
        ignore: &["Edition Name", "Multiverse Id", "Date Added"],
    },
    Profile {
        source: Source::DragonShield,
        marks: &["Folder Name", "Card Name"],
        qty: &["Quantity"],
        name: &["Card Name"],
        id: &[],
        set: &["Set Code"],
        num: &["Card Number"],
        finish: &["Printing"],
        place: &["Folder Name"],
        ignore: &[
            "Trade Quantity",
            "Set Name",
            "Date Bought",
            "LOW",
            "MID",
            "MARKET",
            "AVG",
            "TREND",
        ],
    },
    Profile {
        source: Source::Sorted,
        marks: &["List Name", "Card Name"],
        qty: &["Quantity"],
        name: &["Card Name"],
        id: &[],
        set: &["Set Code"],
        num: &["Card Number"],
        finish: &["Printing"],
        place: &["List Name"],
        ignore: &[
            "List Type",
            "Collection",
            "Format",
            "Board",
            "Set Name",
            "Rarity",
            "Date Bought",
            "Parent List Type",
            "Parent List Name",
            "Current Price (tcgplayer_marketsellprice)",
            "List Cover Image",
            "Parent List Cover Image",
        ],
    },
    Profile {
        source: Source::Tcgplayer,
        marks: &["Simple Name"],
        qty: &["Quantity"],
        // `Name` carries TCGplayer's own tags, `(Borderless)` and the like.
        name: &["Simple Name", "Name"],
        id: &[],
        set: &["Set Code"],
        num: &["Card Number"],
        finish: &["Printing"],
        place: &[],
        ignore: &[
            "Name",
            "Set",
            "Rarity",
            "Product ID",
            "SKU",
            "External ID",
            "Price",
            "Price Each",
        ],
    },
    Profile {
        source: Source::Helvault,
        marks: &["scryfall_id", "extras"],
        qty: &["quantity"],
        name: &["name"],
        id: &["scryfall_id"],
        set: &["set_code"],
        num: &["collector_number"],
        finish: &["extras"],
        place: &[],
        ignore: &["set_name", "rarity", "oracle_id", "estimated_price"],
    },
    Profile {
        source: Source::MtgGoldfish,
        marks: &["Set ID", "Card"],
        qty: &["Quantity"],
        name: &["Card"],
        id: &["Scryfall ID"],
        // MTGO's codes (`DD3_DVD`, `PRM-FNM`), mostly Scryfall's otherwise.
        set: &["Set ID"],
        num: &["Collector Number"],
        finish: &["Foil"],
        place: &[],
        ignore: &["Set Name", "Variation"],
    },
    Profile {
        source: Source::TopDecked,
        marks: &["SETCODE", "COLLECTOR NUMBER"],
        qty: &["QUANTITY"],
        name: &["NAME"],
        // Scryfall's UUID, under a bare `ID`.
        id: &["ID"],
        set: &["SETCODE"],
        num: &["COLLECTOR NUMBER"],
        finish: &["FINISH"],
        place: &[],
        ignore: &["SETNAME", "RARITY", "PRICE", "ACQUIRED DATE"],
    },
    Profile {
        source: Source::Csv,
        marks: &[],
        qty: &["Quantity", "Count", "Qty", "Amount"],
        name: &["Name", "Card Name", "Card"],
        id: &["Scryfall ID", "scryfall_id", "Scryfall Id"],
        set: &["Set Code", "Set code", "Edition Code", "Set", "Edition"],
        num: &[
            "Collector Number",
            "Collector number",
            "Card Number",
            "Number",
        ],
        finish: &["Finish", "Foil", "Printing"],
        place: &["Binder Name", "Binder", "Folder Name", "Folder", "Location"],
        ignore: &["Set Name", "Set name", "Rarity"],
    },
];

/// The finish a cell writes. Every app's words for nonfoil, foil and etched,
/// and Dragon Shield's foil treatments, which are each a foil.
fn finish(cell: &str) -> Option<Finish> {
    let c = cell.trim().to_lowercase();
    match c.as_str() {
        "" | "normal" | "regular" | "nonfoil" | "non-foil" | "false" | "no" | "0" => {
            Some(Finish::Nonfoil)
        }
        "foil" | "true" | "yes" | "1" => Some(Finish::Foil),
        "etched" | "etchedfoil" | "etched foil" | "foil_etched" => Some(Finish::Etched),
        _ if c.ends_with(" foil") => Some(Finish::Foil),
        _ => None,
    }
}

/// A language cell that says English, which the collection takes cards to be.
fn english(cell: &str) -> bool {
    matches!(cell.trim().to_lowercase().as_str(), "en" | "english")
}

struct Columns {
    qty: Option<usize>,
    name: Option<usize>,
    id: Option<usize>,
    set: Option<usize>,
    num: Option<usize>,
    finish: Option<usize>,
    place: Option<usize>,
    /// ManaBox's, to tell a list from a binder.
    binder_type: Option<usize>,
    /// The rest, by index, that the collection keeps nothing of.
    dropped: Vec<usize>,
}

fn find(header: &[String], names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|n| header.iter().position(|h| h.trim().eq_ignore_ascii_case(n)))
}

fn columns(profile: &Profile, header: &[String]) -> Columns {
    let at = |names: &[&str]| find(header, names);
    let mut c = Columns {
        qty: at(profile.qty),
        name: at(profile.name),
        id: at(profile.id),
        set: at(profile.set),
        num: at(profile.num),
        finish: at(profile.finish),
        place: at(profile.place),
        binder_type: (profile.source == Source::ManaBox)
            .then(|| at(&["Binder Type"]))
            .flatten(),
        dropped: Vec::new(),
    };
    let used = [c.qty, c.name, c.id, c.set, c.num, c.finish, c.place];
    c.dropped = (0..header.len())
        .filter(|i| !used.contains(&Some(*i)))
        .filter(|i| find(&[header[*i].clone()], profile.ignore).is_none())
        .filter(|i| !header[*i].trim().is_empty())
        .collect();
    c
}

fn profile_for(header: &[String]) -> Option<&'static Profile> {
    let has = |n: &str| header.iter().any(|h| h.trim().eq_ignore_ascii_case(n));
    PROFILES
        .iter()
        .find(|p| !p.marks.is_empty() && p.marks.iter().all(|m| has(m)))
        .or_else(|| {
            let generic = PROFILES.last()?;
            (find(header, generic.name).is_some() || find(header, generic.id).is_some())
                .then_some(generic)
        })
}

/// The delimiter a header line is written with: Dragon Shield says so on a
/// line of its own, and a spreadsheet saved in a European locale uses `;`.
fn delimiter(header: &str) -> u8 {
    let count = |d: u8| header.bytes().filter(|b| *b == d).count();
    // A tie is a comma's, the commonest.
    b"\t;"
        .iter()
        .copied()
        .filter(|d| count(*d) > count(b','))
        .max_by_key(|d| count(*d))
        .unwrap_or(b',')
}

/// Reads an export, whichever app wrote it, or a text list when the first
/// line is no header this knows.
pub fn read(text: &str) -> Read {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Lines before the header: Dragon Shield's `"sep=,"`, and blank ones.
    let mut skip = 0;
    let mut sep = None;
    for l in text.lines() {
        let t = l.trim().trim_matches('"');
        if t.is_empty() {
            skip += 1;
        } else if let Some(d) = t.strip_prefix("sep=") {
            sep = d.bytes().next();
            skip += 1;
        } else {
            break;
        }
    }
    let body: String = text.lines().skip(skip).collect::<Vec<_>>().join("\n");
    let first = body.lines().next().unwrap_or("");
    let delimiter = sep.unwrap_or_else(|| delimiter(first));
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .has_headers(false)
        .from_reader(body.as_bytes());
    let mut records = reader.records();
    let header: Vec<String> = match records.next() {
        Some(Ok(r)) => r.iter().map(str::to_string).collect(),
        _ => Vec::new(),
    };
    match (header.len() > 1).then(|| profile_for(&header)).flatten() {
        Some(profile) => read_csv(profile, &header, records, skip, text),
        None => read_text(text),
    }
}

fn read_csv(
    profile: &Profile,
    header: &[String],
    records: csv::StringRecordsIter<'_, &[u8]>,
    skip: usize,
    text: &str,
) -> Read {
    let source: Vec<&str> = text.lines().collect();
    let whole = |line: usize| source.get(line - 1).map_or("", |l| l.trim()).to_string();
    let cols = columns(profile, header);
    // A file with no count of copies holds one a row, but one that counts them
    // in columns this does not read (Delver's EchoMTG preset splits `Reg Qty`
    // from `Foil Qty`) would import every row as one copy, a wrong number
    // given without a word.
    let counts: Vec<&str> = header
        .iter()
        .map(|h| h.trim())
        .filter(|h| {
            let h = h.to_lowercase();
            h.contains("qty") || h.contains("quantity") || h.contains("count")
        })
        .collect();
    if cols.qty.is_none() && !counts.is_empty() {
        return Read {
            source: profile.source,
            rows: Vec::new(),
            unreadable: vec![Unreadable {
                line: skip + 1,
                text: whole(skip + 1),
                reason: format!(
                    "the file counts copies in {}, which this does not read, so it \
                     is left out rather than imported one copy a row",
                    counts.join(", ")
                ),
            }],
            dropped: Vec::new(),
            skipped: Vec::new(),
        };
    }
    let mut rows = Vec::new();
    let mut unreadable = Vec::new();
    let mut dropped: Vec<usize> = vec![0; header.len()];
    let mut lists = 0;

    for record in records {
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                let line = e.position().map_or(0, |p| p.line() as usize) + skip;
                unreadable.push(Unreadable {
                    line,
                    text: whole(line),
                    reason: format!("not a row of the file: {e}"),
                });
                continue;
            }
        };
        let line = record.position().map_or(0, |p| p.line() as usize) + skip;
        if record.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let cell = |i: Option<usize>| {
            i.and_then(|i| record.get(i))
                .map(str::trim)
                .filter(|c| !c.is_empty())
        };
        let refuse = |reason: String| Unreadable {
            line,
            text: whole(line),
            reason,
        };

        // A ManaBox list is a wishlist or a trade list, not cards owned.
        if cell(cols.binder_type).is_some_and(|t| t.eq_ignore_ascii_case("list")) {
            lists += 1;
            continue;
        }
        let qty = match cell(cols.qty) {
            None if cols.qty.is_none() => NonZeroU32::MIN,
            None => {
                unreadable.push(refuse("the row has no quantity".into()));
                continue;
            }
            Some(q) => match q.parse::<u32>().ok().and_then(NonZeroU32::new) {
                Some(q) => q,
                None => {
                    unreadable.push(refuse(format!("{q:?} is no quantity of a card")));
                    continue;
                }
            },
        };
        let finish = match cell(cols.finish) {
            None => Finish::Nonfoil,
            Some(f) => match finish(f) {
                Some(f) => f,
                None => {
                    unreadable.push(refuse(format!(
                        "{f:?} is no finish this reads, so the card is left out \
                         rather than imported in the wrong one"
                    )));
                    continue;
                }
            },
        };
        let name = cell(cols.name).map(str::to_string);
        let id = cell(cols.id).map(str::to_string);
        let printing = match (cell(cols.set), cell(cols.num)) {
            (Some(set), Some(num)) => Printing::parse(&format!("{set}/{num}")),
            _ => None,
        };
        let card = match (id, printing, name) {
            (Some(id), _, name) => Named::Id { id, name },
            (None, Some(printing), Some(name)) => Named::Printing { printing, name },
            (None, None, Some(name)) => Named::Name(name),
            (None, _, None) => {
                unreadable.push(refuse("the row names no card".into()));
                continue;
            }
        };
        for &i in &cols.dropped {
            let Some(c) = record.get(i).map(str::trim).filter(|c| !c.is_empty()) else {
                continue;
            };
            let language = header[i].trim().eq_ignore_ascii_case("language");
            let nothing = matches!(c.to_lowercase().as_str(), "false" | "0" | "0.0" | "0.00");
            if !(language && english(c)) && !nothing {
                dropped[i] += 1;
            }
        }
        rows.push(Row {
            line,
            card,
            qty,
            finish,
            place: cell(cols.place).map(str::to_string),
        });
    }

    let mut skipped = Vec::new();
    if lists > 0 {
        skipped.push(Skipped {
            rows: lists,
            reason: "in a ManaBox list, which holds cards wanted or for trade, not owned".into(),
        });
    }
    Read {
        source: profile.source,
        rows,
        unreadable,
        dropped: dropped
            .into_iter()
            .enumerate()
            .filter(|(_, n)| *n > 0)
            .map(|(i, rows)| Dropped {
                column: header[i].trim().to_string(),
                rows,
            })
            .collect(),
        skipped,
    }
}

fn read_text(text: &str) -> Read {
    let source: Vec<&str> = text.lines().collect();
    let mut rows = Vec::new();
    let mut unreadable = Vec::new();
    let mut categories = 0;
    for (line, entry) in crate::lines(text) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                unreadable.push(Unreadable {
                    line,
                    text: source[line - 1].trim().to_string(),
                    reason: "not a card, which reads like `4 Sol Ring (CMM) 400 *F*`".into(),
                });
                continue;
            }
        };
        if !entry.categories.is_empty() {
            categories += 1;
        }
        let printing = match (&entry.set, &entry.num) {
            (Some(set), Some(num)) => Printing::parse(&format!("{set}/{num}")),
            _ => None,
        };
        rows.push(Row {
            line,
            card: match printing {
                Some(printing) => Named::Printing {
                    printing,
                    name: entry.name,
                },
                None => Named::Name(entry.name),
            },
            qty: entry.qty,
            finish: entry.finish,
            place: None,
        });
    }
    Read {
        source: Source::Text,
        rows,
        unreadable,
        dropped: (categories > 0)
            .then(|| Dropped {
                column: "[categories]".into(),
                rows: categories,
            })
            .into_iter()
            .collect(),
        skipped: Vec::new(),
    }
}

/// What Scryfall should be asked about a row, if anything: by id, or by set
/// and number. A name alone is kept as the name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ask {
    Id(String),
    Printing(Printing),
}

impl Row {
    pub fn ask(&self) -> Option<Ask> {
        match &self.card {
            Named::Id { id, .. } => Some(Ask::Id(id.to_lowercase())),
            Named::Printing { printing, .. } => Some(Ask::Printing(printing.clone())),
            Named::Name(_) => None,
        }
    }
}

/// A printing Scryfall answered with, keyed by what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub printing: Printing,
    pub name: String,
}

/// A row that went in, but not as its file named it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub line: usize,
    pub reason: String,
}

/// Each row as the copies it brings, the printing pinned where Scryfall
/// answered for it and agreed on the name. `found` holds Scryfall's answers,
/// and a question it had no answer to is absent. Rows with no place go to
/// `default_place`.
pub fn resolve(
    read: &Read,
    found: &HashMap<Ask, Found>,
    default_place: Option<&str>,
) -> (Vec<Incoming>, Vec<Note>, Vec<Unreadable>) {
    let mut incoming = Vec::new();
    let mut notes = Vec::new();
    let mut unreadable = Vec::new();
    for row in &read.rows {
        let answer = row.ask().map(|a| found.get(&a));
        let card = match (&row.card, answer) {
            (Named::Name(name), _) => Some((CardRef::Name(name.clone()), None)),
            (_, Some(Some(f))) if row.card.name().is_none_or(|n| same_name(n, &f.name)) => {
                Some((CardRef::Printing(f.printing.clone()), Some(f.name.clone())))
            }
            (Named::Id { id, name }, Some(Some(f))) => {
                let name = name.as_deref().unwrap_or_default();
                notes.push(Note {
                    line: row.line,
                    reason: format!(
                        "Scryfall's {id} is {}, not {name}; kept as {name} by name",
                        f.name
                    ),
                });
                Some((CardRef::Name(name.to_string()), None))
            }
            (Named::Printing { printing, name }, Some(Some(f))) => {
                notes.push(Note {
                    line: row.line,
                    reason: format!(
                        "{printing} on Scryfall is {}, not {name}; kept by name",
                        f.name
                    ),
                });
                Some((CardRef::Name(name.clone()), None))
            }
            (Named::Printing { printing, name }, _) => {
                notes.push(Note {
                    line: row.line,
                    reason: format!("Scryfall has no {printing}; kept by name"),
                });
                Some((CardRef::Name(name.clone()), None))
            }
            (
                Named::Id {
                    id,
                    name: Some(name),
                },
                _,
            ) => {
                notes.push(Note {
                    line: row.line,
                    reason: format!("Scryfall has no card {id}; kept by name"),
                });
                Some((CardRef::Name(name.clone()), None))
            }
            (Named::Id { id, name: None }, _) => {
                unreadable.push(Unreadable {
                    line: row.line,
                    text: id.clone(),
                    reason: "Scryfall has no card by that id, and the row gives no name".into(),
                });
                None
            }
        };
        if let Some((card, name)) = card {
            incoming.push(Incoming {
                card,
                name,
                qty: row.qty,
                finish: row.finish,
                at: row
                    .place
                    .clone()
                    .or_else(|| default_place.map(str::to_string)),
            });
        }
    }
    (incoming, notes, unreadable)
}
