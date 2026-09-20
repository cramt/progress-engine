//! Card data and a parser for Scryfall search syntax.
//!
//! Deliberately a subset. The governing rule is that anything unsupported is a
//! *parse error naming the offending term*, never a silent no-match — a query
//! that quietly matches nothing produces a confidently wrong probability, which
//! is the exact failure this crate exists to prevent.
//!
//! The second rule is that where a key exists, it means what Scryfall means.
//! Someone will paste a query out of a Scryfall search, and a key that looks
//! familiar and behaves differently is worse than one that is missing: the
//! missing one says so.

pub mod bulk;
pub mod index;
pub mod legality;
pub mod mana;
mod outside;
mod parse;

pub mod tags;

pub use outside::{outside_library, OutsideLibrary};
pub use parse::{parse, ParseError};

use index::Face;
use legality::Legalities;

/// A card as the matcher sees it: Scryfall's data plus the categories the
/// decklist assigned it.
#[derive(Debug, Clone)]
pub struct CardView<'a> {
    pub name: &'a str,
    /// The whole card's type line, both faces joined by `//` as Scryfall
    /// prints it.
    pub type_line: &'a str,
    /// Oracle text with reminder text removed, which is what `o:` searches.
    pub oracle: &'a str,
    /// Oracle text verbatim, which is what `fo:` searches.
    pub full_oracle: &'a str,
    pub mana_cost: &'a str,
    pub cmc: f64,
    /// Keyword abilities, actions and ability words, as Scryfall prints them.
    pub keywords: &'a [String],
    /// Colour identity letters, e.g. `["W","U"]`. What `id:` reads.
    pub color_identity: &'a [String],
    /// The card's own colours. Not the same thing as its identity.
    pub colors: &'a [String],
    /// The mana symbols this card can make, e.g. `["G","C"]`.
    pub produces: &'a [String],
    pub rarity: &'a str,
    pub set: &'a str,
    pub layout: &'a str,
    /// Every face, including the only one of a single-faced card.
    pub faces: &'a [Face],
    /// The oracle tags this card is in, as of the index's tag fetch. What
    /// `otag:` reads.
    pub tags: &'a [String],
    pub legalities: &'a Legalities,
    pub game_changer: Option<bool>,
    pub reserved: Option<bool>,
    pub categories: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
}

impl Cmp {
    pub fn test_ord(self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::*;
        matches!(
            (self, ordering),
            (Cmp::Lt, Less)
                | (Cmp::Le, Less | Equal)
                | (Cmp::Eq, Equal)
                | (Cmp::Ne, Less | Greater)
                | (Cmp::Ge, Greater | Equal)
                | (Cmp::Gt, Greater)
        )
    }

    fn test_num(self, have: f64, want: f64) -> bool {
        have.partial_cmp(&want).is_some_and(|o| self.test_ord(o))
    }
}

/// A set of mana symbols: WUBRG plus colourless.
///
/// One type for three jobs that are all subset tests — a card's colours, its
/// colour identity, and the mana it produces — because the comparison logic is
/// identical and having written it three times is how `c:` and `id:` come to
/// disagree about what `<=` means.
///
/// Colourless is a *bit* rather than the empty set, because the two are
/// genuinely different questions and only one of them has a symbol. A card with
/// no colours is colourless; a card that taps for `{C}` **produces** colourless
/// while being colourless itself. `from_letters` reads `c` as the first,
/// `from_mana_letters` as the second, and which one applies is decided by the
/// search key rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colors(u8);

impl Colors {
    pub const LETTERS: [(char, u8); 5] = [('w', 1), ('u', 2), ('b', 4), ('r', 8), ('g', 16)];
    const COLORLESS: u8 = 32;

    /// Colour letters, where `c` means colourless and so contributes no bits.
    /// The reading for `c:` and `id:`, where colourless is the absence of
    /// colour rather than a colour of its own.
    pub fn from_letters(s: &str) -> Option<Self> {
        Self::parse(s, false)
    }

