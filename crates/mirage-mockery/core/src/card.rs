//! A custom card as its `.card.toml` says it, parsed into a shape whose
//! corner can only hold what the type line allows.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use facet::Facet;

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawCard {
    name: String,
    #[facet(default)]
    cost: Option<String>,
    #[facet(rename = "type")]
    type_line: String,
    #[facet(default)]
    rules: Option<String>,
    #[facet(default)]
    flavor: Option<String>,
    #[facet(default)]
    pt: Option<String>,
    #[facet(default)]
    loyalty: Option<String>,
    #[facet(default)]
    defense: Option<String>,
    #[facet(default)]
    art: Option<PathBuf>,
    #[facet(default)]
    artist: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub name: String,
    /// The mana cost's symbols, each without its braces: `["2", "U", "U/P"]`.
    pub cost: Vec<String>,
    pub type_line: String,
    /// The rules text, one paragraph per line, as Oracle text is laid out.
    pub rules: Vec<String>,
    pub flavor: Option<String>,
    pub corner: Corner,
    /// Resolved against the card file's directory.
    pub art: Option<PathBuf>,
    pub artist: Option<String>,
}

/// What the bottom-right corner prints. One of these at most, and only the one
/// the type line asks for: a creature or Vehicle has power and toughness
/// (CR 208.1, 301.7a), a planeswalker loyalty (CR 306.5a), a battle defense
/// (CR 310.4a).
#[derive(Debug, Clone, PartialEq)]
pub enum Corner {
    None,
    PowerToughness { power: String, toughness: String },
    Loyalty(String),
    Defense(String),
}

/// The frame's colour, from the colours of the mana cost (CR 105.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    White,
    Blue,
    Black,
    Red,
    Green,
    Gold,
    Artifact,
    Land,
    Colorless,
}

impl Card {
    pub fn open(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        Self::parse(&text, dir).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str, dir: &Path) -> Result<Self> {
        let raw: RawCard = facet_toml::from_str(text).map_err(|e| anyhow::anyhow!("{e}"))?;
        let has = |word: &str| raw.type_line.split_whitespace().any(|w| w == word);
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
            Corner::PowerToughness { .. } => Some(("pt", has("Creature") || has("Vehicle"))),
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
            rules: raw
                .rules
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect(),
            flavor: raw.flavor,
            corner,
            art: raw.art.map(|a| dir.join(a)),
            artist: raw.artist,
        })
    }

    pub fn frame(&self) -> Frame {
        let mut colors: Vec<char> = Vec::new();
        for s in &self.cost {
            for c in s.chars().filter(|c| "WUBRG".contains(*c)) {
                if !colors.contains(&c) {
                    colors.push(c);
                }
            }
        }
        match colors.as_slice() {
            [] if self.type_line.contains("Land") => Frame::Land,
            [] if self.type_line.contains("Artifact") => Frame::Artifact,
            [] => Frame::Colorless,
            ['W'] => Frame::White,
            ['U'] => Frame::Blue,
            ['B'] => Frame::Black,
            ['R'] => Frame::Red,
            ['G'] => Frame::Green,
            _ => Frame::Gold,
        }
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
        assert_eq!(card.rules, ["Flying", "{1}{U}, {T}: Scry 1."]);
        assert_eq!(
            card.corner,
            Corner::PowerToughness {
                power: "2".into(),
                toughness: "3".into()
            }
        );
        assert_eq!(card.frame(), Frame::Blue);
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
        assert_eq!(frame("{R/P}", "Instant"), Frame::Red);
    }

    #[test]
    fn a_cost_is_only_symbols() {
        assert!(parse("name = \"x\"\ncost = \"2U\"\ntype = \"Instant\"").is_err());
    }
}
