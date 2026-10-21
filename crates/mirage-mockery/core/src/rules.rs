//! Rules text as Oracle text writes it, read into the layout it asks for.
//!
//! A card's layout comes from what its text says, never from a list of
//! frames: chapter lines make a Saga's chapters (CR 714), level bars a Class's
//! levels (CR 716), station symbols a Spacecraft's striations (CR 721), and
//! loyalty costs a planeswalker's loyalty abilities (CR 606). The numbering
//! each implies is held so it cannot be out of order: a Saga's chapters are
//! spans counted from I, a Class's levels count from 2, and a station card's
//! thresholds only climb.
//!
//! The type line says which of those markers it has a frame for. Chapters
//! belong to a Saga (CR 714.1) and level bars to a Class (CR 716.1), so either
//! on a card that is not one is refused, as a power and toughness on a
//! noncreature is: it is a type line missing its subtype far more often than
//! a card meant that way. A loyalty ability on a card that is not a
//! planeswalker is legal Magic ("normally, only planeswalkers have loyalty
//! abilities", CR 606.2), and a frame with no loyalty badges prints it as the
//! activated ability it is, so it stays a plain line. Station symbols need
//! no type: a station card is any card printed with them (CR 702.184b).

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
    Planeswalker(Planeswalker),
}

/// What the type line has a frame for, of the layouts rules text can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Frames {
    pub saga: bool,
    pub class: bool,
    pub planeswalker: bool,
}

impl Frames {
    pub fn of(type_line: &str) -> Self {
        let has = |word: &str| type_line.split_whitespace().any(|w| w == word);
        Frames {
            saga: has("Saga"),
            class: has("Class"),
            planeswalker: has("Planeswalker"),
        }
    }
}

/// A planeswalker's text box: its abilities in order, static ones printed
/// across the box and loyalty abilities each in a band beside its cost
/// (CR 606.2). It holds at least one loyalty ability; with none it is plain.
#[derive(Debug, Clone, PartialEq)]
pub struct Planeswalker {
    abilities: Vec<Ability>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ability {
    Static(Line),
    Loyalty(LoyaltyCost, Line),
}

/// A loyalty symbol's cost: loyalty counters put on, removed, or neither
/// (CR 606.4). Adding or removing none is `Zero`, so `+0` cannot be held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoyaltyCost {
    Plus(Amount),
    Minus(Amount),
    Zero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Amount {
    Number(std::num::NonZeroU32),
    X,
}

impl std::fmt::Display for LoyaltyCost {
    /// As Oracle text writes it, with a true minus sign: `+1`, `−X`, `0`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let amount = |a: &Amount| match a {
            Amount::Number(n) => n.to_string(),
            Amount::X => "X".into(),
        };
        match self {
            LoyaltyCost::Plus(a) => write!(f, "+{}", amount(a)),
            LoyaltyCost::Minus(a) => write!(f, "\u{2212}{}", amount(a)),
            LoyaltyCost::Zero => write!(f, "0"),
        }
    }
}

impl Planeswalker {
    pub fn abilities(&self) -> &[Ability] {
        &self.abilities
    }
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
    Loyalty(Result<LoyaltyCost>, &'a str),
    Plain,
}

/// `+1: …`, `−2: …`, `0: …`, `−X: …`, with Oracle's minus sign or a typed
/// hyphen or en dash. A cost of none is written `0`; `+0` or `−0` is the
/// marker of a loyalty ability, but one that is refused.
fn loyalty(line: &str) -> Option<(Result<LoyaltyCost>, &str)> {
    let (cost, text) = line.split_once(": ")?;
    let (sign, amount) = match cost.chars().next()? {
        '+' => (Some(true), &cost[1..]),
        c @ ('\u{2212}' | '-' | '\u{2013}') => (Some(false), &cost[c.len_utf8()..]),
        _ => (None, cost),
    };
    let amount = match amount {
        "X" => Some(Amount::X),
        n if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0') => {
            Some(Amount::Number(n.parse().ok()?))
        }
        "0" => None,
        _ => return None,
    };
    let cost = match (sign, amount) {
        (Some(true), Some(a)) => Ok(LoyaltyCost::Plus(a)),
        (Some(false), Some(a)) => Ok(LoyaltyCost::Minus(a)),
        (None, None) => Ok(LoyaltyCost::Zero),
        (Some(_), None) => Err(anyhow::anyhow!(
            "a loyalty cost of nothing is written \"0:\", not {cost:?}"
        )),
        // A bare number is no loyalty cost: `2: …` is not Oracle text.
        (None, Some(_)) => return None,
    };
    Some((cost, text))
}