    /// Mana symbols, where `c` is the colourless mana symbol and is a value in
    /// its own right. The reading for `produces:`: a Sol Ring produces
    /// something, and it is not "nothing".
    pub fn from_mana_letters(s: &str) -> Option<Self> {
        Self::parse(s, true)
    }

    fn parse(s: &str, colorless_is_a_symbol: bool) -> Option<Self> {
        let mut bits = 0u8;
        for ch in s.to_ascii_lowercase().chars() {
            if ch == 'c' {
                if colorless_is_a_symbol {
                    bits |= Self::COLORLESS;
                }
                continue;
            }
            let (_, bit) = Self::LETTERS.iter().find(|(l, _)| *l == ch)?;
            bits |= bit;
        }
        Some(Colors(bits))
    }

    pub fn from_identity(ci: &[String]) -> Self {
        Colors::from_letters(&ci.concat()).unwrap_or_default()
    }

    pub fn from_mana(symbols: &[String]) -> Self {
        Colors::from_mana_letters(&symbols.concat()).unwrap_or_default()
    }

    /// Whether every symbol of `self` is also in `other`. Both the `id<=`
    /// query and the colour identity legality check are this one test.
    pub fn is_subset_of(self, other: Colors) -> bool {
        self.0 & !other.0 == 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn union(self, other: Colors) -> Colors {
        Colors(self.0 | other.0)
    }

    pub fn intersect(self, other: Colors) -> Colors {
        Colors(self.0 & other.0)
    }

    /// How many symbols, which is what `c>=2` counts.
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }
}

/// Which set of symbols a colour term is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorField {
    /// `c:` / `color:` — the card's own colours.
    Color,
    /// `id:` / `identity:` — its colour identity.
    Identity,
    /// `produces:` / `prod:` — the mana it can make.
    Produces,
}

impl ColorField {
    /// What a bare colon means for this key.
    ///
    /// Scryfall's two keys point opposite ways and this is the single most
    /// pasted-and-misread thing in its syntax: `c:rg` is "red **and** green",
    /// while `id:rg` is "fits inside Gruul". Both readings are here so that
    /// neither has to be remembered.
    fn colon_means(self) -> Cmp {
        match self {
            ColorField::Color | ColorField::Produces => Cmp::Ge,
            ColorField::Identity => Cmp::Le,
        }
    }

    fn of(self, card: &CardView<'_>) -> Colors {
        match self {
            ColorField::Color => Colors::from_identity(card.colors),
            ColorField::Identity => Colors::from_identity(card.color_identity),
            ColorField::Produces => Colors::from_mana(card.produces),
        }
    }
}

/// What the right-hand side of a colour term said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpec {
    /// Letters or a nickname: `wu`, `azorius`, `bant`.
    Set(Colors),
    /// `c` / `colorless`, which asks for *no* symbols rather than for a symbol.
    Colorless,
    /// `m` / `multicolor`: two or more.
    Multicolor,
    /// A bare number: `c>=2`.
    Count(u32),
}

/// A creature or planeswalker statistic, all of which compare numerically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat {
    Power,
    Toughness,
    /// `pt` / `powtou`: power plus toughness.
    PowerPlusToughness,
    Loyalty,
    Defense,
}

impl Stat {
    /// This statistic's value on one face, when it has one that is a number.
    ///
    /// `None` covers both "this face has no power" and "its power is `*`".
    /// Those are different facts, but they answer every numeric comparison the
    /// same way — see [`Query::Stat`].
    fn of(self, face: &Face) -> Option<f64> {
        let num = |v: &Option<String>| v.as_deref().and_then(parse_stat);
        match self {
            Stat::Power => num(&face.power),
            Stat::Toughness => num(&face.toughness),
            Stat::PowerPlusToughness => Some(num(&face.power)? + num(&face.toughness)?),
            Stat::Loyalty => num(&face.loyalty),
            Stat::Defense => num(&face.defense),
        }
    }
}

