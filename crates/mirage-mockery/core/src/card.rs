//! A custom card as its `.card.toml` says it, parsed into a shape whose
//! corner can only hold what the type line allows.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use facet::Facet;

use crate::rules::Frames;
pub use crate::rules::Rules;

/// One face as the file writes it: the top level is the front face (or a
/// split card's left half, or the card an adventure or a prepare spell is
/// inset in), and `[back]`, `[right]` or `[spell]` the other.
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
    /// Two halves on one card, each with its own cost (CR 709.1).
    Split {
        left: Face,
        right: Face,
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

    /// In the order a pair is printed, as hybrid symbols and dual lands are:
    /// W/U, U/B, B/R, R/G, G/W and W/B, U/R, B/G, R/W, G/U. Otherwise WUBRG.
    pub fn ordered(self) -> Vec<Color> {
        let all: Vec<Color> = Color::ALL
            .into_iter()
            .filter(|&c| self.contains(c))
            .collect();
        match all.as_slice() {
            [Color::White, Color::Green] => vec![Color::Green, Color::White],
            [Color::White, Color::Red] => vec![Color::Red, Color::White],
            [Color::Blue, Color::Green] => vec![Color::Green, Color::Blue],
            _ => all,
        }
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
        let seconds = [
            ("back", raw.back.is_some()),
            ("right", raw.right.is_some()),
            ("spell", raw.spell.is_some()),
        ];
        let second = match seconds.iter().filter(|(_, has)| *has).collect::<Vec<_>>()[..] {
            [] => None,
            [(key, _)] => Some(*key),
            _ => bail!("a card has two parts at most: one of [back], [right] and [spell]"),
        };
        // Which table each layout keeps its second part in.
        let wants = |l: &str| match l {
            "transform" | "modal" => Some("back"),
            "split" => Some("right"),
            "adventure" | "prepare" => Some("spell"),
            _ => None,
        };
        let card = match (layout, second) {
            (None, None) => Card::Single(Face::of(raw.front, dir)?),
            (None, Some("back")) => {
                bail!("a [back] needs a layout: layout = \"transform\" or \"modal\"")
            }
            (None, Some("right")) => bail!("a [right] half needs layout = \"split\""),
            (None, Some(_)) => {
                bail!("a [spell] needs a layout: layout = \"adventure\" or \"prepare\"")
            }
            (Some(l), _) if wants(l).is_none() => bail!(
                "layout is \"transform\", \"modal\", \"split\", \"adventure\" or \"prepare\", not {l:?}"
            ),
            (Some(l), None) => match wants(l) {
                Some("back") => bail!("a {l} card needs its [back] face"),
                Some("right") => bail!("a split card needs its [right] half"),
                _ => bail!("an {l} card needs its [spell]"),
            },
            (Some(l), Some(key)) if wants(l) != Some(key) => match key {
                "back" if l == "split" => {
                    bail!("a split card's second half is [right], not [back]: it has a card back")
                }
                "back" => bail!(
                    "{} {l} card's spell is [spell], not [back]: it has a card back",
                    article(l)
                ),
                _ => bail!(
                    "{} {l} card's second part is [{}], not [{key}]",
                    article(l),
                    wants(l).expect("a known layout")
                ),
            },
            (Some("transform"), _) => Card::Transform {
                front: face(raw.front, "front face")?,
                back: face(raw.back.expect("checked"), "back face")?,
            },
            (Some("modal"), _) => Card::Modal {
                front: face(raw.front, "front face")?,
                back: face(raw.back.expect("checked"), "back face")?,
            },
            (Some("split"), _) => {
                let card = Card::Split {
                    left: face(raw.front, "left half")?,
                    right: face(raw.right.expect("checked"), "right half")?,
                };
                // Every printed split card's halves each have a cost, and
                // the card's cost is the two together (CR 709.4b).
                for half in card.faces() {
                    if half.cost.is_empty() {
                        bail!("{:?} is half a split card and needs a cost", half.name);
                    }
                }
                card
            }
            (Some(l), _) => {
                let spell = raw.spell.expect("checked");
                let main = face(raw.front, "card")?;
                let spell = Inset::check(spell, dir, l == "adventure")?;
                if main.rules.is_framed() {
                    bail!(
                        "{:?}'s text shares its box with its {}, so it is plain lines: \
                         no Saga, Class, planeswalker or station frame has room for both",
                        main.name,
                        if l == "adventure" { "Adventure" } else { "prepare spell" }
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
        Ok(card)
    }

    /// The faces it prints, front (or left) first.
    pub fn faces(&self) -> Vec<&Face> {
        match self {
            Card::Single(f) => vec![f],
            Card::Transform { front, back } | Card::Modal { front, back } => vec![front, back],
            Card::Split { left, right } => vec![left, right],
            Card::Adventure { main, spell } | Card::Prepare { main, spell } => vec![main, spell],
        }
    }

    /// How many card images it prints: a double-faced card's two faces are
    /// two, a split card's halves share one.
    pub fn images(&self) -> usize {
        match self {
            Card::Single(_)
            | Card::Split { .. }
            | Card::Adventure { .. }
            | Card::Prepare { .. } => 1,
            Card::Transform { .. } | Card::Modal { .. } => 2,
        }
    }

    /// Both names, `Far // Away` or `Bonecrusher Giant // Stomp`, as Oracle
    /// writes a card of two (CR 709.4a, 712.19).
    pub fn name(&self) -> String {
        self.faces()
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(" // ")
    }

    /// A split card whose halves both have fuse prints the keyword once,
    /// on a bar across both (CR 702.102a): the line, as the left half has it.
    pub fn fuse(&self) -> Option<String> {
        let Card::Split { left, right } = self else {
            return None;
        };
        let fuse = |f: &Face| f.rules.lines().into_iter().find(|l| is_fuse(l));
        fuse(right)?;
        fuse(left)
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

/// `Fuse`, alone or with its reminder.
pub(crate) fn is_fuse(line: &str) -> bool {
    line.strip_prefix("Fuse")
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
        let has = |word: &str| self.type_line.split_whitespace().any(|w| w == word);
        has("Instant") || has("Sorcery")
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
        assert_eq!(colors("WG").ordered(), [Color::Green, Color::White]);
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
        assert!(err(&format!("{front}layout = \"flip\"\n{back}")).contains("not \"flip\""));
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
    fn a_land_face_names_its_mana_ability() {
        let land = parse("name = \"x\"\ntype = \"Land\"\nrules = \"\"\"\nAs this land enters, you may pay 3 life.\n{T}: Add {W}.\n\"\"\"").unwrap();
        assert_eq!(land.mana_ability().as_deref(), Some("{T}: Add {W}."));
    }

    #[test]
    fn a_cost_is_only_symbols() {
        assert!(parse("name = \"x\"\ncost = \"2U\"\ntype = \"Instant\"").is_err());
    }
}
