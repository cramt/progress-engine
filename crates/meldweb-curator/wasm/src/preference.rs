//! Which printing of a card to offer first: `meldweb.toml`'s `[printings]`
//! (ADR-0026).
//!
//! ```toml
//! [printings]
//! rank = [
//!   { avoid = "is:digital" },
//!   { prefer = "t:basic is:fullart" },
//!   { avoid = "is:textless" },
//! ]
//! ```
//!
//! Each rule is a Scryfall query over one printing, read by
//! `chip_scryfall::parse_printing`. The first rule decides, and each later
//! one only breaks the ties of those above it, so the order is the
//! preference: above, a textless full-art Forest loses to a full-art Forest
//! with text and beats a plain one. Printings no rule tells apart go newest
//! first, the closest wording to current oracle text.

use std::cmp::Ordering;

use chip_scryfall::bulk::BulkCard;
use chip_scryfall::printing::Printing;
use chip_scryfall::Query;
use facet::Facet;

/// The rules a repo without `meldweb.toml` ranks by, and what one may copy.
pub const DEFAULT: &str = r#"[printings]
rank = [
  { avoid = "is:digital" },
  { avoid = "border:gold or border:silver or is:oversized or is:playtest or st:memorabilia" },
  { avoid = "-lang:en" },
  { prefer = "t:basic is:fullart" },
  { avoid = "is:textless" },
  { avoid = "set:mp2" },
  { avoid = "is:flavorname" },
  { avoid = "is:sourcematerial" },
  { avoid = "is:showcase or frame:old or frame:future or frame:etched or is:poster" },
  { avoid = "is:ub" },
]
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(rename_all = "camelCase")]
pub enum Verb {
    Prefer,
    Avoid,
}

/// One rule, as the file wrote it.
#[derive(Debug, Clone, Facet)]
pub struct RuleText {
    pub verb: Verb,
    pub query: String,
}

pub struct Rule {
    pub text: RuleText,
    query: Query,
}

