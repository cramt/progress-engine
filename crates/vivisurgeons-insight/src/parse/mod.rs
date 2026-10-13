//! Recursive descent over tokens. Every rule either consumes tokens into a
//! typed node or fails and restores the position: nothing swallows "the rest
//! of the sentence", so a card that parses is a card whose every word was read.

use crate::ast::*;
use crate::lex::{Pt, Signed, Tok};
use crate::vocab::{singulars, vocab};
use crate::walk::{Visitor, Walk};

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
    /// The same for a player: most effect rules ask for a subject first.
    player_memo: Vec<Option<(Option<Player>, usize, usize)>>,
    /// Inside a comparison's value, where "from your graveyard" or "from
    /// among them" after it belongs to the object being compared: "a card
    /// with mana value less than the number of counters on that creature
    /// from your graveyard".
    in_value: bool,
    /// Whether the line is an instant or sorcery's, whose paragraphs are all
    /// one spell's: "the exiled card" there is what another paragraph of
    /// the same spell exiled.
    spell: bool,
    /// A spell's paragraph after its first, which alone may open with
    /// "Then" ([then-paragraph]).
    continues: bool,
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
            player_memo: vec![None; t.len() + 1],
            in_value: false,
            spell: false,
            continues: false,
        }
    }

    pub fn peek(&self) -> Option<&'a Tok> {
        self.t.get(self.i)
    }

    /// Whether the next token is inside a quoted ability. A function of the
    /// position alone, so the memos stay sound.
    fn in_quote(&self) -> bool {
        self.t[..self.i.min(self.t.len())]
            .iter()
            .filter(|t| **t == Tok::Quote)
            .count()
            % 2
            == 1
    }

    /// "… you gain half X life and draw half X cards. Round down each
    /// time.": the rounding a later sentence of the line says for every
    /// half before it, `Some(true)` for up. A function of the position
    /// alone, so the memos stay sound.
    fn rounds_later(&self) -> Option<bool> {
        let rest = &self.t[self.i.min(self.t.len())..];
        rest.windows(5).find_map(|w| match w {
            [Tok::Period, Tok::Word(r), Tok::Word(d), Tok::Word(e), Tok::Word(t)]
                if r == "round" && e == "each" && t == "time" && (d == "up" || d == "down") =>
            {
                Some(d == "up")
            }
            _ => None,
        })
    }

    /// Whether a "half" was said earlier on the line.
    fn halved_before(&self) -> bool {
        self.t[..self.i.min(self.t.len())]
            .iter()
            .any(|t| matches!(t, Tok::Word(w) if w == "half"))
    }

    /// The card's name: the card itself, or inside a quoted ability the
    /// object that granted it (CR 201.5a).
    fn card_name(&mut self) -> Option<Objects> {
        let granted = self.in_quote();
        self.tok(&Tok::SelfRef)?;
        Some(Objects {
            quant: if granted {
                Quant::Granter
            } else {
                Quant::SelfRef
            },
            filter: Filter::default(),
        })
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
        let open = before.iter().filter(|t| **t == Tok::Quote).count() % 2 == 1;
        let closing = self.peek() == Some(&Tok::Quote) && open;
        // `has "Whenever …, draw a card," and is a Rogue`: a quoted ability
        // in a list ends in the list's comma, set inside the quote.
        // Only before "and": a comma before anything else is the sentence's
        // own ("with "…," where X is …"), which `quoted` has no place for.
        if open
            && self.peek() == Some(&Tok::Comma)
            && self.t.get(self.i + 1) == Some(&Tok::Quote)
            && matches!(self.t.get(self.i + 2), Some(Tok::Word(w)) if w == "and")
        {
            self.bump();
            return Some(());
        }
        (quoted_period || closing).then_some(())
    }

    /// Whether `ws` is said somewhere before this sentence's period: a cheap
    /// look ahead that spares a rule reading a long phrase only to fail on
    /// the words that make it what it is.
    fn ahead(&self, ws: &str) -> bool {
        let want: Vec<&str> = ws.split(' ').collect();
        let rest = &self.t[self.i.min(self.t.len())..];
        let end = rest
            .iter()
            .position(|t| *t == Tok::Period)
            .unwrap_or(rest.len());
        rest[..end].windows(want.len()).any(|w| {
            w.iter().zip(&want).all(|(t, s)| {
                matches!(t, Tok::Word(x) if x == s) || (*s == "\"" && *t == Tok::Quote)
            })
        })
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
pub(crate) fn third_person(base: &str, w: &str) -> bool {
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

pub(crate) const NUMBERS: [&str; 21] = [
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
            // "Activate only if … and only once each turn": "activate only"
            // said once for several limits.
            |p| {
                p.words("activate only")?;
                let mut v = vec![activate_only(p)?];
                while let Some(l) = p.attempt(|p| {
                    p.one_of(&[", and only", "and only"])?;
                    activate_only(p)
                }) {
                    v.push(l);
                }
                Some(v)
            },
            |p| p.words("this ability triggers only once each turn").map(|_| vec![Limit::OncePerTurn]),
            // "This ability can't be copied and X can't be 0."
            |p| {
                p.words("this ability can't be copied")?;
                let mut v = vec![Limit::CantBeCopied];
                if let Some(x) = p.attempt(|p| { p.word("and")?; x_limit(p) }) {
                    v.push(Limit::X(x));
                }
                Some(v)
            },
            |p| x_limit(p).map(|x| vec![Limit::X(x)]),
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
                let mut cond = p.attempt(|p| { p.word("if")?; condition(p) });
                // "This ability costs {1} less to activate if it targets a
                // creature with power 3 or less": "it" is this ability, the
                // subject of the sentence.
                if let Some(Condition::Targets { what, .. }) = cond.as_mut() {
                    if *what == antecedent() {
                        what.quant = Quant::ThisAbility;
                    }
                }
                Some(vec![Limit::CostChange { by, less, per, cond }])
            },
        )?;
        p.tok(&Tok::Period)?;
        Some(l)
    }) {
        v.extend(l);
    }
    v
}

/// "X can't be 0", "X can't be greater than …" (CR 107.3a).
pub(super) fn x_limit(p: &mut P) -> Option<XLimit> {
    p.words("x can't be")?;
    alt!(p;
        |p| p.tok(&Tok::Num(0)).map(|_| XLimit::NotZero),
        |p| { p.words("greater than")?; measure(p).map(XLimit::AtMost) },
    )
}

/// What follows "activate only": "as a sorcery", "once each turn", "if …".
fn activate_only(p: &mut P) -> Option<Limit> {
    alt!(p;
        |p| p.words("as a sorcery").map(|_| Limit::SorcerySpeed),
        |p| p.words("as an instant").map(|_| Limit::InstantSpeed),
        |p| p.words("once each turn").map(|_| Limit::OncePerTurn),
        |p| p.word("once").map(|_| Limit::Once),
        |p| p.words("during your turn").map(|_| Limit::OnlyDuringYourTurn),
        |p| p.words("during your upkeep").map(|_| Limit::OnlyDuringYourUpkeep),
        |p| { p.word("if")?; condition(p).map(Limit::OnlyIf) },
    )
}

