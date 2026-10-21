//! A custom card as its `.card.toml` says it, parsed into a shape whose
//! corner can only hold what the type line allows.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use facet::Facet;

use crate::rules::Frames;
pub use crate::rules::Rules;

/// One face as the file writes it: the top level is the front face (or a
/// split card's left half, the card an adventure or a prepare spell is inset
/// in, a flip card's upright half or a meld pair's first card), and
/// `[back]`, `[right]`, `[spell]`, `[flipped]`, `[counterpart]` or `[melded]`
/// another.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
pub(crate) struct RawFace {
    pub(crate) name: String,
    #[facet(default)]
    pub(crate) cost: Option<String>,
    #[facet(rename = "type")]
    pub(crate) type_line: String,
    #[facet(default)]
    pub(crate) rules: Option<String>,
    #[facet(default)]
    pub(crate) flavor: Option<String>,
    #[facet(default)]
    pub(crate) pt: Option<String>,
    #[facet(default)]
    pub(crate) loyalty: Option<String>,
    #[facet(default)]
    pub(crate) defense: Option<String>,
    #[facet(default)]
    pub(crate) color_indicator: Option<String>,
    #[facet(default)]
    pub(crate) art: Option<PathBuf>,
    #[facet(default)]
    pub(crate) artist: Option<String>,
}

/// The file as written, before any of it is checked against the rest.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
pub(crate) struct RawCard {
    #[facet(flatten)]
    pub(crate) front: RawFace,
    #[facet(default)]
    pub(crate) layout: Option<String>,
    #[facet(default)]
    pub(crate) back: Option<RawFace>,
    #[facet(default)]
    pub(crate) right: Option<RawFace>,
    #[facet(default)]
    pub(crate) spell: Option<RawFace>,
    #[facet(default)]
    pub(crate) flipped: Option<RawFace>,
    #[facet(default)]
    pub(crate) counterpart: Option<RawFace>,
    #[facet(default)]
    pub(crate) melded: Option<RawFace>,
}

impl RawCard {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        facet_toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// A card: one face, or two. Which two is the layout the file declares, and
/// each kind has exactly the faces it prints, so a back without a layout, or
/// a layout without its second face, cannot be held.
#[derive(Debug, Clone, PartialEq)]
pub enum Card {
    Single(Face),
    /// A nonmodal double-faced card: the front transforms into the back
    /// (CR 712.2).
    Transform {
        front: Face,
        back: Face,
    },
    /// A modal double-faced card: either face can be cast or played
    /// (CR 712.3).
    Modal {
        front: Face,
        back: Face,
    },
    /// Two halves on one card, each with its own cost (CR 709.1), standing
    /// as what their text and type lines say they are.
    Split {
        left: Face,
        right: Face,
        kind: SplitKind,
    },
    /// A card with an Adventure inset on the left of its text box: an
    /// instant or sorcery it may be cast as instead (CR 715.2, 715.3).
    Adventure {
        main: Face,
        spell: Face,
    },
    /// A card with a prepare spell inset on the right of its text box: an
    /// instant or sorcery a copy of which may be cast while the permanent is
    /// prepared (CR 722.2, 722.3).
    Prepare {
        main: Face,
        spell: Face,
    },
    /// A flip card: one face, the upright half's characteristics its own and
    /// the flipped half's, printed upside down below the art, its
    /// alternative ones once the permanent flips (CR 710.1).
    Flip {
        upright: Face,
        flipped: Face,
    },
    /// A meld pair: two cards, each a front face and half of the melded
    /// card on its back, which the two backs make together (CR 712.4). One
    /// file holds the pair, so half a pair cannot be held. The front at the
    /// top level is the one that melds them (CR 712.4a), and its back holds
    /// the melded card's top half, as every printed pair's does.
    Meld {
        front: Face,
        counterpart: Face,
        melded: Box<Face>,
    },
}

/// How a split card's halves stand, read from what they say rather than
/// declared, as every other frame is.
#[derive(Debug, Clone, PartialEq)]
pub enum SplitKind {
    /// Side by side on a card turned on its side.
    Halves,
    /// Side by side, each with fuse, the line printed once across the foot
    /// of both (CR 702.102a): the line, as the left half has it.
    Fused(String),
    /// The right half has aftermath (CR 702.127): the left half stands
    /// upright on the top of the card and the right lies on its side below
    /// it, read by turning the card, as every printed aftermath card's do.
    Aftermath,
    /// Two doors of one Room (CR 709.5j) under one shared type line
    /// (CR 709.5), side by side on a card turned on its side, their art one
    /// picture across both. The reminder both doors' text ends with is
    /// printed once, in a band across the card under the type line.
    Room { reminder: Option<String> },
}

/// One face of a card, as a single-faced card is its only face.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub name: String,
    /// The mana cost's symbols, each without its braces: `["2", "U", "U/P"]`.
    pub cost: Vec<String>,
    pub type_line: String,
    /// The rules text, read into the layout it asks for.
    pub rules: Rules,
    /// The colour indicator by the type line (CR 204.1), empty for none.
    pub color_indicator: Colors,
    pub flavor: Option<String>,
    pub corner: Corner,
    /// Resolved against the card file's directory.
    pub art: Option<PathBuf>,
    pub artist: Option<String>,
}

/// What the bottom-right corner prints. One of these at most, and only the one
/// the card asks for: a creature or Vehicle has power and toughness
/// (CR 208.1, 301.7a), and so does a station card in the striation where it
/// becomes a creature (CR 721.2b); a planeswalker loyalty (CR 306.5a), a battle
/// defense (CR 310.4a).
#[derive(Debug, Clone, PartialEq)]
pub enum Corner {
    None,
    PowerToughness { power: String, toughness: String },
    Loyalty(String),
    Defense(String),
}

/// The frame: the colours of the mana cost (CR 105.2) and the colour
/// indicator (CR 204.2). A land's frame is a land's, tinted by its colours or,
/// with none, by the colours of mana it adds, as printed lands are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    Mono(Color),
    Gold,
    Artifact,
    Colorless,
    Land(Colors),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
}

impl Color {
    pub const ALL: [Color; 5] = [
        Color::White,
        Color::Blue,
        Color::Black,
        Color::Red,
        Color::Green,
    ];

    pub fn letter(self) -> char {
        match self {
            Color::White => 'W',
            Color::Blue => 'U',
            Color::Black => 'B',
            Color::Red => 'R',
            Color::Green => 'G',
        }
    }

    pub fn of_letter(c: char) -> Option<Self> {
        Color::ALL.into_iter().find(|k| k.letter() == c)
    }

    fn bit(self) -> u8 {
        1 << Color::ALL.iter().position(|&k| k == self).expect("in ALL")
    }
}

/// A set of colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colors(u8);

impl Colors {
    /// `"WR"`: each of W, U, B, R and G at most once.
    pub fn of_letters(letters: &str) -> Result<Self> {
        let mut out = Colors::default();
        for c in letters.chars().filter(|c| !c.is_whitespace()) {
            let Some(color) = Color::of_letter(c.to_ascii_uppercase()) else {
                bail!("a colour is one of W, U, B, R and G, not {c:?}");
            };
            if out.contains(color) {
                bail!("{c} twice in {letters:?}");
            }
            out.insert(color);
        }
        Ok(out)
    }

    pub fn insert(&mut self, c: Color) {
        self.0 |= c.bit();
    }

    pub fn contains(self, c: Color) -> bool {
        self.0 & c.bit() != 0
    }

