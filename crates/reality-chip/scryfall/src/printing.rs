//! What tells two printings of one card apart, and the search terms that ask
//! about it.
//!
//! The index is one record per card, so nothing it holds can answer
//! `is:fullart` or `lang:ja`: a card is not full art, a printing is. These
//! terms exist for a caller holding every printing of a card and choosing
//! between them, and only [`crate::parse_printing`] accepts them.
//! [`crate::parse`] refuses each one by name, so a criteria file asking about a
//! printing is told so rather than counting nothing.
//!
//! Every word below is one Scryfall's search accepts and means what it means
//! there, read off the field Scryfall reads: `is:sourcematerial` is the
//! `sourcematerial` entry in `promo_types`, as Scryfall's own count of 247
//! printings confirms.

/// One printing's own facts, as Scryfall's card object states them.
#[derive(Debug, Clone, Default)]
pub struct Printing {
    pub set: String,
    pub set_type: String,
    pub collector_number: String,
    /// `YYYY-MM-DD`.
    pub released_at: String,
    pub lang: String,
    pub frame: String,
    pub frame_effects: Vec<String>,
    pub border_color: String,
    pub full_art: bool,
    pub textless: bool,
    /// Printed only on MTGO or Arena: not a card anyone can hold.
    pub digital: bool,
    pub promo: bool,
    pub reprint: bool,
    pub oversized: bool,
    pub promo_types: Vec<String>,
    /// Where the printing exists: `paper`, `mtgo`, `arena`.
    pub games: Vec<String>,
    /// The name printed in place of the card's own, as on the Godzilla series.
    pub flavor_name: Option<String>,
}