/// "to cast a creature spell of the chosen type or activate an ability of …".
pub(super) fn spend_uses(p: &mut P) -> Option<Vec<SpendUse>> {
    let one = |p: &mut P| {
        alt!(p;
            |p| { p.words("to cast")?; objects(p).map(SpendUse::Cast) },
            |p| {
                p.attempt(|p| p.word("to"));
                p.one_of(&["activate an ability of", "activate abilities of"])?;
                objects(p).map(SpendUse::ActivateAbilityOf)
            },
            // "Spend this mana only to activate abilities": of anything.
            |p| {
                p.attempt(|p| p.word("to"));
                p.one_of(&["activate abilities", "activate an ability"])?;
                Some(SpendUse::ActivateAny)
            },
            // "Spend this mana only on costs that contain {X}".
            |p| {
                p.words("on costs that contain")?;
                match p.peek()? { Tok::Sym(s) if s == "X" => { p.bump(); Some(SpendUse::CostsWithX) } _ => None }
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
            // "Planeswalkers you control have "[+1]: Add {R}."": inside
            // quotes Oracle text brackets the loyalty cost.
            let bracket = p.attempt(|p| p.tok(&Tok::Other('['))).is_some();
            let change = match p.peek()? {
                Tok::Signed(s) => *s,
                Tok::Num(0) => Signed::N(0),
                _ => return None,
            };
            p.bump();
            if bracket {
                p.tok(&Tok::Other(']'))?;
            }
            p.tok(&Tok::Colon)?;
            let effect = effects(p)?;
            let mut effect = effect;
            refer_to_the_card_in(&[], &mut effect);
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
            // "I — Crescent Fang — Search …": a flavour word between the
            // chapter and its effect, with no rules meaning (CR 207.2d).
            // Tried second, so "I, II — Choose one — …" stays a modal.
            let effect = alt!(p;
                |p| { let e = effects(p)?; finished(p).then_some(e) },
                |p| {
                    let label = flavour_label(p)?;
                    let then = effects(p)?;
                    finished(p).then_some(vec![Effect::Labelled { label, then }])
                },
            )?;
            Some(Ability::Chapter { chapters, effect })
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
            let mut effect = effect;
            refer_to_the_card_in(&cost, &mut effect);
            finished(p).then_some(Ability::Activated { cost, effect, limits })
        },
        // "This spell can't be countered" on an instant is a static, as it
        // is on a creature: it works while the spell is on the stack (CR
        // 113.6g), not as the spell resolves.
        |p| {
            if !spell { return None; }
            let mut s = statics(p)?;
            refer_to_the_card(&mut s);
            other_than_the_subject(&mut s);
            (finished(p) && s.iter().all(about_this_spell)).then_some(Ability::Static(s))
        },
        |p| {
            let trigger = trigger(p)?;
            // "At the beginning of your next upkeep, pay …" on an instant or
            // sorcery is created as the spell resolves, a delayed trigger
            // (CR 603.7a), so it is the spell's effect, not a printed trigger.
            if spell && matches!(trigger, Trigger::At(_)) { return None; }
            p.tok(&Tok::Comma)?;
            alt!(p;
                |p| {
                    p.word("if")?;
                    let mut cond = condition(p)?;
                    p.tok(&Tok::Comma)?;
                    let mut effect = effects(p)?;
                    let limits = limits(p);
                    about_the_trigger(&trigger, Some(&mut cond), &mut effect);
                    x_of_the_spell(&trigger, &mut effect);
                    those_counters(&cond, &mut effect);
                    if let Trigger::When(e) = &trigger { other_than_the_trigger(e, Some(&mut cond), &mut effect); }
                    let trigger = Trigger::InterveningIf(Box::new(trigger.clone()), Box::new(cond));
                    finished(p).then_some(Ability::Triggered { trigger, effect, limits })
                },
                |p| {
                    let mut effect = effects(p)?;
                    let limits = limits(p);
                    about_the_trigger(&trigger, None, &mut effect);
                    x_of_the_spell(&trigger, &mut effect);
                    if let Trigger::When(e) = &trigger { other_than_the_trigger(e, None, &mut effect); }
                    finished(p).then_some(Ability::Triggered { trigger: trigger.clone(), effect, limits })
                },
            )
        },
        |p| { if !spell { return None; } let mut e = effects(p)?; other_than_named_before(&mut e); kicked_it(&mut e); finished(p).then_some(Ability::Spell(e)) },
        |p| { let mut s = statics(p)?; refer_to_the_card(&mut s); other_than_the_subject(&mut s); finished(p).then_some(Ability::Static(s)) },
        // "As this enchantment enters, choose Khans or Dragons. / • Khans —
        // … / • Dragons — …": anchor words (CR 614.12c). Each bullet's
        // ability is read alone, up to the next bullet.
        |p| {
            p.word("as")?;
            let what = objects(p)?;
            if what.quant != Quant::SelfRef { return None; }
            p.words("enters , choose")?;
            let first = p.peek_word()?.to_string();
            p.bump();
            p.word("or")?;
            let second = p.peek_word()?.to_string();
            p.bump();
            p.tok(&Tok::Period)?;
            let mut options = Vec::new();
            for word in [first, second] {
                p.tok(&Tok::Bullet)?;
                p.word(&word)?;
                p.tok(&Tok::Dash)?;
                let end = p.t[p.i..].iter().position(|t| *t == Tok::Bullet).map_or(p.t.len(), |n| p.i + n);
                let mut sub = P::new(&p.t[p.i..end]);
                let a = ability(&mut sub, false)?;
                if !sub.at_end() { return None; }
                p.i = end;
                options.push(AnchoredAbility { word, ability: Box::new(a) });
            }
            finished(p).then_some(Ability::Anchored { what, options })
        },
        // "Tiered / • Fire — {0} — ~ deals 1 damage to each creature. / …":
        // modes, each with its own additional cost (CR 702.183).
        |p| {
            p.word("tiered")?;
            let mut modes = Vec::new();
            while p.attempt(|p| p.tok(&Tok::Bullet)).is_some() {
                let mut label = Vec::new();
                while let Some(w) = p.peek_word() {
                    label.push(w.to_string());
                    p.bump();
                }
                if label.is_empty() || label.len() > 6 { return None; }
                p.tok(&Tok::Dash)?;
                let cost = mana(p)?;
                p.tok(&Tok::Dash)?;
                let mut effect = Vec::new();
                while !matches!(p.peek(), None | Some(Tok::Bullet)) {
                    effect.extend(sentence(p)?);
                }
                modes.push(TieredMode { label: label.join(" "), cost, effect });
            }
            (!modes.is_empty() && finished(p)).then_some(Ability::Tiered(modes))
        },
        // A station threshold is a permanent's (CR 702.184); in an instant
        // or sorcery "15+ |" is a die's result row, below.
        |p| {
            if spell { return None; }
            let at_least = number(p)?;
            p.tok(&Tok::Other('+'))?;
            p.tok(&Tok::Other('|'))?;
            let inner = ability(p, spell)?;
            Some(Ability::Station { at_least, ability: Box::new(inner) })
        },
        // "1—14 | …", "15+ | …", "20 | …": a row of a die roll's results
        // table, its effect done if the result is in its range (CR 706.3a).
        |p| {
            let from = number(p)?;
            let to = if p.attempt(|p| p.tok(&Tok::Other('+'))).is_some() {
                None
            } else if p.attempt(|p| p.tok(&Tok::Dash)).is_some() {
                Some(number(p)?)
            } else {
                Some(from)
            };
            p.tok(&Tok::Other('|'))?;
            let start = p.i;
            match ability(p, true)? {
                Ability::Spell(mut effect) => {
                    // A permanent's row is its own ability's effect, so the
                    // card rule holds there as in an activated ability
                    // (Iron Mastiff's "equal to its power").
                    if !spell {
                        refer_to_the_card_in(&[], &mut effect);
                        sacrifice_it_is_the_card(&p.t[start..p.i], &mut effect);
                    }
                    Some(Ability::DieRow { from, to, effect })
                }
                _ => None,
            }
        },
        |p| {
            p.tok(&Tok::Other('∞'))?;
            p.tok(&Tok::Dash)?;
            let inner = ability(p, spell)?;
            Some(Ability::Harnessed(Box::new(inner)))
        },
        // "Max speed — …", "Exhaust — {3}: …": a keyword before the dash,
        // not an ability word, since it adds rules to the ability (CR
        // 702.178a, 702.177a).
        |p| {
            let keyword = alt!(p;
                |p| p.words("max speed").map(|_| Prefix::MaxSpeed),
                |p| p.word("power-up").map(|_| Prefix::PowerUp),
                |p| p.word("exhaust").map(|_| Prefix::Exhaust),
                |p| p.word("boast").map(|_| Prefix::Boast),
                |p| p.word("forecast").map(|_| Prefix::Forecast),
                |p| p.word("solved").map(|_| Prefix::Solved),
            )?;
            p.tok(&Tok::Dash)?;
            let inner = ability(p, spell)?;
            let activated_only = !matches!(keyword, Prefix::MaxSpeed | Prefix::Solved);
            if activated_only && !matches!(inner, Ability::Activated { .. }) {
                return None;
            }
            Some(Ability::KeywordOn { keyword, ability: Box::new(inner) })
        },
        |p| {
            // An ability word or flavour word: a label, then the ability.
            let label = flavour_label(p)?;
            let inner = ability(p, spell)?;
            Some(Ability::Labelled { label, ability: Box::new(inner) })
        },
    )
}

/// An ability word or flavour word and its dash: "Landfall —", "Hero's
/// Sundering —", "Do You Like Squirrels? —", "~ Beam —". Neither has a
/// rules meaning (CR 207.2c, 207.2d), so the words are kept only to be said
/// again. A flavour word may name the card or end in "!" or "?".
pub(super) fn flavour_label(p: &mut P) -> Option<String> {
    let mut label: Vec<String> = Vec::new();
    loop {
        match p.peek()? {
            Tok::Word(_) | Tok::SelfRef => {
                label.push(crate::show(p.peek()?));
                p.bump();
            }
            // "Shiva's Aid", "Do You Like Squirrels?": the possessive and the
            // mark close up on the word before, as printed.
            Tok::Possessive | Tok::Other('!' | '?') => {
                let s = crate::show(p.peek()?);
                match label.last_mut() {
                    Some(last) => last.push_str(&s),
                    None => label.push(s),
                }
                p.bump();
            }
            Tok::Dash => break,
            _ => return None,
        }
        if label.len() > 6 {
            return None;
        }
    }
    p.tok(&Tok::Dash)?;
    let words = label.iter().any(|w| w.starts_with(char::is_alphabetic));
    words.then(|| label.join(" "))
}

/// Whether a static is a rule about the spell it is printed on, which an
/// instant or sorcery's line would otherwise read as something the spell
/// does as it resolves.
fn about_this_spell(s: &Static) -> bool {
    match s {
        Static::Restrict { what, .. } => what.quant == Quant::SelfRef,
        Static::If { then, .. } | Static::AsLongAs { then, .. } => {
            then.iter().all(about_this_spell)
        }
        _ => false,
    }
}

/// The Ozolith: "Whenever a creature you control leaves the battlefield, if
/// it had counters on it, put those counters on ~": "those counters" are
/// the counters the intervening "if" said it had, every one of them on that
/// object, as "put its counters on" says (Parish-Blade Trainee).
fn those_counters(cond: &Condition, effect: &mut [Effect]) {
    let c = match cond {
        Condition::Past(c) => &**c,
        c => c,
    };
    let Condition::HasCounter { what, kind } = c else {
        return;
    };
    for e in effect {
        if let Effect::PutThoseCounters { those, .. } = e {
            let bare = those.quant == Quant::Antecedent
                && those.filter.posts.is_empty()
                && those.filter.alts.is_empty()
                && those.filter.kind == Some(Noun::Counter(kind.clone()));
            if bare {
                those.quant = Quant::All;
                those.filter.posts.push(Post::On(Box::new(what.clone())));
            }
        }
    }
}

/// The object an event names, if it names one; of several events, the
/// first that does.
fn event_object(e: &Event) -> Option<&Objects> {
    match e {
        Event::Enters { what, .. }
        | Event::Dies(what)
        | Event::Sacrifices { what, .. }
        | Event::PutInto { what, .. } => Some(what),
        Event::Or(v) => v.iter().find_map(event_object),
        _ => None,
    }
}

/// Words a triggered ability says of the object its event named, which the
/// text leaves unstated. Valakut: "Whenever a Mountain you control enters,
/// if you control at least five other Mountains": other than the one that
/// entered (its ruling: "five or more Mountains other than that new one").
/// Orah: "return target Cleric card with lesser mana value": lesser than
/// the Cleric that died, where nothing before it in the effect names an
/// object to compare with.
fn about_the_trigger(t: &Trigger, mut cond: Option<&mut Condition>, effect: &mut [Effect]) {
    // "At the beginning of your draw step, if this artifact is tapped, it
    // deals 1 damage to you": no event object, but the card is named.
    let Trigger::When(e) = t else {
        return refer(None, false, cond, effect);
    };
    if matches!(**e, Event::Casts { .. }) || event_object(e).is_some() {
        lesser_than_the_trigger(effect);
    }
    let Some(subject) = event_object(e) else {
        refer_to_the_trigger(e, cond, effect);
        return;
    };
    let triggering = || Objects {
        quant: Quant::Triggering,
        filter: Filter::default(),
    };
    if let Some(Condition::YouControl(counted)) = cond.as_deref_mut() {
        let same_kind = |g: &Vec<Adj>| {
            g.first() == Some(&Adj::State(State::Other))
                && subject.filter.alts == vec![g[1..].to_vec()]
        };
        if subject.quant != Quant::SelfRef
            && counted.filter.alts.len() == 1
            && same_kind(&counted.filter.alts[0])
        {
            counted.filter.alts[0][0] = Adj::OtherThan(Box::new(triggering()));
        }
    }
    fn lesser(e: &mut Effect, to: &Objects) {
        let what = match e {
            Effect::May { then, .. } => return then.iter_mut().take(1).for_each(|e| lesser(e, to)),
            Effect::Move { what, .. } | Effect::Cast { what, .. } | Effect::Search { what, .. } => {
                what
            }
            Effect::UntilFound { until, .. } => until,
            _ => return,
        };
        for q in &mut what.filter.posts {
            if let Post::With(Compare {
                value: Amount::ManaValueOf(x),
                ..
            }) = q
            {
                if **x == antecedent() {
                    **x = to.clone();
                }
            }
        }
    }
    if let Some(first) = effect.first_mut() {
        lesser(first, &triggering());
    }
    refer_to_the_trigger(e, cond, effect);
}

/// Rashmi: "reveal the top card of your library. You may cast it … if it's a
/// spell with lesser mana value": "it" is the revealed card, which is not
/// compared with itself, and the reveal names no other object, so the
/// comparison is with the trigger's spell ([lesser-mana-value]).
fn lesser_than_the_trigger(effect: &mut [Effect]) {
    let triggering = || Objects {
        quant: Quant::Triggering,
        filter: Filter::default(),
    };
    let unnamed = |e: &Effect| matches!(e, Effect::RevealTop { .. });
    if let Some(i) = effect.iter().position(|e| !unnamed(e)) {
        if i > 0 {
            if let Effect::If {
                cond: Condition::Is { what, filter },
                ..
            } = &mut effect[i]
            {
                if *what == antecedent() {
                    for q in &mut filter.posts {
                        if let Post::With(Compare {
                            value: Amount::ManaValueOf(x),
                            ..
                        }) = q
                        {
                            if **x == antecedent() {
                                **x = triggering();
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The one object a trigger event names, which the effect may call "it",
/// "its", "that card" or "that spell": the creature that died, the spell
/// cast. None where the event names two (a spell that targets a creature),
/// so "that creature" could be either.
fn trigger_object(e: &Event) -> Option<Objects> {
    let what = match e {
        Event::Enters { what, .. }
        | Event::Dies(what)
        | Event::Leaves { what, .. }
        | Event::Attacks(what)
        | Event::Blocks(what)
        | Event::AttacksOrBlocks(what)
        | Event::BecomesBlocked(what)
        | Event::DealsCombatDamageToPlayer(what)
        | Event::DealsCombatDamage(what)
        | Event::DealsDamage(what)
        | Event::IsDealtDamage(what)
        | Event::Casts { what, .. }
        | Event::Copies { what, .. }
        | Event::WasCast(what)
        | Event::CastFrom { what, .. }
        | Event::CastUsing { what, .. }
        | Event::Countered(what)
        | Event::Discards { what, .. }
        | Event::Sacrifices { what, .. }
        | Event::PutInto { what, .. }
        | Event::Puts { what, .. }
        | Event::Exiles { what, .. }
        | Event::Exerts { what, .. }
        | Event::Exiled(what)
        | Event::ReturnedToHand(what)
        | Event::LeftUnderControlOf { what, .. }
        | Event::BecomesTapped(what)
        | Event::TappedForMana(what)
        | Event::TapsForMana { what, .. }
        | Event::TapsFor { what, .. }
        | Event::TappedForAtLeast { what, .. }
        | Event::CounterPutOn(what)
        | Event::CountersPut { on: what, .. }
        | Event::NthCounterPutOn { on: what, .. }
        | Event::Creates { what, .. }
        | Event::Activates { what, .. }
        | Event::BecomesLevel { what, .. }
        | Event::AttacksWith { what, .. }
        | Event::AttacksAlone(what)
        | Event::AttacksUnblocked { what, .. }
        // "Whenever a creature attacks enchanted player, put a +1/+1 counter
        // on it", "Whenever this creature attacks a battle, it gets +1/+1":
        // what is attacked is said only to say which attacks trigger it, as
        // `AttacksUnblocked`'s player is ([attack-recipient]).
        | Event::AttacksAnyOf { what, .. }
        // "Whenever this creature becomes the target of a spell or ability
        // you control …, put a +1/+1 counter on it": the spell is said
        // only to say what targets it; a reference back to the spell names
        // it as one.
        | Event::BecomesTarget { what, .. }
        | Event::TransformsInto { what, .. }
        | Event::Cycles {
            what: Some(what), ..
        } => what,
        // "Whenever a creature you control with deathtouch deals damage to a
        // player or planeswalker, put a +1/+1 counter on that creature"
        // (Vraska): a recipient that is never a creature says only which
        // damage triggers it, as an attack's does ([attack-recipient]).
        // A planeswalker or battle recipient is still an object: "Whenever
        // this token deals damage to a planeswalker, destroy that
        // planeswalker" names it, so the source must be said by a card type
        // the reference can be told apart by.
        Event::DealsCombatDamageTo { what, to }
            if !may_be_creature(to) && (typed(what) || !names_object(to)) =>
        {
            what
        }
        Event::DealsDamageTo { what, to }
            if !to.iter().any(may_be_creature)
                && (typed(what) || !to.iter().any(names_object)) =>
        {
            what
        }
        Event::While { event, .. }
        | Event::AndIf { event, .. }
        | Event::InterveningIf { event, .. }
        | Event::FirstTimeEachTurn(event) => {
            return trigger_object(event);
        }
        Event::Or(v) => {
            let all: Vec<Objects> = v.iter().map(trigger_object).collect::<Option<_>>()?;
            return all.windows(2).all(|w| w[0] == w[1]).then(|| all[0].clone());
        }
        _ => return None,
    };
    // "Whenever ~ or another creature you control enters, if you cast it
    // …": one object entered, the card or another, and "it" is that one.
    if matches!(what.quant, Quant::SelfOrAnother(_) | Quant::SelfOrA(_)) {
        return Some(what.clone());
    }
    // "a spell that targets a creature you control", "a spell that targets
    // this creature": two objects, and "it" may be either.
    struct Count(usize);
    impl Visitor for Count {
        fn enter(&mut self, _: &mut Objects) -> bool {
            self.0 += 1;
            true
        }
    }
    let mut count = Count(0);
    // "Whenever a creature dealt damage by this creature this turn dies":
    // the source of the damage only says which creature, as an attack's
    // recipient does ([attack-recipient]); the card is said again as "this
    // creature". Another source is an object a reference may be: Vampiric
    // Embrace's "put a +1/+1 counter on that creature" is the enchanted
    // creature, the dead one being in a graveyard.
    let mut said = what.clone();
    for p in said.filter.posts.iter_mut() {
        if let Post::WasDealtDamageThisTurn { by, .. } = p {
            if by.as_ref().is_some_and(|b| b.quant == Quant::SelfRef) {
                *by = None;
            }
        }
    }
    said.walk(&mut count);
    (count.0 == 1).then(|| what.clone())
}

/// Whether a damage recipient may be a creature, so another object a
/// "that creature" could name: a player, a planeswalker or a battle is not.
fn may_be_creature(r: &Recipient) -> bool {
    match r {
        Recipient::Player(_) => false,
        Recipient::Or(v) => v.iter().any(may_be_creature),
        Recipient::Objects(o) => {
            o.filter.or.is_some()
                || o.filter.alts.is_empty()
                || o.filter.alts.iter().any(|g| {
                    !g.iter()
                        .any(|a| matches!(a, Adj::Type(t) if t == "planeswalker" || t == "battle"))
                })
        }
        _ => true,
    }
}

/// Whether a damage recipient names an object, not only players.
fn names_object(r: &Recipient) -> bool {
    match r {
        Recipient::Player(_) => false,
        Recipient::Or(v) => v.iter().any(names_object),
        _ => true,
    }
}

/// Whether an event's object is said by a type or subtype in each of its
/// alternatives, none of them a planeswalker's or battle's, so a
/// reference said with the recipient's ("that planeswalker") is told apart
/// from it.
fn typed(o: &Objects) -> bool {
    let recipient = |a: &Adj| matches!(a, Adj::Type(t) if t == "planeswalker" || t == "battle");
    !o.filter.alts.is_empty()
        && o.filter.alts.iter().all(|g| {
            g.iter()
                .any(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_)))
        })
        && !o.filter.alts.iter().flatten().any(recipient)
}

/// "~ has hexproof as long as it entered this turn", "This spell costs {3}
/// less to cast if it targets a tapped creature", "As long as this creature
/// is attacking, it gets +2/+0": a static about the card whose condition
/// says "it" for the card, before or after naming it. Only the card: in
/// "Each land gets +2/+2 as long as it's a creature" "it" is each land in
/// turn, which no one object in the tree is.
fn refer_to_the_card(statics: &mut [Static]) {
    struct First(Option<Objects>);
    impl Visitor for First {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if self.0.is_none() {
                self.0 = Some(o.clone());
            }
            false
        }
    }
    let first = |w: &mut dyn FnMut(&mut First)| {
        let mut f = First(None);
        w(&mut f);
        f.0
    };
    let is_card = |o: &Option<Objects>| o.as_ref().is_some_and(|o| o.quant == Quant::SelfRef);
    let is_it = |o: &Option<Objects>| o.as_ref().is_some_and(|o| *o == antecedent());
    for st in statics {
        match st {
            // "As this land enters, you may pay 2 life. If you don't, it
            // enters tapped."
            Static::AsEnters { what, then } if what.quant == Quant::SelfRef => {
                refer_after_the_card(then);
                continue;
            }
            // Mox Diamond: "If this artifact would enter, you may discard a
            // land card instead. If you do, put this artifact onto the
            // battlefield. If you don't, put it into its owner's graveyard."
            Static::Replacement { event, instead }
                if event_object(event).is_some_and(|o| o.quant == Quant::SelfRef) =>
            {
                refer_after_the_card(instead);
                continue;
            }
            Static::AsLongAs { .. } | Static::If { .. } => {}
            // "You may exert this creature as it attacks. When you do, it
            // deals 4 damage …": the card, named before the linked trigger.
            Static::ExertAsAttacks { what, then } if what.quant == Quant::SelfRef => {
                refer_after_the_card(then);
                continue;
            }
            // "Creatures with power less than this creature's power can't
            // block it": the attacker a static's blockers can't block is the
            // card; the plural subject would be "them".
            Static::Restrict {
                rule: Restriction::CantBlockObject(it),
                ..
            } if *it == antecedent() => {
                it.quant = Quant::SelfRef;
                continue;
            }
            // "This creature enters with a +1/+1 counter on it for each
            // color of mana spent to cast it": a static whose subject is the
            // card says "it" for the card, as the condition in SEMANTICS [static-it-is-card] does.
            st => {
                if is_card(&first(&mut |f: &mut First| st.walk(f))) {
                    st.walk(&mut Bare(Quant::SelfRef));
                }
                continue;
            }
        }
        let (Static::AsLongAs { cond, then } | Static::If { cond, then }) = st else {
            continue;
        };
        // "As long as this creature has three or more +1/+1 counters on it,
        // it has flying": the condition names only the card, the counters
        // being on it.
        struct OnlyCard(bool, bool);
        impl Visitor for OnlyCard {
            fn enter(&mut self, o: &mut Objects) -> bool {
                match (&o.quant, &o.filter.kind) {
                    (Quant::SelfRef, _) => self.0 = true,
                    (_, Some(Noun::Counter(_))) => {}
                    _ => self.1 = true,
                }
                true
            }
        }
        let mut only = OnlyCard(false, false);
        cond.walk(&mut only);
        if only.0
            && !only.1
            && then
                .iter_mut()
                .any(|t| is_it(&first(&mut |f: &mut First| t.walk(f))))
        {
            for t in then.iter_mut() {
                t.walk(&mut FirstBare(false));
            }
            continue;
        }
        let subjects: Vec<Option<Objects>> = then
            .iter_mut()
            .map(|t| first(&mut |f: &mut First| t.walk(f)))
            .collect();
        // "If ~ was kicked, it enters with five +1/+1 counters on it": the
        // kicked spell is the card, which `WasKicked` holds no object for.
        let named = if *cond == Condition::WasKicked {
            Some(Objects {
                quant: Quant::SelfRef,
                filter: Filter::default(),
            })
        } else {
            first(&mut |f: &mut First| cond.walk(f))
        };
        // "~ has hexproof as long as it's untapped": the subject is the card.
        if !subjects.is_empty() && subjects.iter().all(is_card) && is_it(&named) {
            cond.walk(&mut Bare(Quant::SelfRef));
        }
        // "Enchanted creature gets +1/+2 as long as it's white", "Each
        // Vehicle you control gets +1/+1 as long as it's a creature": the
        // one subject, or each of them in turn ([trailing-if]).
        if is_it(&named) {
            if let Some(Some(s)) = subjects.first() {
                if subjects.iter().all(|t| t.as_ref() == Some(s)) {
                    if let Some(to) = clause_object(&s.quant).filter(|_| s.quant != Quant::SelfRef)
                    {
                        others_than_clause_object(cond, &to, s);
                        cond.walk(&mut Bare(to));
                    }
                }
            }
        }
        // "As long as this creature is attacking, it gets +2/+0".
        if is_card(&named) && subjects.iter().any(is_it) {
            for t in then.iter_mut() {
                let mut one = First(None);
                t.walk(&mut one);
                if is_it(&one.0) {
                    t.walk(&mut FirstBare(false));
                }
            }
        }
    }
}

/// The first object of a static, its subject, when it is a bare "it".
struct FirstBare(bool);

impl Visitor for FirstBare {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if !self.0 && *o == antecedent() {
            o.quant = Quant::SelfRef;
        }
        self.0 = true;
        false
    }
}

/// Every bare "it" in a part of a clause, said of one object.
struct Bare(Quant);

impl Visitor for Bare {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if o.quant == Quant::Antecedent && o.filter == Filter::default() {
            o.quant = self.0.clone();
        }
        true
    }
}

/// An "if" said after its clause: its references back are the one object
/// the clause names, when the clause names one by a noun of its own
/// ([trailing-if]). A clause that names its object by a reference back
/// leaves them alone: both point at the same earlier noun.
pub(super) fn after_clause(clause: &Effect, cond: &mut Condition) {
    struct Named(Vec<Objects>);
    impl Visitor for Named {
        fn enter(&mut self, o: &mut Objects) -> bool {
            self.0.push(o.clone());
            false
        }
    }
    let mut named = Named(Vec::new());
    clause.clone().walk(&mut named);
    let [one] = named.0.as_slice() else { return };
    let Some(to) = clause_object(&one.quant) else {
        return;
    };
    others_than_clause_object(cond, &to, one);
    struct Bind(Quant);
    impl Visitor for Bind {
        fn enter(&mut self, o: &mut Objects) -> bool {
            // "the revealed card's power", "the sacrificed creature's
            // power": what a cost or an earlier verb marked keeps its own
            // referent; only the pronoun is the clause's object.
            let marked = o.filter.posts.iter().any(|p| matches!(p, Post::Marked(_)));
            if o.quant == Quant::Antecedent && !marked {
                o.quant = self.0.clone();
            }
            true
        }
    }
    cond.walk(&mut Bind(to));
}

/// "each creature you control gets +1/+1 … if it has the same name as
/// another creature you control", "Enchanted creature has shroud as long as
/// its controller controls another creature": "another" of the clause
/// object's kind in its trailing condition is other than that object, the
/// one bound or each in turn ([other-than-subject]): `Quant::One` with
/// `OtherThan`. `Quant::Another` alone is other than the card itself.
fn others_than_clause_object(cond: &mut Condition, to: &Quant, of: &Objects) {
    struct Others<'a>(Objects, Vec<&'a Adj>);
    impl Visitor for Others<'_> {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if o.quant == Quant::Another {
                other_than(o, &self.0, Some(&self.1));
            }
            true
        }
    }
    let like = kinds(&of.filter);
    if like.is_empty() {
        return;
    }
    let than = Objects {
        quant: to.clone(),
        filter: Filter::default(),
    };
    cond.walk(&mut Others(than, like));
}