    pub fn union(self, other: Colors) -> Colors {
        Colors(self.0 | other.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// In the order Oracle text writes them, round the colour wheel WUBRG:
    /// from the first of a run of neighbours (W/U, G/W, Bant's G W U, a
    /// four-colour R G W U), or from a centre colour to its two enemies
    /// (W/B, R/W, Jeskai's U R W). Every colour set is one or the other.
    pub fn ordered(self) -> Vec<Color> {
        let at: Vec<usize> = (0..5).filter(|&i| self.contains(Color::ALL[i])).collect();
        let n = at.len();
        let neighbours: Vec<usize> = (0..n).collect();
        let enemies: Vec<usize> = (0..n).map(|i| 2 * i).collect();
        at.iter()
            .find_map(|&start| {
                let mut steps: Vec<usize> = at.iter().map(|&i| (i + 5 - start) % 5).collect();
                steps.sort_unstable();
                (steps == neighbours || steps == enemies)
                    .then(|| steps.iter().map(|s| Color::ALL[(start + s) % 5]).collect())
            })
            .unwrap_or_default()
    }

    fn of_symbols<'a>(symbols: impl IntoIterator<Item = &'a str>) -> Colors {
        let mut out = Colors::default();
        for s in symbols {
            for c in s.chars().filter_map(Color::of_letter) {
                out.insert(c);
            }
        }
        out
    }
}

impl Card {
    pub fn open(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, dir).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str, dir: &Path) -> Result<Self> {
        let raw = RawCard::parse(text)?;
        let face = |raw: RawFace, which: &str| {
            Face::of(raw, dir).with_context(|| format!("in the {which}"))
        };
        let layout = raw.layout.as_deref().map(str::trim);
        let tables: Vec<&str> = [
            ("back", raw.back.is_some()),
            ("right", raw.right.is_some()),
            ("spell", raw.spell.is_some()),
            ("flipped", raw.flipped.is_some()),
            ("counterpart", raw.counterpart.is_some()),
            ("melded", raw.melded.is_some()),
        ]
        .into_iter()
        .filter_map(|(key, has)| has.then_some(key))
        .collect();
        // The tables each layout keeps its other parts in.
        let wants = |l: &str| -> Option<&[&str]> {
            match l {
                "transform" | "modal" => Some(&["back"]),
                "split" => Some(&["right"]),
                "adventure" | "prepare" => Some(&["spell"]),
                "flip" => Some(&["flipped"]),
                "meld" => Some(&["counterpart", "melded"]),
                _ => None,
            }
        };
        let meld_tables = |k: &&str| matches!(*k, "counterpart" | "melded");
        match (layout, &tables[..]) {
            (None, []) => {}
            (None, ["back", ..]) => {
                bail!("a [back] needs a layout: layout = \"transform\" or \"modal\"")
            }
            (None, ["right", ..]) => bail!("a [right] half needs layout = \"split\""),
            (None, ["spell", ..]) => {
                bail!("a [spell] needs a layout: layout = \"adventure\" or \"prepare\"")
            }
            (None, ["flipped", ..]) => bail!("a [flipped] half needs layout = \"flip\""),
            (None, _) => {
                bail!("a meld pair's [counterpart] and [melded] card need layout = \"meld\"")
            }
            (Some(l), _) if wants(l).is_none() => bail!(
                "layout is \"transform\", \"modal\", \"split\", \"adventure\", \"prepare\", \
                 \"flip\" or \"meld\", not {l:?}"
            ),
            (Some("meld"), _) => {
                if let Some(other) = tables.iter().find(|k| !meld_tables(k)) {
                    bail!(
                        "a meld pair is its card, its [counterpart] and the [melded] card \
                         their backs make, not a [{other}]"
                    );
                }
                for (key, what) in [
                    ("counterpart", "the other card of the pair"),
                    ("melded", "the card the pair's backs make"),
                ] {
                    if !tables.contains(&key) {
                        bail!(
                            "a meld card is never printed without its pair: the file holds \
                             both cards and what they meld into (CR 712.4), and this one has \
                             no [{key}], {what}"
                        );
                    }
                }
            }
            (Some(_), [_, _, ..]) => bail!(
                "a card has two parts at most, a meld pair's three aside: one of [back], \
                 [right], [spell] and [flipped]"
            ),
            (Some(l), []) => match wants(l).expect("a known layout")[0] {
                "back" => bail!("a {l} card needs its [back] face"),
                "right" => bail!("a split card needs its [right] half"),
                "flipped" => bail!("a flip card needs its [flipped] half"),
                _ => bail!("an {l} card needs its [spell]"),
            },
            (Some(l), [key]) if wants(l).expect("a known layout") != [*key] => match *key {
                "back" if l == "split" => {
                    bail!("a split card's second half is [right], not [back]: it has a card back")
                }
                "back" if l == "flip" => bail!(
                    "a flip card's second half is [flipped], not [back]: both halves are on \
                     its face, and its back is a card back (CR 710.1)"
                ),
                "back" => bail!(
                    "{} {l} card's spell is [spell], not [back]: it has a card back",
                    article(l)
                ),
                _ => bail!(
                    "{} {l} card's second part is [{}], not [{key}]",
                    article(l),
                    wants(l).expect("a known layout")[0]
                ),
            },
            (Some(_), [_]) => {}
        }
        let card = match layout {
            None => Card::Single(Face::of(raw.front, dir)?),
            Some("transform") => Card::Transform {
                front: face(raw.front, "front face")?,
                back: face(raw.back.expect("checked"), "back face")?,
            },
            Some("modal") => Card::Modal {
                front: face(raw.front, "front face")?,
                back: face(raw.back.expect("checked"), "back face")?,
            },
            Some("split") => {
                let left = face(raw.front, "left half")?;
                let right = face(raw.right.expect("checked"), "right half")?;
                // Every printed split card's halves each have a cost, and
                // the card's cost is the two together (CR 709.4b).
                for half in [&left, &right] {
                    if half.cost.is_empty() {
                        bail!("{:?} is half a split card and needs a cost", half.name);
                    }
                }
                let kind = SplitKind::of(&left, &right)?;
                Card::Split { left, right, kind }
            }
            Some("flip") => Flip::check(raw.front, raw.flipped.expect("checked"), dir)?,
            Some("meld") => Meld::check(
                raw.front,
                raw.counterpart.expect("checked"),
                raw.melded.expect("checked"),
                dir,
            )?,
            Some(l) => {
                let spell = raw.spell.expect("checked");
                let main = face(raw.front, "card")?;
                let spell = Inset::check(spell, dir, l == "adventure")?;
                if main.rules.is_framed() {
                    bail!(
                        "{:?}'s text shares its box with its {}, so it is plain lines: \
                         no Saga, Class, planeswalker or station frame has room for both",
                        main.name,
                        if l == "adventure" {
                            "Adventure"
                        } else {
                            "prepare spell"
                        }
                    );
                }
                if l == "adventure" {
                    Card::Adventure { main, spell }
                } else {
                    // Prepared is a permanent's designation, and only a
                    // permanent with a prepare spell can have it (CR 722.3a).
                    if main.is_spell() {
                        bail!(
                            "{:?} is a {:?}, and only a permanent is ever prepared (CR 722.3a)",
                            main.name,
                            main.type_line
                        );
                    }
                    Card::Prepare { main, spell }
                }
            }
        };
        card.check_battles()?;
        Ok(card)
    }

    /// A battle prints sideways on a card to itself, and a Siege is cast
    /// transformed when it is defeated (CR 310.12b), so it is always a
    /// transforming card's front. A battle with no battle type, or any other
    /// type than Siege, may stand alone (CR 310.8, 310.9a).
    fn check_battles(&self) -> Result<()> {
        if let Card::Split { .. }
        | Card::Adventure { .. }
        | Card::Prepare { .. }
        | Card::Flip { .. }
        | Card::Meld { .. } = self
        {
            if let Some(battle) = self.faces().into_iter().find(|f| f.is_battle()) {
                bail!(
                    "{:?} is a battle, which prints sideways on a card to itself: \
                     no half of a split or flip card, no text box shared with a spell, \
                     and no part of a meld pair",
                    battle.name
                );
            }
        }
        let front = match self {
            Card::Transform { front, .. } => Some(front),
            _ => None,
        };
        for face in self.faces() {
            if face.is_siege() && !front.is_some_and(|f| std::ptr::eq(f, face)) {
                bail!(
                    "{:?} is a Siege, cast transformed when it is defeated (CR 310.12b), \
                     so it is the front face of a layout = \"transform\" card",
                    face.name
                );
            }
        }
        Ok(())
    }

