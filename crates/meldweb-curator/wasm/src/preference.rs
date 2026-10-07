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
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
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

impl Verb {
    fn word(self) -> &'static str {
        match self {
            Verb::Prefer => "prefer",
            Verb::Avoid => "avoid",
        }
    }
}

impl RuleText {
    fn label(&self) -> String {
        format!("{} {}", self.verb.word(), self.query)
    }
}

/// The rules `meldweb.toml` wrote, in order, with no query read yet: what the
/// settings page loads, so a rule that does not parse can be shown and fixed
/// rather than refusing the whole file. [`DEFAULT`]'s rules when there is no
/// file.
pub fn read_rules(text: Option<&str>) -> Result<Vec<RuleText>, String> {
    let raw: RawFile = facet_toml::from_str(text.unwrap_or(DEFAULT))
        .map_err(|e| format!("meldweb.toml is not a settings file: {e}"))?;
    let raw = raw.printings.map_or(Vec::new(), |p| p.rank);
    raw.into_iter()
        .enumerate()
        .map(|(i, r)| match (r.prefer, r.avoid) {
            (Some(query), None) => Ok(RuleText {
                verb: Verb::Prefer,
                query,
            }),
            (None, Some(query)) => Ok(RuleText {
                verb: Verb::Avoid,
                query,
            }),
            _ => Err(format!(
                "meldweb.toml: rule {} of [printings] rank must be \
                 {{ prefer = \"...\" }} or {{ avoid = \"...\" }}, one of them",
                i + 1
            )),
        })
        .collect()
}

/// Why `query` is not a rule, or `None` when it is one.
pub fn check(query: &str) -> Option<String> {
    if query.trim().is_empty() {
        return Some("a rule needs a query".to_string());
    }
    chip_scryfall::parse_printing(query)
        .err()
        .map(|e| e.to_string())
}

/// `rules` as a `meldweb.toml`. Every rule must parse, so the file written
/// reads back as the same rules.
pub fn write(rules: &[RuleText]) -> Result<String, String> {
    let mut out = String::from(
        "# Meldweb Curator's settings.\n\
         \n\
         [printings]\n\
         # Which printing of a card to offer first. Each rule is a Scryfall query\n\
         # over one printing. The first rule that tells two printings apart\n\
         # decides, and later ones only break ties; untold apart, newest first.\n",
    );
    if rules.is_empty() {
        out.push_str("rank = []\n");
        return Ok(out);
    }
    out.push_str("rank = [\n");
    for (i, rule) in rules.iter().enumerate() {
        if let Some(e) = check(&rule.query) {
            return Err(format!("rule {}: {e}", i + 1));
        }
        out.push_str(&format!(
            "  {{ {} = {} }},\n",
            rule.verb.word(),
            toml_string(&rule.query)
        ));
    }
    out.push_str("]\n");
    Ok(out)
}

