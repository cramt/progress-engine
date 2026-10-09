//! The copy of Scryfall: its Default Cards and Oracle Tags bulk files, kept
//! and answered from locally (ADR-0030, ADR-0031).
//!
//! A [`Builder`] is fed the bulk files a chunk of lines at a time, so neither
//! is ever held whole, and ends with a [`Stored`] whose text is what a
//! platform keeps. A [`ScryfallCopy`] reads that text back and answers card
//! facts without asking Scryfall: a printing by its number, its id or its
//! card's name, every printing of a card, today's prices, search and quick
//! add. Pictures are not card facts: they stay on Scryfall's image CDN, and
//! the copy only knows their URLs.
//!
//! What it keeps is Scryfall's card objects, split in two: what a card says,
//! once for every printing that says it alike, and what each printing says
//! of itself. Joining the two gives back the object Scryfall wrote, every
//! field read here; the tests hold it to that against real records.
//!
//! None of it does I/O or knows a platform. Where the text is kept and how
//! the bulk files arrive are the caller's, and so is which printing of a
//! card to offer first: that is a product's preference (Curator's is
//! `meldweb.toml`'s, ADR-0026), handed in as a [`Ranking`] to each lookup
//! that picks one.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::bulk::{BulkCard, ImageUris, NOT_CARD_LAYOUTS};
use crate::index::{Card, KeywordVocabulary, TagGap, TagVocabulary};
use crate::printing::Printing;
use crate::{parse_printing, CardView, Query};
use facet::Facet;

pub mod store;

/// Which printing of a card is offered first, when a lookup has several to
/// pick from: the least [`Self::Key`] wins, and of printings with equal keys,
/// the one the copy holds first.
///
/// A key rather than an index into the candidates, so no ranking can name a
/// printing that was not offered, or none at all.
pub trait Ranking {
    type Key: Ord;

    /// `printing`'s key, beside its `card` as that printing has it.
    fn key(&self, card: &CardView<'_>, printing: &Printing) -> Self::Key;
}

/// Bumped whenever what [`Stored`] holds changes, so a copy an older build
/// wrote is downloaded again rather than misread.
pub const FORMAT: u32 = 4;

/// Layouts Scryfall's search and autocomplete leave out unless asked for
/// extras: the ones that are not cards, and the oversized cards of casual
/// formats.
const EXTRA_LAYOUTS: [&str; 3] = ["planar", "scheme", "vanguard"];

/// Where Scryfall's pictures live: `{size}/{side}/{a}/{b}/{id}.jpg?{version}`,
/// which every printing in the bulk file follows, so a printing keeps one
/// version rather than its URLs.
const IMAGES: &str = "https://cards.scryfall.io";

/// What a Scryfall card object says beyond the [`BulkCard`] fields, read off
/// the same line by a second parse: facet's `flatten` reads the two at once
/// but eleven times slower, a minute and a half of Default Cards against
/// eleven seconds.
#[derive(Facet)]
struct Raw {
    id: String,
    #[facet(default)]
    set_name: Option<String>,
    #[facet(default)]
    finishes: Vec<String>,
    #[facet(default)]
    prices: RawPrices,
    #[facet(default)]
    image_uris: Option<ImageUris>,
}

/// Scryfall's prices: decimal strings, or null where it has none.
#[derive(Facet, Default)]
struct RawPrices {
    #[facet(default)]
    usd: Option<String>,
    #[facet(default)]
    usd_foil: Option<String>,
    #[facet(default)]
    usd_etched: Option<String>,
    #[facet(default)]
    eur: Option<String>,
    #[facet(default)]
    eur_foil: Option<String>,
    #[facet(default)]
    eur_etched: Option<String>,
}

/// One oracle tag off the Oracle Tags bulk file.
#[derive(Facet)]
struct RawTag {
    id: String,
    slug: String,
    #[facet(default)]
    aliases: Vec<String>,
    #[facet(default)]
    parent_ids: Vec<String>,
    #[facet(default)]
    taggings: Vec<Tagging>,
}

#[derive(Facet)]
struct Tagging {
    oracle_id: String,
}

/// What one copy of one card sells for, by finish.
#[derive(Facet, Debug, Clone, Default, PartialEq)]
pub struct FinishPrices {
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub nonfoil: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub foil: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub etched: Option<f64>,
}

/// A printing's prices, by currency and finish, where Scryfall knows them.
#[derive(Facet, Debug, Clone, Default, PartialEq)]
pub struct CardPrices {
    #[facet(default)]
    pub eur: FinishPrices,
    #[facet(default)]
    pub usd: FinishPrices,
}

impl RawPrices {
    fn read(&self) -> StoredPrices {
        let n = |s: &Option<String>| s.as_deref().and_then(|s| s.parse::<f64>().ok());
        StoredPrices {
            usd: n(&self.usd),
            usd_foil: n(&self.usd_foil),
            usd_etched: n(&self.usd_etched),
            eur: n(&self.eur),
            eur_foil: n(&self.eur_foil),
            eur_etched: n(&self.eur_etched),
        }
    }
}

/// The copy as a platform keeps it, written by [`Stored::to_text`].
pub struct Stored {
    pub format: u32,
    /// When Scryfall wrote the Default Cards file this was made from.
    pub updated_at: String,
    pub cards: Vec<StoredCard>,
    pub printings: Vec<StoredPrinting>,
    pub sets: Vec<StoredSet>,
    pub looks: Vec<Look>,
    /// Every oracle tag, by the index [`StoredCard::tags`] names it by.
    pub tags: Vec<String>,
    /// The other names a tag goes by, each beside its tag's index.
    pub aliases: Vec<Alias>,
    /// Every format a card's legalities name, in the order
    /// [`StoredCard::legal`] spells them.
    pub formats: Vec<String>,
}

/// What a card says: a Scryfall card object without any printing's facts.
#[derive(Facet)]
pub struct StoredCard {
    /// Its card object, with legalities only where [`Self::legal`] cannot
    /// spell them.
    pub facts: BulkCard,
    /// Its legality in each of [`Stored::formats`], a letter each (see
    /// [`LEGALITIES`]). Twenty-three formats as a map are half of what a card
    /// is kept as, and two thirds of the time it takes to read back.
    #[facet(default, skip_serializing_if = String::is_empty)]
    pub legal: String,
    /// The oracle tags it is in, a tag's ancestors included, as Scryfall's
    /// `otag:` counts them.
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub tags: Vec<u32>,
}

/// What one printing says of itself, beside what it shares with others:
/// what a thousand printings say alike is said once, in [`Stored::sets`] and
/// [`Stored::looks`], and a printing names it.
#[derive(Debug, Clone)]
pub struct StoredPrinting {
    pub id: String,
    /// Which of [`Stored::cards`] it is a printing of.
    pub card: u32,
    /// Which of [`Stored::sets`] it is in.
    pub set: u32,
    /// Which of [`Stored::looks`] it has.
    pub look: u32,
    /// The fields of its card object no other printing shares.
    pub collector_number: Option<String>,
    pub released_at: Option<String>,
    pub flavor_name: Option<String>,
    pub prices: StoredPrices,
    /// The version its pictures' URLs end in; absent for a printing Scryfall
    /// has no picture of.
    pub image: Option<String>,
    /// Whether its other face has a picture of its own.
    pub back_image: bool,
}

/// A set, as its printings' card objects state it.
#[derive(Facet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoredSet {
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub code: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub name: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub set_type: Option<String>,
}

/// How a printing looks and what it was printed as: its language, rarity,
/// frame, border, flags, games and finishes, which thousands of printings
/// share.
#[derive(Facet, Debug, Clone, PartialEq)]
pub struct Look {
    /// Those fields of its card object, the others absent.
    pub own: BulkCard,
    #[facet(default, skip_serializing_if = Vec::is_empty)]
    pub finishes: Vec<String>,
}

/// [`CardPrices`], flat, which is fewer values to read back.
#[derive(Facet, Debug, Clone, Default, PartialEq)]
pub struct StoredPrices {
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub usd: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub usd_foil: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub usd_etched: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub eur: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub eur_foil: Option<f64>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub eur_etched: Option<f64>,
}