/// What a statistic is being compared against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatOperand {
    Number(f64),
    /// `pow>tou` — the other statistic on the same face.
    Stat(Stat),
}

/// A printed power or toughness read as a number, when it is one.
///
/// `*`, `1+*`, `∞`, `?` and `*²` are all real printed values and none is a
/// number. Rather than call them zero — which would put Tarmogoyf in `pow=0`
/// and quietly out of `pow>=1` — they are *not a number*, and a card whose
/// value is not a number satisfies no numeric comparison in either direction.
/// `-pow>=3` therefore finds them, which is the honest place for "we cannot
/// say" to land.
fn parse_stat(printed: &str) -> Option<f64> {
    printed.trim().parse::<f64>().ok()
}

/// Rarities, in the order Scryfall compares them, so `r>=rare` works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Special,
    Mythic,
    Bonus,
}

impl Rarity {
    pub const NAMES: [(&'static str, Rarity); 6] = [
        ("common", Rarity::Common),
        ("uncommon", Rarity::Uncommon),
        ("rare", Rarity::Rare),
        ("special", Rarity::Special),
        ("mythic", Rarity::Mythic),
        ("bonus", Rarity::Bonus),
    ];

    /// Read a rarity, accepting Scryfall's one-letter abbreviations.
    ///
    /// `r` is rare rather than a prefix match, because `r:r` is a documented
    /// Scryfall query and prefix-matching it would also accept nothing else.
    pub fn parse(s: &str) -> Option<Rarity> {
        let s = s.to_ascii_lowercase();
        match s.as_str() {
            "c" => return Some(Rarity::Common),
            "u" => return Some(Rarity::Uncommon),
            "r" => return Some(Rarity::Rare),
            "s" => return Some(Rarity::Special),
            "m" => return Some(Rarity::Mythic),
            "b" => return Some(Rarity::Bonus),
            _ => {}
        }
        Rarity::NAMES
            .iter()
            .find(|(name, _)| *name == s)
            .map(|&(_, r)| r)
    }
}

/// What a format term is asking about a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatStatus {
    /// `f:` — legal to play, which includes restricted.
    Legal,
    /// `banned:`
    Banned,
    /// `restricted:`
    Restricted,
}