/// Every reference back in a clause said before a trailing "for each".
pub(super) struct BeforeEach;

impl Visitor for BeforeEach {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if o.quant == Quant::Antecedent {
            o.quant = Quant::BeforeEach;
        }
        true
    }
}

/// "Whenever another nontoken creature you control dies, if its mana value
/// was 1 or greater, … return that card to the battlefield": a reference
/// back said before the effect names any other object is to the trigger's
/// own object, `Triggering`, or the card itself where that is what the event
/// named. A bare "it" after the card was named is the card ("put a +1/+1
/// counter on this creature and it can't be blocked"). Once another object
/// is named, "it" may be that one, and stays a plain back-reference.
fn refer_to_the_trigger(e: &Event, cond: Option<&mut Condition>, effect: &mut [Effect]) {
    // "Whenever this creature or another artifact you control enters, put a
    // +1/+1 counter on this creature. It can't be blocked this turn": with
    // no one object, the card half of the rule still holds.
    let object = trigger_object(e);
    refer(object.as_ref(), false, cond, effect);
}

/// "{S}{S}{S}: If this creature is a Warrior, put a flying counter on it and
/// it becomes …": in an ability with no trigger, a bare "it" after the card
/// is named in a finished clause is the card, by the same rule as in a
/// trigger; nothing else changes. "Remove a +1/+1 counter from this
/// creature: It deals 1 damage to any target": a cost that names the card
/// and nothing else has named it before the effect.
pub(super) fn refer_to_the_card_in(cost: &[Cost], effect: &mut [Effect]) {
    struct Named {
        card: bool,
        other: bool,
    }
    impl Visitor for Named {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if o.quant == Quant::SelfRef {
                self.card = true;
            } else {
                self.other = true;
            }
            false
        }
    }
    let mut named = Named {
        card: false,
        other: false,
    };
    for c in cost {
        c.clone().walk(&mut named);
    }
    refer(None, named.card && !named.other, None, effect);
}

