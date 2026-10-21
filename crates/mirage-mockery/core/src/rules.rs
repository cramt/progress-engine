//! Rules text as Oracle text writes it, read into the layout it asks for.
//!
//! A card's layout comes from what its text says, never from a list of
//! frames: chapter lines make a Saga's chapters (CR 714), level bars a Class's
//! levels (CR 716), station symbols a Spacecraft's striations (CR 721). The
//! numbering each implies is held so it cannot be out of order: a Saga's
//! chapters are spans counted from I, a Class's levels count from 2, and a
//! station card's thresholds only climb.

use std::num::NonZeroU8;

use anyhow::{bail, Result};

/// One paragraph of rules text, as written.
pub type Line = String;

#[derive(Debug, Clone, PartialEq)]
pub enum Rules {
    /// One paragraph after another.
    Plain(Vec<Line>),
    Saga(Saga),
    Class(Class),
    Station(Station),
}

/// A Saga's text box: what comes before the chapters (its reminder, read
/// ahead), the chapters, and what follows them, which on a Saga creature is
/// a text box of its own below the type line (CR 714.1a).
#[derive(Debug, Clone, PartialEq)]
pub struct Saga {
    pub before: Vec<Line>,
    chapters: Vec<Chapter>,
    pub after: Vec<Line>,
}

/// One chapter ability. `I, II — …` is one ability with two chapter symbols
/// (CR 714.2c), so a chapter covers a span of numbers; the first starts at I
/// and each the one after the last.
#[derive(Debug, Clone, PartialEq)]
struct Chapter {
    span: NonZeroU8,
    text: Line,
}

impl Saga {
    /// Each chapter's numbers and its text.
    pub fn chapters(&self) -> impl Iterator<Item = (std::ops::RangeInclusive<u8>, &str)> {
        let mut next = 1u8;
        self.chapters.iter().map(move |c| {
            let first = next;
            next += c.span.get();
            (first..=next - 1, c.text.as_str())
        })
    }
}

/// A Class's text box: its level 1 section, reminder and all, then a section
/// for each level bar, level 2 upward (CR 716.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub before: Vec<Line>,
    levels: Vec<Level>,
}

#[derive(Debug, Clone, PartialEq)]
struct Level {
    cost: String,
    text: Vec<Line>,
}

impl Class {
    /// Each level bar's cost as written (`{2}{U}`), its level, and the abilities in its section.
    pub fn levels(&self) -> impl Iterator<Item = (&str, u32, &[Line])> {
        self.levels
            .iter()
            .zip(2u32..)
            .map(|(l, n)| (l.cost.as_str(), n, l.text.as_slice()))
    }
}

/// A station card's text box: what holds at all times, the station keyword
/// among it, then a striation per station symbol, each threshold above the
/// last (CR 721.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    pub before: Vec<Line>,
    thresholds: Vec<Threshold>,
}

#[derive(Debug, Clone, PartialEq)]
struct Threshold {
    at: u32,
    text: Vec<Line>,
}

impl Station {
    /// Each station symbol's number and the abilities in its striation.
    pub fn thresholds(&self) -> impl Iterator<Item = (u32, &[Line])> {
        self.thresholds.iter().map(|t| (t.at, t.text.as_slice()))
    }
}