impl StoredPrices {
    fn read(&self) -> CardPrices {
        CardPrices {
            eur: FinishPrices {
                nonfoil: self.eur,
                foil: self.eur_foil,
                etched: self.eur_etched,
            },
            usd: FinishPrices {
                nonfoil: self.usd,
                foil: self.usd_foil,
                etched: self.usd_etched,
            },
        }
    }
}

#[derive(Facet)]
pub struct Alias {
    pub alias: String,
    pub tag: u32,
}

/// What [`Stored::to_text`] writes on its first line: everything but the
/// cards and printings, which follow it a line each.
#[derive(Facet)]
struct Head {
    format: u32,
    updated_at: String,
    sets: Vec<StoredSet>,
    looks: Vec<Look>,
    tags: Vec<String>,
    aliases: Vec<Alias>,
    formats: Vec<String>,
    cards: u32,
    printings: u32,
}

/// A field of a card's or a printing's line, which a tab or a line break
/// would cut in two. Scryfall writes neither in any field kept this way.
fn plain<'a>(field: &'a str, what: &str) -> Result<&'a str, String> {
    if field.contains(['\t', '\n', '\r']) {
        return Err(format!("{what} holds a tab or a line break: {field:?}"));
    }
    Ok(field)
}

impl Stored {
    /// The copy as the text a platform keeps: [`Head`] as JSON, then a line
    /// per card and a line per printing, their fields between tabs.
    ///
    /// The browser reads this back on every page load, and reading JSON costs
    /// a few microseconds a value through facet in wasm, a minute for the whole
    /// copy; a line of tab-separated fields is split by hand instead, and a
    /// card's own JSON is left unread until something asks for that card.
    pub fn to_text(&self) -> Result<String, String> {
        use std::fmt::Write;
        let head = Head {
            format: self.format,
            updated_at: self.updated_at.clone(),
            sets: self.sets.clone(),
            looks: self.looks.clone(),
            tags: self.tags.clone(),
            aliases: self
                .aliases
                .iter()
                .map(|a| Alias {
                    alias: a.alias.clone(),
                    tag: a.tag,
                })
                .collect(),
            formats: self.formats.clone(),
            cards: self.cards.len() as u32,
            printings: self.printings.len() as u32,
        };
        let mut out = facet_json::to_string(&head).map_err(|e| e.to_string())?;
        out.push('\n');
        for card in &self.cards {
            let facts = facet_json::to_string(&card.facts).map_err(|e| e.to_string())?;
            let tags: Vec<String> = card.tags.iter().map(u32::to_string).collect();
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                plain(&card.facts.name, "a card's name")?,
                plain(oracle_id(&card.facts).unwrap_or_default(), "an oracle id")?,
                plain(card.facts.layout.as_deref().unwrap_or_default(), "a layout")?,
                card.legal,
                tags.join(","),
                // JSON escapes every control character inside a string.
                facts,
            )
            .expect("a string takes any write");
        }
        fn opt<'a>(o: &'a Option<String>, what: &str) -> Result<&'a str, String> {
            // An empty field reads back as `None`; no printing in the bulk
            // file has an empty one, and the full round trip would say so.
            debug_assert!(o.as_deref() != Some(""), "{what} is empty, not absent");
            plain(o.as_deref().unwrap_or_default(), what)
        }
        let price = |p: Option<f64>| p.map(|p| p.to_string()).unwrap_or_default();
        for p in &self.printings {
            let StoredPrices {
                usd,
                usd_foil,
                usd_etched,
                eur,
                eur_foil,
                eur_etched,
            } = &p.prices;
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                plain(&p.id, "a printing's id")?,
                p.card,
                p.set,
                p.look,
                opt(&p.collector_number, "a collector number")?,
                opt(&p.released_at, "a release date")?,
                opt(&p.flavor_name, "a flavour name")?,
                opt(&p.image, "a picture's version")?,
                if p.back_image { "1" } else { "" },
                price(*usd),
                price(*usd_foil),
                price(*usd_etched),
                price(*eur),
                price(*eur_foil),
                price(*eur_etched),
            )
            .expect("a string takes any write");
        }
        Ok(out)
    }
}

/// A card as [`Stored::to_text`] wrote it, its facts not yet read.
struct KeptCard {
    name: String,
    oracle_id: Option<String>,
    /// Whether search and quick add leave it out unless asked for extras.
    extra: bool,
    legal: Box<str>,
    tags: Vec<u32>,
    facts: Box<str>,
}

fn read_card(line: &str) -> Option<KeptCard> {
    let mut f = line.splitn(6, '\t');
    let name = f.next()?.to_string();
    let oracle = f.next()?;
    let layout = f.next()?;
    let legal = f.next()?.into();
    let tags = f.next()?;
    let tags = if tags.is_empty() {
        Vec::new()
    } else {
        tags.split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?
    };
    Some(KeptCard {
        name,
        oracle_id: (!oracle.is_empty()).then(|| oracle.to_string()),
        extra: is_extra(layout),
        legal,
        tags,
        facts: f.next()?.into(),
    })
}

fn read_printing(line: &str) -> Option<StoredPrinting> {
    let mut f = line.split('\t');
    let mut next = || f.next();
    let text = |s: &str| (!s.is_empty()).then(|| s.to_string());
    let id = next()?.to_string();
    let card = next()?.parse().ok()?;
    let set = next()?.parse().ok()?;
    let look = next()?.parse().ok()?;
    let collector_number = text(next()?);
    let released_at = text(next()?);
    let flavor_name = text(next()?);
    let image = text(next()?);
    let back_image = next()? == "1";
    let mut price = || -> Option<Option<f64>> {
        let p = next()?;
        if p.is_empty() {
            Some(None)
        } else {
            p.parse().ok().map(Some)
        }
    };
    let prices = StoredPrices {
        usd: price()?,
        usd_foil: price()?,
        usd_etched: price()?,
        eur: price()?,
        eur_foil: price()?,
        eur_etched: price()?,
    };
    Some(StoredPrinting {
        id,
        card,
        set,
        look,
        collector_number,
        released_at,
        flavor_name,
        prices,
        image,
        back_image,
    })
}

/// Moves each named field from one card object to another.
macro_rules! move_fields {
    ($from:expr => $to:expr; $($field:ident),*) => {
        $( $to.$field = $from.$field.take(); )*
    };
}

/// Copies each named field from one card object to another.
macro_rules! copy_fields {
    ($from:expr => $to:expr; $($field:ident),*) => {
        $( $to.$field = $from.$field.clone(); )*
    };
}

/// A card object's printing facts, which [`split`] and [`join`] agree on by
/// both naming this list.
macro_rules! printing_fields {
    ($m:ident, $from:expr => $to:expr) => {
        $m!($from => $to; lang, rarity, set, set_type, collector_number, released_at,
            frame, frame_effects, border_color, full_art, textless, digital, promo,
            reprint, oversized, promo_types, games, flavor_name)
    };
}

/// A card object as the card's facts and the printing's own. A face's
/// pictures belong to the printing, and are kept as [`StoredPrinting`]'s
/// version instead.
fn split(mut card: BulkCard) -> (BulkCard, BulkCard) {
    let mut own = BulkCard::default();
    printing_fields!(move_fields, card => own);
    for face in card.card_faces.iter_mut().flatten() {
        face.image_uris = None;
    }
    (card, own)
}

/// The card object [`split`] and [`Builder`] took apart, pictures aside.
pub fn join(card: &BulkCard, set: &StoredSet, look: &Look, own: &StoredPrinting) -> BulkCard {
    let mut out = card.clone();
    printing_fields!(copy_fields, look.own => out);
    out.set = set.code.clone();
    out.set_type = set.set_type.clone();
    out.collector_number = own.collector_number.clone();
    out.released_at = own.released_at.clone();
    out.flavor_name = own.flavor_name.clone();
    out
}

/// The oracle id a card object states, at the top or, for a reversible
/// card, on its faces.
fn oracle_id(card: &BulkCard) -> Option<&str> {
    card.oracle_id.as_deref().or_else(|| {
        card.card_faces
            .iter()
            .flatten()
            .find_map(|f| f.oracle_id.as_deref())
    })
}