/// Journey to the Lost City's "20 | Put all permanent cards exiled with this
/// enchantment onto the battlefield, then sacrifice it": the singular "it"
/// is not the cards, said in the plural, so it is the card named with them.
/// Only where nothing singular but the card was named.
fn sacrifice_it_is_the_card(toks: &[Tok], effect: &mut [Effect]) {
    struct Named {
        card: bool,
        singular: bool,
    }
    impl Visitor for Named {
        fn enter(&mut self, o: &mut Objects) -> bool {
            match o.quant {
                Quant::SelfRef => self.card = true,
                Quant::All | Quant::Each | Quant::Plural | Quant::Antecedent => {}
                _ => self.singular = true,
            }
            true
        }
    }
    let said = toks.windows(2).any(
        |w| matches!((&w[0], &w[1]), (Tok::Word(a), Tok::Word(b)) if a == "sacrifice" && b == "it"),
    );
    let Some((
        Effect::Sacrifice {
            who: Player::You,
            what,
        },
        before,
    )) = effect.split_last_mut()
    else {
        return;
    };
    if !said || *what != antecedent() {
        return;
    }
    let mut named = Named {
        card: false,
        singular: false,
    };
    for e in before.iter_mut() {
        e.walk(&mut named);
    }
    if named.card && !named.singular {
        what.quant = Quant::SelfRef;
    }
}

/// The card half of the rule over an effect a static says of the card:
/// "As this land enters, you may pay 2 life. If you don't, it enters
/// tapped", Mox Diamond's "If this artifact would enter, …".
fn refer_after_the_card(effect: &mut [Effect]) {
    refer(None, true, None, effect);
}