/// A search term about a printing rather than a card.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintingTerm {
    /// `lang:ja`; `lang:any` is every language.
    Lang(Lang),
    /// `st:masterpiece`.
    SetType(&'static str),
    /// `frame:1997`, `frame:showcase`.
    Frame(Frame),
    /// `border:borderless`.
    Border(&'static str),
    /// `game:paper`.
    Game(&'static str),
    /// `cn:263`: the collector number, as the set prints it.
    CollectorNumber(String),
    /// `is:fullart`, `is:ub`.
    Is(PrintingProperty),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Any,
    Code(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// One of the frame generations, by the year Scryfall names it for.
    Year(&'static str),
    /// `frame:old`: the 1993 and 1997 frames together, as `is:retro` is.
    Old,
    /// A frame effect, such as `showcase` or `extendedart`.
    Effect(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintingProperty {
    FullArt,
    Textless,
    Digital,
    Promo,
    Reprint,
    Oversized,
    /// Printed under a name other than the card's own.
    FlavorName,
    Borderless,
    /// The 1993 or 1997 frame.
    Retro,
    /// A frame effect Scryfall also answers as `is:` — `is:showcase` is
    /// `frame:showcase`.
    FrameEffect(&'static str),
    /// An entry in `promo_types`: `is:sourcematerial`, `is:poster`.
    PromoType(&'static str),
}

/// The languages Scryfall prints cards in, by the code `lang:` takes.
pub const LANGS: &[&str] = &[
    "en", "es", "fr", "de", "it", "pt", "ja", "ko", "ru", "zhs", "zht", "he", "la", "grc", "ar",
    "sa", "ph", "qya",
];

pub const SET_TYPES: &[&str] = &[
    "core",
    "expansion",
    "masters",
    "alchemy",
    "masterpiece",
    "arsenal",
    "from_the_vault",
    "spellbook",
    "premium_deck",
    "duel_deck",
    "draft_innovation",
    "treasure_chest",
    "commander",
    "planechase",
    "archenemy",
    "vanguard",
    "funny",
    "starter",
    "box",
    "promo",
    "token",
    "memorabilia",
    "minigame",
    "eternal",
];

pub const FRAME_YEARS: &[&str] = &["1993", "1997", "2003", "2015", "future"];

/// Scryfall's frame effects, which `frame:` also takes.
pub const FRAME_EFFECTS: &[&str] = &[
    "legendary",
    "miracle",
    "enchantment",
    "draft",
    "devoid",
    "tombstone",
    "colorshifted",
    "inverted",
    "sunmoondfc",
    "compasslanddfc",
    "originpwdfc",
    "mooneldrazidfc",
    "waxingandwaningmoondfc",
    "showcase",
    "extendedart",
    "companion",
    "etched",
    "snow",
    "lesson",
    "shatteredglass",
    "convertdfc",
    "fandfc",
    "upsidedowndfc",
    "spree",
];

pub const BORDERS: &[&str] = &["black", "white", "borderless", "silver", "gold", "yellow"];

pub const GAMES: &[&str] = &["paper", "mtgo", "arena"];

/// The `is:` words that ask about a printing, and what each reads.
pub const PRINTING_IS: &[(&str, PrintingProperty)] = &[
    ("fullart", PrintingProperty::FullArt),
    ("textless", PrintingProperty::Textless),
    ("digital", PrintingProperty::Digital),
    ("promo", PrintingProperty::Promo),
    ("reprint", PrintingProperty::Reprint),
    ("oversized", PrintingProperty::Oversized),
    ("flavorname", PrintingProperty::FlavorName),
    ("borderless", PrintingProperty::Borderless),
    ("retro", PrintingProperty::Retro),
    ("showcase", PrintingProperty::FrameEffect("showcase")),
    ("extendedart", PrintingProperty::FrameEffect("extendedart")),
    ("ub", PrintingProperty::PromoType("universesbeyond")),
    (
        "universesbeyond",
        PrintingProperty::PromoType("universesbeyond"),
    ),
    (
        "sourcematerial",
        PrintingProperty::PromoType("sourcematerial"),
    ),
    ("boosterfun", PrintingProperty::PromoType("boosterfun")),
    ("poster", PrintingProperty::PromoType("poster")),
    ("stamped", PrintingProperty::PromoType("stamped")),
    ("datestamped", PrintingProperty::PromoType("datestamped")),
    ("prerelease", PrintingProperty::PromoType("prerelease")),
    ("playtest", PrintingProperty::PromoType("playtest")),
    ("serialized", PrintingProperty::PromoType("serialized")),
];

/// Every key that asks about a printing, one row per key and its spellings.
pub const PRINTING_KEYS: &[&[&str]] = &[
    &["lang", "language"],
    &["st", "settype"],
    &["frame"],
    &["border"],
    &["game"],
    &["cn", "number"],
];

impl PrintingTerm {
    pub fn matches(&self, p: &Printing) -> bool {
        match self {
            PrintingTerm::Lang(Lang::Any) => true,
            PrintingTerm::Lang(Lang::Code(code)) => p.lang.eq_ignore_ascii_case(code),
            PrintingTerm::SetType(t) => p.set_type.eq_ignore_ascii_case(t),
            PrintingTerm::Frame(Frame::Year(y)) => p.frame.eq_ignore_ascii_case(y),
            PrintingTerm::Frame(Frame::Old) => PrintingProperty::Retro.matches(p),
            PrintingTerm::Frame(Frame::Effect(e)) => has(&p.frame_effects, e),
            PrintingTerm::Border(b) => p.border_color.eq_ignore_ascii_case(b),
            PrintingTerm::Game(g) => has(&p.games, g),
            PrintingTerm::CollectorNumber(n) => p.collector_number.eq_ignore_ascii_case(n),
            PrintingTerm::Is(property) => property.matches(p),
        }
    }
}

impl PrintingProperty {
    fn matches(self, p: &Printing) -> bool {
        match self {
            PrintingProperty::FullArt => p.full_art,
            PrintingProperty::Textless => p.textless,
            PrintingProperty::Digital => p.digital,
            PrintingProperty::Promo => p.promo,
            PrintingProperty::Reprint => p.reprint,
            PrintingProperty::Oversized => p.oversized,
            PrintingProperty::FlavorName => p.flavor_name.is_some(),
            PrintingProperty::Borderless => p.border_color.eq_ignore_ascii_case("borderless"),
            PrintingProperty::Retro => p.frame == "1993" || p.frame == "1997",
            PrintingProperty::FrameEffect(e) => has(&p.frame_effects, e),
            PrintingProperty::PromoType(t) => has(&p.promo_types, t),
        }
    }
}

impl From<Lang> for PrintingTerm {
    fn from(l: Lang) -> Self {
        PrintingTerm::Lang(l)
    }
}

impl From<Frame> for PrintingTerm {
    fn from(f: Frame) -> Self {
        PrintingTerm::Frame(f)
    }
}

fn has(list: &[String], want: &str) -> bool {
    list.iter().any(|x| x.eq_ignore_ascii_case(want))
}

/// The entry in `table` spelled `value`, ignoring case.
pub(crate) fn closed(table: &[&'static str], value: &str) -> Option<&'static str> {
    table
        .iter()
        .copied()
        .find(|t| t.eq_ignore_ascii_case(value))
}