/// The version a picture's URL ends in.
fn image_version(url: &str) -> Option<&str> {
    url.rsplit_once('?').map(|(_, v)| v)
}

/// Reads Scryfall's bulk files into a [`Stored`], a chunk of lines at a time,
/// so neither file is ever held whole.
#[derive(Default)]
pub struct Builder {
    updated_at: String,
    cards: Vec<BulkCard>,
    /// The cards stating each oracle id, to find one a printing says alike.
    by_oracle: HashMap<String, Vec<u32>>,
    printings: Vec<StoredPrinting>,
    sets: Vec<StoredSet>,
    set_at: HashMap<StoredSet, u32>,
    looks: Vec<Look>,
    /// Each look by its JSON, which is what tells two apart.
    look_at: HashMap<String, u32>,
    tags: Vec<RawTag>,
    /// Lines that were not a card object or a tag, by the first's error.
    unreadable: Vec<String>,
}

impl Builder {
    pub fn new(updated_at: &str) -> Self {
        Builder {
            updated_at: updated_at.to_string(),
            ..Builder::default()
        }
    }

    /// Default Cards' lines, whole ones only.
    pub fn cards(&mut self, lines: &str) {
        for line in lines.lines().filter(|l| !l.trim().is_empty()) {
            let read = facet_json::from_str::<BulkCard>(line)
                .and_then(|card| Ok((card, facet_json::from_str::<Raw>(line)?)));
            match read {
                Ok((card, raw)) => self.card(card, raw),
                Err(e) => self.unreadable.push(e.to_string()),
            }
        }
    }

    fn card(&mut self, card: BulkCard, raw: Raw) {
        let Raw {
            id,
            set_name,
            finishes,
            prices,
            image_uris,
        } = raw;
        let faces = card.card_faces.as_deref().unwrap_or_default();
        let front = image_uris
            .as_ref()
            .or_else(|| faces.first().and_then(|f| f.image_uris.as_ref()))
            .and_then(|i| i.normal.as_deref());
        let image = front.and_then(image_version).map(str::to_string);
        let back_image = faces
            .get(1)
            .and_then(|f| f.image_uris.as_ref())
            .is_some_and(|i| i.normal.is_some());
        let (facts, mut look) = split(card);
        let set = StoredSet {
            code: look.set.take(),
            name: set_name,
            set_type: look.set_type.take(),
        };
        let set = *self.set_at.entry(set).or_insert_with_key(|set| {
            self.sets.push(set.clone());
            self.sets.len() as u32 - 1
        });
        let (collector_number, released_at, flavor_name) = (
            look.collector_number.take(),
            look.released_at.take(),
            look.flavor_name.take(),
        );
        let look = Look {
            own: look,
            finishes,
        };
        let key = facet_json::to_string(&look).expect("a look serialises");
        let look = *self.look_at.entry(key).or_insert_with(|| {
            self.looks.push(look);
            self.looks.len() as u32 - 1
        });
        let key = oracle_id(&facts).unwrap_or(&id).to_string();
        let same = self.by_oracle.get(&key).and_then(|cards| {
            cards
                .iter()
                .copied()
                .find(|&i| self.cards[i as usize] == facts)
        });
        let card = same.unwrap_or_else(|| {
            let i = self.cards.len() as u32;
            self.cards.push(facts);
            self.by_oracle.entry(key).or_default().push(i);
            i
        });
        self.printings.push(StoredPrinting {
            id,
            card,
            set,
            look,
            collector_number,
            released_at,
            flavor_name,
            prices: prices.read(),
            image,
            back_image,
        });
    }

    /// Oracle Tags' lines, whole ones only.
    pub fn tags(&mut self, lines: &str) {
        for line in lines.lines().filter(|l| !l.trim().is_empty()) {
            match facet_json::from_str::<RawTag>(line) {
                Ok(tag) => self.tags.push(tag),
                Err(e) => self.unreadable.push(e.to_string()),
            }
        }
    }

    pub fn unreadable(&self) -> &[String] {
        &self.unreadable
    }

    pub fn finish(self) -> Stored {
        let Builder {
            updated_at,
            cards,
            by_oracle,
            printings,
            sets,
            looks,
            tags,
            ..
        } = self;
        let at: HashMap<&str, u32> = tags
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.as_str(), i as u32))
            .collect();
        // Scryfall's `otag:removal` finds the cards tagged with any tag below
        // it, so a card is in a tag's ancestors too.
        let mut lineage: Vec<Option<BTreeSet<u32>>> = vec![None; tags.len()];
        fn ancestors(
            i: u32,
            tags: &[RawTag],
            at: &HashMap<&str, u32>,
            lineage: &mut Vec<Option<BTreeSet<u32>>>,
            seen: &mut HashSet<u32>,
        ) -> BTreeSet<u32> {
            if let Some(known) = &lineage[i as usize] {
                return known.clone();
            }
            let mut out = BTreeSet::from([i]);
            // Tagger's graph has no cycles today; this keeps one from hanging
            // the build if it ever does.
            if seen.insert(i) {
                for parent in &tags[i as usize].parent_ids {
                    if let Some(&p) = at.get(parent.as_str()) {
                        out.extend(ancestors(p, tags, at, lineage, seen));
                    }
                }
            }
            lineage[i as usize] = Some(out.clone());
            out
        }
        let mut of_oracle: HashMap<&str, BTreeSet<u32>> = HashMap::new();
        for (i, tag) in tags.iter().enumerate() {
            let line = ancestors(i as u32, &tags, &at, &mut lineage, &mut HashSet::new());
            for t in &tag.taggings {
                if by_oracle.contains_key(&t.oracle_id) {
                    of_oracle
                        .entry(t.oracle_id.as_str())
                        .or_default()
                        .extend(&line);
                }
            }
        }
        let cards = cards
            .into_iter()
            .map(|facts| {
                let tags = oracle_id(&facts)
                    .and_then(|o| of_oracle.get(o))
                    .map(|t| t.iter().copied().collect())
                    .unwrap_or_default();
                StoredCard {
                    facts,
                    legal: String::new(),
                    tags,
                }
            })
            .collect::<Vec<_>>();
        let formats: Vec<String> = cards
            .iter()
            .flat_map(|c| c.facts.legalities.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let cards = cards
            .into_iter()
            .map(|mut c| {
                if let Some(legal) = spell_legalities(&c.facts.legalities, &formats) {
                    c.legal = legal;
                    c.facts.legalities.clear();
                }
                c
            })
            .collect();
        let aliases = tags
            .iter()
            .enumerate()
            .flat_map(|(i, t)| {
                t.aliases.iter().map(move |a| Alias {
                    alias: a.clone(),
                    tag: i as u32,
                })
            })
            .collect();
        Stored {
            format: FORMAT,
            updated_at,
            cards,
            printings,
            sets,
            looks,
            tags: tags.into_iter().map(|t| t.slug).collect(),
            aliases,
            formats,
        }
    }
}

/// How a card's picture is turned to be read.
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Turn {
    Upright,
    Sideways,
    UpsideDown,
}

/// A printing's other face, as it is shown.
#[derive(Facet, Debug, Clone, PartialEq)]
pub struct Face {
    pub image: String,
    pub turn: Turn,
}

/// One printing, with what any view of it asks.
#[derive(Facet, Debug, Clone, PartialEq)]
#[facet(rename_all = "camelCase")]
pub struct Found {
    /// Scryfall's id for the printing.
    pub id: String,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub oracle_id: Option<String>,
    pub name: String,
    /// Lowercased, as the deck names it.
    pub set: String,
    pub num: String,
    pub set_name: String,
    /// `YYYY-MM-DD`.
    pub released: String,
    /// The front's `normal` picture.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub image: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub small: Option<String>,
    pub color_identity: Vec<String>,
    /// The whole card's type line, both faces' where it has two.
    pub type_line: String,
    /// The front face's alone: `Battle — Siege` for an Invasion.
    pub front_type_line: String,
    pub mana_cost: String,
    /// Scryfall's search for every printing of the card, which
    /// [`ScryfallCopy::prints`] also answers.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub prints: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub turn: Option<Turn>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub back: Option<Face>,
    pub finishes: Vec<String>,
    pub prices: CardPrices,
}