/// A TOML basic string, so a query's own quotes (`name:"Lim-Dûl"`) survive.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The commit for one save of `meldweb.toml`: a subject naming the rules
/// added, dropped, turned around and moved, and the whole ranking as the
/// body. `before` is `""` when the save creates the file.
pub fn commit_message(before: &str, after: &str) -> String {
    let new = read_rules(Some(after)).unwrap_or_default();
    let body = if new.is_empty() {
        "No rules: printings are offered newest first.".to_string()
    } else {
        new.iter()
            .enumerate()
            .map(|(i, r)| format!("{}. {}", i + 1, r.label()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let subject = if before.trim().is_empty() {
        let n = new.len();
        format!(
            "meldweb.toml: rank printings by {n} rule{}",
            if n == 1 { "" } else { "s" }
        )
    } else {
        let old = read_rules(Some(before)).unwrap_or_default();
        let mut parts = Vec::new();
        for r in new.iter().filter(|r| !old.contains(r)) {
            let turned = old.iter().any(|o| o.query == r.query);
            parts.push(format!(
                "{}{}",
                if turned { "now " } else { "+" },
                r.label()
            ));
        }
        for r in old
            .iter()
            .filter(|r| !new.iter().any(|n| n.query == r.query))
        {
            parts.push(format!("-{}", r.label()));
        }
        // The rules both sides have, in each side's order.
        let kept = |list: &[RuleText], other: &[RuleText]| -> Vec<RuleText> {
            list.iter().filter(|r| other.contains(r)).cloned().collect()
        };
        if kept(&old, &new) != kept(&new, &old) {
            parts.push("reorder".to_string());
        }
        if parts.is_empty() {
            "meldweb.toml: rewrite, same rules".to_string()
        } else {
            format!("meldweb.toml: {}", parts.join(", "))
        }
    };
    format!("{subject}\n\n{body}\n")
}

impl Preference {
    /// `meldweb.toml`'s text, or [`DEFAULT`]'s rules when there is no file.
    pub fn parse(text: Option<&str>) -> Result<Self, String> {
        let rules = read_rules(text)?
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let query = chip_scryfall::parse_printing(&text.query).map_err(|e| {
                    format!("meldweb.toml: rule {} of [printings] rank: {e}", i + 1)
                })?;
                Ok(Rule { text, query })
            })
            .collect::<Result<_, String>>()?;
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

    fn rule(verb: Verb, query: &str) -> RuleText {
        RuleText {
            verb,
            query: query.to_string(),
        }
    }

    #[test]
    fn what_is_written_reads_back_as_the_same_rules_quotes_and_all() {
        let rules = vec![
            rule(Verb::Avoid, "is:digital"),
            rule(
                Verb::Prefer,
                r#"t:basic is:fullart -name:"Snow-Covered Forest""#,
            ),
            rule(Verb::Avoid, r#"name:"a\b""#),
        ];
        let text = write(&rules).unwrap();
        assert_eq!(read_rules(Some(&text)).unwrap(), rules, "{text}");
        assert_eq!(Preference::parse(Some(&text)).unwrap().rules.len(), 3);
        let none = write(&[]).unwrap();
        assert_eq!(read_rules(Some(&none)).unwrap(), vec![]);
    }

    #[test]
    fn a_rule_that_does_not_parse_is_not_written() {
        let err = write(&[rule(Verb::Avoid, "is:ub"), rule(Verb::Avoid, "lang:jp")]).unwrap_err();
        assert!(err.starts_with("rule 2:"), "{err}");
        assert!(check("lang:jp").is_some());
        assert_eq!(check("lang:ja"), None);
        assert!(check("  ").is_some());
    }

    #[test]
    fn a_file_with_a_bad_query_still_reads_so_it_can_be_fixed() {
        let text = "[printings]\nrank = [{ avoid = \"lang:jp\" }]\n";
        assert!(Preference::parse(Some(text)).is_err());
        assert_eq!(
            read_rules(Some(text)).unwrap(),
            vec![rule(Verb::Avoid, "lang:jp")]
        );
    }

    #[test]
    fn a_commit_names_what_changed_and_lists_the_ranking() {
        let before = write(&[
            rule(Verb::Avoid, "is:digital"),
            rule(Verb::Avoid, "is:fullart"),
            rule(Verb::Avoid, "is:ub"),
            rule(Verb::Avoid, "frame:old"),
        ])
        .unwrap();
        let after = write(&[
            rule(Verb::Avoid, "is:ub"),
            rule(Verb::Prefer, "is:fullart"),
            rule(Verb::Avoid, "is:digital"),
            rule(Verb::Avoid, "is:textless"),
        ])
        .unwrap();
        assert_eq!(
            commit_message(&before, &after),
            "meldweb.toml: now prefer is:fullart, +avoid is:textless, -avoid frame:old, reorder\n\
             \n\
             1. avoid is:ub\n\
             2. prefer is:fullart\n\
             3. avoid is:digital\n\
             4. avoid is:textless\n"
        );
        assert!(commit_message("", &after).starts_with("meldweb.toml: rank printings by 4 rules\n"));
        assert!(commit_message(&before, &write(&[]).unwrap()).contains("newest first"));
    }

    #[test]
    fn the_default_rules_parse() {
        assert_eq!(Preference::parse(None).unwrap().rules.len(), 10);
    }
}