/// The `is:` properties this crate can answer from data it actually holds.
///
/// Scryfall's own `is:` list is longer, and the absent ones are absent on
/// purpose. The land cycles — `is:shockland`, `is:fetchland`, `is:tapland` and
/// the rest — are **curated lists** on Scryfall's side, not fields in the bulk
/// data. Reproducing them here would mean either hard-coding a copy that goes
/// stale the day a new cycle prints, or deriving them from oracle text, which
/// is confidently wrong in both directions: `/enters.*tapped/` calls a Temple
/// and a Shockland the same thing. Both are worse than the parse error you get
/// today, which at least says what it cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsProperty {
    Permanent,
    Spell,
    Historic,
    /// A creature with no rules text at all.
    Vanilla,
    /// Scryfall's "bear": a 2/2 that costs two.
    Bear,
    /// Any card with two faces printed on two sides.
    DoubleFaced,
    ModalDoubleFaced,
    Transform,
    Split,
    Flip,
    Meld,
    Leveler,
    Adventure,
    /// A mana cost containing a hybrid symbol, `{W/U}` or `{2/W}`.
    Hybrid,
    /// A mana cost containing a Phyrexian symbol, `{W/P}`.
    Phyrexian,
    /// A card that can be your commander, by any printed route.
    Commander,
    Partner,
    Companion,
    Reserved,
    /// Scryfall's Commander bracket "game changer" flag.
    GameChanger,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    /// `t:land` — substring of the type line, case-insensitive.
    Type(String),
    /// `o:"Add {W}"` — substring of oracle text, reminder text excluded.
    Oracle(String),
    /// `fo:flying` — the same, reminder text included.
    FullOracle(String),
    /// `name:"Rogue's Passage"`, or a bare word.
    Name(String),
    /// `kw:flying`, `kw:"double strike"` — one whole keyword the card has.
    Keyword(String),
    /// `otag:surveil-land`, `otag:tapland` — one Scryfall oracle tag the card
    /// is in.
    ///
    /// Not derived from anything on the card: the index carries whatever
    /// Scryfall's search answered when it was built. That is why this is worth
    /// having at all — "lands that enter tapped" is 495 cards and no reading of
    /// oracle text gets that set right, because the condition is conditional.
    Tag(String),
    /// `cat:"Exile Outlet"` — the one non-Scryfall addition, matching the
    /// decklist's Archidekt categories.
    Category(String),
    /// `mv<=2`, `cmc=3`.
    ManaValue(Cmp, f64),
    /// `mv:even`, `mv:odd`.
    ManaValueParity {
        even: bool,
    },
    /// `c:rg`, `id<=esper`, `produces:w` — one comparison over three fields.
    Colors(ColorField, Cmp, ColorSpec),
    /// `pow>=3`, `pow>tou`, `loy=3`.
    Stat(Stat, Cmp, StatOperand),
    /// `r>=rare`.
    Rarity(Cmp, Rarity),
    /// `s:mh3` — the set of the printing the index carries.
    Set(String),
    /// `f:pauper`, `banned:legacy`.
    Format(FormatStatus, &'static str),
    /// `m:2WW`, `mana>{2}{W}{W}` — the printed cost as a multiset of symbols.
    Mana(Cmp, mana::ManaCost),
    /// `devotion:{u}{u}` — how much a permanent gives to a devotion count,
    /// carrying the colours asked about and the level asked for.
    Devotion(Cmp, Colors, u32),
    /// `layout:transform`.
    Layout(String),
    /// `is:permanent`.
    Is(IsProperty),
    Not(Box<Query>),
    And(Vec<Query>),
    Or(Vec<Query>),
}

