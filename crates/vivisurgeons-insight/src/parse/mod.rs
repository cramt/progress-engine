//! Recursive descent over tokens. Every rule either consumes tokens into a
//! typed node or fails and restores the position: nothing swallows "the rest
//! of the sentence", so a card that parses is a card whose every word was read.

use crate::ast::*;
use crate::lex::{Pt, Signed, Tok};
use crate::vocab::{singulars, vocab};

pub struct P<'a> {
    t: &'a [Tok],
    pub i: usize,
    /// How far any attempt got, for reporting where a failed line broke.
    pub furthest: usize,
    /// The subject a verb inherits when it has none of its own, as after
    /// "that player may".
    implied: Option<Player>,
    /// Inside "Until end of turn, …", whose clauses need no duration of
    /// their own.
    during: bool,
    /// Per starting token: the object phrase read there, where it ended, and
    /// how far reading it got.
    objects_memo: Vec<Option<(Option<Objects>, usize, usize)>>,
    /// Inside a comparison's value, where "from your graveyard" or "from
    /// among them" after it belongs to the object being compared: "a card
    /// with mana value less than the number of counters on that creature
    /// from your graveyard".
    in_value: bool,
}

macro_rules! alt {
    ($p:ident; $($f:expr),+ $(,)?) => {
        'alt: {
            $( if let Some(v) = $p.attempt($f) { break 'alt Some(v); } )+
            None
        }
    };
}

impl<'a> P<'a> {
    pub fn new(t: &'a [Tok]) -> Self {
        P {
            t,
            i: 0,
            furthest: 0,
            implied: None,
            during: false,
            objects_memo: vec![None; t.len() + 1],
            in_value: false,
        }
    }

    pub fn peek(&self) -> Option<&'a Tok> {
        self.t.get(self.i)
    }

    fn peek_word(&self) -> Option<&'a str> {
        match self.peek() {
            Some(Tok::Word(w)) => Some(w.as_str()),
            _ => None,
        }
    }

    pub fn at_end(&self) -> bool {
        self.i >= self.t.len()
    }

    fn bump(&mut self) {
        self.i += 1;
        self.furthest = self.furthest.max(self.i);
    }

    pub fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let start = self.i;
        let r = f(self);
        if r.is_none() {
            self.i = start;
        }
        r
    }

    fn tok(&mut self, t: &Tok) -> Option<()> {
        (self.peek() == Some(t)).then(|| self.bump())
    }

    fn word(&mut self, w: &str) -> Option<()> {
        (self.peek_word() == Some(w)).then(|| self.bump())
    }

    /// A space-separated phrase, all or nothing. `'s` is a possessive, `,`
    /// `.` and `:` are punctuation, and `~` is the card itself.
    fn words(&mut self, ws: &str) -> Option<()> {
        // Most phrases fail on their first word, so that is checked against
        // the phrase's prefix before anything is split: the grammar tries
        // thousands of phrases per line.
        let first = match self.peek() {
            Some(Tok::Word(w)) => w.as_str(),
            Some(Tok::Possessive) if ws.starts_with("'s") => "'s",
            Some(Tok::Possessive) => "'",
            Some(Tok::Comma) => ",",
            Some(Tok::Period) => ".",
            Some(Tok::Colon) => ":",
            Some(Tok::Dash) => "—",
            Some(Tok::SelfRef) => "~",
            _ => return None,
        };
        let rest = ws.strip_prefix(first)?;
        let rest = match rest.strip_prefix(' ') {
            Some(r) => r,
            None if rest.is_empty() => rest,
            None => return None,
        };
        self.attempt(|p| {
            p.bump();
            for w in rest.split(' ').filter(|w| !w.is_empty()) {
                if !p.at(w) {
                    return None;
                }
                p.bump();
            }
            Some(())
        })
    }

    /// Whether the next token is the one `w` writes: a word, or `'s` `,` `.`
    /// `:` `—` `~` for the punctuation and the card itself.
    fn at(&self, w: &str) -> bool {
        let t = match w {
            "'s" | "'" => Tok::Possessive,
            "," => Tok::Comma,
            "." => Tok::Period,
            ":" => Tok::Colon,
            "—" => Tok::Dash,
            "~" => Tok::SelfRef,
            _ => return self.peek_word() == Some(w),
        };
        self.peek() == Some(&t)
    }

    /// The first of `options` that matches, longest written first by the caller.
    fn one_of(&mut self, options: &[&'static str]) -> Option<&'static str> {
        options.iter().copied().find(|o| self.words(o).is_some())
    }

    /// A verb in either number: `gain` or `gains`. Only its first word
    /// takes the number: "phase out", "phases out".
    fn verb(&mut self, base: &str) -> Option<()> {
        let mut words = base.split(' ');
        let first = words.next()?;
        let w = self.peek_word()?;
        if w != first && !third_person(first, w) {
            return None;
        }
        self.attempt(|p| {
            p.bump();
            for w in words {
                p.word(w)?;
            }
            Some(())
        })
    }

    /// A sentence's period, which Oracle text leaves out after a closing
    /// quote whose ability ended with its own: `gains "{T}: Add {C}."`
    fn end_sentence(&mut self) -> Option<()> {
        if self.tok(&Tok::Period).is_some() {
            return Some(());
        }
        let before = &self.t[..self.i];
        let quotes = before
            .iter()
            .rev()
            .take_while(|t| **t == Tok::Quote)
            .count();
        let quoted_period =
            quotes > 0 && before.len() > quotes && before[before.len() - quotes - 1] == Tok::Period;
        // `with "{T}: Add {C}" and loses …`: a quoted ability whose last
        // sentence has no period ends at its closing quote.
        let closing = self.peek() == Some(&Tok::Quote)
            && before.iter().filter(|t| **t == Tok::Quote).count() % 2 == 1;
        (quoted_period || closing).then_some(())
    }

    fn end_of_clause(&self) -> bool {
        matches!(
            self.peek(),
            None | Some(Tok::Period | Tok::Comma | Tok::Quote | Tok::Semi)
        ) || self.peek_word() == Some("and")
            || self.peek_word() == Some("then")
    }
}

/// Whether `w` is `base` in the third person singular, without building it:
/// the grammar asks this on most tokens of every line.
fn third_person(base: &str, w: &str) -> bool {
    match base {
        "have" => return w == "has",
        "do" => return w == "does",
        _ => {}
    }
    let es =
        base.ends_with('s') || base.ends_with("sh") || base.ends_with("ch") || base.ends_with('x');
    if es {
        return w.len() == base.len() + 2 && w.starts_with(base) && w.ends_with("es");
    }
    if let Some(stem) = base.strip_suffix('y').filter(|_| !base.ends_with("ay")) {
        return w.len() == stem.len() + 3 && w.starts_with(stem) && w.ends_with("ies");
    }
    w.len() == base.len() + 1 && w.starts_with(base) && w.ends_with('s')
}

const NUMBERS: [&str; 21] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];