/// A printing beside its card object, which a printing preference reads.
#[derive(Facet, Debug)]
pub struct PrintingFacts {
    pub printing: Found,
    pub facts: BulkCard,
}

/// A card asked for.
#[derive(Facet, Debug)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Wanted {
    /// `set/num`.
    Printing { set: String, num: String },
    /// A card by its name, or its front face's; answered with the printing
    /// the caller's [`Ranking`] puts first.
    Name { name: String },
    /// Scryfall's id for a printing.
    Id { id: String },
    /// A card by name in one set, answered with the printing ranked first
    /// of those it has there.
    InSet { name: String, set: String },
}

#[derive(Facet, Debug)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum SearchAnswer {
    /// `cards` are one page, from `offset`, of `total` cards.
    Page { cards: Vec<Found>, total: u32 },
    /// The query is not one the copy can answer, and this says why.
    Refused { message: String },
}

/// The copy, read and ready to answer.
///
/// Loading reads every printing but no card's facts: a card's JSON is read
/// the first time something asks for that card, and [`Self::warm`] reads the
/// rest a slice at a time once the caller has its answers. A search reads
/// every card it has not yet, which before warming is done takes seconds, so
/// Curator's worker sends searches to the API until then.
pub struct ScryfallCopy {
    updated_at: String,
    kept: Vec<KeptCard>,
    facts: Vec<OnceCell<BulkCard>>,
    cards: Vec<OnceCell<Card>>,
    printings: Vec<StoredPrinting>,
    own: Vec<OnceCell<Printing>>,
    sets: Vec<StoredSet>,
    looks: Vec<Look>,
    formats: Vec<String>,
    tag_names: Vec<String>,
    by_id: HashMap<String, u32>,
    by_number: HashMap<String, u32>,
    /// Real cards' printings by every name that names them, lowercased.
    by_name: HashMap<String, Vec<u32>>,
    /// Extras' the same, asked only for a name no real card has.
    extras_by_name: HashMap<String, Vec<u32>>,
    by_oracle: HashMap<String, Vec<u32>>,
    /// Every name search and quick add can answer, lowercased beside the name.
    names: Vec<(String, String)>,
    keywords: OnceCell<KeywordVocabulary>,
    tags: TagVocabulary,
    aliases: HashMap<String, String>,
    /// How many cards [`Self::warm`] has read.
    warmed: Cell<usize>,
    /// The last search, by query, its cards' printings grouped and sorted,
    /// so a later page costs nothing.
    last: RefCell<Option<(String, Vec<Vec<u32>>)>>,
}