    /// The faces it prints, front (or left) first.
    pub fn faces(&self) -> Vec<&Face> {
        match self {
            Card::Single(f) => vec![f],
            Card::Transform { front, back } | Card::Modal { front, back } => vec![front, back],
            Card::Split { left, right, .. } => vec![left, right],
            Card::Adventure { main, spell } | Card::Prepare { main, spell } => vec![main, spell],
            Card::Flip { upright, flipped } => vec![upright, flipped],
            Card::Meld {
                front,
                counterpart,
                melded,
            } => vec![front, counterpart, melded],
        }
    }

    /// How many card images it prints: a double-faced card's two faces are
    /// two, a split or flip card's halves share one, and a meld pair is two
    /// cards of two faces each.
    pub fn images(&self) -> usize {
        self.image_names().len()
    }

    /// What each card image is, in the order they print: the front's a
    /// `PNG` is named for, and each other's the word it is named with
    /// beside it, `x.back.png`.
    pub fn image_names(&self) -> &'static [&'static str] {
        match self {
            Card::Single(_)
            | Card::Split { .. }
            | Card::Adventure { .. }
            | Card::Prepare { .. }
            | Card::Flip { .. } => &[""],
            Card::Transform { .. } | Card::Modal { .. } => &["", "back"],
            Card::Meld { .. } => &["", "back", "counterpart", "counterpart.back"],
        }
    }

    /// Both names, `Far // Away` or `Bonecrusher Giant // Stomp`, as Oracle
    /// writes a card of two (CR 709.4a, 712.19); a meld pair is two cards,
    /// and the one they meld into: `Bruna + Gisela → Brisela`.
    pub fn name(&self) -> String {
        if let Card::Meld {
            front,
            counterpart,
            melded,
        } = self
        {
            return format!("{} + {} → {}", front.name, counterpart.name, melded.name);
        }
        self.faces()
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(" // ")
    }

    /// A split card whose halves both have fuse prints the keyword once,
    /// on a bar across both (CR 702.102a): the line, as the left half has it.
    pub fn fuse(&self) -> Option<&str> {
        match self {
            Card::Split {
                kind: SplitKind::Fused(line),
                ..
            } => Some(line),
            _ => None,
        }
    }
}

impl SplitKind {
    /// What two halves are, or why no split card is printed so.
    fn of(left: &Face, right: &Face) -> Result<Self> {
        let keyword = |f: &Face, is: fn(&str) -> bool| f.rules.lines().into_iter().find(|l| is(l));
        let (fuse, aftermath) = (
            [keyword(left, is_fuse), keyword(right, is_fuse)],
            [keyword(left, is_aftermath), keyword(right, is_aftermath)],
        );
        if aftermath[0].is_some() {
            bail!(
                "{:?} is the left half, and aftermath is the right half's: the half cast \
                 from a graveyard is the one printed on its side under the other, as on \
                 every printed aftermath card (CR 702.127a)",
                left.name
            );
        }
        if left.is_room() || right.is_room() {
            return Self::room(
                left,
                right,
                fuse.iter().any(Option::is_some) || aftermath[1].is_some(),
            );
        }
        if aftermath[1].is_some() {
            if fuse.iter().any(Option::is_some) {
                bail!(
                    "{:?} has aftermath, cast only from a graveyard (CR 702.127a), so no \
                     fuse can cast it from a hand with the other half (CR 702.102a)",
                    right.name
                );
            }
            for half in [left, right] {
                if !half.is_spell() {
                    bail!(
                        "{:?} is half an aftermath card, an instant or a sorcery as every \
                         printed one's halves are, not a {:?}",
                        half.name,
                        half.type_line
                    );
                }
                if half.rules.is_framed() {
                    bail!(
                        "{:?} is half an aftermath card, a few plain lines: it has no frame \
                         of its own",
                        half.name
                    );
                }
            }
            return Ok(SplitKind::Aftermath);
        }
        Ok(match fuse {
            [Some(line), Some(_)] => SplitKind::Fused(line),
            _ => SplitKind::Halves,
        })
    }

    /// Two doors of one Room: both Room enchantments under one shared type
    /// line (CR 709.5a, 205.3h), plain text with no corner, and the line
    /// both end with lifted into the band if it is reminder text.
    fn room(left: &Face, right: &Face, keyword: bool) -> Result<Self> {
        for door in [left, right] {
            if !(door.is_room() && door.has_type("Enchantment")) {
                bail!(
                    "{:?} is half of a Room, so it is an \"Enchantment — Room\" as the \
                     other half is: a Room's halves share one type line (CR 709.5a), not {:?}",
                    door.name,
                    door.type_line
                );
            }
        }
        if left.type_line.trim() != right.type_line.trim() {
            bail!(
                "a Room's doors share one type line (CR 709.5a), but {:?} says {:?} and {:?} \
                 says {:?}",
                left.name,
                left.type_line,
                right.name,
                right.type_line
            );
        }
        for door in [left, right] {
            if door.corner != Corner::None {
                bail!("{:?} is a Room's door, which has no corner", door.name);
            }
            if door.rules.is_framed() {
                bail!(
                    "{:?} is a Room's door, a few plain lines in a box of its own: it has \
                     no frame of its own",
                    door.name
                );
            }
        }
        if right.art.is_some() {
            bail!(
                "a Room's art is one picture across both doors, so it is the left half's \
                 art, and {:?} has none of its own",
                right.name
            );
        }
        if keyword {
            bail!(
                "a Room is a permanent, each door unlocked on its own (CR 709.5e): no fuse \
                 or aftermath has a place on it"
            );
        }
        let (l, r) = (left.rules.lines(), right.rules.lines());
        let reminder = match (l.last(), r.last()) {
            (Some(a), Some(b)) if a == b && a.starts_with('(') && a.ends_with(')') => {
                Some(a.clone())
            }
            _ => None,
        };
        Ok(SplitKind::Room { reminder })
    }
}

fn article(layout: &str) -> &'static str {
    if layout.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// The spell inset in a card's text box, an Adventure or a prepare spell:
/// checked as a face is, and then as printed ones are.
struct Inset;

impl Inset {
    fn check(raw: RawFace, dir: &Path, adventure: bool) -> Result<Face> {
        let what = if adventure {
            "Adventure"
        } else {
            "prepare spell"
        };
        // It is drawn in the card's text box, under the card's art and
        // artist, and every printed one is coloured by its cost.
        for (field, set) in [
            ("art", raw.art.is_some()),
            ("artist", raw.artist.is_some()),
            ("color_indicator", raw.color_indicator.is_some()),
        ] {
            if set {
                bail!(
                    "{:?} is the card's {what}, drawn in its text box, and has no {field} of its own",
                    raw.name
                );
            }
        }
        let face = Face::of(raw, dir).with_context(|| format!("in the {what}"))?;
        let words: Vec<&str> = face.type_line.split_whitespace().collect();
        let (types, subtypes) = match words.iter().position(|w| *w == "—") {
            Some(at) => (&words[..at], &words[at + 1..]),
            None => (&words[..], &words[..0]),
        };
        let instant_or_sorcery = matches!(types, ["Instant"] | ["Sorcery"]);
        let is_adventure = subtypes.contains(&"Adventure");
        if adventure && !(instant_or_sorcery && is_adventure) {
            // An Adventure is an instant or sorcery spell with the Adventure
            // spell type (CR 715.2, 205.3k).
            bail!(
                "{:?} is an Adventure, an instant or sorcery with the Adventure subtype \
                 like \"Sorcery — Adventure\", not a {:?}",
                face.name,
                face.type_line
            );
        }
        if !adventure && !instant_or_sorcery {
            bail!(
                "{:?} is a prepare spell, an instant or a sorcery, not a {:?}",
                face.name,
                face.type_line
            );
        }
        if !adventure && is_adventure {
            bail!(
                "{:?} has the Adventure subtype, so the card is an adventurer: layout = \"adventure\"",
                face.name
            );
        }
        // Both are cast for their own cost (CR 715.3a, 722.3c), and every
        // printed one has one.
        if face.cost.is_empty() {
            bail!("{:?} is the card's {what} and needs a cost", face.name);
        }
        if face.rules.is_framed() {
            bail!(
                "{:?} is the card's {what}, a few plain lines: it has no frame of its own",
                face.name
            );
        }
        Ok(face)
    }
}