mod amounts;
mod conditions;
mod effects;
mod keywords;
mod mana;
mod objects;
mod sentences;
mod statics;
mod triggers;
mod zones;

use amounts::*;
use conditions::*;
use effects::*;
use keywords::*;
use mana::*;
use objects::*;
use sentences::*;
use statics::*;
use triggers::*;
use zones::*;

// ---------------------------------------------------------------- abilities

fn limits(p: &mut P) -> Vec<Limit> {
    let mut v = Vec::new();
    while let Some(l) = p.attempt(|p| {
        let l = alt!(p;
            |p| p.words("activate only as a sorcery").map(|_| vec![Limit::SorcerySpeed]),
            |p| p.words("activate only as an instant").map(|_| vec![Limit::InstantSpeed]),
            |p| p.words("activate only once each turn").map(|_| vec![Limit::OncePerTurn]),
            |p| p.words("activate only during your turn").map(|_| vec![Limit::OnlyDuringYourTurn]),
            |p| {
                p.words("activate only if")?;
                let mut v = vec![Limit::OnlyIf(condition(p)?)];
                // "… and only as a sorcery": two limits, "activate" said once.
                if p.attempt(|p| p.words("and only as a sorcery")).is_some() {
                    v.push(Limit::SorcerySpeed);
                }
                Some(v)
            },
            |p| p.words("this ability triggers only once each turn").map(|_| vec![Limit::OncePerTurn]),
            |p| p.words("do this only once each turn").map(|_| vec![Limit::DoOnlyOncePerTurn]),
            |p| {
                p.words("spend only mana of")?;
                let pal = palette_named(p)?;
                p.words("to activate this ability")?;
                Some(vec![Limit::SpendOnlyManaOf(pal)])
            },
            |p| {
                p.words("spend this mana only")?;
                let uses = spend_uses(p)?;
                // "…, and that spell can't be countered": a rule for what the mana paid for.
                let then = p.attempt(|p| {
                    p.words(", and")?;
                    let what = pronoun(p)?;
                    let rule = restriction(p)?;
                    Some((what, rule))
                });
                Some(vec![Limit::SpendOnly { uses, then }])
            },
            |p| {
                p.words("this ability costs")?;
                let by = mana(p)?;
                let less = p.one_of(&["less", "more"])? == "less";
                p.words("to activate")?;
                let per = p.attempt(|p| { p.words("for each")?; objects(p) });
                Some(vec![Limit::CostChange { by, less, per }])
            },
        )?;
        p.tok(&Tok::Period)?;
        Some(l)
    }) {
        v.extend(l);
    }
    v
}

/// "to cast a creature spell of the chosen type or activate an ability of …".
fn spend_uses(p: &mut P) -> Option<Vec<SpendUse>> {
    let one = |p: &mut P| {
        alt!(p;
            |p| { p.words("to cast")?; objects(p).map(SpendUse::Cast) },
            |p| {
                p.attempt(|p| p.word("to"));
                p.one_of(&["activate an ability of", "activate abilities of"])?;
                objects(p).map(SpendUse::ActivateAbilityOf)
            },
        )
    };
    let mut v = vec![one(p)?];
    while let Some(u) = p.attempt(|p| {
        p.word("or")?;
        one(p)
    }) {
        v.push(u);
    }
    Some(v)
}