impl Query {
    pub fn matches(&self, card: &CardView<'_>) -> bool {
        match self {
            Query::Type(s) => contains_ci(card.type_line, s),
            Query::Oracle(s) => contains_ci(card.oracle, s),
            Query::FullOracle(s) => contains_ci(card.full_oracle, s),
            Query::Name(s) => contains_ci(card.name, s),
            // Whole value, not substring: a keyword is a discrete entry in a
            // list, so `kw:trample` must not be satisfied by a hypothetical
            // "Trampleover", nor by the word appearing in oracle text.
            // Lowercased in full rather than by the ASCII rule used elsewhere
            // because Scryfall prints keywords such as "Pavitr's Sevā".
            Query::Keyword(s) => {
                let want = s.to_lowercase();
                card.keywords.iter().any(|k| k.to_lowercase() == want)
            }
            // Tags are written lowercase and hyphenated by Scryfall
            // (`surveil-land`), so an ASCII-insensitive compare is enough and
            // there is no locale case to get wrong.
            Query::Tag(s) => card.tags.iter().any(|t| t.eq_ignore_ascii_case(s)),
            Query::Category(s) => card.categories.iter().any(|c| c.eq_ignore_ascii_case(s)),
            Query::ManaValue(cmp, v) => cmp.test_num(card.cmc, *v),
            // A fractional mana value is neither even nor odd. Un-cards have
            // them, and rounding one to answer the question would be an
            // invented fact about a real card.
            Query::ManaValueParity { even } => {
                card.cmc.fract() == 0.0 && ((card.cmc as i64) % 2 == 0) == *even
            }
            Query::Colors(field, cmp, spec) => {
                let have = field.of(card);
                match spec {
                    // Asked of any field, with any operator, "colourless" means
                    // the same thing: no symbols. Threading it through the
                    // subset logic instead would make `c:c` trivially true of
                    // every card, since everything contains the empty set.
                    ColorSpec::Colorless => have.is_empty(),
                    ColorSpec::Multicolor => have.count() >= 2,
                    ColorSpec::Count(n) => cmp.test_num(have.count() as f64, *n as f64),
                    ColorSpec::Set(want) => match cmp {
                        Cmp::Le => have.is_subset_of(*want),
                        Cmp::Lt => have.is_subset_of(*want) && have != *want,
                        Cmp::Ge => want.is_subset_of(have),
                        Cmp::Gt => want.is_subset_of(have) && have != *want,
                        Cmp::Eq => have == *want,
                        Cmp::Ne => have != *want,
                    },
                }
            }
            // Any face may satisfy it. Delver of Secrets is a 1/1 that becomes
            // a 3/2, and a search for three power that missed it would be
            // answering about the front of the card rather than about the card.
            Query::Stat(stat, cmp, operand) => card.faces.iter().any(|face| {
                let Some(have) = stat.of(face) else {
                    return false;
                };
                let want = match operand {
                    StatOperand::Number(n) => Some(*n),
                    StatOperand::Stat(other) => other.of(face),
                };
                want.is_some_and(|want| cmp.test_num(have, want))
            }),
            Query::Rarity(cmp, want) => {
                Rarity::parse(card.rarity).is_some_and(|have| cmp.test_ord(have.cmp(want)))
            }
            Query::Set(s) => card.set.eq_ignore_ascii_case(s),
            Query::Format(status, format) => {
                let Some(word) = card.legalities.get(format) else {
                    return false;
                };
                match status {
                    // Restricted counts as legal, because it is: a restricted
                    // card is one you may play, at one copy.
                    FormatStatus::Legal => word.permits_play() == Some(true),
                    FormatStatus::Banned => word == crate::legality::Legality::Banned,
                    FormatStatus::Restricted => word == crate::legality::Legality::Restricted,
                }
            }
            // Any face may satisfy it, for the same reason a statistic may:
            // half of Wear // Tear costs {1}{R} and the other half {W}, and
            // neither is "the cost of the card".
            Query::Mana(cmp, want) => card.costs().any(|have| match cmp {
                Cmp::Ge => want.is_subset_of(&have),
                Cmp::Gt => want.is_subset_of(&have) && have != *want,
                Cmp::Le => have.is_subset_of(want),
                Cmp::Lt => have.is_subset_of(want) && have != *want,
                Cmp::Eq => have == *want,
                Cmp::Ne => have != *want,
            }),
            // Only a permanent gives devotion, because only a permanent is on
            // the battlefield to give it. Counting the blue symbols on a
            // Counterspell found two thousand cards Scryfall does not.
            Query::Devotion(cmp, colors, level) => {
                IsProperty::Permanent.matches(card)
                    && card
                        .costs()
                        .any(|have| cmp.test_num(have.devotion_to(*colors) as f64, *level as f64))
            }
            Query::Layout(s) => card.layout.eq_ignore_ascii_case(s),
            Query::Is(p) => p.matches(card),
            Query::Not(inner) => !inner.matches(card),
            Query::And(parts) => parts.iter().all(|p| p.matches(card)),
            Query::Or(parts) => parts.iter().any(|p| p.matches(card)),
        }
    }

    /// The `kw:` values in this query that no card in the index carries.
    ///
    /// Scryfall refuses `kw:tramp` with *Unknown keyword "tramp"* instead of
    /// returning nothing, and a caller holding an index should say the same:
    /// a keyword no card has is a misspelling, and a misspelling that matches
    /// zero cards reports a confident 0%. The parser cannot tell — only the
    /// index knows which keywords exist — so this is the check it can make
    /// once a query and an index are in the same place.
    ///
    /// An index that carries no keywords at all yields nothing here: silence
    /// about keywords is not evidence that every keyword is a typo.
    pub fn unknown_keywords(&self, vocabulary: &index::KeywordVocabulary) -> Vec<String> {
        let mut out = Vec::new();
        if !vocabulary.is_empty() {
            self.collect_unknown_keywords(vocabulary, &mut out);
        }
        out
    }