/// A flip card's halves, checked as faces are and then as printed ones
/// are: both share one face, so the upright half holds what the card is
/// everywhere (its cost, colour, art and artist, CR 710.1c, 710.2) and the
/// flipped half only its name, type line, text and power and toughness
/// (CR 710.1b).
struct Flip;

impl Flip {
    fn check(upright: RawFace, flipped: RawFace, dir: &Path) -> Result<Card> {
        for (field, set, why) in [
            (
                "cost",
                flipped.cost.is_some(),
                "a flip card's mana cost is its upright half's, and flipping never changes it (CR 710.1c)",
            ),
            (
                "color_indicator",
                flipped.color_indicator.is_some(),
                "a flip card's colour is its upright half's, and flipping never changes it (CR 710.1c)",
            ),
            (
                "art",
                flipped.art.is_some(),
                "the art is one picture between the halves, the upright half's",
            ),
            (
                "artist",
                flipped.artist.is_some(),
                "the art is one picture between the halves, credited on the upright half",
            ),
        ] {
            if set {
                bail!("{:?} is the flipped half, with no {field} of its own: {why}", flipped.name);
            }
        }
        let upright = Face::of(upright, dir).context("in the upright half")?;
        let flipped = Face::of(flipped, dir).context("in the flipped half")?;
        for half in [&upright, &flipped] {
            // Only a permanent flips: the flipped half is what it is once
            // flipped on the battlefield (CR 710.1b, 710.2).
            if half.is_spell() {
                bail!(
                    "{:?} is half a flip card, which only a permanent on the battlefield \
                     ever flips (CR 710.2), not a {:?}",
                    half.name,
                    half.type_line
                );
            }
            if half.rules.is_framed() {
                bail!(
                    "{:?} is half a flip card, a few plain lines in a box of its own: no \
                     Saga, Class, planeswalker or station frame fits it",
                    half.name
                );
            }
            if matches!(half.corner, Corner::Loyalty(_) | Corner::Defense(_)) {
                bail!(
                    "{:?} is half a flip card, whose type line's end holds power and \
                     toughness or nothing: no loyalty or defense",
                    half.name
                );
            }
        }
        Ok(Card::Flip { upright, flipped })
    }
}

/// A meld pair, checked as its faces are and then as printed pairs are.
struct Meld;

impl Meld {
    fn check(front: RawFace, counterpart: RawFace, melded: RawFace, dir: &Path) -> Result<Card> {
        if melded.cost.is_some() {
            bail!(
                "{:?} is the melded card, which has no mana cost: its mana value is its \
                 fronts' together (CR 202.3c)",
                melded.name
            );
        }
        let front = Face::of(front, dir).context("in the card")?;
        let counterpart = Face::of(counterpart, dir).context("in the counterpart")?;
        let melded = Face::of(melded, dir).context("in the melded card")?;
        // Melding puts both cards onto the battlefield as one permanent
        // (CR 712.4a, 701.42a), so all three are permanents.
        for face in [&front, &counterpart, &melded] {
            if face.is_spell() {
                bail!(
                    "{:?} is part of a meld pair, which melds into a permanent on the \
                     battlefield (CR 712.4a), not a {:?}",
                    face.name,
                    face.type_line
                );
            }
        }
        // One card of the pair melds them (CR 712.4a), and every printed one
        // says so, naming the card they meld into; it is the top level here,
        // its back the melded card's top half, as printed pairs' is.
        let melds = |f: &Face| {
            f.rules
                .lines()
                .iter()
                .any(|l| l.to_lowercase().contains("meld") && l.contains(melded.name.as_str()))
        };
        if !melds(&front) {
            if melds(&counterpart) {
                bail!(
                    "{:?} melds the pair, so it is the card at the top level, its back the \
                     melded card's top half as printed pairs' is, and {:?} is the \
                     [counterpart]",
                    counterpart.name,
                    front.name
                );
            }
            bail!(
                "one card of a meld pair melds them (CR 712.4a): {:?} needs an ability that \
                 melds them into {:?}, naming it, as every printed pair's has",
                front.name,
                melded.name
            );
        }
        Ok(Card::Meld {
            front,
            counterpart,
            melded: Box::new(melded),
        })
    }
}

/// `Fuse`, alone or with its reminder.
pub(crate) fn is_fuse(line: &str) -> bool {
    is_keyword(line, "Fuse")
}

/// `Aftermath`, alone or with its reminder.
fn is_aftermath(line: &str) -> bool {
    is_keyword(line, "Aftermath")
}

fn is_keyword(line: &str, keyword: &str) -> bool {
    line.strip_prefix(keyword)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '(']))
}

impl Face {
    fn of(raw: RawFace, dir: &Path) -> Result<Self> {
        let has = |word: &str| raw.type_line.split_whitespace().any(|w| w == word);
        let rules = Rules::parse(
            raw.rules
                .as_deref()
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect(),
            Frames::of(&raw.type_line),
        )?;
        let stationed = matches!(rules, Rules::Station(_));
        let corner = match (raw.pt, raw.loyalty, raw.defense) {
            (None, None, None) => Corner::None,
            (Some(pt), None, None) => {
                let Some((power, toughness)) = pt.split_once('/') else {
                    bail!("pt is power/toughness, like \"2/3\", not {pt:?}");
                };
                Corner::PowerToughness {
                    power: power.trim().into(),
                    toughness: toughness.trim().into(),
                }
            }
            (None, Some(l), None) => Corner::Loyalty(l),
            (None, None, Some(d)) => Corner::Defense(d),
            _ => bail!("a card has at most one of pt, loyalty and defense"),
        };
        let wanted = match &corner {
            Corner::None => None,
            Corner::PowerToughness { .. } => {
                Some(("pt", has("Creature") || has("Vehicle") || stationed))
            }
            Corner::Loyalty(_) => Some(("loyalty", has("Planeswalker"))),
            Corner::Defense(_) => Some(("defense", has("Battle"))),
        };
        if let Some((field, false)) = wanted {
            bail!(
                "{field} on a {:?}, whose type line has no use for it",
                raw.type_line
            );
        }
        if has("Battle") {
            // Every battle prints its defense in its corner, so the corner
            // holds nothing else (CR 310.4a).
            if !matches!(corner, Corner::Defense(_)) {
                bail!(
                    "a battle needs defense, printed in its corner (CR 310.4a){}",
                    if has("Creature") || has("Planeswalker") {
                        ", so it is no creature or planeswalker as well"
                    } else {
                        ""
                    }
                );
            }
            if rules.is_framed() {
                bail!(
                    "a battle's text box is plain lines, beside its art on a card turned \
                     on its side: no Saga, Class, planeswalker or station frame fits it"
                );
            }
        }
        if has("Creature") && !matches!(corner, Corner::PowerToughness { .. }) {
            bail!("a creature needs pt");
        }
        Ok(Face {
            cost: match raw.cost {
                Some(c) => symbols_of(&c)?,
                None => Vec::new(),
            },
            name: raw.name,
            type_line: raw.type_line,
            rules,
            color_indicator: match raw.color_indicator {
                Some(c) => Colors::of_letters(&c).map_err(|e| anyhow!("color_indicator: {e}"))?,
                None => Colors::default(),
            },
            flavor: raw.flavor,
            corner,
            art: raw.art.map(|a| dir.join(a)),
            artist: raw.artist,
        })
    }

