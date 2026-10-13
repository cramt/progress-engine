//! The Comprehensive Rules as numbered rules, the repo's citations of them,
//! and what moved between two versions.
//!
//! Pure: reading files and the network is `main.rs`'s.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// A rule's number: a section (`613`), a rule (`613.4`) or a subrule
/// (`613.4c`). A subrule letter without a rule can't be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleId {
    pub section: u16,
    pub rule: Option<(u16, Option<char>)>,
}

impl RuleId {
    pub fn parse(s: &str) -> Option<RuleId> {
        let (section, rest) = match s.split_once('.') {
            Some((a, b)) => (a, Some(b)),
            None => (s, None),
        };
        if section.len() != 3 {
            return None;
        }
        let section = section.parse().ok()?;
        let rule = match rest {
            None => None,
            Some(r) => {
                let digits = r.trim_end_matches(|c: char| c.is_ascii_lowercase());
                let letters = &r[digits.len()..];
                let sub = match letters.len() {
                    0 => None,
                    1 => letters.chars().next(),
                    _ => return None,
                };
                Some((digits.parse().ok()?, sub))
            }
        };
        Some(RuleId { section, rule })
    }

    /// Citing a section or a rule cites what is under it too: "CR 613"
    /// is the whole layer system.
    pub fn covers(self, other: RuleId) -> bool {
        self.section == other.section
            && match (self.rule, other.rule) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some((a, None)), Some((b, _))) => a == b,
                (Some(a), Some(b)) => a == b,
            }
    }
}

impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.section)?;
        if let Some((rule, sub)) = self.rule {
            write!(f, ".{rule}")?;
            if let Some(c) = sub {
                write!(f, "{c}")?;
            }
        }
        Ok(())
    }
}

/// One version of the rules: its effective date as the text states it, and
/// every section, rule and subrule with its text, examples included.
pub struct Rules {
    pub effective: Option<String>,
    pub rules: BTreeMap<RuleId, String>,
}

static RULE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{3}(?:\.\d+[a-z]?)?)\.? (.*)$").unwrap());
static CHAPTER_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d\. ").unwrap());

impl Rules {
    pub fn parse(text: &str) -> Rules {
        let lines: Vec<&str> = text
            .lines()
            .map(|l| l.trim_matches(|c: char| c.is_whitespace()))
            .collect();
        let effective = lines.iter().find_map(|l| {
            l.strip_prefix("These rules are effective as of ")
                .map(|d| d.trim_end_matches('.').to_string())
        });
        // The contents list every section heading once before the rules
        // start, and end at "Credits"; the rules end at the glossary.
        let start = lines
            .iter()
            .position(|l| *l == "Credits")
            .map_or(0, |i| i + 1);
        let end = lines[start..]
            .iter()
            .position(|l| *l == "Glossary")
            .map_or(lines.len(), |i| start + i);

        let mut rules = BTreeMap::new();
        let mut current: Option<(RuleId, String)> = None;
        for line in &lines[start..end] {
            if line.is_empty() {
                continue;
            }
            let heading = RULE_LINE
                .captures(line)
                .and_then(|c| Some((RuleId::parse(&c[1])?, c[2].to_string())));
            if let Some((id, text)) = heading {
                rules.extend(current.replace((id, text)));
            } else if CHAPTER_LINE.is_match(line) {
                rules.extend(current.take());
            } else if let Some((_, text)) = current.as_mut() {
                text.push('\n');
                text.push_str(line);
            }
        }
        rules.extend(current);
        Rules { effective, rules }
    }
}

/// Where a file cites the rules, and the rules it names. A range,
/// "CR 102.2–102.3" or "CR 613.4a–c", names every rule between its ends.
#[derive(Debug)]
pub struct Citation {
    pub line: usize,
    pub written: String,
    pub cites: Cited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cited {
    One(RuleId),
    Range(RuleId, RuleId),
}

impl Cited {
    pub fn ends(self) -> Vec<RuleId> {
        match self {
            Cited::One(id) => vec![id],
            Cited::Range(a, b) => vec![a, b],
        }
    }

    pub fn covers(self, id: RuleId) -> bool {
        match self {
            Cited::One(c) => c.covers(id),
            Cited::Range(a, b) => (a..=b).contains(&id) || b.covers(id),
        }
    }
}

const ID: &str = r"\d{3}(?:\.\d+[a-z]?)?";
static CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"\bCR ({ID}(?:[–-](?:{ID}|[a-z]\b))?(?:(?:, | and | or ){ID}(?:[–-](?:{ID}|[a-z]\b))?)*)"
    ))
    .unwrap()
});
static ONE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"({ID})(?:[–-]({ID}|[a-z]\b))?")).unwrap());

/// Every citation in a file's text. Only "CR " followed by a number counts,
/// so a citation is always written that way.
pub fn citations(text: &str) -> Vec<Citation> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        for m in CITATION.captures_iter(line) {
            for one in ONE.captures_iter(&m[1]) {
                let Some(from) = RuleId::parse(&one[1]) else {
                    continue;
                };
                let cites = match one.get(2).map(|e| e.as_str()) {
                    None => Cited::One(from),
                    Some(end) => {
                        let to = match end.chars().next() {
                            Some(c) if end.len() == 1 && c.is_ascii_lowercase() => {
                                from.rule.map(|(r, _)| RuleId {
                                    rule: Some((r, Some(c))),
                                    ..from
                                })
                            }
                            _ => RuleId::parse(end),
                        };
                        match to {
                            Some(to) => Cited::Range(from, to),
                            None => Cited::One(from),
                        }
                    }
                };
                out.push(Citation {
                    line: i + 1,
                    written: one[0].to_string(),
                    cites,
                });
            }
        }
    }
    out
}