impl ScryfallCopy {
    /// Reads back what [`Stored::to_text`] wrote.
    pub fn load(text: &str) -> Result<Self, String> {
        let mut lines = text.split('\n');
        let head = lines.next().unwrap_or_default();
        // The format is checked before the rest is read, since a copy an
        // older build wrote need not parse as this one's head.
        #[derive(Facet)]
        struct Format {
            format: u32,
        }
        let Format { format } = facet_json::from_str::<Format>(head)
            .map_err(|e| format!("the stored copy is unreadable: {e}"))?;
        if format != FORMAT {
            return Err(format!(
                "the stored copy is format {format}, this build reads {FORMAT}"
            ));
        }
        let Head {
            updated_at,
            sets,
            looks,
            tags,
            aliases,
            formats,
            cards,
            printings,
            ..
        } = facet_json::from_str(head)
            .map_err(|e| format!("the stored copy is unreadable: {e}"))?;
        let unreadable =
            |what: &str, i: usize| format!("the stored copy's {what} {i} is unreadable");
        let kept = (0..cards as usize)
            .map(|i| {
                lines
                    .next()
                    .and_then(read_card)
                    .ok_or_else(|| unreadable("card", i))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let printings = (0..printings as usize)
            .map(|i| {
                lines
                    .next()
                    .and_then(read_printing)
                    .filter(|p| {
                        (p.card as usize) < kept.len()
                            && (p.set as usize) < sets.len()
                            && (p.look as usize) < looks.len()
                    })
                    .ok_or_else(|| unreadable("printing", i))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut by_id = HashMap::with_capacity(printings.len());
        let mut by_number = HashMap::with_capacity(printings.len());
        let mut by_name: HashMap<String, Vec<u32>> = HashMap::new();
        let mut extras_by_name: HashMap<String, Vec<u32>> = HashMap::new();
        let mut by_oracle: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, p) in printings.iter().enumerate() {
            let i = i as u32;
            let card = &kept[p.card as usize];
            by_id.insert(p.id.to_lowercase(), i);
            let set = sets[p.set as usize].code.as_deref().unwrap_or_default();
            let num = p.collector_number.as_deref().unwrap_or_default();
            by_number.insert(number_key(set, num), i);
            let into = if card.extra {
                &mut extras_by_name
            } else {
                &mut by_name
            };
            for key in name_keys(&card.name) {
                let list = into.entry(key).or_default();
                if list.last() != Some(&i) {
                    list.push(i);
                }
            }
            if let Some(o) = &card.oracle_id {
                by_oracle.entry(o.clone()).or_default().push(i);
            }
        }
        let mut names: Vec<(String, String)> = kept
            .iter()
            .filter(|c| !c.extra)
            .map(|c| {
                let name = typed_name(&c.name);
                (name.to_lowercase(), name.to_string())
            })
            .collect::<HashMap<_, _>>()
            .into_iter()
            .collect();
        names.sort();
        let aliases = aliases
            .into_iter()
            .filter_map(|a| Some((a.alias.to_lowercase(), tags.get(a.tag as usize)?.clone())))
            .collect();
        Ok(ScryfallCopy {
            updated_at,
            facts: (0..kept.len()).map(|_| OnceCell::new()).collect(),
            cards: (0..kept.len()).map(|_| OnceCell::new()).collect(),
            own: (0..printings.len()).map(|_| OnceCell::new()).collect(),
            kept,
            printings,
            sets,
            looks,
            formats,
            tags: TagVocabulary::new(tags.clone()),
            tag_names: tags,
            by_id,
            by_number,
            by_name,
            extras_by_name,
            by_oracle,
            names,
            keywords: OnceCell::new(),
            aliases,
            warmed: Cell::new(0),
            last: RefCell::new(None),
        })
    }

    /// Card `c`'s card object, read from its JSON the first time asked. The
    /// JSON is what this build's [`Stored::to_text`] wrote, its format checked
    /// at load, so failing to read it is a bug rather than a bad copy.
    fn card_facts(&self, c: u32) -> &BulkCard {
        self.facts[c as usize].get_or_init(|| {
            let kept = &self.kept[c as usize];
            let mut facts: BulkCard =
                facet_json::from_str(&kept.facts).expect("the copy reads the cards it wrote");
            if !kept.legal.is_empty() {
                facts.legalities = read_legalities(&kept.legal, &self.formats)
                    .expect("the copy reads the legalities it wrote");
            }
            facts
        })
    }

    /// Card `c` as search reads it.
    fn card(&self, c: u32) -> &Card {
        self.cards[c as usize].get_or_init(|| {
            let mut card = self.card_facts(c).card_of_any_printing(&mut Vec::new());
            card.tags = self.kept[c as usize]
                .tags
                .iter()
                .filter_map(|&i| self.tag_names.get(i as usize).cloned())
                .collect();
            card
        })
    }

    /// Printing `i`'s own facts, as search and ranking read them.
    fn own(&self, i: u32) -> &Printing {
        self.own[i as usize].get_or_init(|| {
            let p = &self.printings[i as usize];
            join(
                &BulkCard::default(),
                &self.sets[p.set as usize],
                &self.looks[p.look as usize],
                p,
            )
            .printing()
        })
    }

    fn keywords(&self) -> &KeywordVocabulary {
        self.keywords.get_or_init(|| {
            KeywordVocabulary::new(
                (0..self.kept.len() as u32).flat_map(|c| self.card(c).keywords.clone()),
            )
        })
    }

    /// Reads up to `n` more cards or printings as search reads them, cards
    /// first; whether all are read.
    pub fn warm(&self, n: usize) -> bool {
        let (cards, all) = (self.kept.len(), self.kept.len() + self.printings.len());
        let from = self.warmed.get();
        let to = (from + n).min(all);
        for k in from..to {
            if k < cards {
                self.card(k as u32);
            } else {
                self.own((k - cards) as u32);
            }
        }
        self.warmed.set(to);
        if to < all {
            return false;
        }
        self.keywords();
        true
    }

    /// How many cards it holds: a card object less its printings, so a card
    /// whose printings disagree on its legality is more than one.
    pub fn card_count(&self) -> usize {
        self.kept.len()
    }

    pub fn printing_count(&self) -> usize {
        self.printings.len()
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    /// Printing `i`'s card, read as that printing has it.
    fn view(&self, i: u32) -> CardView<'_> {
        let p = &self.printings[i as usize];
        let mut view = self.card(p.card).view(&[]);
        view.set = self.sets[p.set as usize]
            .code
            .as_deref()
            .unwrap_or_default();
        view.rarity = self.looks[p.look as usize]
            .own
            .rarity
            .as_deref()
            .unwrap_or_default();
        view
    }

    /// The printing of `among` that `ranking` puts first.
    fn best(&self, among: &[u32], ranking: &impl Ranking) -> Option<u32> {
        if among.len() < 2 {
            return among.first().copied();
        }
        among
            .iter()
            .copied()
            .min_by_key(|&i| ranking.key(&self.view(i), self.own(i)))
    }

    fn named(&self, name: &str) -> &[u32] {
        let key = name.trim().to_lowercase();
        self.by_name
            .get(&key)
            .or_else(|| self.extras_by_name.get(&key))
            .map_or(&[], Vec::as_slice)
    }

    /// The printing `wanted` names; for a card named without its printing,
    /// the one `ranking` puts first.
    pub fn find(&self, wanted: &Wanted, ranking: &impl Ranking) -> Option<Found> {
        let i = match wanted {
            Wanted::Printing { set, num } => self.by_number.get(&number_key(set, num)).copied(),
            Wanted::Id { id } => self.by_id.get(&id.trim().to_lowercase()).copied(),
            Wanted::Name { name } => self.best(self.named(name), ranking),
            Wanted::InSet { name, set } => {
                let there: Vec<u32> = self
                    .named(name)
                    .iter()
                    .copied()
                    .filter(|&i| {
                        self.sets[self.printings[i as usize].set as usize]
                            .code
                            .as_deref()
                            .is_some_and(|s| s.eq_ignore_ascii_case(set.trim()))
                    })
                    .collect();
                self.best(&there, ranking)
            }
        }?;
        Some(self.found(i))
    }

    /// Printing `i`, as a view of it shows it.
    pub fn found(&self, i: u32) -> Found {
        let p = &self.printings[i as usize];
        let card = self.card_facts(p.card);
        let id = &p.id;
        let picture = |size: &str, side: &str| {
            p.image.as_ref().map(|v| {
                let (a, b) = (&id[..1], &id[1..2]);
                format!("{IMAGES}/{size}/{side}/{a}/{b}/{id}.jpg?{v}")
            })
        };
        let image = picture("normal", "front");
        let faces = card.card_faces.as_deref().unwrap_or_default();
        let front = faces.first();
        let front_type = front
            .and_then(|f| f.type_line.as_deref())
            .or(card.type_line.as_deref())
            .unwrap_or_default();
        let layout = card.layout.as_deref().unwrap_or_default();
        let keywords = card.keywords.as_deref().unwrap_or_default();
        // A battle's front and a split card are read sideways, bar
        // Aftermath, whose top half reads upright.
        let sideways = front_type.starts_with("Battle")
            || (layout == "split" && !keywords.iter().any(|k| k == "Aftermath"));
        let back = if layout == "flip" {
            image.clone().map(|image| Face {
                image,
                turn: Turn::UpsideDown,
            })
        } else if p.back_image {
            picture("normal", "back").map(|image| Face {
                image,
                turn: Turn::Upright,
            })
        } else {
            None
        };
        let type_line = card
            .type_line
            .as_deref()
            .or_else(|| front.and_then(|f| f.type_line.as_deref()))
            .unwrap_or_default();
        Found {
            id: id.clone(),
            oracle_id: self.kept[p.card as usize].oracle_id.clone(),
            name: card.name.clone(),
            set: self.sets[p.set as usize]
                .code
                .as_deref()
                .unwrap_or_default()
                .to_lowercase(),
            num: p.collector_number.clone().unwrap_or_default(),
            set_name: self.sets[p.set as usize].name.clone().unwrap_or_default(),
            released: p.released_at.clone().unwrap_or_default(),
            small: picture("small", "front"),
            image,
            color_identity: card.color_identity.clone().unwrap_or_default(),
            type_line: type_line.to_string(),
            front_type_line: type_line
                .split(" // ")
                .next()
                .unwrap_or_default()
                .to_string(),
            mana_cost: card
                .mana_cost
                .as_deref()
                .or_else(|| front.and_then(|f| f.mana_cost.as_deref()))
                .unwrap_or_default()
                .to_string(),
            prints: self.kept[p.card as usize]
                .oracle_id
                .as_deref()
                .map(prints_uri),
            turn: sideways.then_some(Turn::Sideways),
            back,
            finishes: self.looks[p.look as usize].finishes.clone(),
            prices: p.prices.read(),
        }
    }

    /// Printing `i` as the card object Scryfall wrote, pictures aside.
    pub fn facts(&self, i: u32) -> BulkCard {
        let p = &self.printings[i as usize];
        join(
            self.card_facts(p.card),
            &self.sets[p.set as usize],
            &self.looks[p.look as usize],
            p,
        )
    }

    /// Every printing behind a search for a card's printings: Scryfall's
    /// `prints_search_uri`, by oracle id, or Meldweb Curator's own by exact name.
    /// Newest first, as Scryfall lists them.
    pub fn prints(&self, uri: &str) -> Vec<PrintingFacts> {
        let q = query_param(uri).unwrap_or_default();
        let mut found: Vec<u32> = if let Some(id) = q.strip_prefix("oracleid:") {
            self.by_oracle.get(id.trim()).cloned().unwrap_or_default()
        } else if let Some(name) = q.strip_prefix("!\"") {
            let name = name.split('"').next().unwrap_or_default();
            let mut all = self.named(name).to_vec();
            // A name alone also finds a reversible printing of the card,
            // which states the card under its own oracle id.
            let oracles: HashSet<&str> = all
                .iter()
                .filter_map(|&i| {
                    self.kept[self.printings[i as usize].card as usize]
                        .oracle_id
                        .as_deref()
                })
                .collect();
            for o in oracles {
                all.extend(self.by_oracle.get(o).into_iter().flatten());
            }
            all.sort_unstable();
            all.dedup();
            all
        } else {
            Vec::new()
        };
        let released = |i: u32| &self.printings[i as usize].released_at;
        found.sort_by(|&a, &b| released(b).cmp(released(a)));
        found
            .into_iter()
            .map(|i| PrintingFacts {
                printing: self.found(i),
                facts: self.facts(i),
            })
            .collect()
    }

    /// Cards matching `query`, one printing each, the one `ranking` puts
    /// first, sorted by name: what Scryfall's `/cards/search` answers, by
    /// this crate's reading of the syntax, which refuses what it does not
    /// know rather than dropping it.
    pub fn search(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
        ranking: &impl Ranking,
    ) -> SearchAnswer {
        let groups = {
            let last = self.last.borrow();
            match &*last {
                Some((q, groups)) if q == query => Ok(groups.clone()),
                _ => {
                    drop(last);
                    self.matching(query)
                }
            }
        };
        let groups = match groups {
            Ok(g) => g,
            Err(message) => return SearchAnswer::Refused { message },
        };
        let cards = groups
            .iter()
            .skip(offset)
            .take(limit)
            .filter_map(|g| self.best(g, ranking))
            .map(|i| self.found(i))
            .collect();
        let total = groups.len() as u32;
        *self.last.borrow_mut() = Some((query.to_string(), groups));
        SearchAnswer::Page { cards, total }
    }

    fn matching(&self, query: &str) -> Result<Vec<Vec<u32>>, String> {
        let mut q = parse_printing(query).map_err(|e| e.to_string())?;
        self.resolve_aliases(&mut q);
        if let Some(k) = q.unknown_keywords(self.keywords()).first() {
            return Err(format!("Unknown keyword \u{201c}{k}\u{201d}"));
        }
        if let Some(gap) = q.tag_gap(&self.tags) {
            let named = match &gap {
                TagGap::NotCarried(t) | TagGap::NoneFetched(t) => t.join(", "),
            };
            return Err(format!("Scryfall has no oracle tag named {named}"));
        }
        let per_printing = reads_printing(&q);
        let mut groups: HashMap<&str, Vec<u32>> = HashMap::new();
        let mut card_matches: Vec<Option<bool>> = vec![None; self.cards.len()];
        for (i, p) in self.printings.iter().enumerate() {
            let i = i as u32;
            let card = &self.kept[p.card as usize];
            if card.extra {
                continue;
            }
            let hit = if per_printing {
                q.matches_printing(&self.view(i), self.own(i))
            } else {
                *card_matches[p.card as usize]
                    .get_or_insert_with(|| q.matches_printing(&self.view(i), self.own(i)))
            };
            if hit {
                let key = card.oracle_id.as_deref().unwrap_or(&p.id);
                groups.entry(key).or_default().push(i);
            }
        }
        let mut groups: Vec<Vec<u32>> = groups.into_values().collect();
        let name = |g: &Vec<u32>| &self.kept[self.printings[g[0] as usize].card as usize].name;
        groups.sort_by_cached_key(|g| name(g).to_lowercase());
        Ok(groups)
    }

    /// Each `otag:` term written as one of a tag's other names, rewritten
    /// to the tag's own.
    fn resolve_aliases(&self, q: &mut Query) {
        match q {
            Query::Tag(t) => {
                if let Some(slug) = self.aliases.get(&t.to_lowercase()) {
                    *t = slug.clone();
                }
            }
            Query::Not(inner) => self.resolve_aliases(inner),
            Query::And(parts) | Query::Or(parts) => {
                for part in parts {
                    self.resolve_aliases(part);
                }
            }
            _ => {}
        }
    }

    /// Up to twenty card names `query` could be the start of, nearest first:
    /// a name it begins, then one with a word it begins, then one holding it.
    pub fn autocomplete(&self, query: &str) -> Vec<String> {
        let q = query.trim().to_lowercase();
        if q.chars().count() < 2 {
            return Vec::new();
        }
        let mut hits: Vec<(u8, &str)> = self
            .names
            .iter()
            .filter_map(|(lower, name)| {
                let at = lower.find(&q)?;
                let rank = if at == 0 {
                    0
                } else if lower[..at].ends_with([' ', '-', '/', '(', '"']) {
                    1
                } else {
                    2
                };
                Some((rank, name.as_str()))
            })
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.len().cmp(&b.1.len())));
        hits.into_iter()
            .take(20)
            .map(|(_, n)| n.to_string())
            .collect()
    }
}

/// The legality words, by the letter [`StoredCard::legal`] spells each with;
/// `-` is a format the card's legalities do not name.
pub const LEGALITIES: [(char, &str); 4] = [
    ('l', "legal"),
    ('n', "not_legal"),
    ('b', "banned"),
    ('r', "restricted"),
];

/// `legalities` as a letter per format, or `None` when it holds a word
/// [`LEGALITIES`] has no letter for, so must be kept as it is.
fn spell_legalities(legalities: &BTreeMap<String, String>, formats: &[String]) -> Option<String> {
    formats
        .iter()
        .map(|f| match legalities.get(f) {
            None => Some('-'),
            Some(word) => LEGALITIES.iter().find(|(_, w)| w == word).map(|(c, _)| *c),
        })
        .collect()
}

fn read_legalities(legal: &str, formats: &[String]) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (letter, format) in legal.chars().zip(formats) {
        if letter != '-' {
            let (_, word) = LEGALITIES.iter().find(|(c, _)| *c == letter)?;
            out.insert(format.clone(), word.to_string());
        }
    }
    Some(out)
}