fn finished(p: &P) -> bool {
    matches!(p.peek(), None | Some(Tok::Quote))
}

/// One ability. `spell` says whether a bare effect is the card's spell text
/// (an instant or sorcery) or has to be a static ability (a permanent).
pub fn ability(p: &mut P, spell: bool) -> Option<Ability> {
    alt!(p;
        |p| {
            let k = keyword_line(p)?;
            p.attempt(|p| p.tok(&Tok::Period));
            finished(p).then_some(Ability::Keywords(k))
        },
        |p| {
            let change = match p.peek()? {
                Tok::Signed(s) => *s,
                Tok::Num(0) => Signed::N(0),
                _ => return None,
            };
            p.bump();
            p.tok(&Tok::Colon)?;
            let effect = effects(p)?;
            finished(p).then_some(Ability::Loyalty { change, effect })
        },
        |p| {
            p.word("level")?;
            let from = number(p)?;
            let to = match p.peek()? {
                Tok::Signed(Signed::N(n)) if *n < 0 => Some(n.unsigned_abs()),
                Tok::Other('+') => None,
                _ => return None,
            };
            p.bump();
            finished(p).then_some(Ability::LevelHeader { from, to })
        },
        |p| match p.peek()? {
            Tok::Pt(pt) if !pt.signed => {
                let pt = *pt;
                p.bump();
                finished(p).then_some(Ability::LevelPt(pt))
            }
            _ => None,
        },
        // "+ {1} — Copy target instant spell …": a spree mode.
        |p| {
            p.tok(&Tok::Other('+'))?;
            let cost = mana(p)?;
            p.tok(&Tok::Dash)?;
            let effect = effects(p)?;
            finished(p).then_some(Ability::Spree { cost, effect })
        },
        |p| {
            let mut chapters = Vec::new();
            loop {
                let n = ["i", "ii", "iii", "iv", "v", "vi"].iter().position(|r| p.word(r).is_some())? as u32 + 1;
                chapters.push(n);
                if p.attempt(|p| p.tok(&Tok::Comma)).is_none() { break; }
            }
            p.tok(&Tok::Dash)?;
            let effect = effects(p)?;
            finished(p).then_some(Ability::Chapter { chapters, effect })
        },
        |p| {
            let cost = costs(p)?;
            p.tok(&Tok::Colon)?;
            // A Class's "{1}{G}: Level 2" is the whole ability, with no period.
            let effect = match p.attempt(|p| { p.word("level")?; let n = number(p)?; p.at_end().then_some(n) }) {
                Some(n) => vec![Effect::ClassLevel(n)],
                None => effects(p)?,
            };
            let limits = limits(p);
            finished(p).then_some(Ability::Activated { cost, effect, limits })
        },
        |p| {
            let trigger = trigger(p)?;
            p.tok(&Tok::Comma)?;
            alt!(p;
                |p| {
                    p.word("if")?;
                    let cond = condition(p)?;
                    p.tok(&Tok::Comma)?;
                    let effect = effects(p)?;
                    let limits = limits(p);
                    let trigger = Trigger::InterveningIf(Box::new(trigger.clone()), Box::new(cond));
                    finished(p).then_some(Ability::Triggered { trigger, effect, limits })
                },
                |p| {
                    let effect = effects(p)?;
                    let limits = limits(p);
                    finished(p).then_some(Ability::Triggered { trigger: trigger.clone(), effect, limits })
                },
            )
        },
        |p| { if !spell { return None; } let e = effects(p)?; finished(p).then_some(Ability::Spell(e)) },
        |p| { let s = statics(p)?; finished(p).then_some(Ability::Static(s)) },
        |p| {
            let at_least = number(p)?;
            p.tok(&Tok::Other('+'))?;
            p.tok(&Tok::Other('|'))?;
            let inner = ability(p, spell)?;
            Some(Ability::Station { at_least, ability: Box::new(inner) })
        },
        |p| {
            p.tok(&Tok::Other('∞'))?;
            p.tok(&Tok::Dash)?;
            let inner = ability(p, spell)?;
            Some(Ability::Harnessed(Box::new(inner)))
        },
        |p| {
            // An ability word or flavour word: a label, then the ability.
            let mut label = Vec::new();
            while let Some(w) = p.peek_word() { label.push(w.to_string()); p.bump(); if label.len() > 6 { return None; } }
            if label.is_empty() { return None; }
            p.tok(&Tok::Dash)?;
            let inner = ability(p, spell)?;
            Some(Ability::Labelled { label: label.join(" "), ability: Box::new(inner) })
        },
    )
}

/// A whole ability line, which must be consumed to the last token.
pub fn parse_line(toks: &[Tok], spell: bool) -> Result<Ability, usize> {
    let mut p = P::new(toks);
    match ability(&mut p, spell) {
        Some(a) if p.at_end() => Ok(a),
        _ => Err(p.furthest),
    }
}
