//! A custom card as its `.card.toml` says it, parsed into a shape whose
//! corner can only hold what the type line allows.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use facet::Facet;

pub use crate::rules::Rules;

/// The file as written, before any of it is checked against the rest.
#[derive(Facet)]
#[facet(deny_unknown_fields)]
pub(crate) struct RawCard {
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

impl RawCard {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        facet_toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
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
        Ok(Card {
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

    pub fn frame(&self) -> Frame {
        let colors =
            Colors::of_symbols(self.cost.iter().map(String::as_str)).union(self.color_indicator);
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

    fn parse(text: &str) -> Result<Card> {
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

    #[test]
    fn a_cost_is_only_symbols() {
        assert!(parse("name = \"x\"\ncost = \"2U\"\ntype = \"Instant\"").is_err());
    }
}