pub enum Change {
    Added(RuleId),
    Removed {
        id: RuleId,
        /// The same text under a new number, where there is one.
        moved_to: Option<RuleId>,
    },
    Changed(RuleId),
}

impl Change {
    pub fn id(&self) -> RuleId {
        match self {
            Change::Added(id) | Change::Removed { id, .. } | Change::Changed(id) => *id,
        }
    }
}

pub fn diff(old: &Rules, new: &Rules) -> Vec<Change> {
    let mut out = Vec::new();
    for (id, text) in &old.rules {
        match new.rules.get(id) {
            Some(t) if t == text => {}
            Some(_) => out.push(Change::Changed(*id)),
            None => out.push(Change::Removed {
                id: *id,
                moved_to: new
                    .rules
                    .iter()
                    .find(|(n, t)| *t == text && !old.rules.contains_key(n))
                    .map(|(n, _)| *n),
            }),
        }
    }
    for id in new.rules.keys() {
        if !old.rules.contains_key(id) {
            out.push(Change::Added(*id));
        }
    }
    out.sort_by_key(Change::id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXCERPT: &str = "Magic: The Gathering Comprehensive Rules\r\n\u{a0}\r\n\
These rules are effective as of September 25, 2026.\r\n\u{a0}\r\nContents\r\n\
1. Game Concepts\r\n613. Interaction of Continuous Effects\r\nGlossary\r\nCredits\r\n\
\u{a0}\r\n6. Spells, Abilities, and Effects\r\n\u{a0}\r\n\
613. Interaction of Continuous Effects\r\n\u{a0}\r\n\
613.4. Within layer 7, apply effects in a series of sublayers.\r\n\u{a0}\r\n\
613.4c Layer 7c: Effects and counters that modify power and/or toughness are applied.\r\n\
Example: A 1/3 creature is given +0/+1.\r\n\u{a0}\r\n\
613.4d Layer 7d: Effects that switch are applied.\r\n\u{a0}\r\n\
7. Additional Rules\r\n\u{a0}\r\nGlossary\r\n\u{a0}\r\nAbility\r\n";

    fn id(s: &str) -> RuleId {
        RuleId::parse(s).unwrap()
    }

    #[test]
    fn the_rules_are_read_past_the_contents_and_stop_at_the_glossary() {
        let r = Rules::parse(EXCERPT);
        assert_eq!(r.effective.as_deref(), Some("September 25, 2026"));
        let ids: Vec<String> = r.rules.keys().map(|k| k.to_string()).collect();
        assert_eq!(ids, ["613", "613.4", "613.4c", "613.4d"]);
        assert_eq!(
            r.rules[&id("613.4c")],
            "Layer 7c: Effects and counters that modify power and/or toughness are applied.\n\
             Example: A 1/3 creature is given +0/+1."
        );
    }

    #[test]
    fn rule_ids_order_by_number_not_by_text() {
        assert!(id("613.2") < id("613.10"));
        assert!(id("613") < id("613.1") && id("613.1") < id("613.1a"));
        assert_eq!(RuleId::parse("613.4cd"), None);
        assert_eq!(RuleId::parse("61.4"), None);
    }

    #[test]
    fn citations_are_read_as_written_ranges_and_lists_included() {
        let c = citations(
            "a (CR 613.4c), and CR 102.2–102.3, CR 107.3a, 107.3d.\nCR 613.8a-c; CR 105.3, a creature; CR 613",
        );
        let got: Vec<(usize, Cited)> = c.iter().map(|c| (c.line, c.cites)).collect();
        assert_eq!(
            got,
            [
                (1, Cited::One(id("613.4c"))),
                (1, Cited::Range(id("102.2"), id("102.3"))),
                (1, Cited::One(id("107.3a"))),
                (1, Cited::One(id("107.3d"))),
                (2, Cited::Range(id("613.8a"), id("613.8c"))),
                (2, Cited::One(id("105.3"))),
                (2, Cited::One(id("613"))),
            ]
        );
    }

    #[test]
    fn a_citation_covers_what_is_under_it() {
        assert!(Cited::One(id("613")).covers(id("613.4c")));
        assert!(Cited::One(id("613.4")).covers(id("613.4c")));
        assert!(!Cited::One(id("613.4c")).covers(id("613.4")));
        assert!(Cited::Range(id("613.8a"), id("613.8c")).covers(id("613.8b")));
        assert!(!Cited::Range(id("613.8a"), id("613.8c")).covers(id("613.8d")));
    }

    #[test]
    fn a_rule_renumbered_with_the_same_text_is_a_move() {
        let old = Rules::parse(EXCERPT);
        let new = Rules::parse(&EXCERPT.replace("613.4d Layer 7d", "613.4e Layer 7d"));
        let changes = diff(&old, &new);
        assert!(matches!(
            changes.as_slice(),
            [Change::Removed { moved_to: Some(to), .. }, Change::Added(_)] if *to == id("613.4e")
        ));
    }
}