fn refer(
    object: Option<&Objects>,
    card_named: bool,
    cond: Option<&mut Condition>,
    effect: &mut [Effect],
) {
    struct Refer {
        to: Quant,
        /// What the event's object was said to be.
        object: Filter,
        seen_self: bool,
        /// The card was named in the clause still being read: "this
        /// artifact deals 1 damage to it" would say "itself" for the card.
        self_pending: bool,
        seen_new: bool,
        /// The source of the damage just dealt: "it deals 3 damage to
        /// target opponent and 3 damage to up to one target creature" says
        /// one subject for both.
        damage_source: Option<Objects>,
        /// "draw a card and reveal it": the drawn card, which a draw names
        /// in no `Objects`; otherwise nothing refers back to one.
        drew: bool,
        /// How deep in conditions the walk is.
        in_cond: u32,
        /// What had been named as each `ForEach` still being read began,
        /// before its loop's noun: a reference said before a trailing "for
        /// each" is read against that.
        loops: Vec<(bool, bool)>,
        /// How deep in effects the walk is: 0 between two of the list.
        depth: u32,
        /// Whether the effect of the list being read has named the card,
        /// and anything else.
        named: (bool, bool),
        /// The effect before named the card and nothing else: "If you do,
        /// put this artifact onto the battlefield. If you don't, put it into
        /// its owner's graveyard". A bare "it" is the card until another
        /// object is named.
        after_card: bool,
        /// For each `Or` being read, its depth and what had been named as
        /// it began: each alternative starts from there, since only one is
        /// done ("tap all untapped creatures that share a creature type
        /// with it or untap all tapped creatures that share a creature type
        /// with it").
        ors: Vec<(u32, bool, bool)>,
        /// The window was ended only by tokens created, so a reference to a
        /// card still goes past them.
        tokens_only: bool,
        /// The types and subtypes of what was named after the window
        /// closed: a reference said with one of them may be that.
        named_since: Vec<Adj>,
    }
    impl Visitor for Refer {
        fn enter(&mut self, o: &mut Objects) -> bool {
            let f = &o.filter;
            let before_each = o.quant == Quant::BeforeEach;
            let (seen_new, seen_self) = match self.loops.last() {
                Some(&said) if before_each => said,
                _ => (self.seen_new, self.seen_self),
            };
            // "those counters", "the exiled card": not the trigger's object.
            let referable = (o.quant == Quant::Antecedent || before_each)
                && f.posts.is_empty()
                && f.or.is_none()
                && !matches!(
                    f.kind,
                    Some(Noun::Counter(_) | Noun::Player | Noun::Opponent)
                );
            // "Whenever a land you control enters, … that creature": a card
            // type the event's object was not said to have is another object.
            let types = |f: &Filter| -> Vec<Adj> {
                f.alts
                    .iter()
                    .flatten()
                    .filter(|a| matches!(a, Adj::Type(_)))
                    .cloned()
                    .collect()
            };
            let theirs = types(&self.object);
            let only_types = !self.object.alts.is_empty()
                && self
                    .object
                    .alts
                    .iter()
                    .flatten()
                    .all(|a| !matches!(a, Adj::Subtype(_)));
            let other_type =
                only_types && !theirs.is_empty() && types(f).iter().any(|t| !theirs.contains(t));
            // "that spell" when the event's object was a permanent.
            let spellish = |k: &Option<Noun>| {
                matches!(k, Some(Noun::Spell | Noun::Ability | Noun::SpellOrAbility))
            };
            let other_type = other_type || (spellish(&f.kind) && f.kind != self.object.kind);
            if referable && self.after_card && !before_each && *f == Filter::default() {
                o.quant = Quant::SelfRef;
                return true;
            }
            // "create a … token, then put … counters on it equal to the
            // greatest mana value among those cards": only tokens were
            // named since, and a card is none of them.
            // "create a 0/0 … Fractal creature token. Put X +1/+1 counters on
            // it, where X is that spell's mana value": nor a spell.
            let seen_new = seen_new
                && !(self.tokens_only
                    && !before_each
                    && matches!(f.kind, Some(Noun::Card | Noun::Spell)));
            // "Whenever one or more Dragons you control deal combat damage
            // …, look at the top three cards …. You may return one of those
            // Dragons to its owner's hand" (Zurgo and Ojutai): a reference
            // said with the words the event's object was said with, which
            // nothing named since was said with, is still that object.
            let words = |f: &Filter| -> Vec<Adj> {
                f.alts
                    .iter()
                    .flatten()
                    .filter(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_)))
                    .cloned()
                    .collect()
            };
            let mine = words(f);
            let same_words = self.to == Quant::Triggering
                && !before_each
                && f.kind == self.object.kind
                && !mine.is_empty()
                && mine.iter().all(|a| words(&self.object).contains(a))
                && !mine.iter().any(|a| self.named_since.contains(a));
            let seen_new = seen_new && !same_words;
            if referable && !seen_new && !other_type {
                o.quant = if seen_self && *f == Filter::default() {
                    Quant::SelfRef
                } else if before_each && self.to == Quant::Antecedent {
                    // No trigger object: still an earlier noun outside the loop.
                    Quant::BeforeEach
                } else {
                    self.to.clone()
                };
            }
            true
        }
        fn leave(&mut self, o: &mut Objects) {
            // "the number of counters on it": a counter is never what "it"
            // refers back to, so naming one ends nothing.
            if matches!(o.filter.kind, Some(Noun::Counter(_))) {
                return;
            }
            // A back-reference left as one ("the exiled card") is to
            // another object, which "its" after it may be.
            // "counter it if a card with the same name is in a graveyard":
            // an indefinite in a condition says one exists, and is nothing
            // a later "it" refers back to.
            // A quantified noun ("each other creature's power") is no referent
            // either.
            let indefinite = matches!(
                o.quant,
                Quant::One | Quant::Another | Quant::Plural | Quant::Each | Quant::All
            );
            if o.quant == Quant::SelfRef {
                self.named.0 = true;
            } else {
                self.named.1 = true;
                self.after_card = false;
            }
            // "if one or more of them entered from a graveyard or was cast
            // from a graveyard": some of the trigger's objects, which "them"
            // said again still is.
            let among_trigger = o
                .filter
                .posts
                .iter()
                .any(|p| matches!(p, Post::FromAmong(x) if x.quant == Quant::Triggering));
            match o.quant {
                Quant::SelfRef => self.self_pending = true,
                Quant::Triggering => {}
                _ if among_trigger => {}
                // The tokens said again: still only tokens since the window
                // closed.
                Quant::Created => self.seen_new = true,
                Quant::Antecedent if self.tokens_only && o.filter == Filter::default() => {}
                // Only against a trigger's object: with none, "this land
                // and a creature named Hanweir Garrison … exile them" names
                // both.
                _ if indefinite && self.in_cond > 0 && self.to != Quant::Antecedent => {}
                _ => {
                    self.seen_new = true;
                    self.tokens_only = false;
                    self.named_since
                        .extend(o.filter.alts.iter().flatten().cloned());
                }
            }
        }
        fn condition(&mut self, _: &mut Condition) {
            self.in_cond += 1;
        }
        fn condition_done(&mut self, _: &mut Condition) {
            self.in_cond -= 1;
        }
        fn effect(&mut self, e: &mut Effect) {
            // A clause starting ends the one before it, as "If this
            // enchantment isn't a creature, it becomes …" ends its "if".
            if self.depth == 0 {
                self.after_card = self.named == (true, false);
                self.named = (false, false);
            }
            if let Some(&(d, new, card)) = self.ors.last() {
                if self.depth == d {
                    self.seen_new = new;
                    self.seen_self = card;
                }
            }
            self.depth += 1;
            if matches!(e, Effect::Or(_)) {
                self.ors.push((self.depth, self.seen_new, self.seen_self));
            }
            self.seen_self |= self.self_pending;
            if matches!(e, Effect::ForEach { .. } | Effect::Repeat { .. }) {
                self.loops.push((self.seen_new, self.seen_self));
            }
            if self.drew && matches!(e, Effect::Reveal { what, .. } if *what == antecedent()) {
                self.seen_new = true;
                self.tokens_only = false;
            }
            // "this creature deals damage equal to its power to it": the
            // amount is the card's own, and what it acts on is not the card,
            // which English would call "itself".
            let to = self.to.clone();
            let open = !self.seen_new;
            match e {
                Effect::Damage {
                    source: Some(src),
                    n,
                    to: recipients,
                } if src.quant == Quant::SelfRef => {
                    n.walk(&mut Bare(Quant::SelfRef));
                    if open {
                        let mut bare = Bare(to);
                        for r in recipients.iter_mut() {
                            r.walk(&mut bare);
                        }
                    }
                }
                Effect::Attach { what, to: on } if what.quant == Quant::SelfRef && open => {
                    on.walk(&mut Bare(to));
                }
                // "you may put an Aura card from your hand onto the
                // battlefield attached to it": the Aura is said in the same
                // clause, and English would call it "itself"; "it" is what
                // was named before the clause.
                Effect::EntersAttached { to: on, .. } if self.after_card => {
                    on.walk(&mut Bare(Quant::SelfRef));
                }
                Effect::EntersAttached { to: on, .. } if open => {
                    on.walk(&mut Bare(to));
                }
                _ => {}
            }
            if let Effect::Damage {
                source: Some(src), ..
            } = e
            {
                if src.quant == Quant::Antecedent && src.filter == Filter::default() {
                    // Which object, still said by pronoun.
                    if let Some(same) = &self.damage_source {
                        src.quant = same.quant.clone();
                    }
                }
            }
        }
        // "reveal the top card of your library. If it's a creature card":
        // effects that bring in an object they name in no `Objects`.
        fn effect_done(&mut self, e: &mut Effect) {
            self.depth -= 1;
            if matches!(e, Effect::Or(_)) {
                self.ors.pop();
            }
            if matches!(e, Effect::ForEach { .. } | Effect::Repeat { .. }) {
                self.loops.pop();
            }
            self.seen_self |= self.self_pending;
            self.drew = matches!(e, Effect::Draw { .. });
            // "return that card to the battlefield …. Return this card to
            // the battlefield attached to that creature": the permanent the
            // card became is a new object (CR 400.7), which the trigger's
            // object never was, so what is said of it after refers back.
            if let Effect::Move {
                what,
                to: Zone::Battlefield,
                ..
            } = e
            {
                if what.quant == Quant::Triggering {
                    self.seen_new = true;
                    self.tokens_only = false;
                    let w: Vec<Adj> = kinds(&self.object).into_iter().cloned().collect();
                    self.named_since.extend(w);
                }
            }
            self.damage_source = match e {
                Effect::Damage {
                    source: Some(src), ..
                } if matches!(src.quant, Quant::SelfRef | Quant::Triggering) => Some(src.clone()),
                _ => None,
            };
            // "copy it if you control an artifact, then copy it if you
            // control an enchantment" (Banish into Fable): the copy goes on
            // the stack unnamed, and a later "it" is still the spell copied.
            let copies_own = matches!(
                e,
                Effect::Copy { what, .. }
                    if matches!(what.quant, Quant::SelfRef | Quant::Triggering)
            );
            if !copies_own
                && matches!(
                    e,
                    Effect::RevealTop { .. }
                        | Effect::LookAtTop { .. }
                        | Effect::RevealHand { .. }
                        | Effect::LookAtHand { .. }
                        | Effect::Mill { .. }
                        | Effect::Create { .. }
                        | Effect::CreateCopy { .. }
                        | Effect::CreateThose { .. }
                        | Effect::CreateThosePlus { .. }
                        | Effect::CreateOfEach { .. }
                        | Effect::CreateNamed { .. }
                        | Effect::CreateAttached { .. }
                        | Effect::Copy { .. }
                        | Effect::Amass { .. }
                        | Effect::Action { .. }
                        | Effect::UntilFound { .. }
                        | Effect::DigUntilTotal { .. }
                )
            {
                // Tokens alone: a card is none of them (CR 108.2, 111.1).
                let tokens = matches!(
                    e,
                    Effect::Create { .. }
                        | Effect::CreateCopy { .. }
                        | Effect::CreateOfEach { .. }
                        | Effect::CreateNamed { .. }
                        | Effect::CreateAttached { .. }
                );
                self.tokens_only = tokens && (!self.seen_new || self.tokens_only);
                // "Create a 5/5 red Dragon creature token …": a later "those
                // Dragons" may be the tokens, whose types no `Objects` says.
                if let Effect::Create { token, .. }
                | Effect::CreateNamed { token, .. }
                | Effect::CreateAttached { token, .. } = e
                {
                    for t in &token.types {
                        self.named_since.push(Adj::Type(t.clone()));
                        self.named_since.push(Adj::Subtype(t.clone()));
                    }
                }
                self.seen_new = true;
                // "Create X tokens …. Exile them": the effect named more
                // than the card.
                self.named.1 = true;
                self.after_card = false;
            }
        }
    }
    let mut v = Refer {
        object: object.map(|o| o.filter.clone()).unwrap_or_default(),
        to: match object {
            Some(o) if o.quant == Quant::SelfRef => Quant::SelfRef,
            Some(_) => Quant::Triggering,
            // No trigger: a first reference back stays one.
            None => Quant::Antecedent,
        },
        seen_self: card_named,
        self_pending: false,
        seen_new: false,
        damage_source: None,
        drew: false,
        in_cond: 0,
        loops: Vec::new(),
        depth: 0,
        named: (false, false),
        after_card: false,
        ors: Vec::new(),
        tokens_only: false,
        named_since: Vec::new(),
    };
    if let Some(c) = cond {
        c.walk(&mut v);
        v.seen_self |= v.self_pending;
    }
    for x in effect {
        x.walk(&mut v);
    }
}