fn marked(line: &str) -> Marked<'_> {
    if let Some((cost, text)) = loyalty(line) {
        return Marked::Loyalty(cost, text);
    }
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
    /// Read the lines of rules text into the layout their markers ask for,
    /// on a card whose type line has `frames`.
    pub fn parse(lines: Vec<Line>, frames: Frames) -> Result<Self> {
        let mut kinds: Vec<Marked> = lines.iter().map(|l| marked(l)).collect();
        if !frames.planeswalker {
            for k in &mut kinds {
                if matches!(k, Marked::Loyalty(..)) {
                    *k = Marked::Plain;
                }
            }
        }
        let first = |want: fn(&Marked) -> bool| {
            lines
                .iter()
                .zip(&kinds)
                .find(|(_, k)| want(k))
                .map(|(l, _)| l)
        };
        let saga = first(|k| matches!(k, Marked::Chapter(..)));
        let class = first(|k| matches!(k, Marked::LevelBar(..)));
        let station = first(|k| matches!(k, Marked::Threshold(..)));
        let walker = first(|k| matches!(k, Marked::Loyalty(..)));
        if let (Some(line), false) = (saga, frames.saga) {
            bail!(
                "{line:?} is a Saga's chapter, but the type line has no Saga: \
                 chapters print only on a Saga's frame"
            );
        }
        if let (Some(line), false) = (class, frames.class) {
            bail!(
                "{line:?} is a Class's level bar, but the type line has no Class: \
                 levels print only on a Class's frame"
            );
        }
        match (saga, class, station, walker) {
            (None, None, None, None) => Ok(Rules::Plain(lines)),
            (Some(_), None, None, None) => saga_of(&lines, kinds).map(Rules::Saga),
            (None, Some(_), None, None) => class_of(&lines, kinds).map(Rules::Class),
            (None, None, Some(_), None) => station_of(&lines, kinds).map(Rules::Station),
            (None, None, None, Some(_)) => walker_of(&lines, kinds).map(Rules::Planeswalker),
            _ => bail!(
                "rules text has the markers of more than one of a Saga's chapters, \
                 a Class's levels, a Spacecraft's station thresholds and a \
                 planeswalker's loyalty abilities"
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
            Rules::Planeswalker(p) => p
                .abilities
                .iter()
                .map(|a| match a {
                    Ability::Static(line) => line.clone(),
                    Ability::Loyalty(cost, text) => format!("{cost}: {text}"),
                })
                .collect(),
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

fn walker_of(lines: &[Line], kinds: Vec<Marked>) -> Result<Planeswalker> {
    let abilities = lines
        .iter()
        .zip(kinds)
        .map(|(line, kind)| {
            Ok(match kind {
                Marked::Loyalty(cost, text) => Ability::Loyalty(cost?, text.into()),
                _ => Ability::Static(line.clone()),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Planeswalker { abilities })
}

/// Words that open an ability before a long dash and are printed in
/// italics: an ability word (CR 207.2c) or a flavor word (CR 207.2d), as
/// `Landfall — …` and `Vicious Mockery — …` are. The CR lists ability words
/// but not flavor words, which each set coins, so rather than a list this
/// takes any words at a line's start before " — ", and leaves upright what
/// printed cards set upright in that place:
///
/// - a keyword the CR writes with a long dash after it, as `Companion —` and
///   `Exhaust —` are (CR 702);
/// - anything with a symbol in it, a cost: `Prototype {2}{R} —`, spree's
///   `+ {1} —`;
/// - a mode's name after a bullet, `• Khans —`, which Citadel Siege prints
///   upright;
/// - an instruction whose options follow the dash: `Choose one — …; or …`
///   (CR 700.2) and `… faces a villainous choice — …, or …` (CR 701.55a);
/// - a Saga's chapter numerals, and anything not opening with a letter, as a
///   die roll's `1 —` does.
///
/// Returns the words and the rest of the line from the dash on.
pub fn ability_word(line: &str) -> Option<(&str, &str)> {
    // The CR's own forms, `“Boast — [Cost]: [Effect]”` and the like.
    const DASHED_KEYWORDS: &[&str] = &[
        "Boast",
        "Companion",
        "Exhaust",
        "Forecast",
        "Max speed",
        "Power-up",
        "Solved",
        "To solve",
        "Visit",
    ];
    let at = line.find(" — ")?;
    let (words, rest) = line.split_at(at);
    let opens_with_letter = words.chars().next().is_some_and(char::is_alphabetic);
    if !opens_with_letter
        || words.contains(['{', '(', '"', '“'])
        || rest.trim_start_matches(" — ").is_empty()
        || words.starts_with("Choose ")
        || words.ends_with("villainous choice")
        || words.split(", ").all(|n| roman(n).is_some())
        || DASHED_KEYWORDS
            .iter()
            .any(|k| words.eq_ignore_ascii_case(k))
    {
        return None;
    }
    Some((words, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a card with every frame, as each test reads one layout.
    fn parse(text: &str) -> Result<Rules> {
        let all = Frames {
            saga: true,
            class: true,
            planeswalker: true,
        };
        Rules::parse(text.lines().map(String::from).collect(), all)
    }

    fn parse_on(text: &str, type_line: &str) -> Result<Rules> {
        Rules::parse(
            text.lines().map(String::from).collect(),
            Frames::of(type_line),
        )
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
            "a\n+1: b\n\u{2212}X: c\n0: d\ne",
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

    #[test]
    fn a_planeswalker_is_its_abilities_loyalty_ones_beside_their_costs() {
        // Karn, the Great Creator's Oracle text: a static ability first.
        let karn = "Activated abilities of artifacts your opponents control can't be activated.
+1: Until your next turn, up to one target noncreature artifact becomes an artifact creature with power and toughness each equal to its mana value.
\u{2212}2: You may reveal an artifact card you own from outside the game or choose a face-up artifact card you own in exile. Put that card into your hand.";
        let Rules::Planeswalker(walker) = parse_on(karn, "Legendary Planeswalker — Karn").unwrap()
        else {
            panic!("not a planeswalker")
        };
        let one = Amount::Number(1.try_into().unwrap());
        let two = Amount::Number(2.try_into().unwrap());
        let costs: Vec<_> = walker
            .abilities()
            .iter()
            .map(|a| match a {
                Ability::Static(_) => None,
                Ability::Loyalty(cost, _) => Some(*cost),
            })
            .collect();
        assert_eq!(
            costs,
            [
                None,
                Some(LoyaltyCost::Plus(one)),
                Some(LoyaltyCost::Minus(two))
            ]
        );
    }

    #[test]
    fn a_loyalty_cost_may_be_typed_with_a_hyphen_and_is_written_with_a_minus() {
        // Ugin, the Spirit Dragon's −X, typed as -X.
        let rules = parse_on(
            "+2: Ugin deals 3 damage to any target.\n-X: Exile each permanent with mana value X or less that's one or more colors.\n0: Draw a card.",
            "Legendary Planeswalker — Ugin",
        )
        .unwrap();
        let lines = rules.lines();
        assert!(lines[1].starts_with("\u{2212}X: Exile"), "{lines:?}");
        assert!(lines[2].starts_with("0: Draw"));
    }

    #[test]
    fn a_loyalty_cost_of_plus_or_minus_nothing_is_refused() {
        let e = format!(
            "{:#}",
            parse_on("+0: Draw a card.", "Planeswalker — Jace").unwrap_err()
        );
        assert!(e.contains("written \"0:\""), "{e}");
    }

    #[test]
    fn loyalty_abilities_on_a_card_that_is_no_planeswalker_are_plain_lines() {
        let text = "+1: Draw a card.\n\u{2212}2: Discard a card.";
        assert!(matches!(
            parse_on(text, "Legendary Creature — Human Wizard").unwrap(),
            Rules::Plain(_)
        ));
        // And a bare number is no cost at all.
        assert!(matches!(
            parse_on("2: Draw a card.", "Planeswalker — Jace").unwrap(),
            Rules::Plain(_)
        ));
    }

    #[test]
    fn chapters_and_levels_need_the_type_line_that_has_their_frame() {
        let err = |text: &str, ty: &str| format!("{:#}", parse_on(text, ty).unwrap_err());
        assert!(err(ELDEST, "Enchantment").contains("has no Saga"));
        assert!(err("a\n{2}{U}: Level 2\nb", "Enchantment — Aura").contains("has no Class"));
        assert!(matches!(
            parse_on(ELDEST, "Enchantment — Saga").unwrap(),
            Rules::Saga(_)
        ));
        // A station card is any card with station symbols (CR 702.184b).
        assert!(matches!(
            parse_on("Station\n3+ | Flying", "Artifact").unwrap(),
            Rules::Station(_)
        ));
    }

    #[test]
    fn ability_and_flavor_words_are_the_words_before_a_dash_that_are_no_keyword() {
        fn word(line: &str) -> Option<&str> {
            ability_word(line).map(|(w, _)| w)
        }
        // Lotus Cobra, Vivisection Evangelist (corrupted is not in CR
        // 207.2c's list), Insufferable Balladeer and Kain, Traitorous
        // Dragoon, as Scryfall has them.
        for (line, want) in [
            ("Landfall — Whenever a land you control enters, add one mana of any color.", "Landfall"),
            ("Corrupted — When this creature enters, if an opponent has three or more poison counters, destroy target creature or planeswalker an opponent controls.", "Corrupted"),
            ("Vicious Mockery — When this creature enters, target creature an opponent controls can't block this turn.", "Vicious Mockery"),
            ("Jump — During your turn, Kain has flying.", "Jump"),
            ("Mega Flare — This creature deals damage equal to the total mana value of other permanents you control to each opponent.", "Mega Flare"),
            ("Descend 4 — This creature gets +1/+1.", "Descend 4"),
        ] {
            assert_eq!(word(line), Some(want), "{line}");
        }
        for line in [
            "Companion — Each nonland card in your starting deck has a different name.",
            "Exhaust — {4}: Draw a card.",
            "• Khans — At the beginning of combat on your turn, put two +1/+1 counters on target creature you control.",
            "Choose one — Noxious Hydra Breath deals 5 damage to each player; or destroy each tapped non-Head creature.",
            "Each opponent faces a villainous choice — They lose 3 life, or you draw a card.",
            "Prototype {2}{R} — 3/2",
            "I, II — Draw a card.",
            "Suspend 4—{U}",
            "Flying",
            "Choose one —",
        ] {
            assert_eq!(word(line), None, "{line}");
        }
    }
}