    /// An instant or a sorcery: a card that is never a permanent.
    pub fn is_spell(&self) -> bool {
        self.has_type("Instant") || self.has_type("Sorcery")
    }

    fn has_type(&self, word: &str) -> bool {
        let types = self
            .type_line
            .split_once('—')
            .map_or(&*self.type_line, |(t, _)| t);
        types.split_whitespace().any(|w| w == word)
    }

    /// A door of a Room, the enchantment type (CR 205.3h).
    pub fn is_room(&self) -> bool {
        let subtypes = self.type_line.split_once('—').map_or("", |(_, s)| s);
        subtypes.split_whitespace().any(|w| w == "Room")
    }

    /// A battle prints on a card turned on its side, its defense in a
    /// shield in the corner (CR 310).
    pub fn is_battle(&self) -> bool {
        self.type_line.split_whitespace().any(|w| w == "Battle")
    }

    /// A battle of the Siege type (CR 310.12).
    pub fn is_siege(&self) -> bool {
        let subtypes = self.type_line.split_once('—').map_or("", |(_, s)| s);
        self.is_battle() && subtypes.split_whitespace().any(|w| w == "Siege")
    }

    /// The face's colours: its cost's and its colour indicator's (CR 202.2,
    /// 204).
    pub fn colors(&self) -> Colors {
        Colors::of_symbols(self.cost.iter().map(String::as_str)).union(self.color_indicator)
    }

    pub fn frame(&self) -> Frame {
        let colors = self.colors();
        let has = |word: &str| self.type_line.split_whitespace().any(|w| w == word);
        if has("Land") {
            return Frame::Land(if colors.is_empty() {
                self.mana_made()
            } else {
                colors
            });
        }
        match colors.ordered().as_slice() {
            [] if has("Artifact") => Frame::Artifact,
            [] => Frame::Colorless,
            [one] => Frame::Mono(*one),
            _ => Frame::Gold,
        }
    }

    /// The mana ability a land prints, `{T}: Add {G}.`, to its first full
    /// stop: what a modal double-faced card's hint names of a land face
    /// (CR 712.3c).
    pub fn mana_ability(&self) -> Option<String> {
        self.rules.lines().into_iter().find_map(|line| {
            let at = line.find("{T}: Add ")?;
            let rest = &line[at..];
            Some(match rest.find('.') {
                Some(stop) => rest[..=stop].to_string(),
                None => rest.trim_end_matches(')').to_string(),
            })
        })
    }

    /// The colours of mana the rules text adds: the symbols after "Add" up to
    /// the end of its sentence, in `{T}: Add {G}.` and `({T}: Add {B} or {G}.)`.
    fn mana_made(&self) -> Colors {
        let mut out = Colors::default();
        for line in self.rules.lines() {
            for (i, _) in line.match_indices("Add ") {
                let sentence = line[i..].split('.').next().unwrap_or_default();
                let symbols = sentence
                    .split('{')
                    .skip(1)
                    .filter_map(|s| s.split_once('}').map(|(sym, _)| sym));
                out = out.union(Colors::of_symbols(symbols));
            }
        }
        out
    }
}