/// Whether a query reads anything that differs between two printings of a
/// card, so must be asked of each.
fn reads_printing(q: &Query) -> bool {
    match q {
        Query::Printing(_) | Query::Set(_) | Query::Rarity(..) => true,
        Query::Not(inner) => reads_printing(inner),
        Query::And(parts) | Query::Or(parts) => parts.iter().any(reads_printing),
        _ => false,
    }
}

fn is_extra(layout: &str) -> bool {
    NOT_CARD_LAYOUTS.contains(&layout) || EXTRA_LAYOUTS.contains(&layout)
}

/// `set/num`, the set lowercased; the collector number as printed, since
/// The List's `RIX-1` and a set's `1` are different printings.
fn number_key(set: &str, num: &str) -> String {
    format!("{}/{}", set.trim().to_lowercase(), num.trim())
}

/// A card's name as someone types it: a reversible printing names its card
/// `Sol Ring // Sol Ring`, which Scryfall's autocomplete offers as `Sol Ring`.
fn typed_name(name: &str) -> &str {
    match name.split_once(" // ") {
        Some((front, back)) if front == back => front,
        _ => name,
    }
}

/// Every name that names a card: its own and, for a card of two faces, its
/// front face's, lowercased.
fn name_keys(name: &str) -> Vec<String> {
    let whole = name.to_lowercase();
    match whole.split_once(" // ") {
        Some((front, _)) => vec![front.to_string(), whole.clone()],
        None => vec![whole],
    }
}

/// Scryfall's `prints_search_uri` for a card, as it writes it.
fn prints_uri(oracle_id: &str) -> String {
    format!(
        "https://api.scryfall.com/cards/search?order=released&q=oracleid%3A{oracle_id}&unique=prints"
    )
}

/// The `q` of a search URL, decoded.
fn query_param(uri: &str) -> Option<String> {
    let (_, query) = uri.split_once('?')?;
    let q = query.split('&').find_map(|kv| kv.strip_prefix("q="))?;
    Some(percent_decode(q))
}

fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = match bytes.get(i..i + 3) {
            Some([b'%', hi, lo]) => hex(*hi).zip(hex(*lo)).map(|(h, l)| h * 16 + l),
            _ => None,
        };
        match (escaped, bytes[i]) {
            (Some(b), _) => {
                out.push(b);
                i += 3;
                continue;
            }
            (None, b'+') => out.push(b' '),
            (None, b) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/scryfall-copy.jsonl");

    /// Newest first, as a ranking with no preference of its own would go.
    struct Newest;

    impl Ranking for Newest {
        type Key = std::cmp::Reverse<String>;

        fn key(&self, _: &CardView<'_>, printing: &Printing) -> Self::Key {
            std::cmp::Reverse(printing.released_at.clone())
        }
    }

    /// Digital printings first, which no product wants, so a test can tell
    /// the caller's ranking from any the copy might have of its own.
    struct DigitalFirst;

    impl Ranking for DigitalFirst {
        type Key = (bool, std::cmp::Reverse<String>);

        fn key(&self, card: &CardView<'_>, printing: &Printing) -> Self::Key {
            (!printing.digital, Newest.key(card, printing))
        }
    }

    /// A bulk line as Scryfall wrote it, read for what the copy must give
    /// back.
    #[derive(Facet)]
    struct Original {
        id: String,
        #[facet(default)]
        set_name: Option<String>,
        #[facet(default)]
        finishes: Vec<String>,
        #[facet(default)]
        prices: RawPrices,
        #[facet(default)]
        image_uris: Option<ImageUris>,
        #[facet(default)]
        prints_search_uri: Option<String>,
    }

    /// A copy built from `lines` and read back from the text a platform
    /// keeps, as a later load would.
    fn stored_and_loaded(cards: &str, tags: &str) -> ScryfallCopy {
        let mut b = Builder::new("2026-10-09T09:05:44.334+00:00");
        b.cards(cards);
        b.tags(tags);
        assert_eq!(b.unreadable(), &[] as &[String]);
        ScryfallCopy::load(&b.finish().to_text().unwrap()).unwrap()
    }

    /// Every way printing `copy`'s answer for `original` differs from it.
    fn differences(copy: &ScryfallCopy, card: &BulkCard, original: &Original) -> Vec<String> {
        let mut out = Vec::new();
        let mut differ = |what: &str, have: String, want: String| {
            if have != want {
                out.push(format!(
                    "{} {what}: rebuilt {have} but Scryfall wrote {want}",
                    original.id
                ));
            }
        };
        let Some(&i) = copy.by_id.get(&original.id) else {
            return vec![format!("{} is not in the copy", original.id)];
        };
        let found = copy.found(i);
        let mut want_facts = card.clone();
        for face in want_facts.card_faces.iter_mut().flatten() {
            face.image_uris = None;
        }
        differ(
            "card object",
            facet_json::to_string(&copy.facts(i)).unwrap(),
            facet_json::to_string(&want_facts).unwrap(),
        );
        differ("name", found.name.clone(), card.name.clone());
        differ(
            "set",
            found.set.clone(),
            card.set.clone().unwrap_or_default().to_lowercase(),
        );
        differ(
            "number",
            found.num.clone(),
            card.collector_number.clone().unwrap_or_default(),
        );
        differ(
            "set name",
            found.set_name.clone(),
            original.set_name.clone().unwrap_or_default(),
        );
        differ(
            "released",
            found.released.clone(),
            card.released_at.clone().unwrap_or_default(),
        );
        differ(
            "finishes",
            format!("{:?}", found.finishes),
            format!("{:?}", original.finishes),
        );
        differ(
            "prices",
            format!("{:?}", found.prices),
            format!("{:?}", original.prices.read().read()),
        );
        differ(
            "colour identity",
            format!("{:?}", found.color_identity),
            format!("{:?}", card.color_identity.clone().unwrap_or_default()),
        );
        differ(
            "prints",
            format!("{:?}", found.prints),
            format!("{:?}", original.prints_search_uri),
        );
        let faces = card.card_faces.as_deref().unwrap_or_default();
        let front = original
            .image_uris
            .as_ref()
            .or_else(|| faces.first().and_then(|f| f.image_uris.as_ref()));
        differ(
            "picture",
            format!("{:?}", found.image),
            format!("{:?}", front.and_then(|i| i.normal.clone())),
        );
        differ(
            "small picture",
            format!("{:?}", found.small),
            format!("{:?}", front.and_then(|i| i.small.clone())),
        );
        if card.layout.as_deref() != Some("flip") {
            differ(
                "back picture",
                format!("{:?}", found.back.map(|b| b.image)),
                format!(
                    "{:?}",
                    faces
                        .get(1)
                        .and_then(|f| f.image_uris.as_ref()?.normal.clone())
                ),
            );
        }
        out
    }

    fn assert_round_trip(lines: &str) -> ScryfallCopy {
        let copy = stored_and_loaded(lines, "");
        let mut wrong = Vec::new();
        let mut n = 0;
        for line in lines.lines().filter(|l| !l.trim().is_empty()) {
            let card: BulkCard = facet_json::from_str(line).unwrap();
            let original: Original = facet_json::from_str(line).unwrap();
            wrong.extend(differences(&copy, &card, &original));
            n += 1;
        }
        assert!(
            wrong.is_empty(),
            "{} of {n} printings came back different, the first:\n{}",
            wrong.len(),
            wrong[..wrong.len().min(10)].join("\n")
        );
        assert_eq!(copy.printings.len(), n, "a printing went missing");
        copy
    }

    #[test]
    fn every_printing_comes_back_as_scryfall_wrote_it() {
        let copy = assert_round_trip(FIXTURE);
        // Printings saying the same of their card share it.
        assert!(copy.kept.len() < copy.printings.len());
    }

    /// The same over all of Default Cards:
    /// `SCRYFALL_BULK=default-cards.jsonl cargo test -p chip-scryfall
    /// --release copy -- --nocapture`. Without the file it checks nothing.
    #[test]
    fn every_printing_of_the_bulk_file_comes_back() {
        let Some(path) = std::env::var_os("SCRYFALL_BULK") else {
            return;
        };
        let lines = std::fs::read_to_string(path).unwrap();
        let start = std::time::Instant::now();
        let mut b = Builder::new("now");
        b.cards(&lines);
        let built = start.elapsed();
        let text = b.finish().to_text().unwrap();
        let written = start.elapsed();
        let load = std::time::Instant::now();
        let copy = ScryfallCopy::load(&text).unwrap();
        println!(
            "{} printings of {} cards: built in {built:?}, written {} MB in {:?}, loaded in {:?}",
            copy.printings.len(),
            copy.kept.len(),
            text.len() / 1_000_000,
            written - built,
            load.elapsed()
        );
        drop(copy);
        assert_round_trip(&lines);
    }

    /// How long a load takes to read the kept copy back, by step, five times
    /// over: `SCRYFALL_BULK=default-cards.jsonl cargo test -p
    /// chip-scryfall --release load_speed -- --nocapture`. The copy is built
    /// once and kept beside the bulk file, so a second run only loads.
    #[test]
    fn load_speed() {
        let Some(path) = std::env::var_os("SCRYFALL_BULK") else {
            return;
        };
        let kept = std::path::PathBuf::from(&path).with_extension(format!("copy-{FORMAT}.txt"));
        let text = std::fs::read_to_string(&kept).unwrap_or_else(|_| {
            let mut b = Builder::new("now");
            b.cards(&std::fs::read_to_string(&path).unwrap());
            let text = b.finish().to_text().unwrap();
            std::fs::write(&kept, &text).unwrap();
            text
        });
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let copy = ScryfallCopy::load(&text).unwrap();
            let read = start.elapsed();
            let warm = std::time::Instant::now();
            while !copy.warm(2000) {}
            let indexed = warm.elapsed();
            let ask = std::time::Instant::now();
            let n = ["Sol Ring", "Island", "Lightning Bolt"]
                .into_iter()
                .filter_map(|name| copy.find(&Wanted::Name { name: name.into() }, &Newest))
                .count();
            println!(
                "loaded {read:?}, warmed {indexed:?}, {n} names found in {:?}",
                ask.elapsed()
            );
        }
    }

    fn found(copy: &ScryfallCopy, wanted: Wanted) -> Found {
        copy.find(&wanted, &Newest)
            .unwrap_or_else(|| panic!("nothing for {wanted:?}"))
    }

    #[test]
    fn a_card_is_found_by_number_id_and_name() {
        let copy = stored_and_loaded(FIXTURE, "");
        let any = copy.found(0);
        let by_number = found(
            &copy,
            Wanted::Printing {
                set: any.set.to_uppercase(),
                num: any.num.clone(),
            },
        );
        assert_eq!(by_number.id, any.id);
        assert_eq!(
            found(
                &copy,
                Wanted::Id {
                    id: any.id.to_uppercase()
                }
            )
            .id,
            any.id
        );
        // A front face names a card of two, and a name finds a real card
        // before a token of the same name.
        let delver = found(
            &copy,
            Wanted::Name {
                name: "delver of secrets".into(),
            },
        );
        assert_eq!(delver.name, "Delver of Secrets // Insectile Aberration");
        assert!(delver.back.is_some());
        let island = found(
            &copy,
            Wanted::Name {
                name: "Island".into(),
            },
        );
        let in_set = found(
            &copy,
            Wanted::InSet {
                name: "Island".into(),
                set: island.set.clone(),
            },
        );
        assert_eq!(in_set.set, island.set);
        assert!(copy
            .find(
                &Wanted::Name {
                    name: "Not A Card".into()
                },
                &Newest
            )
            .is_none());
    }

    #[test]
    fn the_callers_ranking_picks_the_printing_a_name_gets() {
        let copy = stored_and_loaded(FIXTURE, "");
        let bolt = || Wanted::Name {
            name: "Lightning Bolt".into(),
        };
        let digital = |f: Option<Found>| copy.own(copy.by_id[&f.unwrap().id]).digital;
        assert!(!digital(copy.find(&bolt(), &Newest)));
        assert!(digital(copy.find(&bolt(), &DigitalFirst)));
        // A search answers each card with the same printing a name would.
        let searched = match copy.search("!\"Lightning Bolt\"", 0, 1, &DigitalFirst) {
            SearchAnswer::Page { mut cards, .. } => cards.pop(),
            SearchAnswer::Refused { message } => panic!("{message}"),
        };
        assert!(digital(searched));
    }

    #[test]
    fn pictures_are_turned_as_the_card_is_read() {
        let copy = stored_and_loaded(FIXTURE, "");
        let fire = found(
            &copy,
            Wanted::Name {
                name: "Fire // Ice".into(),
            },
        );
        assert_eq!(fire.turn, Some(Turn::Sideways));
        let cut = found(
            &copy,
            Wanted::Name {
                name: "Cut // Ribbons".into(),
            },
        );
        assert_eq!(cut.turn, None);
        let invasion = found(
            &copy,
            Wanted::Name {
                name: "Invasion of Zendikar".into(),
            },
        );
        assert_eq!(invasion.turn, Some(Turn::Sideways));
        assert_eq!(invasion.front_type_line, "Battle — Siege");
        let erayo = found(
            &copy,
            Wanted::Name {
                name: "Erayo, Soratami Ascendant".into(),
            },
        );
        assert_eq!(
            erayo.back.map(|b| (b.image, b.turn)),
            erayo.image.map(|i| (i, Turn::UpsideDown))
        );
    }

    #[test]
    fn every_printing_of_a_card_by_oracle_id_or_name() {
        let copy = stored_and_loaded(FIXTURE, "");
        let ring = found(
            &copy,
            Wanted::Name {
                name: "Sol Ring".into(),
            },
        );
        let by_oracle = copy.prints(ring.prints.as_deref().unwrap());
        assert!(by_oracle.len() > 1);
        assert!(by_oracle
            .windows(2)
            .all(|w| w[0].printing.released >= w[1].printing.released));
        // Meldweb's own search by name, `!"Sol Ring"` as the page encodes it,
        // finds the same, the reversible printing included: Scryfall's misses
        // it (scryfall-in-the-browser.md), but the copy reads its front face.
        let by_name =
            copy.prints("https://api.scryfall.com/cards/search?q=!%22Sol%20Ring%22&unique=prints");
        assert_eq!(by_name.len(), by_oracle.len());
        assert!(by_oracle
            .iter()
            .any(|p| p.facts.layout.as_deref() == Some("reversible_card")));
        assert!(by_name
            .iter()
            .any(|p| p.facts.layout.as_deref() == Some("reversible_card")));
    }

    fn page(copy: &ScryfallCopy, query: &str) -> (Vec<String>, u32) {
        match copy.search(query, 0, 175, &Newest) {
            SearchAnswer::Page { cards, total } => {
                (cards.into_iter().map(|c| c.name).collect(), total)
            }
            SearchAnswer::Refused { message } => panic!("{query}: {message}"),
        }
    }

    #[test]
    fn search_finds_one_printing_per_card_sorted_by_name_without_extras() {
        let copy = stored_and_loaded(FIXTURE, "");
        let (names, total) = page(&copy, "t:land");
        assert_eq!(names.len() as u32, total);
        assert!(names.contains(&"Island".to_string()));
        assert_eq!(names.iter().filter(|n| *n == "Island").count(), 1);
        assert!(names
            .windows(2)
            .all(|w| w[0].to_lowercase() <= w[1].to_lowercase()));
        // A token's layout keeps it out, as Scryfall does; a Role is a flip
        // card typed Token, which Scryfall answers `t:token` with too.
        let (tokens, _) = page(&copy, "t:token");
        assert!(!tokens.contains(&"Treasure".to_string()), "{tokens:?}");
        assert!(
            tokens.contains(&"Monster // Virtuous".to_string()),
            "{tokens:?}"
        );
        // A printing term finds the card through the printing that has it.
        let ring = found(
            &copy,
            Wanted::Name {
                name: "Sol Ring".into(),
            },
        );
        let (sol, _) = page(&copy, &format!("!\"Sol Ring\" s:{}", ring.set));
        assert_eq!(sol, ["Sol Ring"]);
        let (none, _) = page(&copy, "!\"Sol Ring\" s:lea is:fullart");
        assert!(none.is_empty(), "{none:?}");
        match copy.search("year:2020", 0, 10, &Newest) {
            SearchAnswer::Refused { message } => assert!(message.contains("year"), "{message}"),
            other => panic!("{other:?}"),
        }
        match copy.search("kw:flyign", 0, 10, &Newest) {
            SearchAnswer::Refused { message } => assert!(message.contains("flyign"), "{message}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quick_add_offers_a_reversible_card_by_its_own_name() {
        let copy = stored_and_loaded(FIXTURE, "");
        assert_eq!(copy.autocomplete("sol r"), ["Sol Ring"]);
        assert_eq!(
            copy.autocomplete("delver of"),
            ["Delver of Secrets // Insectile Aberration"]
        );
    }

    #[test]
    fn an_oracle_tag_holds_its_childrens_cards_and_answers_to_its_aliases() {
        let ring = FIXTURE
            .lines()
            .find(|l| l.contains(r#""name":"Sol Ring""#))
            .unwrap();
        let oracle = facet_json::from_str::<BulkCard>(ring)
            .unwrap()
            .oracle_id
            .unwrap();
        let tags = format!(
            "{}\n{}\n",
            r#"{"id":"p","slug":"ramp","aliases":["mana-ramp"],"parent_ids":[],"taggings":[]}"#,
            format_args!(
                r#"{{"id":"c","slug":"mana-rock","parent_ids":["p"],"taggings":[{{"oracle_id":"{oracle}"}}]}}"#
            ),
        );
        let copy = stored_and_loaded(FIXTURE, &tags);
        assert_eq!(page(&copy, "otag:mana-rock").0, ["Sol Ring"]);
        assert_eq!(page(&copy, "otag:ramp").0, ["Sol Ring"]);
        assert_eq!(page(&copy, "otag:mana-ramp").0, ["Sol Ring"]);
        match copy.search("otag:rmap", 0, 10, &Newest) {
            SearchAnswer::Refused { message } => assert!(message.contains("rmap"), "{message}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn quick_add_offers_names_it_begins_first() {
        let copy = stored_and_loaded(FIXTURE, "");
        let names = copy.autocomplete("ring");
        assert!(names.contains(&"Sol Ring".to_string()), "{names:?}");
        assert_eq!(
            copy.autocomplete("sol ri").first().map(String::as_str),
            Some("Sol Ring")
        );
        assert!(copy.autocomplete("s").is_empty());
        assert!(!copy.autocomplete("treas").iter().any(|n| n == "Treasure"));
    }

    /// How long searches take over all of Default Cards and Oracle Tags:
    /// `SCRYFALL_BULK=default-cards.jsonl
    /// SCRYFALL_TAGS=oracle-tags.jsonl cargo test -p chip-scryfall
    /// --release search_speed -- --nocapture`. Without them it checks nothing.
    #[test]
    fn search_speed() {
        let (Some(cards), Some(tags)) = (
            std::env::var_os("SCRYFALL_BULK"),
            std::env::var_os("SCRYFALL_TAGS"),
        ) else {
            return;
        };
        let cards = std::fs::read_to_string(cards).unwrap();
        let tags = std::fs::read_to_string(tags).unwrap();
        let copy = stored_and_loaded(&cards, &tags);
        for query in [
            "lightning",
            "!\"Sol Ring\"",
            "t:creature",
            "o:draw",
            "otag:ramp",
            "otag:removal id<=wg f:commander",
            "(o:draw or o:scry) id<=wg f:commander",
            "s:mh3",
            "t:basic is:fullart",
            "mv<=2 t:artifact o:\"add {c}\"",
        ] {
            let start = std::time::Instant::now();
            let answer = copy.search(query, 0, 175, &Newest);
            let cold = start.elapsed();
            *copy.last.borrow_mut() = None;
            let total = match answer {
                SearchAnswer::Page { total, .. } => total.to_string(),
                SearchAnswer::Refused { message } => message,
            };
            println!("{query:45} {total:>6} cards in {cold:?}");
        }
        for prefix in ["so", "sol r", "ragavan"] {
            let start = std::time::Instant::now();
            let n = copy.autocomplete(prefix).len();
            println!(
                "autocomplete {prefix:32} {n:>6} names in {:?}",
                start.elapsed()
            );
        }
    }
}