// ------------------------------------------------- "other" against a referent

/// The card and creature types a filter says.
fn kinds(f: &Filter) -> Vec<&Adj> {
    f.alts
        .iter()
        .flatten()
        .filter(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_)))
        .collect()
}

/// "each other creature", "another creature you control" said against
/// `than`: `State::Other` becomes `OtherThan(than)`, and "another" one
/// object other than it. `like`, when given, is the kinds the object must
/// share some of; an object of a kind no card is said of here (a spell, a
/// player) keeps its "other".
fn other_than(o: &mut Objects, than: &Objects, like: Option<&[&Adj]>) -> bool {
    if matches!(
        o.filter.kind,
        Some(Noun::Spell | Noun::Player | Noun::Opponent | Noun::Ability | Noun::SpellOrAbility)
    ) {
        return false;
    }
    let shares = |g: &[Adj]| {
        like.is_none_or(|like| {
            g.iter()
                .any(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_)) && like.contains(&a))
        })
    };
    let to = Adj::OtherThan(Box::new(than.clone()));
    let mut moved = false;
    if o.quant == Quant::Another && o.filter.alts.iter().all(|g| shares(g)) {
        if o.filter.alts.is_empty() {
            if like.is_some() {
                return false;
            }
            o.filter.alts.push(vec![]);
        }
        o.quant = Quant::One;
        for g in &mut o.filter.alts {
            g.insert(0, to.clone());
        }
        return true;
    }
    for g in &mut o.filter.alts {
        if !shares(g) {
            continue;
        }
        for a in g.iter_mut() {
            if *a == Adj::State(State::Other) {
                *a = to.clone();
                moved = true;
            }
        }
    }
    moved
}

/// Selvala's "if its power is greater than each other creature's power",
/// Shared Animosity's "each other attacking creature that shares a creature
/// type with it", Guardian Project's "the same name as another creature you
/// control": "other" said of the event's own kind in a triggered ability is
/// other than the object that triggered it, as Valakut's is (its ruling).
fn other_than_the_trigger(e: &Event, cond: Option<&mut Condition>, effect: &mut [Effect]) {
    let Some(object) = trigger_object(e) else {
        return;
    };
    if object.quant == Quant::SelfRef {
        return;
    }
    let like: Vec<Adj> = kinds(&object.filter).into_iter().cloned().collect();
    if like.is_empty() {
        return;
    }
    struct V(Vec<Adj>);
    impl Visitor for V {
        fn enter(&mut self, o: &mut Objects) -> bool {
            let like: Vec<&Adj> = self.0.iter().collect();
            let triggering = Objects {
                quant: Quant::Triggering,
                filter: Filter::default(),
            };
            other_than(o, &triggering, Some(&like));
            true
        }
    }
    let mut v = V(like);
    if let Some(c) = cond {
        c.walk(&mut v);
    }
    for x in effect {
        x.walk(&mut v);
    }
}

/// Chandra's Ignition: "Target creature you control deals damage equal to
/// its power to each other creature": other than the creature dealing it,
/// where that is not the card.
struct OtherThanTheSource;

impl Visitor for OtherThanTheSource {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect(&mut self, e: &mut Effect) {
        let Effect::Damage {
            source: Some(src),
            to,
            ..
        } = e
        else {
            return;
        };
        let than = match src.quant {
            Quant::SelfRef => return,
            Quant::Triggering => src.clone(),
            _ => antecedent(),
        };
        let like = kinds(&src.filter);
        let like = (!like.is_empty()).then_some(&like[..]);
        for r in to.iter_mut() {
            if let Recipient::Objects(o) = r {
                other_than(o, &than, like);
            }
        }
    }
}

/// Coat of Arms: "Each creature gets +1/+1 for each other creature on the
/// battlefield that shares at least one creature type with it": other than
/// the creature getting the bonus, each in turn, which "it" also names.
fn other_than_the_subject(statics: &mut [Static]) {
    for st in statics {
        match st {
            Static::Modify {
                what,
                per: Some(Amount::CountOf(counted)),
                ..
            } if what.quant != Quant::SelfRef => {
                // The creature getting the bonus, each in turn ([each-itself]):
                // "other" is other than it, and "with it" is it.
                let itself = Objects {
                    quant: Quant::Itself,
                    filter: Filter::default(),
                };
                let like = kinds(&what.filter);
                if !like.is_empty() {
                    other_than(counted, &itself, Some(&like));
                    counted.walk(&mut Bare(Quant::Itself));
                }
            }
            Static::AsLongAs { then, .. } | Static::If { then, .. } => other_than_the_subject(then),
            _ => {}
        }
    }
}

/// "Each player chooses any number of creatures they control …, then
/// sacrifices all other creatures they control", "Put that card into your
/// hand and exile all other cards revealed this way": an instant or sorcery
/// is never the creature or card "other" excludes, so it is other than what
/// the spell named before it. With nothing named before (Mirrorweave's
/// "Each other creature becomes a copy of target …") it stays as said.
fn other_than_named_before(effect: &mut [Effect]) {
    if let [Effect::Modal { modes, .. }] = effect {
        for m in modes {
            other_than_named_before(m);
        }
        return;
    }
    struct V(bool);
    impl Visitor for V {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if self.0 {
                other_than(o, &antecedent(), None);
            }
            true
        }
        fn leave(&mut self, o: &mut Objects) {
            if o.quant != Quant::SelfRef
                && !matches!(o.filter.kind, Some(Noun::Player | Noun::Opponent))
            {
                self.0 = true;
            }
        }
        fn effect_done(&mut self, e: &mut Effect) {
            // "Create X 1/1 … tokens. If X is 5 or more, destroy all other
            // creatures."
            if matches!(e, Effect::Create { .. } | Effect::CreateCopy { .. }) {
                self.0 = true;
            }
        }
    }
    let mut v = V(false);
    for x in effect {
        x.walk(&mut v);
    }
}

/// "The first spell you cast from exile each turn has cascade", "Players
/// can't cast spells from graveyards": a spell is a card on the stack (CR
/// 112.1), so a zone said of one is where it was cast from, as `casts`
/// reads it in an event ([spell-cast-from]).
struct SpellCastFrom;