    fn collect_unknown_keywords(
        &self,
        vocabulary: &index::KeywordVocabulary,
        out: &mut Vec<String>,
    ) {
        match self {
            Query::Keyword(k) if !vocabulary.contains(k) => out.push(k.clone()),
            Query::Not(inner) => inner.collect_unknown_keywords(vocabulary, out),
            Query::And(parts) | Query::Or(parts) => {
                for part in parts {
                    part.collect_unknown_keywords(vocabulary, out);
                }
            }
            _ => {}
        }
    }

    /// Which `otag:` terms this index cannot answer, and why.
    ///
    /// The same contract as [`Self::unknown_keywords`], and it matters more
    /// here. A mistyped keyword is at least a keyword; an index simply never
    /// told to fetch `surveil-land` will answer `otag:surveil-land` with zero
    /// cards and no indication that it was never asked. Both readings produce
    /// an empty set, so the vocabulary is the only thing that tells them apart.
    ///
    /// Unlike keywords, an index carrying *none* is not silence: keywords are
    /// derived from the cards, so an old index genuinely cannot know whether
    /// `flyign` is real, but tags are fetched and the header states which. "No
    /// tags were fetched" is therefore a fact the index asserts about itself,
    /// and it is reported as [`index::TagGap::NoneFetched`] rather than
    /// swallowed — an index that answers `otag:` with a confident nothing while
    /// admitting in its own header that it never asked is this tool's defining
    /// failure mode wearing its own cache as a costume.
    pub fn tag_gap(&self, vocabulary: &index::TagVocabulary) -> Option<index::TagGap> {
        let mut out = Vec::new();
        self.collect_unknown_tags(vocabulary, &mut out);
        if out.is_empty() {
            return None;
        }
        Some(if vocabulary.is_empty() {
            index::TagGap::NoneFetched(out)
        } else {
            index::TagGap::NotCarried(out)
        })
    }

    fn collect_unknown_tags(&self, vocabulary: &index::TagVocabulary, out: &mut Vec<String>) {
        match self {
            // Named twice is still one gap: a query is refused with the term
            // once, not once per mention.
            Query::Tag(t) if !vocabulary.contains(t) && !out.contains(t) => out.push(t.clone()),
            Query::Not(inner) => inner.collect_unknown_tags(vocabulary, out),
            Query::And(parts) | Query::Or(parts) => {
                for part in parts {
                    part.collect_unknown_tags(vocabulary, out);
                }
            }
            _ => {}
        }
    }
}

impl CardView<'_> {
    /// Every mana symbol printed on the card, in its cost and in its text.
    ///
    /// Both, because an activation cost is where most Phyrexian mana lives and
    /// Scryfall counts it: Blinding Souleater's mana cost is `{3}`.
    fn symbols(&self) -> impl Iterator<Item = &str> {
        mana_symbols(self.mana_cost).chain(mana_symbols(self.oracle))
    }

    /// The printed cost of each face, parsed.
    ///
    /// One per face rather than one per card: the index stores a split card's
    /// cost as `{1}{R} // {W}`, and reading that as a single multiset would
    /// invent a three-mana two-colour spell that nobody can cast.
    fn costs(&self) -> impl Iterator<Item = mana::ManaCost> + '_ {
        self.mana_cost.split("//").map(mana::ManaCost::parse)
    }
}