/// The line's kind, by the marker it opens with.
enum Marked<'a> {
    Chapter(Vec<u8>, &'a str),
    LevelBar(&'a str, u32),
    Threshold(u32, &'a str),
    Plain,
}

fn marked(line: &str) -> Marked<'_> {
    if let Some((head, text)) = line.split_once(" — ") {
        let numerals: Option<Vec<u8>> = head.split(", ").map(roman).collect();
        if let Some(numerals) = numerals {
            return Marked::Chapter(numerals, text);
        }
    }
    if let Some((cost, level)) = line.split_once(": Level ") {
        let is_cost = cost.starts_with('{')
            && cost.ends_with('}')
            && cost[1..cost.len() - 1]
                .split("}{")
                .all(|s| !s.is_empty() && !s.contains(['{', '}']));
        if let (true, Ok(n)) = (is_cost, level.parse()) {
            return Marked::LevelBar(cost, n);
        }
    }
    if let Some((n, text)) = line.split_once("+ | ") {
        if let Ok(n) = n.parse() {
            return Marked::Threshold(n, text);
        }
    }
    Marked::Plain
}

/// A Roman numeral from I to XX, written the one way.
fn roman(s: &str) -> Option<u8> {
    const ALL: [&str; 20] = [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV",
        "XV", "XVI", "XVII", "XVIII", "XIX", "XX",
    ];
    ALL.iter().position(|r| *r == s).map(|i| i as u8 + 1)
}

/// `n` as the numeral a chapter symbol prints.
pub(crate) fn numeral(n: u8) -> String {
    let (tens, ones) = (n / 10, n % 10);
    let ones = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX"][ones as usize];
    "X".repeat(tens as usize) + ones
}

impl Rules {
    /// Read the lines of rules text into the layout their markers ask for.
    pub fn parse(lines: Vec<Line>) -> Result<Self> {
        let kinds: Vec<Marked> = lines.iter().map(|l| marked(l)).collect();
        let saga = kinds.iter().any(|k| matches!(k, Marked::Chapter(..)));
        let class = kinds.iter().any(|k| matches!(k, Marked::LevelBar(..)));
        let station = kinds.iter().any(|k| matches!(k, Marked::Threshold(..)));
        match (saga, class, station) {
            (false, false, false) => Ok(Rules::Plain(lines)),
            (true, false, false) => saga_of(&lines, kinds).map(Rules::Saga),
            (false, true, false) => class_of(&lines, kinds).map(Rules::Class),
            (false, false, true) => station_of(&lines, kinds).map(Rules::Station),
            _ => bail!(
                "rules text has the markers of more than one of a Saga's chapters, \
                 a Class's levels and a Spacecraft's station thresholds"
            ),
        }
    }

    /// The lines as Oracle text writes them.
    pub fn lines(&self) -> Vec<Line> {
        match self {
            Rules::Plain(lines) => lines.clone(),
            Rules::Saga(s) => {
                let mut out = s.before.clone();
                for (span, text) in s.chapters() {
                    let numerals: Vec<String> = span.map(numeral).collect();
                    out.push(format!("{} — {text}", numerals.join(", ")));
                }
                out.extend(s.after.iter().cloned());
                out
            }
            Rules::Class(c) => {
                let mut out = c.before.clone();
                for (cost, n, text) in c.levels() {
                    out.push(format!("{cost}: Level {n}"));
                    out.extend(text.iter().cloned());
                }
                out
            }
            Rules::Station(s) => {
                let mut out = s.before.clone();
                for (at, text) in s.thresholds() {
                    let (first, rest) = text.split_first().expect("a threshold has its line");
                    out.push(format!("{at}+ | {first}"));
                    out.extend(rest.iter().cloned());
                }
                out
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Rules::Plain(lines) if lines.is_empty())
    }
}

fn saga_of(lines: &[Line], kinds: Vec<Marked>) -> Result<Saga> {
    let mut saga = Saga {
        before: Vec::new(),
        chapters: Vec::new(),
        after: Vec::new(),
    };
    let mut next = 1u8;
    for (line, kind) in lines.iter().zip(kinds) {
        match kind {
            Marked::Chapter(numerals, text) => {
                if !saga.after.is_empty() {
                    bail!(
                        "a Saga's chapters come together; {line:?} follows a line that is not one"
                    );
                }
                let expected: Vec<u8> = (next..next + numerals.len() as u8).collect();
                if numerals != expected {
                    let want: Vec<String> = expected.into_iter().map(numeral).collect();
                    bail!(
                        "a Saga's chapters count up from I with none missed: {line:?} should be {} —",
                        want.join(", ")
                    );
                }
                next += numerals.len() as u8;
                saga.chapters.push(Chapter {
                    span: NonZeroU8::new(numerals.len() as u8).expect("a chapter has a numeral"),
                    text: text.into(),
                });
            }
            _ if saga.chapters.is_empty() => saga.before.push(line.clone()),
            _ => saga.after.push(line.clone()),
        }
    }
    Ok(saga)
}

fn class_of(lines: &[Line], kinds: Vec<Marked>) -> Result<Class> {
    let mut class = Class {
        before: Vec::new(),
        levels: Vec::new(),
    };
    for (line, kind) in lines.iter().zip(kinds) {
        match kind {
            Marked::LevelBar(cost, n) => {
                let want = class.levels.len() as u32 + 2;
                if n != want {
                    bail!(
                        "a Class's levels count up from 2 with none missed: {line:?} should be level {want}"
                    );
                }
                class.levels.push(Level {
                    cost: cost.into(),
                    text: Vec::new(),
                });
            }
            _ => match class.levels.last_mut() {
                None => class.before.push(line.clone()),
                Some(level) => level.text.push(line.clone()),
            },
        }
    }
    Ok(class)
}

fn station_of(lines: &[Line], kinds: Vec<Marked>) -> Result<Station> {
    let mut station = Station {
        before: Vec::new(),
        thresholds: Vec::new(),
    };
    for (line, kind) in lines.iter().zip(kinds) {
        match kind {
            Marked::Threshold(at, text) => {
                if let Some(last) = station.thresholds.last() {
                    if at <= last.at {
                        bail!(
                            "a station card's thresholds climb: {line:?} comes after {}+",
                            last.at
                        );
                    }
                }
                station.thresholds.push(Threshold {
                    at,
                    text: vec![text.into()],
                });
            }
            _ => match station.thresholds.last_mut() {
                None => station.before.push(line.clone()),
                Some(t) => t.text.push(line.clone()),
            },
        }
    }
    Ok(station)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Rules> {
        Rules::parse(text.lines().map(String::from).collect())
    }

    const ELDEST: &str = "(As this Saga enters and after your draw step, add a lore counter. Sacrifice after III.)
I — Each opponent sacrifices a creature or planeswalker of their choice.
II — Each opponent discards a card.
III — Put target creature or planeswalker card from a graveyard onto the battlefield under your control.";

    #[test]
    fn a_saga_is_its_reminder_then_its_chapters() {
        let Rules::Saga(saga) = parse(ELDEST).unwrap() else {
            panic!("not a saga")
        };
        assert_eq!(saga.before.len(), 1);
        let spans: Vec<_> = saga.chapters().map(|(s, _)| s).collect();
        assert_eq!(spans, [1..=1, 2..=2, 3..=3]);
        assert!(saga.after.is_empty());
    }

    #[test]
    fn chapters_can_share_an_ability_and_a_saga_creature_has_lines_after() {
        // Summon: Bahamut's Oracle text, as Scryfall has it.
        let text = "(As this Saga enters and after your draw step, add a lore counter. Sacrifice after IV.)
I, II — Destroy up to one target nonland permanent.
III — Draw two cards.
IV — Mega Flare — This creature deals damage equal to the total mana value of other permanents you control to each opponent.
Flying";
        let Rules::Saga(saga) = parse(text).unwrap() else {
            panic!("not a saga")
        };
        let chapters: Vec<_> = saga.chapters().collect();
        assert_eq!(chapters[0].0, 1..=2);
        assert_eq!(chapters[2].1, "Mega Flare — This creature deals damage equal to the total mana value of other permanents you control to each opponent.");
        assert_eq!(saga.after, ["Flying"]);
    }

    #[test]
    fn chapters_out_of_order_or_with_a_gap_are_refused() {
        let err = |t: &str| format!("{:#}", parse(t).unwrap_err());
        assert!(err("I — a\nIII — b").contains("should be II —"));
        assert!(err("II — a\nIII — b").contains("should be I —"));
        assert!(err("I, III — a").contains("should be I, II —"));
        assert!(err("I — a\nFlying\nII — b").contains("come together"));
    }

    #[test]
    fn a_class_is_its_first_section_then_a_section_a_level() {
        // Ranger Class's Oracle text.
        let text = "(Gain the next level as a sorcery to add its ability.)
When this Class enters, create a 2/2 green Wolf creature token.
{1}{G}: Level 2
Whenever you attack, put a +1/+1 counter on target attacking creature.
{3}{G}: Level 3
You may look at the top card of your library any time.
You may cast creature spells from the top of your library.";
        let Rules::Class(class) = parse(text).unwrap() else {
            panic!("not a class")
        };
        assert_eq!(class.before.len(), 2);
        let levels: Vec<_> = class.levels().map(|(c, n, t)| (c, n, t.len())).collect();
        assert_eq!(levels, [("{1}{G}", 2, 1), ("{3}{G}", 3, 2)]);
    }

    #[test]
    fn levels_out_of_order_are_refused() {
        let e = format!("{:#}", parse("a\n{2}{U}: Level 3\nb").unwrap_err());
        assert!(e.contains("should be level 2"), "{e}");
        assert!(parse("a\n{2}{U}: Level 2\nb\n{1}: Level 2\nc").is_err());
    }

    #[test]
    fn a_station_card_is_its_abilities_then_a_striation_a_threshold() {
        // The Eternity Elevator's Oracle text: an ability before the keyword.
        let text = "{T}: Add {C}{C}{C}.
Station (Tap another creature you control: Put charge counters equal to its power on this Spacecraft. Station only as a sorcery.)
20+ | {T}: Add X mana of any one color, where X is the number of charge counters on The Eternity Elevator.";
        let Rules::Station(station) = parse(text).unwrap() else {
            panic!("not a station card")
        };
        assert_eq!(station.before.len(), 2);
        assert_eq!(
            station.thresholds().map(|(n, _)| n).collect::<Vec<_>>(),
            [20]
        );
    }

    #[test]
    fn thresholds_that_do_not_climb_are_refused() {
        assert!(parse("Station\n8+ | Flying\n3+ | Trample").is_err());
        assert!(parse("Station\n3+ | Flying\n3+ | Trample").is_err());
    }

    #[test]
    fn mixed_markers_are_refused() {
        assert!(parse("I — a\n{1}: Level 2").is_err());
    }

    #[test]
    fn every_layout_writes_back_the_lines_it_read() {
        for text in [
            ELDEST,
            "a\n{2}{U}: Level 2\nb\nc\n{4}{U}: Level 3\nd",
            "Station (x)\n3+ | a\n8+ | Flying\nb",
            "Flying\n{T}: Add {G}.",
            "Read ahead\nI, II, III — a\nIV — b\nMenace",
        ] {
            assert_eq!(parse(text).unwrap().lines().join("\n"), text);
        }
    }

    #[test]
    fn plain_text_that_merely_mentions_a_level_or_a_dash_stays_plain() {
        for text in [
            "Suspend 4—{U} (Rather than cast this card from your hand, pay {U}.)",
            "Landfall — Whenever a land you control enters, draw a card.",
            "Level up {2}",
            "As long as this has 3+ counters, it has flying.",
        ] {
            assert!(matches!(parse(text).unwrap(), Rules::Plain(_)), "{text}");
        }
    }
}