impl Visitor for SpellCastFrom {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let mut f = Some(&mut o.filter);
        while let Some(filter) = f {
            if filter.kind == Some(Noun::Spell) {
                for q in filter.posts.iter_mut() {
                    if let Post::In(z) = q {
                        if *z != Zone::Stack {
                            *q = Post::CastFrom(z.clone());
                        }
                    }
                }
            }
            f = filter.or.as_deref_mut();
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// "Exile the top card of each player's library": the possessive "each
/// player's" distributes, so the count is per library, one card from each.
struct FromEachLibrary;

impl Visitor for FromEachLibrary {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if let (Quant::N(n), [Post::In(Zone::TopOfLibrary(Player::Each | Player::EachOpponent))]) =
            (&o.quant, &o.filter.posts[..])
        {
            o.quant = Quant::NFromEach(n.clone());
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// Selvala: "its controller may draw a card if its power is greater than
/// each other creature's power": the verb after "may" inherits the may's
/// subject (`subject_player`), so it is the same player, whatever objects
/// were named between. The referent rule resolves the may's own "its" before
/// "each other creature" closes the window, and the draw's copy after it;
/// here the copy follows the may.
struct MaySubject;

impl Visitor for MaySubject {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect(&mut self, e: &mut Effect) {
        let Effect::May { who, then } = e else {
            return;
        };
        // The may's subject as it was said, before the referent rule.
        let said = match who {
            Player::ControllerOf(o)
                if o.quant != Quant::Antecedent && o.filter == Filter::default() =>
            {
                Player::ControllerOf(Box::new(antecedent()))
            }
            Player::OwnerOf(o) if o.quant != Quant::Antecedent && o.filter == Filter::default() => {
                Player::OwnerOf(Box::new(antecedent()))
            }
            _ => return,
        };
        struct Same<'a>(&'a Player, &'a Player);
        impl Visitor for Same<'_> {
            fn enter(&mut self, _: &mut Objects) -> bool {
                false
            }
            fn effect(&mut self, e: &mut Effect) {
                if let Some(s) = sentences::subject_mut(e) {
                    if s == self.0 {
                        *s = self.1.clone();
                    }
                }
            }
        }
        let resolved = who.clone();
        for t in then.iter_mut() {
            t.walk(&mut Same(&said, &resolved));
        }
    }
}

/// Animate Dead: "When this Aura leaves the battlefield, that creature's
/// controller sacrifices it". A delayed trigger refers to the particular
/// object the ability that made it named (CR 603.7c), and its own event's
/// object is no referent of a reference made inside the clause: "it" after
/// "that creature" in one clause is that creature, the returned one, not
/// the Aura whose leaving is the event. The tree says it as the same
/// object as the subject's, typed, so no resolver can take the Aura.
struct DelayedSameClause;

impl Visitor for DelayedSameClause {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect(&mut self, e: &mut Effect) {
        let (Effect::DelayedWhen { event, then }
        | Effect::DelayedWhenever { event, then }
        | Effect::DelayedNext { event, then, .. }) = e
        else {
            return;
        };
        let Some(own) = trigger_object(event) else {
            return;
        };
        for t in then.iter_mut() {
            if let Effect::Sacrifice {
                who: Player::ControllerOf(x) | Player::OwnerOf(x),
                what,
            } = t
            {
                let typed = x.quant == Quant::Antecedent && x.filter != Filter::default();
                if typed && *what == antecedent() && x.filter != own.filter {
                    *what = (**x).clone();
                }
            }
        }
    }
}

/// Kiki-Jiki: "Create a token that's a copy of target nonlegendary creature
/// you control, except it has haste. Sacrifice it at the beginning of the
/// next end step"; Sokenzan: "Create two 1/1 colorless Spirit creature
/// tokens. They gain haste until end of turn". The tokens are named by no
/// `Objects`, so a bare reference after them would point past them to the
/// copied creature. A reference back ("it", "them", "the token", "those
/// tokens") said after an effect that created tokens, before any other
/// object is named, is `Created`. Not:
/// - a bare "it" in a condition, which tests what was named before the
///   tokens ("exile target card …. If it was a creature card, create a 2/2
///   …. If it was a land card, …"); "If the token isn't a creature" is;
/// - an object in a `Where`'s definition of X, which is worked out before
///   its effect creates anything ("create X Junk tokens, where X is the
///   number of +1/+1 counters on it");
/// - "it deals" after more than one token was created ("create three 1/1
///   … Hamster creature tokens, then it deals X damage"): the singular verb
///   is the Vehicle's, where the tokens would be "they";
/// - in the other branch of an "if" ("If it's a land card, create a 2/2
///   … Ox creature token. Otherwise, you may cast it") or another mode,
///   where nothing was created; nor after an effect that brings in an
///   object of its own ("create a Treasure token, then look at the top
///   card of that player's library and exile it face down").
struct CreatedTokens {
    /// The tokens just created, and whether they were one.
    created: Option<bool>,
    in_cond: u32,
    depth: u32,
    /// The depths of the `Where`s being read, each with what its effects
    /// created, kept while its X is read (Ovika: "create X … tokens, where
    /// X is the mana value of that spell. They gain haste").
    wheres: Vec<(u32, Option<Option<bool>>)>,
    /// Inside a `Modal` whose modes were read on their own.
    skip: Option<u32>,
    /// Tokens were created and no other token named since: "the token"
    /// still means them after other objects (Ajani's Chosen's "If that
    /// enchantment is an Aura, you may attach it to the token").
    tokens: bool,
    /// The card and creature types of the tokens just created.
    types: Vec<String>,
}

impl CreatedTokens {
    fn new() -> Self {
        CreatedTokens {
            created: None,
            in_cond: 0,
            depth: 0,
            wheres: Vec::new(),
            skip: None,
            tokens: false,
            types: Vec::new(),
        }
    }
}

impl Visitor for CreatedTokens {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if self.skip.is_some() {
            return true;
        }
        let bare = o.filter.alts.is_empty() && o.filter.posts.is_empty() && o.filter.or.is_none();
        let fits = match o.filter.kind {
            Some(Noun::Token) => true,
            None => self.in_cond == 0,
            _ => false,
        };
        if self.created.is_some() && o.quant == Quant::Antecedent && bare && fits {
            o.quant = Quant::Created;
        }
        // "Create a 1/1 blue and red Otter creature token with prowess. If
        // this spell was cast from anywhere other than your hand, put a
        // +1/+1 counter on that creature": said with the token's own types.
        let typed = match o.filter.alts.as_slice() {
            [group] => {
                !group.is_empty()
                    && group.iter().all(|a| match a {
                        Adj::Type(t) | Adj::Subtype(t) => self.types.contains(t),
                        _ => false,
                    })
            }
            _ => false,
        };
        if self.created.is_some()
            && o.quant == Quant::Antecedent
            && typed
            && o.filter.kind.is_none()
            && o.filter.posts.is_empty()
            && o.filter.or.is_none()
            && self.in_cond == 0
        {
            o.quant = Quant::Created;
        }
        if self.tokens && o.quant == Quant::Antecedent && bare && o.filter.kind == Some(Noun::Token)
        {
            o.quant = Quant::Created;
        }
        true
    }
    fn leave(&mut self, o: &mut Objects) {
        if o.filter.kind == Some(Noun::Token) && o.quant != Quant::Created {
            self.tokens = false;
        }
        let in_cond_card = self.in_cond > 0 && o.quant == Quant::SelfRef;
        if o.quant != Quant::Created && !in_cond_card {
            self.created = None;
        }
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        // A quoted ability inside an effect is its own text; a label's
        // ability is the line's.
        self.depth == 0
    }
    fn effect(&mut self, e: &mut Effect) {
        self.depth += 1;
        if self.skip.is_some() {
            return;
        }
        match e {
            // "put a number of +1/+1 counters on it equal to the greatest
            // mana value among those cards": "it" is said before the amount,
            // which the tree holds first.
            Effect::PutCounters { on, .. } if self.created.is_some() && *on == antecedent() => {
                on.quant = Quant::Created;
            }
            // "Create a 1/1 red Goblin creature token, then attach this
            // Equipment to it": the card named in the clause is not "it".
            Effect::Attach { what, to }
                if self.created.is_some()
                    && what.quant == Quant::SelfRef
                    && *to == antecedent() =>
            {
                to.quant = Quant::Created;
            }
            Effect::Where { .. } => self.wheres.push((self.depth, None)),
            Effect::Otherwise(_) | Effect::IfYouDont(_) => {
                self.created = None;
                self.tokens = false;
            }
            Effect::Damage {
                source: Some(src), ..
            } if *src == antecedent() && self.created == Some(false) => self.created = None,
            Effect::Modal { modes, .. } => {
                for m in modes.iter_mut() {
                    let mut v = CreatedTokens::new();
                    for x in m.iter_mut() {
                        x.walk(&mut v);
                    }
                }
                self.skip = Some(self.depth);
            }
            _ => {}
        }
    }
    fn amount(&mut self, _: &mut Amount) {
        // The `Where`'s own X, read after its effects.
        if self.skip.is_some() {
            return;
        }
        if let Some((d, saved)) = self.wheres.last_mut() {
            if *d == self.depth {
                if saved.is_none() {
                    *saved = Some(self.created);
                }
                self.created = None;
            }
        }
    }
    fn condition(&mut self, _: &mut Condition) {
        self.in_cond += 1;
    }
    fn condition_done(&mut self, _: &mut Condition) {
        self.in_cond -= 1;
    }
    fn effect_done(&mut self, e: &mut Effect) {
        if self.skip == Some(self.depth) {
            self.skip = None;
        }
        if self.skip.is_some() {
            self.depth -= 1;
            return;
        }
        if self.wheres.last().map(|w| w.0) == Some(self.depth) && matches!(e, Effect::Where { .. })
        {
            if let Some((_, Some(saved))) = self.wheres.pop() {
                self.created = saved;
            }
        }
        self.depth -= 1;
        let one = |n: &Amount| *n == Amount::N(1);
        self.created = match e {
            Effect::Create { n, .. }
            | Effect::CreateCopy { n, .. }
            | Effect::CreateNamed { n, .. }
            | Effect::CreateAttached { n, .. } => Some(one(n)),
            Effect::CreateOfEach { .. } => Some(false),
            // Effects that bring in an object they name in no `Objects`.
            Effect::RevealTop { .. }
            | Effect::LookAtTop { .. }
            | Effect::RevealHand { .. }
            | Effect::LookAtHand { .. }
            | Effect::Mill { .. }
            | Effect::Scry { .. }
            | Effect::Surveil { .. }
            | Effect::Draw { .. }
            | Effect::Copy { .. }
            | Effect::Amass { .. }
            | Effect::Action { .. }
            | Effect::UntilFound { .. }
            | Effect::DigUntilTotal { .. } => None,
            _ => self.created,
        };
        match e {
            Effect::Create { token, .. }
            | Effect::CreateNamed { token, .. }
            | Effect::CreateAttached { token, .. } => self.types = token.types.clone(),
            Effect::CreateCopy { .. } | Effect::CreateOfEach { .. } => self.types.clear(),
            _ => {}
        }
        if self.created.is_some() {
            self.tokens = true;
        }
    }
}

/// Deceit: "Target opponent reveals their hand. You choose a nonland card
/// from it": "it" is the hand just revealed, which no `Objects` names, so
/// a reference back would point at whatever object came before (Deceit's
/// creature). After a player reveals or is looked at their hand, "from it"
/// is that player's hand.
struct FromRevealedHand(Option<Player>);

impl Visitor for FromRevealedHand {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if let Some(who) = &self.0 {
            for q in o.filter.posts.iter_mut() {
                if matches!(q, Post::From(x) if **x == antecedent()) {
                    *q = Post::FromHand(who.clone());
                }
            }
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect_done(&mut self, e: &mut Effect) {
        match e {
            Effect::RevealHand(who) | Effect::LookAtHand { whose: who, .. } => {
                self.0 = Some(who.clone());
            }
            Effect::Modal { .. } => self.0 = None,
            _ => {}
        }
    }
}

/// Canopy Gargantuan: "put a number of +1/+1 counters on each other
/// creature you control equal to that creature's toughness"; Lukka: "Each
/// creature you control deals damage equal to its power to each opponent".
/// The amount is worked out for each object the effect acts on, each in
/// turn, so its "its" or "that creature's" is `Quant::Itself`, as in a
/// filter's comparison (Kutzil), where a bare `Antecedent` would be one
/// earlier object.
struct EachItself;

impl Visitor for EachItself {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect(&mut self, e: &mut Effect) {
        let (each, n) = match e {
            Effect::PutCounters { n, on, .. } => (&*on, n),
            Effect::Damage {
                source: Some(src),
                n,
                ..
            } => (&*src, n),
            _ => return,
        };
        if !matches!(each.quant, Quant::Each | Quant::All | Quant::Plural) {
            return;
        }
        let theirs: Vec<&Adj> = kinds(&each.filter);
        let one = |x: &Objects| {
            x.quant == Quant::Antecedent
                && x.filter.posts.is_empty()
                && x.filter.or.is_none()
                && x.filter.kind.is_none()
                && kinds(&x.filter).iter().all(|a| theirs.contains(a))
        };
        if let Amount::PowerOf(x) | Amount::ToughnessOf(x) | Amount::ManaValueOf(x) = n {
            if one(x) {
                x.quant = Quant::Itself;
            }
        }
    }
}

/// Corrosion: "put a rust counter on each artifact target opponent
/// controls. Then destroy each artifact with mana value less than or equal
/// to the number of rust counters on it": where the ability puts that kind
/// of counter on other objects, "on it" in a filter's comparison is the
/// object tested, each in turn ([itself-in-with]). Sphere of Annihilation's
/// void counters, which the card itself holds, stay a reference back.
struct CountersOnItself(Vec<CounterKind>);

impl Visitor for CountersOnItself {
    fn enter(&mut self, o: &mut Objects) -> bool {
        for q in o.filter.posts.iter_mut() {
            let Post::With(c) = q else { continue };
            let Amount::CountOf(n) = &mut c.value else {
                continue;
            };
            let Some(Noun::Counter(Some(k))) = &n.filter.kind else {
                continue;
            };
            if !self.0.contains(k) {
                continue;
            }
            for q in n.filter.posts.iter_mut() {
                if let Post::On(x) = q {
                    if **x == antecedent() {
                        x.quant = Quant::Itself;
                    }
                }
            }
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

fn counters_on_itself(a: &mut Ability) {
    struct Kinds(Vec<CounterKind>);
    impl Visitor for Kinds {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
        fn effect(&mut self, e: &mut Effect) {
            if let Effect::PutCounters { kind, on, .. } = e {
                if on.quant != Quant::SelfRef {
                    self.0.push(kind.clone());
                }
            }
        }
    }
    let mut kinds = Kinds(Vec::new());
    a.walk(&mut kinds);
    if !kinds.0.is_empty() {
        a.walk(&mut CountersOnItself(kinds.0));
    }
}

/// "Put that card onto the battlefield …, then put all other cards revealed
/// this way on the bottom of your library" (Shifting Shadow, Hermit Druid):
/// the permanent whose ability it is was never revealed or exiled this way,
/// so "other" among those cards is other than the card named before
/// ([other-than-named-before]), in any ability, as in a spell.
struct OtherMarkedCards;

impl Visitor for OtherMarkedCards {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let marked = o.filter.kind == Some(Noun::Card)
            && o.filter.posts.iter().any(|q| matches!(q, Post::Marked(_)));
        if marked {
            other_than(o, &antecedent(), None);
        }
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// "a creature card that shares a creature type with enchanted creature
/// from among them" (Call to the Kindred): "from among them" is where the
/// card is chosen from, not the creature it shares a type with, so it goes
/// on the outer objects, after the shared type ([after-self-noun]).
struct FromAmongAfterShares;

impl Visitor for FromAmongAfterShares {
    fn enter(&mut self, o: &mut Objects) -> bool {
        let mut moved = Vec::new();
        for q in o.filter.posts.iter_mut() {
            if let Post::SharesType { with: Some(w), .. } = q {
                if matches!(
                    w.filter.posts.last(),
                    Some(Post::FromAmongThem | Post::FromAmong(_))
                ) {
                    moved.extend(w.filter.posts.pop());
                }
            }
        }
        o.filter.posts.extend(moved);
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
}

/// A whole ability line, which must be consumed to the last token.
pub fn parse_line(toks: &[Tok], spell: bool, continues: bool) -> Result<Ability, usize> {
    let mut p = P::new(toks);
    p.spell = spell;
    p.continues = continues;
    match ability(&mut p, spell) {
        Some(mut a) if p.at_end() => {
            a.walk(&mut OtherThanTheSource);
            a.walk(&mut FromEachLibrary);
            a.walk(&mut SpellCastFrom);
            a.walk(&mut MaySubject);
            a.walk(&mut DelayedSameClause);
            a.walk(&mut CreatedTokens::new());
            a.walk(&mut FromRevealedHand(None));
            a.walk(&mut EachItself);
            a.walk(&mut DelayedX);
            a.walk(&mut OtherMarkedCards);
            a.walk(&mut FromAmongAfterShares);
            counters_on_itself(&mut a);
            Ok(a)
        }
        _ => Err(p.furthest),
    }
}

/// "you cast a legendary spell from your hand": the zone said after the
/// spell is where it was cast from, read as `Post::CastFrom`. The object
/// rule reads it as a zone the spell is in, and a spell is on the stack.
pub(super) fn casts(who: Player, mut what: Objects) -> Event {
    for q in what.filter.posts.iter_mut() {
        if let Post::In(z) = q {
            *q = Post::CastFrom(z.clone());
        }
    }
    Event::Casts { who, what }
}

/// "Whenever you cast a spell with {X} in its mana cost, put X +1/+1
/// counters on ~": the ability's X is the spell's (CR 107.3e), where
/// nothing else defines one ([x-of-the-spell]).
fn x_of_the_spell(t: &Trigger, effect: &mut [Effect]) {
    let Trigger::When(e) = t else { return };
    x_of_event(e, effect);
}

/// Brass Infiniscope: "When you next cast a spell with {X} in its mana cost
/// this turn, you draw a card and gain half X life": a delayed trigger's X
/// is its spell's too ([x-of-the-spell]).
struct DelayedX;

impl Visitor for DelayedX {
    fn enter(&mut self, _: &mut Objects) -> bool {
        true
    }
    fn ability(&mut self, _: &mut Ability) -> bool {
        true
    }
    fn effect(&mut self, e: &mut Effect) {
        if let Effect::DelayedNext { event, then, .. }
        | Effect::DelayedWhen { event, then }
        | Effect::DelayedWhenever { event, then } = e
        {
            x_of_event(event, then);
        }
    }
}

fn x_of_event(e: &Event, effect: &mut [Effect]) {
    let Some(spell) = trigger_object(e) else {
        return;
    };
    if !spell.filter.posts.contains(&Post::XInManaCost) {
        return;
    }
    struct V(u32);
    impl Visitor for V {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn effect(&mut self, e: &mut Effect) {
            self.0 += u32::from(matches!(e, Effect::Where { .. }));
        }
        fn effect_done(&mut self, e: &mut Effect) {
            self.0 -= u32::from(matches!(e, Effect::Where { .. }));
        }
        fn amount(&mut self, a: &mut Amount) {
            if *a == Amount::X && self.0 == 0 {
                *a = Amount::XOf(Box::new(Objects {
                    quant: Quant::Triggering,
                    filter: Filter::default(),
                }));
            }
        }
    }
    for e in effect {
        e.walk(&mut V(0));
    }
}

/// What a reference back in a condition said after its clause is, when the
/// clause names one object by a noun of its own ([trailing-if]): that
/// object, or each of a set in turn.
fn clause_object(q: &Quant) -> Option<Quant> {
    match q {
        Quant::Target { .. }
        | Quant::AnyTarget { .. }
        | Quant::AnotherTarget
        | Quant::OtherTarget { .. }
        | Quant::SecondTarget
        | Quant::Enchanted
        | Quant::Equipped
        // "You may put the exiled card onto the battlefield if it's a
        // creature card" (The Creation of Avacyn).
        | Quant::Linked => Some(Quant::ClauseObject),
        Quant::Each | Quant::All | Quant::Plural => Some(Quant::Itself),
        _ => None,
    }
}

/// "Damage can't be prevented this turn. If this spell was kicked, it deals
/// 5 damage …": with no object named before, the "it" after "this spell was
/// kicked" is the spell, as in a static ([kicked-it]).
fn kicked_it(effect: &mut [Effect]) {
    struct Any(bool);
    impl Visitor for Any {
        fn enter(&mut self, _: &mut Objects) -> bool {
            self.0 = true;
            false
        }
    }
    for e in effect {
        if let Effect::If {
            cond: Condition::WasKicked,
            then,
        } = e
        {
            let mut bare = FirstBare(false);
            for t in then.iter_mut() {
                t.walk(&mut bare);
            }
            return;
        }
        let mut any = Any(false);
        e.walk(&mut any);
        if any.0 {
            return;
        }
    }
}