impl IsProperty {
    fn matches(self, card: &CardView<'_>) -> bool {
        let layout_is = |want: &str| card.layout.eq_ignore_ascii_case(want);
        let has_keyword = |want: &str| {
            card.keywords
                .iter()
                .any(|k| k.eq_ignore_ascii_case(want) || k.to_lowercase().starts_with(want))
        };
        match self {
            IsProperty::Permanent => [
                "artifact",
                "creature",
                "enchantment",
                "land",
                "planeswalker",
                "battle",
            ]
            .iter()
            .any(|t| contains_ci(card.type_line, t)),
            IsProperty::Spell => !contains_ci(card.type_line, "land"),
            // Historic is artifact, legendary or saga (the Teshar test).
            IsProperty::Historic => ["artifact", "legendary", "saga"]
                .iter()
                .any(|t| contains_ci(card.type_line, t)),
            IsProperty::Vanilla => {
                contains_ci(card.type_line, "creature") && card.oracle.trim().is_empty()
            }
            IsProperty::Bear => {
                card.cmc == 2.0
                    && contains_ci(card.type_line, "creature")
                    && card.faces.iter().any(|f| {
                        Stat::Power.of(f) == Some(2.0) && Stat::Toughness.of(f) == Some(2.0)
                    })
            }
            IsProperty::DoubleFaced => ["transform", "modal_dfc", "reversible_card", "meld"]
                .iter()
                .any(|l| layout_is(l)),
            IsProperty::ModalDoubleFaced => layout_is("modal_dfc"),
            IsProperty::Transform => layout_is("transform"),
            IsProperty::Split => layout_is("split"),
            IsProperty::Flip => layout_is("flip"),
            IsProperty::Meld => layout_is("meld"),
            IsProperty::Leveler => layout_is("leveler"),
            IsProperty::Adventure => layout_is("adventure"),
            // Read off the symbols rather than the colours, because a `{2/W}`
            // card is mono-white and a `{W/U}` one is two colours: neither
            // fact is the question being asked.
            //
            // Cost *and* text, because Blinding Souleater's only Phyrexian
            // symbol is in an activation cost and Scryfall counts it. Reading
            // the mana cost alone missed thirty-three of the seventy-three
            // cards Scryfall finds.
            // Cost only, unlike Phyrexian below. That asymmetry is Scryfall's
            // rather than ours: counting hybrid symbols in activation costs
            // finds a hundred and twenty-four cards it does not.
            IsProperty::Hybrid => mana_symbols(card.mana_cost)
                .any(|s| s.contains('/') && !s.to_ascii_lowercase().contains("/p")),
            IsProperty::Phyrexian => card
                .symbols()
                .any(|s| s.to_ascii_lowercase().contains("/p")),
            IsProperty::Commander => {
                legality::commander_route(card.name, card.type_line, card.oracle).is_some()
            }
            // Every flavour of the mechanic prints as its own keyword, and
            // Scryfall's `is:partner` means any of them. Enumerated rather than
            // prefix-matched on "partner", which found none of the hundred and
            // forty cards that say "Doctor's companion" or "Choose a
            // background" instead.
            IsProperty::Partner => {
                ["partner", "friends forever", "doctor's companion", "choose a background"]
                    .iter()
                    .any(|k| has_keyword(k))
                    // A Background carries no keyword of its own — it is a
                    // subtype, and it is the other half of the mechanic.
                    // Scryfall counts them, and without them this found
                    // twenty-seven fewer cards than it should.
                    || legality::is_background(card.type_line)
            }
            IsProperty::Companion => has_keyword("companion"),
            IsProperty::Reserved => card.reserved.unwrap_or(false),
            IsProperty::GameChanger => card.game_changer.unwrap_or(false),
        }
    }
}

/// The `{...}` symbols in a string, without their braces.
fn mana_symbols(text: &str) -> impl Iterator<Item = &str> {
    text.split('{')
        .skip(1)
        .filter_map(|s| s.split_once('}'))
        .map(|(sym, _)| sym)
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