/// `{2}{U}{U/P}` as `["2", "U", "U/P"]`. A cost is nothing but symbols.
fn symbols_of(cost: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = cost.trim();
    while !rest.is_empty() {
        let Some(inner) = rest.strip_prefix('{').and_then(|r| r.split_once('}')) else {
            bail!("a cost is symbols in braces, like {{2}}{{U}}; {cost:?} is not");
        };
        out.push(inner.0.to_string());
        rest = inner.1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Face> {
        match Card::parse(text, Path::new("."))? {
            Card::Single(face) => Ok(face),
            other => panic!("one face expected, not {other:?}"),
        }
    }

    fn card(text: &str) -> Result<Card> {
        Card::parse(text, Path::new("."))
    }

    #[test]
    fn a_creature_reads_its_cost_rules_and_corner() {
        let card = parse(
            r#"
name = "Proofsheet Drone"
cost = "{2}{U}{U/P}"
type = "Artifact Creature — Phyrexian Drone"
rules = """
Flying
{1}{U}, {T}: Scry 1.
"""
pt = "2/3"
"#,
        )
        .unwrap();
        assert_eq!(card.cost, ["2", "U", "U/P"]);
        assert_eq!(
            card.rules,
            Rules::Plain(vec!["Flying".into(), "{1}{U}, {T}: Scry 1.".into()])
        );
        assert_eq!(
            card.corner,
            Corner::PowerToughness {
                power: "2".into(),
                toughness: "3".into()
            }
        );
        assert_eq!(card.frame(), Frame::Mono(Color::Blue));
    }

    #[test]
    fn the_corner_is_the_one_the_type_line_asks_for() {
        assert!(parse("name = \"x\"\ntype = \"Creature — Elf\"").is_err());
        assert!(parse("name = \"x\"\ntype = \"Sorcery\"\npt = \"1/1\"").is_err());
        assert!(parse("name = \"x\"\ntype = \"Artifact — Vehicle\"\npt = \"3/3\"").is_ok());
        assert!(
            parse("name = \"x\"\ntype = \"Creature — Elf\"\npt = \"1/1\"\nloyalty = \"3\"")
                .is_err()
        );
        assert!(
            parse("name = \"x\"\ntype = \"Legendary Planeswalker — Jace\"\nloyalty = \"3\"")
                .is_ok()
        );
    }

    #[test]
    fn two_colours_are_gold_and_none_is_by_type() {
        let frame = |cost: &str, ty: &str| {
            parse(&format!("name = \"x\"\ncost = \"{cost}\"\ntype = \"{ty}\""))
                .unwrap()
                .frame()
        };
        assert_eq!(frame("{1}{W}{U}", "Instant"), Frame::Gold);
        assert_eq!(frame("{W/U}", "Instant"), Frame::Gold);
        assert_eq!(frame("{3}", "Artifact"), Frame::Artifact);
        assert_eq!(frame("{R/P}", "Instant"), Frame::Mono(Color::Red));
    }

    #[test]
    fn a_spacecraft_prints_the_pt_it_has_at_its_threshold() {
        // Exploration Broodship prints 4/4 (CR 721.2b).
        let broodship = "name = \"x\"\ncost = \"{G}\"\ntype = \"Artifact — Spacecraft\"\npt = \"4/4\"\nrules = \"\"\"\nStation (Tap another creature you control.)\n3+ | You may play an additional land on each of your turns.\n8+ | Flying\n\"\"\"";
        assert!(parse(broodship).is_ok());
        let unstationed = "name = \"x\"\ntype = \"Artifact — Spacecraft\"\npt = \"4/4\"";
        assert!(parse(unstationed).is_err());
    }

    #[test]
    fn a_colour_indicator_colours_a_card_with_no_cost() {
        let card = |indicator: &str| {
            parse(&format!(
                "name = \"x\"\ntype = \"Creature — Human\"\npt = \"4/4\"\ncolor_indicator = \"{indicator}\""
            ))
        };
        assert_eq!(card("G").unwrap().frame(), Frame::Mono(Color::Green));
        assert_eq!(card("WR").unwrap().frame(), Frame::Gold);
        assert!(card("WW").is_err());
        assert!(card("K").is_err());
    }

    #[test]
    fn a_land_is_tinted_by_the_mana_it_adds() {
        let land = |rules: &str| {
            parse(&format!(
                "name = \"x\"\ntype = \"Land\"\nrules = \"\"\"\n{rules}\n\"\"\""
            ))
            .unwrap()
            .frame()
        };
        let colors = |l: &str| Colors::of_letters(l).unwrap();
        assert_eq!(land("{T}: Add {G}."), Frame::Land(colors("G")));
        assert_eq!(
            land("({T}: Add {B} or {G}.)\nAs this land enters, you may pay 2 life."),
            Frame::Land(colors("BG"))
        );
        assert_eq!(land("{T}: Add {C}."), Frame::Land(colors("")));
        // Gemstone Caverns: "any color" names no colour.
        assert_eq!(
            land("{T}: Add {C}. If this land has a luck counter on it, instead add one mana of any color."),
            Frame::Land(colors(""))
        );
        let ordered = |letters: &str| -> String {
            colors(letters)
                .ordered()
                .into_iter()
                .map(Color::letter)
                .collect()
        };
        for (set, printed) in [
            ("WG", "GW"),
            ("WB", "WB"),
            ("WR", "RW"),
            ("UG", "GU"),
            ("WUG", "GWU"),
            ("WRG", "RGW"),
            ("WUR", "URW"),
            ("WBG", "WBG"),
            ("UBG", "BGU"),
            ("WURG", "RGWU"),
            ("WUBRG", "WUBRG"),
            ("", ""),
        ] {
            assert_eq!(ordered(set), printed, "{set}");
        }
    }

    const FAR: &str = "name = \"Far\"\ncost = \"{1}{U}\"\ntype = \"Instant\"\nrules = \"\"\"\nReturn target creature to its owner's hand.\nFuse (You may cast one or both halves of this card from your hand.)\n\"\"\"\nlayout = \"split\"\n";
    const AWAY: &str = "[right]\nname = \"Away\"\ncost = \"{2}{B}\"\ntype = \"Instant\"\nrules = \"\"\"\nTarget player sacrifices a creature of their choice.\nFuse (You may cast one or both halves of this card from your hand.)\n\"\"\"\n";

    #[test]
    fn a_second_face_and_its_layout_come_together() {
        let front = "name = \"Delver of Secrets\"\ncost = \"{U}\"\ntype = \"Creature — Human Wizard\"\npt = \"1/1\"\n";
        let back = "[back]\nname = \"Insectile Aberration\"\ntype = \"Creature — Human Insect\"\npt = \"3/2\"\ncolor_indicator = \"U\"\n";
        let err = |text: &str| format!("{:#}", card(text).unwrap_err());
        assert!(matches!(
            card(&format!("{front}layout = \"transform\"\n{back}")).unwrap(),
            Card::Transform { .. }
        ));
        assert!(matches!(
            card(&format!("{front}layout = \"modal\"\n{back}")).unwrap(),
            Card::Modal { .. }
        ));
        assert!(err(&format!("{front}{back}")).contains("needs a layout"));
        assert!(err(&format!("{front}layout = \"transform\"\n")).contains("needs its [back]"));
        assert!(err(&format!("{front}layout = \"split\"\n{back}")).contains("[right], not [back]"));
        assert!(err(&format!("{front}layout = \"flip\"\n{back}")).contains("[flipped], not [back]"));
        assert!(err(&format!("{front}layout = \"omen\"\n{back}")).contains("not \"omen\""));
        // A face is checked as a card is: the back's creature needs its pt.
        let no_pt = back.replace("pt = \"3/2\"\n", "");
        assert!(err(&format!("{front}layout = \"transform\"\n{no_pt}")).contains("back face"));
    }

    #[test]
    fn a_transform_back_may_have_a_cost() {
        // Plant a Sapling // Fully-Grown Treefolk's back costs {G}.
        let text = "name = \"Plant a Sapling\"\ncost = \"{G}\"\ntype = \"Sorcery\"\nlayout = \"transform\"\n[back]\nname = \"Fully-Grown Treefolk\"\ncost = \"{G}\"\ntype = \"Creature — Treefolk\"\npt = \"3/3\"\n";
        assert!(card(text).is_ok());
    }

    #[test]
    fn split_halves_each_need_a_cost_and_fuse_is_read_from_both() {
        let far_away = card(&format!("{FAR}{AWAY}")).unwrap();
        assert_eq!(far_away.name(), "Far // Away");
        assert!(far_away.fuse().is_some());
        // Fuse on one half only is not a fused card.
        let one = AWAY.replace(
            "Fuse (You may cast one or both halves of this card from your hand.)\n",
            "",
        );
        assert_eq!(card(&format!("{FAR}{one}")).unwrap().fuse(), None);
        let costless = AWAY.replace("cost = \"{2}{B}\"\n", "");
        assert!(
            format!("{:#}", card(&format!("{FAR}{costless}")).unwrap_err())
                .contains("needs a cost")
        );
    }

    const CUT: &str = "name = \"Cut\"\ncost = \"{1}{R}\"\ntype = \"Sorcery\"\nrules = \"Cut deals 4 damage to target creature.\"\nlayout = \"split\"\n";
    const RIBBONS: &str = "[right]\nname = \"Ribbons\"\ncost = \"{X}{B}{B}\"\ntype = \"Sorcery\"\nrules = \"\"\"\nAftermath (Cast this spell only from your graveyard. Then exile it.)\nEach opponent loses X life.\n\"\"\"\n";

    #[test]
    fn aftermath_is_read_from_the_right_half_and_refused_where_print_has_none() {
        let split = |text: &str| match card(text).unwrap() {
            Card::Split { kind, .. } => kind,
            other => panic!("a split card expected, not {other:?}"),
        };
        let err = |text: String| format!("{:#}", card(&text).unwrap_err());
        assert_eq!(split(&format!("{CUT}{RIBBONS}")), SplitKind::Aftermath);
        // On the left half, the one printed upright, it is refused.
        let left = CUT.replace(
            "rules = \"Cut",
            "rules = \"Aftermath (Cast this spell only from your graveyard. Then exile it.)\\nCut",
        );
        let plain = RIBBONS.replace(
            "Aftermath (Cast this spell only from your graveyard. Then exile it.)\n",
            "",
        );
        assert!(err(format!("{left}{plain}")).contains("aftermath is the right half's"));
        // Fuse casts from a hand, which an aftermath half never is.
        let fused = RIBBONS.replace("Each opponent", "Fuse\nEach opponent");
        assert!(err(format!("{CUT}{fused}")).contains("no fuse"));
        // Every printed aftermath card's halves are instants or sorceries.
        let creature = CUT.replace(
            "type = \"Sorcery\"",
            "type = \"Creature — Ogre\"\npt = \"2/2\"",
        );
        assert!(err(format!("{creature}{RIBBONS}")).contains("an instant or a sorcery"));
    }

    const POOL: &str = "name = \"Bottomless Pool\"\ncost = \"{U}\"\ntype = \"Enchantment — Room\"\nrules = \"\"\"\nWhen you unlock this door, return up to one target creature to its owner's hand.\n(You may cast either half. That door unlocks on the battlefield. As a sorcery, you may pay the mana cost of a locked door to unlock it.)\n\"\"\"\nlayout = \"split\"\n";
    const LOCKER: &str = "[right]\nname = \"Locker Room\"\ncost = \"{4}{U}\"\ntype = \"Enchantment — Room\"\nrules = \"\"\"\nWhenever one or more creatures you control deal combat damage to a player, draw a card.\n(You may cast either half. That door unlocks on the battlefield. As a sorcery, you may pay the mana cost of a locked door to unlock it.)\n\"\"\"\n";

    #[test]
    fn a_room_is_two_enchantment_rooms_under_one_type_line() {
        let Card::Split { kind, .. } = card(&format!("{POOL}{LOCKER}")).unwrap() else {
            panic!("a Room is a split card");
        };
        assert_eq!(
            kind,
            SplitKind::Room {
                reminder: Some("(You may cast either half. That door unlocks on the battlefield. As a sorcery, you may pay the mana cost of a locked door to unlock it.)".into())
            }
        );
        // Without the same reminder on both doors, none is lifted.
        let bare = LOCKER.replace("(You may cast either half. That door unlocks on the battlefield. As a sorcery, you may pay the mana cost of a locked door to unlock it.)\n", "");
        let Card::Split { kind, .. } = card(&format!("{POOL}{bare}")).unwrap() else {
            panic!("a Room is a split card");
        };
        assert_eq!(kind, SplitKind::Room { reminder: None });
        let err = |text: String| format!("{:#}", card(&text).unwrap_err());
        let instant = LOCKER.replace("type = \"Enchantment — Room\"", "type = \"Instant\"");
        assert!(
            err(format!("{POOL}{instant}")).contains("\"Enchantment — Room\" as the other half is")
        );
        let artifact = LOCKER.replace(
            "type = \"Enchantment — Room\"",
            "type = \"Artifact — Room\"",
        );
        assert!(err(format!("{POOL}{artifact}")).contains("share one type line"));
        let legendary = LOCKER.replace("type = \"Enchantment", "type = \"Legendary Enchantment");
        assert!(err(format!("{POOL}{legendary}")).contains("share one type line"));
        let art = LOCKER.replace("[right]\n", "[right]\nart = \"x.png\"\n");
        assert!(err(format!("{POOL}{art}")).contains("one picture across both doors"));
        let fused = LOCKER.replace("Whenever", "Fuse\nWhenever");
        assert!(err(format!("{POOL}{fused}")).contains("no fuse or aftermath"));
    }

    const GIANT: &str = "name = \"Bonecrusher Giant\"\ncost = \"{2}{R}\"\ntype = \"Creature — Giant\"\npt = \"4/3\"\n";
    const STOMP: &str = "name = \"Stomp\"\ncost = \"{1}{R}\"\ntype = \"Instant — Adventure\"\nrules = \"Damage can't be prevented this turn. Stomp deals 2 damage to any target.\"\n";
    const EATER: &str = "name = \"Adventurous Eater\"\ncost = \"{2}{B}\"\ntype = \"Creature — Human Warlock\"\npt = \"3/2\"\nrules = \"This creature enters prepared.\"\n";
    const BITE: &str = "name = \"Have a Bite\"\ncost = \"{B}\"\ntype = \"Sorcery\"\nrules = \"Put a +1/+1 counter on target creature. You gain 1 life.\"\n";

    #[test]
    fn an_adventure_and_a_prepare_spell_are_inset_spells() {
        let giant = card(&format!("{GIANT}layout = \"adventure\"\n[spell]\n{STOMP}")).unwrap();
        assert!(matches!(giant, Card::Adventure { .. }));
        assert_eq!(giant.name(), "Bonecrusher Giant // Stomp");
        assert_eq!(giant.images(), 1);
        let eater = card(&format!("{EATER}layout = \"prepare\"\n[spell]\n{BITE}")).unwrap();
        assert!(matches!(eater, Card::Prepare { .. }));
        // Twice Upon a Time is an adventurer sorcery, and the Virtues
        // enchantments: an adventurer is any card.
        let sorcery = GIANT
            .replace("Creature — Giant", "Sorcery")
            .replace("pt = \"4/3\"\n", "");
        assert!(card(&format!(
            "{sorcery}layout = \"adventure\"\n[spell]\n{STOMP}"
        ))
        .is_ok());
    }

    #[test]
    fn an_inset_spell_that_does_not_fit_is_refused_by_name() {
        let err = |text: String| format!("{:#}", card(&text).unwrap_err());
        let adventure = |spell: &str| format!("{GIANT}layout = \"adventure\"\n[spell]\n{spell}");
        let prepare =
            |main: &str, spell: &str| format!("{main}layout = \"prepare\"\n[spell]\n{spell}");
        assert!(err(format!("{GIANT}layout = \"adventure\"\n")).contains("needs its [spell]"));
        assert!(err(format!("{GIANT}[spell]\n{STOMP}")).contains("needs a layout"));
        assert!(
            err(format!("{GIANT}layout = \"adventure\"\n[back]\n{STOMP}"))
                .contains("[spell], not [back]")
        );
        assert!(
            err(format!("{GIANT}layout = \"transform\"\n[spell]\n{STOMP}"))
                .contains("[back], not [spell]")
        );
        assert!(err(format!("{GIANT}layout = \"split\"\n[spell]\n{STOMP}"))
            .contains("[right], not [spell]"));
        assert!(err(format!(
            "{GIANT}layout = \"adventure\"\n[spell]\n{STOMP}[back]\n{STOMP}"
        ))
        .contains("two parts at most"));
        // An Adventure is an instant or sorcery with the Adventure subtype.
        assert!(err(adventure(&STOMP.replace(" — Adventure", ""))).contains("Adventure subtype"));
        assert!(err(adventure(&STOMP.replace("Instant", "Enchantment"))).contains("not a"));
        assert!(err(adventure(&STOMP.replace("Adventure", "Omen"))).contains("Adventure subtype"));
        assert!(err(adventure(&STOMP.replace("cost = \"{1}{R}\"\n", ""))).contains("needs a cost"));
        assert!(err(adventure(&format!("{STOMP}art = \"stomp.png\"\n"))).contains("no art"));
        // A prepare spell is an instant or a sorcery, and no Adventure.
        assert!(err(prepare(
            EATER,
            &BITE.replace(
                "type = \"Sorcery\"",
                "type = \"Creature — Bear\"\npt = \"2/2\""
            )
        ))
        .contains("prepare spell"));
        assert!(err(prepare(EATER, STOMP)).contains("layout = \"adventure\""));
        // Only a permanent is ever prepared.
        let instant = EATER
            .replace("Creature — Human Warlock", "Instant")
            .replace("pt = \"3/2\"\n", "");
        assert!(err(prepare(&instant, BITE)).contains("only a permanent"));
        // The two share a text box, so neither is a framed layout.
        let saga = "name = \"x\"\ncost = \"{1}{R}\"\ntype = \"Enchantment — Saga\"\nrules = \"\"\"\nI — Draw a card.\nII — Draw a card.\n\"\"\"\n";
        assert!(
            err(format!("{saga}layout = \"adventure\"\n[spell]\n{STOMP}")).contains("plain lines")
        );
    }

    #[test]
    fn a_battle_has_defense_and_a_siege_transforms() {
        let err = |text: &str| format!("{:#}", card(text).unwrap_err());
        const SIEGE: &str =
            "name = \"Siege\"\ncost = \"{2}{W}\"\ntype = \"Battle — Siege\"\ndefense = \"4\"\n";
        const BACK: &str =
            "name = \"Back\"\ntype = \"Creature — Human\"\ncolor_indicator = \"W\"\npt = \"3/3\"\n";
        assert!(matches!(
            card(&format!("{SIEGE}layout = \"transform\"\n[back]\n{BACK}")),
            Ok(Card::Transform { .. })
        ));
        // A Siege is cast transformed when it is defeated, so it is always a
        // transforming card's front (CR 310.12b).
        assert!(err(SIEGE).contains("front face of a layout = \"transform\""));
        assert!(err(&format!("{SIEGE}layout = \"modal\"\n[back]\n{BACK}")).contains("Siege"));
        assert!(err(&format!(
            "{}layout = \"transform\"\n[back]\n{}",
            BACK.replace("Back", "Front"),
            SIEGE
        ))
        .contains("Siege"));
        // A battle of no type, or another type, may stand alone (CR 310.8).
        let lone = parse("name = \"x\"\ntype = \"Battle\"\ndefense = \"3\"").unwrap();
        assert!(lone.is_battle() && !lone.is_siege());
        // Every battle prints its defense (CR 310.4a), in a corner that holds
        // nothing else.
        assert!(err("name = \"x\"\ntype = \"Battle\"").contains("needs defense"));
        assert!(
            err("name = \"x\"\ntype = \"Battle Creature\"\npt = \"1/1\"")
                .contains("no creature or planeswalker")
        );
        assert!(err("name = \"x\"\ntype = \"Battle\"\ndefense = \"3\"\nrules = \"\"\"\n10+ | Flying\n\"\"\"")
            .contains("plain lines"));
        // It prints on a card turned on its side, to itself.
        assert!(err(
            "name = \"x\"\ncost = \"{1}\"\ntype = \"Battle\"\ndefense = \"3\"\nlayout = \"split\"\n[right]\nname = \"y\"\ncost = \"{1}\"\ntype = \"Instant\"\n"
        )
        .contains("sideways"));
    }

    const BUDOKA: &str = "name = \"Budoka Gardener\"\ncost = \"{1}{G}\"\ntype = \"Creature — Human Monk\"\nrules = \"{T}: You may put a land card from your hand onto the battlefield. If you control ten or more lands, flip Budoka Gardener.\"\npt = \"2/1\"\nlayout = \"flip\"\n";
    const DOKAI: &str = "[flipped]\nname = \"Dokai, Weaver of Life\"\ntype = \"Legendary Creature — Human Monk\"\nrules = \"{4}{G}{G}, {T}: Create an X/X green Elemental creature token, where X is the number of lands you control.\"\npt = \"3/3\"\n";

    #[test]
    fn a_flip_card_is_one_image_whose_flipped_half_has_only_what_flipping_changes() {
        let budoka = card(&format!("{BUDOKA}{DOKAI}")).unwrap();
        assert!(matches!(budoka, Card::Flip { .. }));
        assert_eq!(budoka.name(), "Budoka Gardener // Dokai, Weaver of Life");
        assert_eq!(budoka.images(), 1);
        let err = |text: String| format!("{:#}", card(&text).unwrap_err());
        assert!(err(BUDOKA.to_string()).contains("needs its [flipped] half"));
        assert!(
            err(format!("name = \"x\"\ntype = \"Enchantment\"\n{DOKAI}"))
                .contains("layout = \"flip\"")
        );
        // Flipping changes neither cost nor colour (CR 710.1c), and the art
        // is the upright half's.
        for (field, line) in [
            ("cost", "cost = \"{G}\"\n"),
            ("color_indicator", "color_indicator = \"G\"\n"),
            ("art", "art = \"x.png\"\n"),
            ("artist", "artist = \"x\"\n"),
        ] {
            let flipped = DOKAI.replace("[flipped]\n", &format!("[flipped]\n{line}"));
            assert!(
                err(format!("{BUDOKA}{flipped}")).contains(&format!("no {field}")),
                "{field}"
            );
        }
        // Only a permanent flips.
        let sorcery = BUDOKA
            .replace("type = \"Creature — Human Monk\"", "type = \"Sorcery\"")
            .replace("pt = \"2/1\"\n", "");
        assert!(err(format!("{sorcery}{DOKAI}")).contains("only a permanent"));
        let walker = DOKAI
            .replace(
                "Legendary Creature — Human Monk",
                "Legendary Planeswalker — Dokai",
            )
            .replace("pt = \"3/3\"", "loyalty = \"3\"");
        assert!(err(format!("{BUDOKA}{walker}")).contains("no loyalty or defense"));
        let saga = DOKAI
            .replace("Legendary Creature — Human Monk", "Enchantment — Saga")
            .replace("pt = \"3/3\"\n", "")
            .replace("rules = \"{4}", "rules = \"I — Draw a card.\\nII — {4}");
        assert!(err(format!("{BUDOKA}{saga}")).contains("plain lines"));
        let battle = DOKAI
            .replace("Legendary Creature — Human Monk", "Battle")
            .replace("pt = \"3/3\"", "defense = \"3\"");
        assert!(err(format!("{BUDOKA}{battle}")).contains("no loyalty or defense"));
    }

    const GISELA: &str = "name = \"Gisela, the Broken Blade\"\ncost = \"{2}{W}{W}\"\ntype = \"Legendary Creature — Angel Horror\"\nrules = \"\"\"\nFlying, first strike, lifelink\nAt the beginning of your end step, if you both own and control Gisela and a creature named Bruna, the Fading Light, exile them, then meld them into Brisela, Voice of Nightmares.\n\"\"\"\npt = \"4/3\"\nlayout = \"meld\"\n";
    const BRUNA: &str = "[counterpart]\nname = \"Bruna, the Fading Light\"\ncost = \"{5}{W}{W}\"\ntype = \"Legendary Creature — Angel Horror\"\nrules = \"\"\"\nFlying, vigilance\n(Melds with Gisela, the Broken Blade.)\n\"\"\"\npt = \"5/7\"\n";
    const BRISELA: &str = "[melded]\nname = \"Brisela, Voice of Nightmares\"\ntype = \"Legendary Creature — Eldrazi Angel\"\nrules = \"Flying, first strike, vigilance, lifelink\"\npt = \"9/10\"\n";

    #[test]
    fn a_meld_pair_is_one_file_of_two_cards_and_never_half_of_one() {
        let pair = card(&format!("{GISELA}{BRUNA}{BRISELA}")).unwrap();
        assert!(matches!(pair, Card::Meld { .. }));
        assert_eq!(pair.images(), 4);
        assert_eq!(
            pair.name(),
            "Gisela, the Broken Blade + Bruna, the Fading Light → Brisela, Voice of Nightmares"
        );
        let err = |text: String| format!("{:#}", card(&text).unwrap_err());
        // Half a pair is refused, either way it is half.
        assert!(err(format!("{GISELA}{BRUNA}")).contains("no [melded]"));
        assert!(err(format!("{GISELA}{BRISELA}")).contains("no [counterpart]"));
        assert!(err(GISELA.to_string()).contains("never printed without its pair"));
        assert!(
            err(format!("name = \"x\"\ntype = \"Land\"\n{BRUNA}")).contains("layout = \"meld\"")
        );
        assert!(err(format!(
            "{GISELA}{BRUNA}{BRISELA}[back]\nname = \"x\"\ntype = \"Land\"\n"
        ))
        .contains("not a [back]"));
        // The card that melds them is the top level, whose back is the
        // melded card's top half.
        let gisela_below = BRUNA
            .replace("[counterpart]", "")
            .replace("pt = \"5/7\"\n", "pt = \"5/7\"\nlayout = \"meld\"\n");
        let gisela_as_counterpart = format!(
            "[counterpart]\n{}",
            GISELA.replace("layout = \"meld\"\n", "")
        );
        assert!(
            err(format!("{gisela_below}{gisela_as_counterpart}{BRISELA}"))
                .contains("so it is the card at the top level")
        );
        let neither = GISELA.replace("then meld them into", "then return");
        assert!(err(format!("{neither}{BRUNA}{BRISELA}")).contains("melds them into"));
        // The melded card has no cost (CR 202.3c), and all three are
        // permanents.
        let costed = BRISELA.replace("[melded]\n", "[melded]\ncost = \"{9}\"\n");
        assert!(err(format!("{GISELA}{BRUNA}{costed}")).contains("no mana cost"));
        let instant = BRUNA
            .replace("Legendary Creature — Angel Horror", "Instant")
            .replace("pt = \"5/7\"\n", "");
        assert!(err(format!("{GISELA}{instant}{BRISELA}")).contains("permanent on the battlefield"));
    }

    #[test]
    fn a_land_face_names_its_mana_ability() {
        let land = parse("name = \"x\"\ntype = \"Land\"\nrules = \"\"\"\nAs this land enters, you may pay 3 life.\n{T}: Add {W}.\n\"\"\"").unwrap();
        assert_eq!(land.mana_ability().as_deref(), Some("{T}: Add {W}."));
    }

    #[test]
    fn a_cost_is_only_symbols() {
        assert!(parse("name = \"x\"\ncost = \"2U\"\ntype = \"Instant\"").is_err());
    }
}
