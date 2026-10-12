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
                let cond = p.attempt(|p| { p.word("if")?; condition(p) });
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
fn spend_uses(p: &mut P) -> Option<Vec<SpendUse>> {
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
                    if let Trigger::When(e) = &trigger { other_than_the_trigger(e, Some(&mut cond), &mut effect); }
                    let trigger = Trigger::InterveningIf(Box::new(trigger.clone()), Box::new(cond));
                    finished(p).then_some(Ability::Triggered { trigger, effect, limits })
                },
                |p| {
                    let mut effect = effects(p)?;
                    let limits = limits(p);
                    about_the_trigger(&trigger, None, &mut effect);
                    if let Trigger::When(e) = &trigger { other_than_the_trigger(e, None, &mut effect); }
                    finished(p).then_some(Ability::Triggered { trigger: trigger.clone(), effect, limits })
                },
            )
        },
        |p| { if !spell { return None; } let mut e = effects(p)?; other_than_named_before(&mut e); finished(p).then_some(Ability::Spell(e)) },
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
            Tok::Word(_) | Tok::SelfRef | Tok::Possessive | Tok::Other('!' | '?') => {
                label.push(crate::show(p.peek()?));
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
        // "Whenever this creature becomes the target of a spell or ability
        // you control …, put a +1/+1 counter on it": the spell is said
        // only to say what targets it; a reference back to the spell names
        // it as one.
        | Event::BecomesTarget { what, .. }
        | Event::TransformsInto { what, .. }
        | Event::Cycles {
            what: Some(what), ..
        } => what,
        Event::While { event, .. }
        | Event::AndIf { event, .. }
        | Event::FirstTimeEachTurn(event) => {
            return trigger_object(event);
        }
        Event::Or(v) => {
            let all: Vec<Objects> = v.iter().map(trigger_object).collect::<Option<_>>()?;
            return all.windows(2).all(|w| w[0] == w[1]).then(|| all[0].clone());
        }
        _ => return None,
    };
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
    what.clone().walk(&mut count);
    (count.0 == 1).then(|| what.clone())
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
            // "This creature enters with a +1/+1 counter on it for each
            // color of mana spent to cast it": a static whose subject is the
            // card says "it" for the card, as row 86's condition does.
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
        let named = first(&mut |f: &mut First| cond.walk(f));
        // "~ has hexproof as long as it's untapped": the subject is the card.
        if !subjects.is_empty() && subjects.iter().all(is_card) && is_it(&named) {
            cond.walk(&mut Bare(Quant::SelfRef));
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
            // A back-reference left as one ("the exiled card") is to
            // another object, which "its" after it may be.
            // "counter it if a card with the same name is in a graveyard":
            // an indefinite in a condition says one exists, and is nothing
            // a later "it" refers back to.
            let indefinite = matches!(o.quant, Quant::One | Quant::Another | Quant::Plural);
            if o.quant == Quant::SelfRef {
                self.named.0 = true;
            } else {
                self.named.1 = true;
                self.after_card = false;
            }
            match o.quant {
                Quant::SelfRef => self.self_pending = true,
                Quant::Triggering => {}
                // Only against a trigger's object: with none, "this land
                // and a creature named Hanweir Garrison … exile them" names
                // both.
                _ if indefinite && self.in_cond > 0 && self.to != Quant::Antecedent => {}
                _ => self.seen_new = true,
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
            self.depth += 1;
            self.seen_self |= self.self_pending;
            if matches!(e, Effect::ForEach { .. }) {
                self.loops.push((self.seen_new, self.seen_self));
            }
            if self.drew && matches!(e, Effect::Reveal { what, .. } if *what == antecedent()) {
                self.seen_new = true;
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
            if matches!(e, Effect::ForEach { .. }) {
                self.loops.pop();
            }
            self.seen_self |= self.self_pending;
            self.drew = matches!(e, Effect::Draw { .. });
            self.damage_source = match e {
                Effect::Damage {
                    source: Some(src), ..
                } if matches!(src.quant, Quant::SelfRef | Quant::Triggering) => Some(src.clone()),
                _ => None,
            };
            if matches!(
                e,
                Effect::RevealTop { .. }
                    | Effect::LookAtTop { .. }
                    | Effect::RevealHand { .. }
                    | Effect::LookAtHand { .. }
                    | Effect::Mill { .. }
                    | Effect::Scry { .. }
                    | Effect::Surveil { .. }
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
            ) {
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
                let like = kinds(&what.filter);
                if !like.is_empty() {
                    other_than(counted, &antecedent(), Some(&like));
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

/// A whole ability line, which must be consumed to the last token.
pub fn parse_line(toks: &[Tok], spell: bool) -> Result<Ability, usize> {
    let mut p = P::new(toks);
    p.spell = spell;
    match ability(&mut p, spell) {
        Some(mut a) if p.at_end() => {
            a.walk(&mut OtherThanTheSource);
            a.walk(&mut FromEachLibrary);
            a.walk(&mut MaySubject);
            Ok(a)
        }
        _ => Err(p.furthest),
    }
}