pub struct Preference {
    pub rules: Vec<Rule>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawFile {
    #[facet(default)]
    printings: Option<RawPrintings>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawPrintings {
    #[facet(default)]
    rank: Vec<RawRule>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawRule {
    prefer: Option<String>,
    avoid: Option<String>,
}

impl Preference {
    /// `meldweb.toml`'s text, or [`DEFAULT`]'s rules when there is no file.
    pub fn parse(text: Option<&str>) -> Result<Self, String> {
        let raw: RawFile = facet_toml::from_str(text.unwrap_or(DEFAULT))
            .map_err(|e| format!("meldweb.toml is not a settings file: {e}"))?;
        let raw = raw.printings.map_or(Vec::new(), |p| p.rank);
        let rules = raw
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let n = i + 1;
                let (verb, query) = match (r.prefer, r.avoid) {
                    (Some(q), None) => (Verb::Prefer, q),
                    (None, Some(q)) => (Verb::Avoid, q),
                    _ => {
                        return Err(format!(
                            "meldweb.toml: rule {n} of [printings] rank must be \
                             {{ prefer = \"...\" }} or {{ avoid = \"...\" }}, one of them"
                        ))
                    }
                };
                let parsed = chip_scryfall::parse_printing(&query)
                    .map_err(|e| format!("meldweb.toml: rule {n} of [printings] rank: {e}"))?;
                Ok(Rule {
                    text: RuleText { verb, query },
                    query: parsed,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Preference { rules })
    }

    /// Which rules `card` matches, by index.
    fn matched(&self, card: &BulkCard, printing: &Printing) -> Vec<u32> {
        let oracle = card.card_of_any_printing(&mut Vec::new());
        let view = oracle.view(&[]);
        self.rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.query.matches_printing(&view, printing))
            .map(|(i, _)| i as u32)
            .collect()
    }

    /// `cards`' indices, best first, each with the rules it matched.
    pub fn rank(&self, cards: &[BulkCard]) -> Vec<(usize, Vec<u32>)> {
        let mut ranked: Vec<(usize, Printing, Vec<u32>)> = cards
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let p = c.printing();
                let matched = self.matched(c, &p);
                (i, p, matched)
            })
            .collect();
        ranked.sort_by(|(_, pa, ma), (_, pb, mb)| {
            self.by_rules(ma, mb)
                .then_with(|| pb.released_at.cmp(&pa.released_at))
                .then_with(|| pa.set.cmp(&pb.set))
                .then_with(|| by_collector_number(&pa.collector_number, &pb.collector_number))
        });
        ranked.into_iter().map(|(i, _, m)| (i, m)).collect()
    }

    /// The first rule that tells two printings apart decides between them.
    fn by_rules(&self, a: &[u32], b: &[u32]) -> Ordering {
        for (i, rule) in self.rules.iter().enumerate() {
            let i = i as u32;
            let (in_a, in_b) = (a.contains(&i), b.contains(&i));
            if in_a != in_b {
                let a_first = in_a == (rule.text.verb == Verb::Prefer);
                return if a_first {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
        }
        Ordering::Equal
    }
}

/// `9` before `10`, and `10` before `10a`, as a set's numbering reads.
fn by_collector_number(a: &str, b: &str) -> Ordering {
    let split = |s: &str| {
        let digits = s.chars().take_while(char::is_ascii_digit).count();
        (s[..digits].parse::<u64>().ok(), s[digits..].to_string())
    };
    split(a).cmp(&split(b)).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINTINGS: &str =
        include_str!("../../../reality-chip/scryfall/tests/fixtures/printings.jsonl");

    fn printings_of(name: &str) -> Vec<BulkCard> {
        PRINTINGS
            .lines()
            .map(|l| facet_json::from_str::<BulkCard>(l).unwrap())
            .filter(|c| c.name == name)
            .collect()
    }

    /// `set/num`, best first.
    fn ranked(settings: Option<&str>, name: &str) -> Vec<String> {
        let cards = printings_of(name);
        Preference::parse(settings)
            .unwrap()
            .rank(&cards)
            .into_iter()
            .map(|(i, _)| {
                let p = cards[i].printing();
                format!("{}/{}", p.set, p.collector_number)
            })
            .collect()
    }

    fn position(order: &[String], printing: &str) -> usize {
        order
            .iter()
            .position(|p| p == printing)
            .unwrap_or_else(|| panic!("{printing} is not in {order:?}"))
    }

    #[test]
    fn heroic_intervention_offers_the_newest_plain_printing_and_sinks_the_comics() {
        let order = ranked(None, "Heroic Intervention");
        assert_eq!(order[0], "cmm/295");
        let plain_ub = position(&order, "pip/202");
        for comic in ["mar/80", "mar/79", "mar/78", "tle/43", "mar/34"] {
            assert!(position(&order, comic) > plain_ub, "{comic}: {order:?}");
        }
        assert_eq!(
            order.last().map(String::as_str),
            Some("prm/81988"),
            "{order:?}"
        );
    }

    #[test]
    fn a_textless_full_art_forest_is_below_full_art_with_text_and_above_plain() {
        let order = ranked(None, "Forest");
        let textless = position(&order, "sld/2120");
        assert!(textless > position(&order, "fra/394"), "{order:?}");
        assert!(textless < position(&order, "m21/274"), "{order:?}");
    }

    #[test]
    fn a_foreign_counterspell_is_below_every_english_paper_one() {
        let order = ranked(None, "Counterspell");
        let foreign = ["pmei/2021-1", "sta/78", "4bb/65", "fbb/54"].map(|p| position(&order, p));
        let mp2 = position(&order, "mp2/10");
        assert!(foreign.iter().all(|&f| f > mp2), "{order:?}");
        assert!(
            position(&order, "vma/64") > foreign[0],
            "digital sinks below foreign: {order:?}"
        );
    }

    #[test]
    fn rules_apply_in_the_order_written() {
        // Full art first, digital after: the file says full art matters more.
        let backwards = r#"[printings]
rank = [{ prefer = "is:fullart" }, { avoid = "is:digital" }]
"#;
        let order = ranked(Some(backwards), "Heroic Intervention");
        assert!(order[0].starts_with("mar/"), "{order:?}");
    }

    #[test]
    fn no_rules_is_newest_first() {
        let order = ranked(Some("[printings]\nrank = []\n"), "Heroic Intervention");
        assert!(order[0].starts_with("mar/"), "{order:?}");
        assert_eq!(ranked(Some(""), "Counterspell").len(), 88);
    }

    #[test]
    fn a_rule_that_is_not_one_is_refused_with_its_number() {
        let both = r#"[printings]
rank = [{ avoid = "is:ub" }, { prefer = "is:fullart", avoid = "is:ub" }]
"#;
        let err = Preference::parse(Some(both)).err().unwrap();
        assert!(err.contains("rule 2"), "{err}");
        let typo = r#"[printings]
rank = [{ avoid = "lang:jp" }]
"#;
        let err = Preference::parse(Some(typo)).err().unwrap();
        assert!(err.contains("rule 1") && err.contains("language"), "{err}");
        let unknown = "[printings]\nrank = []\nsort = \"newest\"\n";
        assert!(Preference::parse(Some(unknown)).is_err());
    }

    #[test]
    fn the_default_rules_parse() {
        assert_eq!(Preference::parse(None).unwrap().rules.len(), 10);
    }
}
