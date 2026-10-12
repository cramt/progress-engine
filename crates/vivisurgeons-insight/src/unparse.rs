//! Trees back into Oracle text: the grammar run backwards.
//!
//! A tree that parsed completely can still have lost a word the text said (a
//! dropped "bottom" of a library, a subject read as "you"). Such a tree cannot
//! say the word again, so rendering every tree and comparing it with the line
//! it came from catches the loss mechanically, where reading by eye caught it
//! by luck. Every word is rendered from a typed node; nothing here holds the
//! source text.
//!
//! A rendering is a sequence of tokens with a few places where English, not
//! the tree, picks the form. Those places are all built in `syn`, each with
//! the reason the forms mean the same, so the list of what the comparison
//! forgives is one module long.

use crate::ast::*;
use crate::lex::{self, Pt, PtPart, Signed, Tok};
use crate::parse::third_person;
use crate::vocab::singulars;

/// Whether a noun or verb is singular, plural, or decided by an antecedent
/// the tree holds by position only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Num {
    Sg,
    Pl,
    Any,
}

#[derive(Clone, Debug)]
enum Piece {
    Toks(Vec<Tok>),
    /// A noun given in the singular, inflected for `Num`.
    Noun(String, Num),
    /// A verb given in its base form; `Sg` is the third person singular,
    /// `Pl` the base form (plural, imperative, after "may").
    Verb(String, Num),
    /// Built only by `syn`.
    Alt(Vec<Vec<Piece>>),
}

#[derive(Clone, Debug, Default)]
pub struct Out(Vec<Piece>);

impl Out {
    fn w(&mut self, s: &str) {
        let toks = lex::lex(&s.replace('~', "\u{E000}"));
        if !toks.is_empty() {
            self.0.push(Piece::Toks(toks));
        }
    }

    fn tok(&mut self, t: Tok) {
        self.0.push(Piece::Toks(vec![t]));
    }

    fn noun(&mut self, sg: &str, n: Num) {
        // A noun of two words inflects its last: "spell or ability" is
        // rendered by the caller; "time lord" inflects "lord".
        match sg.rsplit_once(' ') {
            Some((head, last)) => {
                self.w(head);
                self.0.push(Piece::Noun(last.into(), n));
            }
            None => self.0.push(Piece::Noun(sg.into(), n)),
        }
    }

    fn verb(&mut self, base: &str, n: Num) {
        let (first, rest) = base.split_once(' ').unwrap_or((base, ""));
        self.0.push(Piece::Verb(first.into(), n));
        self.w(rest);
    }

    fn add(&mut self, o: Out) {
        self.0.extend(o.0);
    }

    /// A sentence's period. After a quoted ability that ended in its own,
    /// Oracle text leaves it out, as `end_sentence` reads. Where the text
    /// ends in a choice, each form ends its own way.
    fn period(&mut self) {
        add_period(&mut self.0);
    }

    /// The first form of every choice, as display text.
    pub fn text(&self) -> String {
        let mut toks = Vec::new();
        flatten(&self.0, &mut toks);
        let mut s = String::new();
        // An opening quote glues to what follows it, a closing one to what
        // precedes it.
        let mut quote_open = false;
        let mut after_open = false;
        for t in &toks {
            let closing = t == "\"" && quote_open;
            let glue = matches!(t.as_str(), "," | "." | ":" | ";" | "'s" | "'" | "\n•") || closing;
            if !s.is_empty() && !glue && !after_open {
                s.push(' ');
            }
            after_open = t == "\"" && !quote_open;
            if t == "\"" {
                quote_open = !quote_open;
            }
            s.push_str(t);
        }
        s
    }
}

fn add_period(ps: &mut Vec<Piece>) {
    if let Some(Piece::Alt(alts)) = ps.last_mut() {
        for a in alts {
            add_period(a);
        }
        return;
    }
    let mut tail: Vec<&Tok> = Vec::new();
    for p in ps.iter().rev() {
        match p {
            Piece::Toks(ts) => tail.splice(0..0, ts.iter()),
            _ => break,
        };
    }
    // A closing quote whose ability may end in its own period, which
    // `quoted` leaves optional, so the sentence's own is optional too.
    let closes_quote = tail.last() == Some(&&Tok::Quote);
    let mut o = Out(std::mem::take(ps));
    if closes_quote {
        syn::quoted_period(&mut o);
    } else {
        o.tok(Tok::Period);
    }
    *ps = o.0;
}

fn flatten(ps: &[Piece], out: &mut Vec<String>) {
    for p in ps {
        match p {
            Piece::Toks(ts) => out.extend(ts.iter().map(show)),
            Piece::Noun(sg, n) => out.push(match n {
                Num::Pl => plural(sg),
                _ => sg.clone(),
            }),
            Piece::Verb(b, n) => out.push(match n {
                Num::Sg => third(b),
                _ => b.clone(),
            }),
            Piece::Alt(alts) => flatten(&alts[0], out),
        }
    }
}

fn show(t: &Tok) -> String {
    match t {
        Tok::Signed(Signed::N(n)) if *n < 0 => format!("−{}", -n),
        Tok::Signed(Signed::N(n)) => format!("+{n}"),
        Tok::Signed(Signed::PlusX) => "+X".into(),
        Tok::Signed(Signed::MinusX) => "−X".into(),
        Tok::Pt(pt) => pt_text(pt),
        Tok::Bullet => "\n•".into(),
        t => crate::show(t),
    }
}

/// Nouns whose plural is the singular: "seven or more Plains", "untapped
/// Myr", "Equipment you control". The creature types from "samurai" on are
/// never said with an "s" in Oracle text ("all Samurai you control",
/// "other Kavu", "Exile all creatures and Spacecraft").
const SAME_PLURAL: &[&str] = &[
    "plains",
    "myr",
    "equipment",
    "kor",
    "merfolk",
    "kithkin",
    "treefolk",
    "moonfolk",
    "fish",
    "sheep",
    "starfish",
    "djinn",
    "elk",
    "zubera",
    "arcane",
    "eldrazi",
    "samurai",
    "kavu",
    "aetherborn",
    "spacecraft",
    "nephilim",
];

fn plural(sg: &str) -> String {
    if SAME_PLURAL.contains(&sg) {
        return sg.to_string();
    }
    for (pl, s) in [("mice", "mouse"), ("men", "man"), ("teeth", "tooth")] {
        if let Some(stem) = sg.strip_suffix(s) {
            return format!("{stem}{pl}");
        }
    }
    if let Some(stem) = sg.strip_suffix('f') {
        if ["elf", "wolf", "dwarf"].iter().any(|w| sg.ends_with(w)) {
            return format!("{stem}ves");
        }
    }
    if let Some(stem) = sg
        .strip_suffix('y')
        .filter(|s| !s.ends_with(['a', 'e', 'o', 'u']))
    {
        return format!("{stem}ies");
    }
    if sg.ends_with('s') || sg.ends_with("sh") || sg.ends_with("ch") || sg.ends_with('x') {
        return format!("{sg}es");
    }
    format!("{sg}s")
}

fn third(base: &str) -> String {
    match base {
        "have" => "has".into(),
        "do" => "does".into(),
        "can't" | "can" | "may" | "doesn't" | "isn't" | "is" | "was" => base.into(),
        "don't" => "doesn't".into(),
        "aren't" => "isn't".into(),
        "are" => "is".into(),
        "were" => "was".into(),
        "weren't" => "wasn't".into(),
        _ => plural(base),
    }
}

// ---------------------------------------------------------------- matching

/// Where a rendering and its line first part: the tokens read, what the
/// rendering wanted next, and what the line had.
pub struct Diff {
    pub at: usize,
    pub want: Vec<String>,
    pub found: String,
}

struct Far {
    at: usize,
    want: Vec<String>,
}

impl Far {
    fn miss(&mut self, at: usize, want: String) {
        if at > self.at {
            self.at = at;
            self.want.clear();
        }
        if at == self.at && !self.want.contains(&want) {
            self.want.push(want);
        }
    }
}

fn noun_eq(t: &str, sg: &str, n: Num) -> bool {
    let is_pl = plural(sg) == t || (t != sg && singulars(t).iter().any(|s| s == sg));
    match n {
        Num::Sg => t == sg,
        Num::Pl => is_pl,
        Num::Any => t == sg || is_pl,
    }
}

fn verb_eq(t: &str, base: &str, n: Num) -> bool {
    let sg = t == third(base) || third_person(base, t);
    match n {
        Num::Sg => sg,
        Num::Pl => t == base,
        Num::Any => t == base || sg,
    }
}

/// "a" and "an" are one article: which one is said follows the sound of the
/// next word, which no tree records.
fn tok_eq(a: &Tok, b: &Tok) -> bool {
    match (a, b) {
        (Tok::Word(x), Tok::Word(y)) => x == y || (is_article(x) && is_article(y)),
        _ => a == b,
    }
}

fn is_article(w: &str) -> bool {
    w == "a" || w == "an"
}

fn m(
    ps: &[Piece],
    j: usize,
    toks: &[Tok],
    far: &mut Far,
    k: &mut dyn FnMut(usize, &mut Far) -> bool,
) -> bool {
    let Some((first, rest)) = ps.split_first() else {
        return k(j, far);
    };
    match first {
        Piece::Toks(ts) => {
            let mut jj = j;
            for t in ts {
                match toks.get(jj) {
                    Some(x) if tok_eq(x, t) => jj += 1,
                    _ => {
                        far.miss(jj, show(t));
                        return false;
                    }
                }
            }
            m(rest, jj, toks, far, k)
        }
        Piece::Noun(sg, n) => match toks.get(j) {
            Some(Tok::Word(w)) if noun_eq(w, sg, *n) => m(rest, j + 1, toks, far, k),
            _ => {
                far.miss(
                    j,
                    if *n == Num::Pl {
                        plural(sg)
                    } else {
                        sg.clone()
                    },
                );
                false
            }
        },
        Piece::Verb(b, n) => match toks.get(j) {
            Some(Tok::Word(w)) if verb_eq(w, b, *n) => m(rest, j + 1, toks, far, k),
            _ => {
                far.miss(j, if *n == Num::Sg { third(b) } else { b.clone() });
                false
            }
        },
        Piece::Alt(alts) => alts
            .iter()
            .any(|a| m(a, j, toks, far, &mut |j2, far| m(rest, j2, toks, far, k))),
    }
}

/// Whether the rendering can be read as exactly these tokens.
pub fn compare(out: &Out, toks: &[Tok]) -> Result<(), Diff> {
    let mut far = Far {
        at: 0,
        want: Vec::new(),
    };
    let n = toks.len();
    let ok = m(&out.0, 0, toks, &mut far, &mut |j, far| {
        if j == n {
            true
        } else {
            far.miss(j, "<end>".into());
            false
        }
    });
    if ok {
        return Ok(());
    }
    Err(Diff {
        at: far.at,
        want: far.want,
        found: toks.get(far.at).map(show).unwrap_or_else(|| "<end>".into()),
    })
}

/// The line's own text, read the way the lexer reads it: reminder text
/// dropped, case folded, whitespace ignored, the card's name as `~`.
pub fn line_tokens(text: &str) -> Vec<Tok> {
    lex::lex(&text.replace('~', "\u{E000}"))
}

// ---------------------------------------------------------------- synonyms

/// Every place two surface forms are read into one node and the comparison
/// accepts either. Nothing outside this module builds a choice, so this is
/// the whole list of what the round trip forgives. Each entry says why the
/// forms mean the same: English grammar first (agreement and punctuation,
/// then ellipsis and word order, then back-references, then words the
/// rules read the same with or without), and last the few the
/// Comprehensive Rules make identical, with the rule. Two forms that change
/// what happens in the game are kept apart in the tree instead ("another
/// target" and "a second target", CR 115.3); a form that only words the
/// same thing differently belongs here, not in the tree.
mod syn {
    use super::*;

    fn alt(o: &mut Out, alts: Vec<Out>) {
        o.0.push(Piece::Alt(alts.into_iter().map(|a| a.0).collect()));
    }

    fn of(s: &str) -> Out {
        let mut o = Out::default();
        o.w(s);
        o
    }

    fn any_of(o: &mut Out, forms: &[&str]) {
        alt(o, forms.iter().map(|s| of(s)).collect());
    }

    // ---------------------------------------- agreement and punctuation

    /// A number in digits or in words: "3 damage", "three cards". Oracle
    /// style picks by what is counted; the number is the same.
    pub fn number(o: &mut Out, n: u32) {
        match crate::parse::NUMBERS.get(n as usize) {
            Some(w) => alt(o, vec![of(&n.to_string()), of(w)]),
            None => o.w(&n.to_string()),
        }
    }

    /// One, said as the article: "draw a card", "add one mana".
    pub fn one(o: &mut Out) {
        any_of(o, &["a", "one", "1"]);
    }

    /// "that many" and "that much": the second for life, damage and mana,
    /// which English does not count.
    pub fn that_many(o: &mut Out) {
        any_of(o, &["that many", "that much"]);
    }

    /// "that each have a different power" and "with different powers": the
    /// singular says of each object what the plural says of the set, that
    /// no two share the value.
    /// "with a cycling ability", "that has a cycling ability", "with
    /// cycling abilities": a relative clause or a "with" phrase saying the
    /// object has one, in the number of its noun.
    pub fn with_ability(o: &mut Out, kind: Out, n: Num) {
        let form = |lead: &str, noun: &str| {
            let mut f = of(lead);
            f.add(kind.clone());
            f.w(noun);
            f
        };
        let mut forms = vec![form("with a", "ability"), form("that has a", "ability")];
        if n != Num::Sg {
            forms.push(form("with", "abilities"));
            forms.push(form("that have a", "ability"));
        }
        alt(o, forms);
    }

    /// "an opponent", "one of your opponents": any one player who is your
    /// opponent (CR 102.2), said with or without "your".
    pub fn an_opponent(o: &mut Out) {
        any_of(o, &["an opponent", "one of your opponents"]);
    }

    /// "is an Insect creature with base power and toughness 0/1" and "has
    /// base power and toughness 9/9": one base size (CR 613.4b), said with
    /// "with" after a type and with "has" on its own.
    pub fn with_base_pt(o: &mut Out, with: Out, have: Out) {
        alt(o, vec![with, have]);
    }

    /// "your life total becomes equal to your starting life total" and "…
    /// becomes half your starting life total": the value, with or without
    /// "equal to".
    pub fn becomes_equal_to(o: &mut Out) {
        alt(o, vec![of("equal to"), Out::default()]);
    }

    /// "your first creature spell each turn" and "the first creature spell
    /// you cast each turn": the spell is the one you cast, said by the
    /// possessive or by "you cast".
    pub fn nth_spell(o: &mut Out, yours: Out, cast: Out) {
        alt(o, vec![yours, cast]);
    }

    /// "a creature card is exiled this way", "… was exiled this way": the
    /// tense of a passive said of what this very ability did changes
    /// nothing; the verb agrees with its subject.
    pub fn passive_be(o: &mut Out, n: Num) {
        match n {
            Num::Sg => any_of(o, &["is", "was"]),
            Num::Pl => any_of(o, &["are", "were"]),
            Num::Any => any_of(o, &["is", "was", "are", "were"]),
        }
    }

    /// "this enchantment has no charge counters on it" and "there are no
    /// charge counters on this enchantment": one object's counters, said
    /// with it as subject or with "there are".
    pub fn there_are_on(o: &mut Out, has: Out, there: Out) {
        alt(o, vec![has, there]);
    }

    /// "it is" and "it's": a pronoun and its verb, contracted or not.
    pub fn contraction(o: &mut Out, full: Out, short: Out) {
        alt(o, vec![full, short]);
    }

    /// "of the chosen quality" and "of that quality": the parity just
    /// chosen, by either back-reference.
    pub fn chosen_quality(o: &mut Out) {
        any_of(o, &["of the chosen quality", "of that quality"]);
    }

    pub fn different_values(o: &mut Out, s: Stat) {
        let plural = match s {
            Stat::Power => "powers",
            Stat::Toughness => "toughnesses",
            Stat::ManaValue => "mana values",
        };
        alt(
            o,
            vec![
                of(&format!("that each have a different {}", stat_name(s))),
                of(&format!("with different {plural}")),
            ],
        );
    }

    /// "that's", "that are"; "it's attacked", "it has attacked"; "there's",
    /// "there is": the verb agrees with its subject, or is contracted.
    pub fn that_is(o: &mut Out) {
        any_of(o, &["that's", "that are"]);
    }

    pub fn it_has(o: &mut Out) {
        any_of(o, &["it's", "it has"]);
    }

    pub fn there_is(o: &mut Out) {
        any_of(o, &["there is", "there's"]);
    }

    /// The number of a bare noun in a rule about any one of them: "with a
    /// counter on it", "with counters on them"; "an ability of",
    /// "abilities of"; "a new target", "new targets" (as many as the copied
    /// spell has). "With one or more counters" restates "with a counter".
    pub fn with_counter(o: &mut Out) {
        any_of(o, &["with a", "with", "with one or more"]);
    }

    pub fn an_ability_of(o: &mut Out) {
        any_of(o, &["abilities of", "an ability of"]);
    }

    /// The same with nothing after it: "to activate abilities".
    pub fn an_ability(o: &mut Out) {
        any_of(o, &["abilities", "an ability"]);
    }

    pub fn new_targets(o: &mut Out) {
        any_of(o, &["new targets", "a new target"]);
    }

    /// The conjunction in a list of kinds: "instant and sorcery spells",
    /// "instant or sorcery spell", "artifacts and/or enchantments". A filter
    /// names every object of any listed kind, so the list is a union
    /// whichever word joins it; English picks "and" for plurals. An article
    /// may be said again after "or": "a Mountain or a Forest".
    pub fn list_conj(o: &mut Out, comma: bool) {
        if comma {
            any_of(o, &[", or", ", and", ", and/or", ", or a"]);
        } else {
            any_of(o, &["or", "and", "and/or", "or a"]);
        }
    }

    /// The commas of a list ending in "or", or between adjectives that each
    /// narrow a noun ("a nonartifact, nonland card"): punctuation.
    pub fn or_sep(o: &mut Out, i: usize, len: usize) {
        if i + 1 < len {
            any_of(o, &[",", ", or"]);
        } else {
            any_of(o, &[", or", "or"]);
        }
    }

    pub fn adj_comma(o: &mut Out) {
        alt(o, vec![Out::default(), of(",")]);
    }

    /// "{2}, sacrifice a creature", "pay 1 life and exile a blue card": a
    /// cost is paid whole (CR 601.2h), so how its parts are listed changes
    /// nothing.
    pub fn cost_sep(o: &mut Out) {
        any_of(o, &[",", "and", ", and"]);
    }

    /// Joins two clauses of one ability. A paragraph is one ability (CR
    /// 113.2c) and its instructions are followed in the order written (CR
    /// 608.2c), so a period, a comma, "and" and "then" between them say the
    /// same; the tree keeps the order, which is what they share. After a
    /// quoted ability that ended in its own period, the quote ends the
    /// sentence and the next clause starts bare.
    pub fn joiner(o: &mut Out, after_quote: bool) {
        if after_quote {
            let mut forms: Vec<Out> = JOINERS.iter().map(|s| of(s)).collect();
            forms.push(Out::default());
            alt(o, forms);
            return;
        }
        any_of(o, JOINERS);
    }

    /// "Coven — Then if you control …, draw a card." on an instant's second
    /// paragraph: the paragraphs of an instant or sorcery are its spell
    /// abilities (CR 113.3a), followed in the order written (CR 608.2c), so
    /// "then" says the order the card's abilities already have, as it does
    /// between two clauses of one paragraph (`joiner`). Offered only on a
    /// spell's paragraph after its first; `paragraph` checks it.
    pub fn then_paragraph(o: &mut Out) {
        alt(o, vec![Out::default(), of("then")]);
    }

    const JOINERS: &[&str] = &[
        ". ",
        ", then ",
        " and ",
        ", and ",
        ", ",
        " then ",
        ", and then ",
        " and then ",
        ". then ",
    ];

    /// The period after a closing quote whose ability ended with its own
    /// (`gains "{T}: Add {C}."`), and a quoted ability's last period before
    /// its closing quote mid-sentence (`with "{T}: Add {C}" and loses …`):
    /// Oracle style drops one of the two.
    pub fn quoted_period(o: &mut Out) {
        alt(o, vec![of("."), Out::default()]);
    }

    /// A quoted ability's last stop before its closing quote: its period,
    /// none (as above), or the comma of the list the quote is an item of,
    /// which American punctuation sets inside the quote: `has "Whenever
    /// …, draw a card," and is a Rogue`.
    pub fn quoted_last_stop(o: &mut Out) {
        alt(o, vec![of("."), Out::default(), of(",")]);
    }

    /// "spend mana as though it were mana of any type" and "mana of any
    /// type can be spent": CR 609.4b makes the two one permission.
    pub fn any_type_spent(o: &mut Out) {
        any_of(
            o,
            &[
                "spend mana as though it were mana of any type",
                "mana of any type can be spent",
            ],
        );
    }

    /// "Activate only if … . Activate only as a sorcery." and "Activate
    /// only if … and only as a sorcery": two limits on one ability, each
    /// its own in the tree, with "activate only" said again or, by English
    /// ellipsis, once.
    pub fn only_again(o: &mut Out, phrase: Out) {
        let again = out(|o| {
            o.w(". activate only");
            o.add(phrase.clone());
        });
        let and = out(|o| {
            o.w("and only");
            o.add(phrase.clone());
        });
        let comma = out(|o| {
            o.w(", and only");
            o.add(phrase);
        });
        alt(o, vec![again, and, comma]);
    }

    /// "a creature you control died this turn" and "a creature died under
    /// your control this turn": who controlled it as it died (CR 603.10a).
    pub fn died_under(o: &mut Out, said: Out, under: Out) {
        alt(o, vec![said, under]);
    }

    /// "mills half their library" and "mills cards equal to half the number
    /// of cards in their library": the same count.
    pub fn half_library(o: &mut Out, short: Out, long: Out) {
        alt(o, vec![short, long]);
    }

    /// "had counters on it", "had a counter on it", "had one or more
    /// counters on it": each holds if there was at least one ("one or
    /// more" is `AtLeast(N(1))`, SEMANTICS [one-or-more]).
    pub fn any_counters(o: &mut Out) {
        any_of(o, &["counters", "a counter", "one or more counters"]);
    }

    /// "your main phase" and "each of your main phases": a turn has two
    /// (CR 505.1), and a step names each one it begins.
    pub fn your_main_phase(o: &mut Out) {
        any_of(o, &["your main phase", "each of your main phases"]);
    }

    /// "a number of +1/+1 counters on it equal to N" and "X +1/+1 counters
    /// on it, where X is N": the same count, named or defined.
    pub fn where_x(o: &mut Out, a_number: Out, where_x: Out) {
        alt(o, vec![a_number, where_x]);
    }

    /// "You control your opponents while they're searching their libraries"
    /// and "While an opponent is searching their library, you control that
    /// player": a plural object whose condition says "they" holds of each
    /// one while that one meets it, as "Creatures you control get +1/+1 as
    /// long as they're attacking" does of each creature (Opposition Agent's
    /// ruling: "While controlling an opponent, you make all choices … for
    /// that player").
    pub fn each_while_they(o: &mut Out, plural: Out, one: Out) {
        alt(o, vec![plural, one]);
    }

    /// "Look at the top X cards of your library, where X is your devotion
    /// to blue. … If X is greater than or equal to …": X defined once holds
    /// in every sentence of the ability that says X (CR 107.3i, all
    /// instances of X on an object have the same value), so the definition
    /// may follow any of them.
    pub fn where_x_after(o: &mut Out, forms: Vec<Out>) {
        alt(o, forms);
    }

    /// "You may spend mana as though it were mana of any type" and "mana
    /// of any type can be spent": a permission said to the player who has
    /// it, or in the passive; CR 609.4b makes them one.
    pub fn may_or_passive(o: &mut Out, you_may: Out, passive: Out) {
        alt(o, vec![you_may, passive]);
    }

    /// The separator before item `i` of a list after a quoted ability,
    /// whose comma may stand inside the quote instead (`haste, "When …,"
    /// and "At …"`): the same list either way.
    pub fn quote_list_sep(o: &mut Out, i: usize, len: usize, after_quote: bool) {
        let with = out(|o| list_sep(o, i, len, "and"));
        if !after_quote || i == 0 {
            o.add(with);
            return;
        }
        let without = if i + 1 < len {
            Out::default()
        } else {
            of("and")
        };
        alt(o, vec![with, without]);
    }

    /// The period after a keyword line: none on a card, one inside a
    /// quoted ability or after a cost said in words ("Flashback—Sacrifice
    /// three creatures.").
    pub fn keyword_period(o: &mut Out) {
        alt(o, vec![Out::default(), of(".")]);
    }

    // ---------------------------------------- ellipsis and word order

    /// A clause whose subject is the one before it, or "you" for an
    /// instruction with none: "Each player discards their hand, then draws
    /// seven cards"; "This creature gets +1/+0 and deals 1 damage to you";
    /// "Draw a card" is said to the ability's controller, who is "you" (CR
    /// 109.5). Saying the subject again changes nothing.
    pub fn subject(o: &mut Out, said: Out, verb_alone: Out, verb_after: Out) {
        let mut full = said;
        full.add(verb_after);
        alt(o, vec![verb_alone, full]);
    }

    /// One subject with a list of predicates, or each predicate its own
    /// sentence with the subject said again: "Equipped creature gets
    /// +1/+1 and has flying" holds the same statics as two sentences.
    pub fn said_again(o: &mut Out, once: Out, again: Out) {
        alt(o, vec![once, again]);
    }

    /// "You may put a creature card from your hand onto the battlefield.
    /// That creature gains haste." and "…, then have it gain haste": a step
    /// after the option is part of it, said in the option's sentence or as
    /// a sentence of its own. Either way it happens only if the option was
    /// taken, and is no second option.
    pub fn option_steps(o: &mut Out, forms: Vec<Out>) {
        alt(o, forms);
    }

    /// A verb said once for several objects: "put one onto the battlefield
    /// and the other into your hand", "destroy target artifact, target
    /// creature, and target land", "You and permanents you control gain
    /// hexproof". Each object is its own clause in the tree.
    pub fn gapped(o: &mut Out, with_verb: Out, without: Out) {
        alt(o, vec![with_verb, without]);
    }

    /// "a noncreature artifact or enchantment" and "a noncreature artifact
    /// or noncreature enchantment": the word before the first type belongs
    /// to every type after it, said once or again.
    pub fn distributed(o: &mut Out, once: Out, again: Out) {
        alt(o, vec![once, again]);
    }

    /// A qualifier before or after the clause it qualifies: "If you do,
    /// draw a card" and "draw a card if you do"; the order of two
    /// predicates of one subject; "return to the battlefield target
    /// creature card" and "return target creature card to the battlefield";
    /// "the exiled card" and "the card exiled this way"; a token "with
    /// "Equipped creature gets +1/+1" and equip {2}", whose keywords and
    /// quoted abilities the tree keeps in two lists. English lets them sit
    /// either way with the same meaning.
    pub fn position(o: &mut Out, before: Out, after: Out) {
        alt(o, vec![before, after]);
    }

    /// "Put … onto the battlefield with an additional -1/-1 counter on it"
    /// and "Put … onto the battlefield …. Each of them enters with an
    /// additional -1/-1 counter on it": the counters are a replacement on
    /// the move's entering either way (CR 614.1c), said with the move or
    /// in the sentence after it.
    pub fn enters_with_after(o: &mut Out, with: Out, after: Out) {
        alt(o, vec![with, after]);
    }

    /// "If you control a creature with power 4 or greater, instead search
    /// …" and "… search … instead": "instead" before or after the clauses
    /// it replaces with, or unsaid where they say "instead of" themselves.
    pub fn instead(o: &mut Out, forms: Vec<Out>) {
        alt(o, forms);
    }

    /// "the controller of that creature" and "that creature's controller":
    /// the two English genitives.
    pub fn genitive(o: &mut Out, of_form: Out, s_form: Out) {
        alt(o, vec![of_form, s_form]);
    }

    /// "an exiled card", "a card in exile": an adjective or a phrase.
    pub fn exiled(o: &mut Out, adjective: Out, phrase: Out) {
        alt(o, vec![adjective, phrase]);
    }

    /// The preposition before a zone in an object phrase follows the verb:
    /// "return target card from your graveyard", "the number of cards in
    /// your graveyard". The object is in that zone either way.
    pub fn in_from(o: &mut Out) {
        any_of(o, &["from", "in"]);
    }

    /// The same for the battlefield, which English puts things "on":
    /// "exile all creatures … from the battlefield", "each creature on the
    /// battlefield".
    pub fn on_from(o: &mut Out) {
        any_of(o, &["on", "from"]);
    }

    /// "2 plus the sacrificed creature's mana value": a sum is the same in
    /// either order.
    pub fn sum(o: &mut Out, a: Out, b: Out) {
        let mut ab = a.clone();
        ab.w("plus");
        ab.add(b.clone());
        let mut ba = b;
        ba.w("plus");
        ba.add(a);
        alt(o, vec![ab, ba]);
    }

    /// "unless it has the same name as …": "unless" is "if not". "You
    /// control no Demons", "you don't control a Demon": none of them is not
    /// one of them.
    pub fn unless(o: &mut Out, if_not: Out, unless: Out) {
        alt(o, vec![if_not, unless]);
    }

    pub fn control_no(o: &mut Out, no: Out, dont: Out) {
        alt(o, vec![no, dont]);
    }

    /// "this artifact has three or more brick counters on it", "there are
    /// three or more brick counters on this artifact": one count, said of
    /// the object or of the counters.
    pub fn counters_on(o: &mut Out, has: Out, there_are: Out) {
        alt(o, vec![has, there_are]);
    }

    /// "any of the exiled card's colors" and "any color among legendary
    /// creatures you control": both pick from the union of the colors those
    /// objects have.
    pub fn colors_of(o: &mut Out, possessive: Out, among: Out) {
        alt(o, vec![possessive, among]);
    }

    // ---------------------------------------- back-references

    /// "it", "them", "they"; "that", "those", "the"; "its", "their":
    /// a pronoun or a definite article agrees with the noun it points back
    /// to, which the tree holds by position, not by word. A character card
    /// is "he" or "she", "him" or "her", where another object is "it".
    pub fn it(o: &mut Out) {
        any_of(o, &["it", "them", "they", "him", "her", "he", "she"]);
    }

    pub fn that(o: &mut Out) {
        any_of(o, &["that", "those", "the"]);
    }

    /// "that creature's controller sacrifices it": an object said a second
    /// time in one clause is said by pronoun, English naming a noun once
    /// per clause. The tree holds both as the same object, so the pronoun
    /// can be nothing else (Animate Dead's delayed trigger).
    pub fn again_it(o: &mut Out, named: Out) {
        alt(o, vec![of("it"), of("them"), named]);
    }

    /// "~", "he", "him": a character card is "he" or "she" in its own
    /// text, and Oracle text gives a gendered pronoun to nothing else.
    pub fn self_named(o: &mut Out) {
        any_of(o, &["~", "he", "she", "him", "her"]);
    }

    /// "cards exiled with it", "cards exiled with ~": "it" there is the
    /// object whose ability exiled them, the card itself (CR 607.2a).
    pub fn exiled_with_it(o: &mut Out, named: Out) {
        alt(o, vec![named, of("it")]);
    }

    /// "becomes an artifact creature" and "… in addition to its other
    /// types": an object that becomes an artifact creature keeps its prior
    /// card types and subtypes either way (CR 205.1b); "becomes an Island"
    /// keeps all but its land types with nothing said (CR 205.1a).
    pub fn keeps_implied(o: &mut Out, said: Out) {
        alt(o, vec![Out::default(), said]);
    }

    /// "when each opponent casts their first spell of the game, …" and "for
    /// each opponent, when that player casts …": "each" said inside the
    /// event or before it, one delayed trigger per opponent either way.
    pub fn each_in_trigger(o: &mut Out, per: Out, inside: Out) {
        alt(o, vec![per, inside]);
    }

    /// "Tyvar gains indestructible until end of turn. Tap it." and "… Tap
    /// Tyvar.": after a sentence whose only object is the card, "it" can
    /// name nothing else.
    pub fn it_self(o: &mut Out, named: Out, it: Out) {
        alt(o, vec![named, it]);
    }

    /// "When this creature dies, return it to its owner's hand", "~ has
    /// hexproof as long as it entered this turn": the card itself, said by
    /// its name or by a pronoun where the text refers back to it. The tree
    /// holds which object the pronoun is (SEMANTICS [referent-trigger]); which
    /// word says it is English.
    pub fn the_card(o: &mut Out, named: Out, pronoun: Out) {
        alt(o, vec![named, pronoun]);
    }

    /// "deals 1 damage to that player", "… to them": the singular "they"
    /// of a player, in the object case. Plural objects are "each of them".
    pub fn them(o: &mut Out) {
        any_of(o, &["that player", "them", "the player", "they"]);
    }

    pub fn its(o: &mut Out) {
        any_of(o, &["its", "their", "his", "her"]);
    }

    /// "put its counters on", "put those counters on": every counter on an
    /// object referred back to. The parser reads "those counters" so only
    /// after an intervening "if it had counters on it" (The Ozolith), where
    /// "those" points back to the counters that clause said were on it; the
    /// determiner changes which noun the reference goes through, not which
    /// counters are put.
    pub fn its_counters(o: &mut Out) {
        any_of(o, &["its", "their", "his", "her", "those"]);
    }

    pub fn it_is(o: &mut Out) {
        any_of(o, &["it's", "he's", "she's"]);
    }

    /// "it", "that ability", "the token": the same back-reference, said by
    /// pronoun or by noun. Likewise "from among them" and "from among those
    /// cards", "if it was kicked" and "if this spell was kicked", and "It
    /// can't be regenerated" and "A creature destroyed this way can't be
    /// regenerated".
    pub fn anaphor(o: &mut Out, noun: &str) {
        alt(
            o,
            vec![
                of("it"),
                of(&format!("that {noun}")),
                of(&format!("the {noun}")),
            ],
        );
    }

    pub fn from_among_them(o: &mut Out) {
        any_of(o, &["from among them", "from among those cards"]);
    }

    pub fn kicked(o: &mut Out) {
        any_of(o, &["this spell", "it", "~"]);
    }

    /// "It can't be regenerated", "The creature can't be regenerated",
    /// "Creatures destroyed this way can't be regenerated": what the destroy
    /// before named, by pronoun, by a definite noun of a type it was said
    /// to have, or as what it destroyed. Every destroyed object is a
    /// permanent (CR 701.8a), so that noun is always licensed.
    pub fn destroyed(o: &mut Out, types: &[String]) {
        let mut forms = vec![of("it"), of("they")];
        for t in types.iter().map(String::as_str).chain(["permanent"]) {
            let pl = plural(t);
            for f in [
                format!("that {t}"),
                format!("the {t}"),
                format!("those {pl}"),
                format!("a {t} destroyed this way"),
                format!("{pl} destroyed this way"),
            ] {
                forms.push(of(&f));
            }
        }
        alt(o, forms);
    }

    /// "twice that many of each of those kinds of counters", "… of those
    /// counters": the counters an event named, kind by kind either way.
    pub fn those_counters(o: &mut Out) {
        any_of(
            o,
            &[
                "each of those kinds of counters",
                "those counters",
                "that counter",
            ],
        );
    }

    /// "that player", "they", "the player"; "their", "that player's": the
    /// player an earlier clause named. The verb agrees with the form.
    pub fn that_player(o: &mut Out) {
        any_of(o, &["that player", "they", "the player", "those players"]);
    }

    pub fn their(o: &mut Out) {
        alt(o, vec![of("their"), of("that player's")]);
    }

    /// "they've cast this turn", "that player has cast this turn": the
    /// player by pronoun or by name, the verb contracted or not.
    pub fn they_have(o: &mut Out) {
        any_of(o, &["they've", "they have", "that player has"]);
    }

    /// "if that player does", "if the player does": the player an earlier
    /// clause offered a choice, by its name or by anaphor.
    pub fn anaphor_player(o: &mut Out, said: Out, pronoun: Out) {
        alt(o, vec![said, pronoun]);
    }

    /// "the rest", "the other", "the others": what an earlier choice left,
    /// in the number it has.
    pub fn the_rest(o: &mut Out) {
        any_of(o, &["the rest", "the other", "the others"]);
    }

    /// "cards in hand", "cards in your hand": the subject's own hand, said
    /// or not.
    pub fn own(o: &mut Out, who: &Player) {
        let whose = if *who == Player::You { "your" } else { "their" };
        alt(o, vec![Out::default(), of(whose)]);
    }

    // ---------------------------------------- words read the same either way

    /// "each of those creatures", "each of up to two target creatures";
    /// "each with power greater than …": "each" says distributively what a
    /// rule about the objects says of each of them anyway.
    pub fn each_of(o: &mut Out) {
        alt(o, vec![Out::default(), of("each of")]);
    }

    pub fn each_with(o: &mut Out) {
        any_of(o, &["with", "each with"]);
    }

    /// "permanents you control also gain indestructible": "also" says the
    /// effect adds to the one before it, which a later effect does anyway.
    pub fn also(o: &mut Out) {
        alt(o, vec![Out::default(), of("also")]);
    }

    /// "One or two target creatures each get +2/+2": "each" after a plural
    /// subject says distributively what a bonus to the objects gives each
    /// of them anyway, as "each of" before it does.
    pub fn each_after(o: &mut Out) {
        alt(o, vec![Out::default(), of("each")]);
    }

    /// "with the same name as one another": the objects are compared with
    /// one another. "Whichever comes first": an "until A or B" ends at the
    /// first. "Shares at least one creature type": "shares a creature type".
    pub fn same_name(o: &mut Out) {
        any_of(
            o,
            &["with the same name", "with the same name as one another"],
        );
    }

    /// "a card with the same name is in a graveyard" and "… with the same
    /// name as it": one object has no "one another", so the name it shares
    /// is the one named before, said or not.
    pub fn same_name_as_it(o: &mut Out, said: Out) {
        alt(o, vec![of("with the same name"), said]);
    }

    /// "becomes a 4/4 artifact creature with flying" and "… and gains
    /// flying": the abilities one effect adds, in layer 6 either way (CR
    /// 613.1f, 613.6), for the same duration.
    pub fn becomes_with(o: &mut Out, with: Out, and_gains: Out) {
        alt(o, vec![with, and_gains]);
    }

    /// "+1: Add {R}." on a planeswalker and "[+1]: Add {R}." inside a
    /// quoted ability: Oracle text brackets a loyalty cost inside quotes,
    /// the same cost (CR 107.7 writes the loyalty symbol "[+N]").
    pub fn loyalty_cost(o: &mut Out, plain: Out, bracketed: Out) {
        alt(o, vec![plain, bracketed]);
    }

    pub fn whichever_first(o: &mut Out) {
        alt(o, vec![Out::default(), of(", whichever comes first")]);
    }

    pub fn shares_a(o: &mut Out) {
        any_of(o, &["a", "at least one"]);
    }

    /// "10 or more", "4 or greater": both are "at least".
    pub fn or_more(o: &mut Out) {
        any_of(o, &["or greater", "or more"]);
    }

    /// "five or more other Mountains", "at least five other Mountains": the
    /// same lower bound, said after the number or before it.
    pub fn at_least(o: &mut Out, or_more: Out, at_least: Out) {
        alt(o, vec![or_more, at_least]);
    }

    /// "the top three cards of your library", "three cards from the top of
    /// your library": the same cards, named from the top down either way.
    pub fn top_cards(o: &mut Out, the_top: Out, from_the_top: Out) {
        alt(o, vec![the_top, from_the_top]);
    }

    /// "a +1/+1 counter on it for each creature that died this turn" and
    /// "a number of +1/+1 counters on it equal to the number of creatures
    /// that died this turn": one counter per object is that many counters.
    pub fn for_each_count(o: &mut Out, for_each: Out, a_number: Out) {
        alt(o, vec![for_each, a_number]);
    }

    /// "the creature with the greatest power", "a creature they control with
    /// the greatest power": one object picked out by the post; where several
    /// tie, the chooser picks among them either way.
    pub fn the_greatest(o: &mut Out) {
        any_of(o, &["the", "a"]);
    }

    /// "from among the nonland permanents they control", "… all nonland
    /// permanents they control": a description that names its set in full
    /// (here by who controls it) picks out every object in it either way.
    pub fn all_or_the(o: &mut Out, all: Out, the: Out) {
        alt(o, vec![all, the]);
    }

    /// "all the cards from your hand", "all cards from your hand": the
    /// article adds nothing to "all".
    pub fn all_the(o: &mut Out) {
        any_of(o, &["all", "all the"]);
    }

    /// "a graveyard", "graveyards" in "target cards from graveyards": a
    /// card is in one graveyard either way, the noun's number agrees with
    /// the cards'.
    pub fn a_graveyard(o: &mut Out) {
        any_of(o, &["a graveyard", "graveyards"]);
    }

    pub fn a_library(o: &mut Out) {
        any_of(o, &["a library", "libraries"]);
    }

    /// "your third card each turn", "your third card in a turn": the count
    /// starts again each turn either way.
    pub fn each_turn(o: &mut Out) {
        any_of(o, &["each turn", "in a turn"]);
    }

    /// "damage that would be dealt to creatures you control by red sources"
    /// and "damage that red sources would deal to creatures you control";
    /// "until they reveal a nonland card" and "until a nonland card is
    /// revealed": the passive and the active say the same doer and object.
    pub fn voice(o: &mut Out, one: Out, other: Out) {
        alt(o, vec![one, other]);
    }

    /// "for as long as this creature is on the battlefield" and "… remains
    /// on the battlefield": a permanent that leaves becomes a new object
    /// (CR 400.7), so the one named is there only for as long as it stays.
    pub fn remains(o: &mut Out, is: Out, remains: Out) {
        alt(o, vec![is, remains]);
    }

    /// "creatures get an additional -2/-2": the bonus adds to the earlier
    /// one, as any later bonus does (as with "also").
    pub fn additional(o: &mut Out) {
        alt(o, vec![Out::default(), of("an additional")]);
    }

    /// "a card that isn't on the battlefield", "a card not on the
    /// battlefield": the relative clause, or the same said without "that".
    pub fn not_in(o: &mut Out, that_isnt: Out, not: Out) {
        alt(o, vec![that_isnt, not]);
    }

    /// "create a token with "…"" and "create a token. It has "…"": what the
    /// token is created with, said inside the clause or as a sentence of
    /// its own.
    pub fn token_has(o: &mut Out, with: Out, it_has: Out) {
        alt(o, vec![with, it_has]);
    }

    /// "becomes a 2/2 creature that's still a land until end of turn" and
    /// "becomes a 2/2 creature until end of turn. It's still a land.": what
    /// it keeps, said inside the clause or as a sentence of its own.
    pub fn still_a(o: &mut Out, clause: Out, sentence: Out) {
        alt(o, vec![clause, sentence]);
    }

    /// "has a loyalty counter on it", "has one or more loyalty counters on
    /// it": having one is having at least one.
    pub fn a_counter(o: &mut Out, a: Out, one_or_more: Out) {
        alt(o, vec![a, one_or_more]);
    }

    /// "If this is the first time this ability has resolved this turn, … If
    /// it's the second time, …": after it is said once, the rest of the
    /// phrase goes unsaid; the count is of this ability either way.
    pub fn resolved_again(o: &mut Out, full: Out, short: Out) {
        alt(o, vec![full, short]);
    }

    /// "has cast … this turn" and "cast … this turn"; "who searches a
    /// library this way" and "who searched a library this way": with "this
    /// turn" or "this way" the tense says nothing more.
    pub fn has_cast(o: &mut Out) {
        any_of(o, &["cast", "has cast", "have cast"]);
    }

    /// "if this land entered this turn or if you control a basic land" and
    /// "… or you control a basic land": "if" said once for both, or again.
    pub fn or_if(o: &mut Out) {
        any_of(o, &["or", "or if"]);
    }

    /// "you had a land enter the battlefield under your control this turn"
    /// and "a land entered the battlefield under your control this turn":
    /// the same event, said of the player or of the land.
    pub fn had_enter(o: &mut Out, had: Out, entered: Out) {
        alt(o, vec![had, entered]);
    }

    /// The same said of "you", where "you've" contracts "you have".
    pub fn you_have_cast(o: &mut Out) {
        any_of(o, &["you cast", "you have cast", "you've cast"]);
    }

    /// "you haven't cast a spell this turn", "you didn't cast a spell this
    /// turn": with "this turn" the tense says nothing more.
    pub fn hasnt_cast(o: &mut Out) {
        any_of(o, &["haven't cast", "hasn't cast", "didn't cast"]);
    }

    /// "the life you lost this turn" and "the amount of life you lost this
    /// turn": the same amount, the second counted aloud.
    pub fn the_life(o: &mut Out) {
        any_of(o, &["the life", "the amount of life"]);
    }

    /// "you gained 3 or more life this turn" and "you've gained …": with
    /// "this turn" the simple past and the present perfect say the same.
    pub fn perfect(o: &mut Out, simple: Out, perfect: Out) {
        alt(o, vec![simple, perfect]);
    }

    pub fn did_tense(o: &mut Out, past: &str, present: Out) {
        alt(o, vec![of(past), present]);
    }

    /// "each graveyard", "all graveyards"; "each opponent", "your
    /// opponents" (the opponents of CR 102.2–102.3): the same set, one at a
    /// time or together.
    pub fn all_graveyards(o: &mut Out) {
        any_of(o, &["all graveyards", "each graveyard"]);
    }

    pub fn each_opponent(o: &mut Out) {
        any_of(o, &["each opponent", "your opponents", "all opponents"]);
    }

    /// "Each player can't gain life", "Players can't gain life": a rule
    /// said of every player, one at a time or together.
    pub fn each_player(o: &mut Out) {
        any_of(o, &["each player", "players"]);
    }

    /// "shuffle", "shuffle your library": a player shuffles only a library
    /// (CR 701.24a), and the one an instruction means is their own.
    pub fn a_any(o: &mut Out) {
        any_of(o, &["a", "any"]);
    }

    /// "each upkeep", "each player's upkeep": every step is the active
    /// player's (CR 102.1, the player whose turn it is), so each one is
    /// some player's.
    pub fn each_step(o: &mut Out) {
        any_of(o, &["each", "each player's"]);
    }

    /// "is put into a graveyard", "… from anywhere": an event that names
    /// no zone it comes from happens from any.
    pub fn from_anywhere(o: &mut Out, said: Out) {
        alt(o, vec![Out::default(), said]);
    }

    /// "put one of those cards back on top of your library": "back" says
    /// the card goes where it was looked at, which the zone already names.
    pub fn back(o: &mut Out) {
        alt(o, vec![Out::default(), of("back")]);
    }

    pub fn shuffle_library(o: &mut Out) {
        alt(
            o,
            vec![Out::default(), of("your library"), of("their library")],
        );
    }

    // ---------------------------------------- made identical by the rules

    /// "Pay {E}{E}: …", "Replicate—Pay {E}{E}{E}" and "{E}{E}: …": a cost
    /// of energy symbols is paid whether or not the verb is said; to pay
    /// {E} is to remove an energy counter (CR 107.14).
    pub fn pay(o: &mut Out, bare: Out, said: Out) {
        alt(o, vec![bare, said]);
    }

    /// "When" and "Whenever" introduce the same kind of trigger (CR 603.1:
    /// "[When/Whenever/At] [trigger condition or event], [effect]"); Oracle
    /// style picks "when" for events that happen once.
    pub fn when(o: &mut Out) {
        any_of(o, &["whenever", "when"]);
    }

    /// "When this creature enters and whenever an opponent draws …" and
    /// "Whenever this creature enters or an opponent draws …": one
    /// ability with two trigger conditions either way (CR 603.1b).
    pub fn trigger_conditions(o: &mut Out, one: Out, two: Out) {
        alt(o, vec![one, two]);
    }

    /// A move between zones with "put" or "return", or "put them back"
    /// after looking at the top of a library: the zone change is the same
    /// event whatever the verb (CR 400.6 names it by where the object goes
    /// from and to), and "back" is the top of the library they came from.
    pub fn move_verb(o: &mut Out, forms: Vec<Out>) {
        alt(o, forms);
    }

    /// "protection from each color" and "protection from white, from blue,
    /// from black, from red, and from green": there are five colors (CR
    /// 105.1).
    pub fn each_color(o: &mut Out, each: Out, listed: Out) {
        alt(o, vec![each, listed]);
    }

    /// "becomes a 4/4" and "has base power and toughness 4/4": both set
    /// power and toughness in layer 7b (CR 613.4b).
    pub fn base_pt(o: &mut Out, becomes: Out, base: Out) {
        alt(o, vec![becomes, base]);
    }

    /// "enters" and "enters the battlefield": the phrase "has been
    /// shortened to simply 'enters' in rules text" (CR glossary, "Enters
    /// the Battlefield").
    pub fn the_battlefield(o: &mut Out) {
        alt(o, vec![Out::default(), of("the battlefield")]);
    }

    /// "precombat main phase" and "first main phase": only the first main
    /// phase of a turn is precombat (CR 505.1a). "Second main phase" is not
    /// "postcombat main phase" (CR 505.1b counts main phases), so the tree
    /// keeps those two apart.
    pub fn precombat_main(o: &mut Out) {
        any_of(o, &["precombat main phase", "first main phase"]);
    }

    /// The library a card goes to: "its owner's library", "their library",
    /// "your library"; and "on top" for "on top of its owner's library". A
    /// card that would go to another player's library goes to its owner's
    /// (CR 400.3), so each says the same.
    pub fn owners_library(o: &mut Out) {
        any_of(
            o,
            &[
                "its owner's library",
                "their library",
                "your library",
                "their owners' libraries",
                "their owner's library",
            ],
        );
    }

    pub fn of_library(o: &mut Out, said: Out) {
        alt(o, vec![said, Out::default()]);
    }

    /// "protection from spells, protection from Spirits" and "protection
    /// from spells and from Spirits": the second is shorthand for the
    /// first, two separate protection abilities (CR 702.16g).
    pub fn and_from(o: &mut Out, listed: Out, and_from: Out) {
        alt(o, vec![listed, and_from]);
    }

    /// "its graveyard", "its owner's graveyard": an object's hand, library
    /// or graveyard is only ever its owner's (CR 400.3), so a bare "its"
    /// before one names the owner's. "Its controller's graveyard" is
    /// another player's for a stolen permanent, and its own tree.
    pub fn its_owners(o: &mut Out) {
        any_of(o, &["its owner's", "their owner's", "its"]);
    }

    /// "in addition to its other types" and "… its other land types" after
    /// land subtypes alone: a land given a land type in addition to its own
    /// keeps its land types (CR 305.7), and no land subtype touches a card
    /// type or supertype, so the rest it keeps either way (CR 205.1b).
    /// "are Juggernauts in addition to their other types" and "… their
    /// other creature types": setting a creature type in addition keeps the
    /// creature types it had, and no creature subtype touches a card type,
    /// supertype or other subtype (CR 205.1a, 205.1b).
    pub fn other_creature_types(o: &mut Out) {
        any_of(
            o,
            &[
                "in addition to its other types",
                "in addition to its other creature types",
                "in addition to their other types",
                "in addition to their other creature types",
            ],
        );
    }

    pub fn other_land_types(o: &mut Out) {
        any_of(
            o,
            &[
                "in addition to its other types",
                "in addition to its other land types",
                "in addition to their other types",
                "in addition to their other land types",
            ],
        );
    }
}

// ---------------------------------------------------------------- context

/// What the clauses being rendered inherit, as the parser's `implied` and
/// `during` do.
#[derive(Clone, Default)]
struct Cx {
    /// The subject a verb inherits when it says none.
    implied: Option<Player>,
    /// The last clause's subject, which a verb in the third person may
    /// continue: "Each player discards their hand, then draws seven cards".
    continues: Option<Player>,
    /// After "may": the verb is in its base form whoever does it.
    modal: bool,
}

fn out(f: impl FnOnce(&mut Out)) -> Out {
    let mut o = Out::default();
    f(&mut o);
    o
}

// ---------------------------------------------------------------- abilities

pub fn ability(a: &Ability) -> Out {
    // The trigger's own object is only ever said by referring back to it:
    // "it", "its", "that card", "with lesser mana value". So is a reference
    // said before a trailing "for each", which only that position tells
    // from the loop's own object.
    struct Said;
    impl crate::walk::Visitor for Said {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if matches!(
                o.quant,
                Quant::Triggering
                    | Quant::BeforeEach
                    | Quant::ClauseObject
                    | Quant::Listed
                    | Quant::Itself
                    | Quant::Created
                    | Quant::ThisAbility
                    | Quant::SourceOf(_)
                    | Quant::Linked
                    | Quant::OriginalAndCopies
            ) {
                o.quant = Quant::Antecedent;
            }
            // "the top card of each player's library" says the count once,
            // the possessive making it one per library.
            if let Quant::NFromEach(n) = &o.quant {
                o.quant = Quant::N(n.clone());
            }
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut a = a.clone();
    crate::walk::Walk::walk(&mut a, &mut Said);
    out(|o| ability_into(o, &a))
}

/// One line of a card: its ability, and whether the line continues a
/// spell's instructions from the paragraph before it, which licenses an
/// opening "Then" ([then-paragraph]).
pub fn paragraph(a: &Ability, continues: bool) -> Out {
    match a {
        Ability::Spell(_) if continues => out(|o| {
            syn::then_paragraph(o);
            o.add(ability(a));
        }),
        Ability::Labelled {
            label,
            ability: inner,
        } if continues && matches!(**inner, Ability::Spell(_)) => out(|o| {
            o.w(label);
            o.tok(Tok::Dash);
            syn::then_paragraph(o);
            o.add(ability(inner));
        }),
        _ => ability(a),
    }
}

fn ability_into(o: &mut Out, a: &Ability) {
    let cx = Cx::default();
    match a {
        Ability::Keywords(ks) => {
            for (i, k) in ks.iter().enumerate() {
                let protection_objects = |k: &Keyword| {
                    k.name == "protection" && matches!(k.param, Some(KeywordParam::Objects(_)))
                };
                match (i, &k.param) {
                    (0, _) => keyword(o, k),
                    // "protection from spells and from permanents …": two
                    // protection abilities said as one (CR 702.16g).
                    (_, Some(KeywordParam::Objects(x)))
                        if protection_objects(k) && protection_objects(&ks[i - 1]) =>
                    {
                        let listed = out(|o| {
                            o.w(",");
                            keyword(o, k);
                        });
                        let and_from = out(|o| {
                            o.w("and from");
                            objects(o, x);
                        });
                        syn::and_from(o, listed, and_from);
                    }
                    _ => {
                        o.w(",");
                        keyword(o, k);
                    }
                }
            }
            syn::keyword_period(o);
        }
        Ability::Activated {
            cost,
            effect,
            limits: ls,
        } => {
            costs(o, cost);
            o.w(":");
            if let [Effect::ClassLevel(n)] = &effect[..] {
                o.w("level");
                syn::number(o, *n);
            } else {
                sentences(o, effect, &cx);
            }
            limits(o, ls, false);
        }
        Ability::Loyalty { change, effect } => {
            let cost = match change {
                Signed::N(0) => Tok::Num(0),
                s => Tok::Signed(*s),
            };
            let plain = out(|o| o.tok(cost.clone()));
            let bracketed = out(|o| {
                o.tok(Tok::Other('['));
                o.tok(cost.clone());
                o.tok(Tok::Other(']'));
            });
            syn::loyalty_cost(o, plain, bracketed);
            o.w(":");
            sentences(o, effect, &cx);
        }
        Ability::Triggered {
            trigger: t,
            effect,
            limits: ls,
        } => {
            trigger(o, t);
            o.w(",");
            sentences(o, effect, &cx);
            limits(o, ls, true);
        }
        Ability::Static(ss) => statics(o, ss),
        Ability::Spell(es) => sentences(o, es, &cx),
        Ability::Spree { cost, effect } => {
            o.tok(Tok::Other('+'));
            mana(o, cost);
            o.tok(Tok::Dash);
            sentences(o, effect, &cx);
        }
        Ability::LevelHeader { from, to } => {
            o.w("level");
            o.tok(Tok::Num(*from));
            match to {
                Some(t) => o.tok(Tok::Signed(Signed::N(-(*t as i32)))),
                None => o.tok(Tok::Other('+')),
            }
        }
        Ability::LevelPt(pt) => o.tok(Tok::Pt(*pt)),
        Ability::Chapter { chapters, effect } => {
            for (i, c) in chapters.iter().enumerate() {
                if i > 0 {
                    o.w(",");
                }
                o.w(["i", "ii", "iii", "iv", "v", "vi"][*c as usize - 1]);
            }
            o.tok(Tok::Dash);
            sentences_in_mode(o, effect, &cx);
        }
        Ability::Station { at_least, ability } => {
            o.tok(Tok::Num(*at_least));
            o.tok(Tok::Other('+'));
            o.tok(Tok::Other('|'));
            ability_into(o, ability);
        }
        Ability::Harnessed(a) => {
            o.tok(Tok::Other('∞'));
            o.tok(Tok::Dash);
            ability_into(o, a);
        }
        Ability::KeywordOn {
            keyword,
            ability: a,
        } => {
            o.w(match keyword {
                Prefix::MaxSpeed => "max speed",
                Prefix::PowerUp => "power-up",
                Prefix::Exhaust => "exhaust",
                Prefix::Boast => "boast",
                Prefix::Forecast => "forecast",
                Prefix::Solved => "solved",
            });
            o.tok(Tok::Dash);
            ability_into(o, a);
        }
        Ability::Labelled { label, ability } => {
            o.w(label);
            o.tok(Tok::Dash);
            ability_into(o, ability);
        }
        Ability::Anchored { what, options } => {
            o.w("as");
            objects(o, what);
            o.w("enters, choose");
            for (i, a) in options.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                o.w(&a.word);
            }
            o.tok(Tok::Period);
            for a in options {
                o.tok(Tok::Bullet);
                o.w(&a.word);
                o.tok(Tok::Dash);
                ability_into(o, &a.ability);
            }
        }
        Ability::Tiered(modes) => {
            o.w("tiered");
            for m in modes {
                o.tok(Tok::Bullet);
                o.w(&m.label);
                o.tok(Tok::Dash);
                mana(o, &m.cost);
                o.tok(Tok::Dash);
                sentences(o, &m.effect, &cx);
            }
        }
    }
}

fn limits(o: &mut Out, ls: &[Limit], triggered: bool) {
    let mut i = 0;
    while i < ls.len() {
        let l = &ls[i];
        i += 1;
        // "Activate only if … and only once each turn.": "activate only"
        // said once for the limits after it.
        if let Some(first) = activate_only(l, triggered) {
            o.w("activate only");
            o.add(first);
            while let Some(next) = ls.get(i).and_then(|l| activate_only(l, triggered)) {
                syn::only_again(o, next);
                i += 1;
            }
            o.tok(Tok::Period);
            continue;
        }
        match l {
            Limit::SorcerySpeed
            | Limit::InstantSpeed
            | Limit::Once
            | Limit::OnlyDuringYourTurn
            | Limit::OnlyDuringYourUpkeep
            | Limit::OnlyIf(_) => unreachable!("an activation limit"),
            Limit::OncePerTurn => o.w("this ability triggers only once each turn"),
            Limit::SpendOnly { uses, then } => {
                o.w("spend this mana only");
                spend_uses(o, uses);
                if let Some((what, rule)) = then {
                    o.w(", and");
                    objects(o, what);
                    restriction(o, rule, obj_num(what));
                }
            }
            Limit::CostChange {
                by,
                less,
                per,
                cond,
            } => {
                o.w("this ability costs");
                mana(o, by);
                o.w(if *less { "less" } else { "more" });
                o.w("to activate");
                if let Some(per) = per {
                    o.w("for each");
                    each_objects(o, per);
                }
                if let Some(c) = cond {
                    o.w("if");
                    condition(o, c);
                }
            }
            Limit::SpendOnlyManaOf(pal) => {
                o.w("spend only mana of");
                palette_named(o, pal);
                o.w("to activate this ability");
            }
            Limit::DoOnlyOncePerTurn => o.w("do this only once each turn"),
        }
        o.tok(Tok::Period);
    }
}

/// What a limit says after "activate only", if it is one.
fn activate_only(l: &Limit, triggered: bool) -> Option<Out> {
    Some(out(|o| match l {
        Limit::SorcerySpeed => o.w("as a sorcery"),
        Limit::InstantSpeed => o.w("as an instant"),
        Limit::OncePerTurn if !triggered => o.w("once each turn"),
        Limit::Once => o.w("once"),
        Limit::OnlyDuringYourTurn => o.w("during your turn"),
        Limit::OnlyDuringYourUpkeep => o.w("during your upkeep"),
        Limit::OnlyIf(c) => {
            o.w("if");
            condition(o, c);
        }
        _ => {}
    }))
    .filter(|x| !x.0.is_empty())
}

fn spend_uses(o: &mut Out, uses: &[SpendUse]) {
    for (i, u) in uses.iter().enumerate() {
        if i > 0 {
            o.w("or");
        }
        // "to cast creature spells or activate abilities of creatures":
        // "to" said once for both.
        let verb = match u {
            SpendUse::Cast(_) => "cast",
            SpendUse::ActivateAbilityOf(_) | SpendUse::ActivateAny => "activate",
        };
        let with_to = out(|o| o.w(&format!("to {verb}")));
        if i > 0 {
            syn::gapped(o, with_to, out(|o| o.w(verb)));
        } else {
            o.add(with_to);
        }
        match u {
            SpendUse::Cast(x) => objects(o, x),
            SpendUse::ActivateAbilityOf(x) => {
                syn::an_ability_of(o);
                objects(o, x);
            }
            SpendUse::ActivateAny => syn::an_ability(o),
        }
    }
}

// ---------------------------------------------------------------- keywords

fn keyword(o: &mut Out, k: &Keyword) {
    match &k.param {
        Some(KeywordParam::Colors(cs))
            if k.name == "protection"
                && cs[..]
                    == [
                        Color::White,
                        Color::Blue,
                        Color::Black,
                        Color::Red,
                        Color::Green,
                    ] =>
        {
            let each = out(|o| o.w("protection from each color"));
            let listed = out(|o| {
                o.w("protection from");
                for (i, c) in cs.iter().enumerate() {
                    if i > 0 {
                        o.w(if i + 1 == cs.len() {
                            "and from"
                        } else {
                            ", from"
                        });
                    }
                    o.w(color_name(*c));
                }
            });
            syn::each_color(o, each, listed);
        }
        Some(KeywordParam::Colors(cs)) if k.name == "protection" || k.name == "hexproof" => {
            o.w(&k.name);
            o.w("from");
            for (i, c) in cs.iter().enumerate() {
                if i > 0 {
                    o.w(if i + 1 == cs.len() {
                        "and from"
                    } else {
                        ", from"
                    });
                }
                o.w(color_name(*c));
            }
        }
        Some(KeywordParam::PartnerWith(name)) => {
            o.w("partner with");
            o.w(name);
        }
        Some(KeywordParam::ColorsNotIn(_)) => {
            o.w("protection from each color that's not in your commander's color identity")
        }
        Some(KeywordParam::Color(Palette::Chosen)) => o.w("protection from the chosen color"),
        Some(KeywordParam::Color(Palette::That)) => o.w("protection from that color"),
        Some(KeywordParam::Color(_)) => o.w("protection from the color of your choice"),
        Some(KeywordParam::Objects(x)) if k.name == "protection" => {
            o.w("protection from");
            if x.quant == Quant::All && x.filter == Filter::default() {
                o.w("everything");
            } else {
                objects(o, x);
            }
        }
        Some(KeywordParam::NCost { n, cost: c }) => {
            o.w(&k.name);
            amount(o, n);
            o.tok(Tok::Dash);
            costs(o, c);
        }
        Some(KeywordParam::EitherOrBoth(v)) => {
            o.w(&k.name);
            for (i, m) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and/or");
                }
                mana(o, m);
            }
        }
        Some(KeywordParam::Condition(c)) => {
            o.w(&k.name);
            o.tok(Tok::Dash);
            condition(o, c);
        }
        Some(KeywordParam::Gift(g)) => {
            o.w("gift");
            match g {
                Gift::Card => o.w("a card"),
                Gift::ExtraTurn => o.w("an extra turn"),
                Gift::Token { tapped, named } => {
                    o.w("a");
                    if *tapped {
                        o.w("tapped");
                    }
                    o.w(named);
                }
            }
        }
        Some(KeywordParam::Splice { onto, cost }) => {
            o.w(&k.name);
            o.w("onto");
            filter(o, onto, Num::Sg);
            keyword_cost(o, cost);
        }
        Some(KeywordParam::CostFor { what, cost }) => {
            o.w(&k.name);
            each_objects(o, what);
            keyword_cost(o, cost);
        }
        Some(KeywordParam::Cost(c)) => {
            o.w(&k.name);
            keyword_cost(o, c);
        }
        // "Firebending X, where X is this creature's power".
        Some(KeywordParam::N(n)) if is_measure(n) => {
            o.w(&k.name);
            o.w("x, where x is");
            measure(o, n);
        }
        Some(KeywordParam::N(n)) => {
            o.w(&k.name);
            amount(o, n);
        }
        Some(KeywordParam::Objects(x)) => {
            o.w(&k.name);
            match k.name.as_str() {
                "affinity" => o.w("for"),
                "enchant" | "equip" => {
                    each_objects(o, x);
                    return;
                }
                _ => o.w("from"),
            }
            objects(o, x);
        }

        Some(KeywordParam::Colors(_)) | None => o.w(&k.name),
    }
}

/// A keyword's cost: mana alone follows the name, anything else a dash.
fn keyword_cost(o: &mut Out, c: &[Cost]) {
    match c {
        [Cost::Mana(m)] => mana(o, m),
        _ => {
            o.tok(Tok::Dash);
            costs(o, c);
            syn::keyword_period(o);
        }
    }
}

fn keyword_list(o: &mut Out, ks: &[Keyword]) {
    for (i, k) in ks.iter().enumerate() {
        if i > 0 {
            o.w(match (i + 1 == ks.len(), ks.len()) {
                (true, 2) => "and",
                (true, _) => ", and",
                _ => ",",
            });
        }
        keyword(o, k);
    }
}

fn quoted(o: &mut Out, a: &Ability) {
    o.tok(Tok::Quote);
    let mut inner = ability(a);
    // `with "{T}: Add {C}" and loses …`: a quoted ability's last sentence
    // may end at its closing quote, as `end_sentence` reads.
    optional_final_period(&mut inner.0);
    o.add(inner);
    o.tok(Tok::Quote);
}

fn optional_final_period(ps: &mut Vec<Piece>) {
    match ps.last_mut() {
        Some(Piece::Alt(alts)) => {
            for a in alts {
                optional_final_period(a);
            }
        }
        Some(Piece::Toks(ts)) if ts.last() == Some(&Tok::Period) => {
            ts.pop();
            if ts.is_empty() {
                ps.pop();
            }
            let mut o = Out(std::mem::take(ps));
            syn::quoted_last_stop(&mut o);
            *ps = o.0;
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- costs

fn mana(o: &mut Out, m: &Mana) {
    for s in &m.0 {
        o.tok(Tok::Sym(s.clone()));
    }
}

fn costs(o: &mut Out, cs: &[Cost]) {
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            syn::cost_sep(o);
        }
        cost(o, c);
    }
}

/// A cost said in a sentence, where mana takes the verb a cost before a
/// colon leaves out: "you may pay {B} rather than pay …", "pay 5 life or
/// pay {2}".
fn costs_said(o: &mut Out, cs: &[Cost]) {
    for (i, c) in cs.iter().enumerate() {
        if i > 0 {
            syn::cost_sep(o);
        }
        cost_said(o, c);
    }
}

fn cost_said(o: &mut Out, c: &Cost) {
    match c {
        Cost::Mana(m) => {
            o.w("pay");
            mana(o, m);
        }
        Cost::OneOf(v) => {
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                cost_said(o, c);
            }
        }
        c => cost(o, c),
    }
}

fn cost(o: &mut Out, c: &Cost) {
    match c {
        Cost::Mana(m) => mana(o, m),
        Cost::Tap => o.tok(Tok::Sym("T".into())),
        Cost::Untap => o.tok(Tok::Sym("Q".into())),
        Cost::Sacrifice(x) => {
            o.w("sacrifice");
            objects(o, x);
        }
        Cost::Discard(x) => {
            o.w("discard");
            objects(o, x);
        }
        Cost::PayLife(n) if is_measure(n) => {
            o.w("pay life equal to");
            measure(o, n);
        }
        Cost::PayLife(n) => {
            o.w("pay");
            amount(o, n);
            o.w("life");
        }
        Cost::Exile(x) => {
            o.w("exile");
            objects(o, x);
        }
        Cost::RemoveCounters { n, kind, from } => {
            o.w("remove");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("from");
            objects(o, from);
        }
        Cost::PutCounters { n, kind, on } => {
            o.w("put");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("on");
            objects(o, on);
        }
        Cost::TapUntapped(x) => {
            o.w("tap");
            objects(o, x);
        }
        Cost::ReturnToHand(x) => {
            o.w("return");
            objects(o, x);
            o.w("to");
            zone(o, &Zone::OwnersHand);
        }
        Cost::Energy(n) => {
            let bare = out(|o| {
                for _ in 0..*n {
                    o.tok(Tok::Sym("E".into()));
                }
            });
            let said = out(|o| {
                o.w("pay");
                o.add(bare.clone());
            });
            syn::pay(o, bare, said);
        }
        Cost::Reveal(x) => {
            o.w("reveal");
            objects(o, x);
        }
        Cost::Mill(n) => {
            o.w("mill");
            amount_a(o, n);
            o.noun("card", amount_num(n));
        }
        Cost::Exert(x) => {
            o.w("exert");
            objects(o, x);
        }
        Cost::ManaCostOf(x) => {
            objects(o, x);
            o.w("'s mana cost");
        }
        Cost::OneOf(v) => {
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                cost(o, c);
            }
        }
        Cost::DiscardHand => o.w("discard your hand"),
        Cost::PayEnergy(n) => {
            o.w("pay");
            amount(o, n);
            o.tok(Tok::Sym("E".into()));
        }
        Cost::ManaTimes { n, mana: m } => {
            o.w("pay");
            amount(o, n);
            mana(o, m);
        }
        Cost::Action { name, n } => {
            o.w(name);
            amount(o, n);
        }
        Cost::ActionMana { name, mana: m } => {
            o.w(name);
            mana(o, m);
        }
        Cost::RemoveCountersAmong { n, kind, among } => {
            o.w("remove");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("from among");
            objects(o, among);
        }
        Cost::DiscardAtRandom(x) => {
            o.w("discard");
            objects(o, x);
            o.w("at random");
        }
        Cost::ActionAlone(name) => o.w(name),
        Cost::PlayerGainsLife { who, n } => {
            subj(o, who, "gain", &Cx::default());
            life(o, n);
        }
        Cost::RemoveAnyCounters { n, from } => {
            o.w("remove");
            amount_a(o, n);
            o.noun("counter", amount_num(n));
            o.w("from");
            objects(o, from);
        }
        Cost::ActionOn { name, what } => {
            o.w(name);
            objects(o, what);
        }
    }
}

/// A counter's kind, a word or a printed `+1/+1`, then the noun.
fn counter_noun(o: &mut Out, kind: &str, n: Num) {
    counter_kind(o, kind);
    o.noun("counter", n);
}

fn counter_kind(o: &mut Out, kind: &str) {
    match pt_of_debug(kind) {
        Some(pt) => o.tok(Tok::Pt(pt)),
        None => o.w(kind),
    }
}

/// `counter_kind` keeps a printed `+1/+1` as its `Debug`; read it back.
fn pt_of_debug(s: &str) -> Option<Pt> {
    let inner = s.strip_prefix("Pt { power: ")?.strip_suffix(" }")?;
    let (power, rest) = inner.split_once(", toughness: ")?;
    let (toughness, signed) = rest.split_once(", signed: ")?;
    let part = |p: &str| -> Option<PtPart> {
        Some(match p {
            "X" => PtPart::X,
            "MinusX" => PtPart::MinusX,
            "Star" => PtPart::Star,
            _ => PtPart::N(p.strip_prefix("N(")?.strip_suffix(')')?.parse().ok()?),
        })
    };
    Some(Pt {
        power: part(power)?,
        toughness: part(toughness)?,
        signed: signed == "true",
    })
}

fn pt_text(pt: &Pt) -> String {
    let part = |p: PtPart| match (p, pt.signed) {
        (PtPart::N(n), true) if n < 0 => format!("-{}", -n),
        (PtPart::N(n), true) => format!("+{n}"),
        (PtPart::N(n), false) => n.to_string(),
        (PtPart::X, true) => "+X".into(),
        (PtPart::X, false) => "X".into(),
        (PtPart::MinusX, _) => "-X".into(),
        (PtPart::Star, _) => "*".into(),
    };
    format!("{}/{}", part(pt.power), part(pt.toughness))
}

// ---------------------------------------------------------------- amounts

/// Amounts said before a noun: a number, X, "that many".
fn is_measure(a: &Amount) -> bool {
    match a {
        Amount::N(_)
        | Amount::AnyOf(_)
        | Amount::X
        | Amount::ThatMany
        | Amount::Between(..)
        | Amount::AnyAmount => false,
        Amount::UpTo(a) | Amount::Twice(a) | Amount::Thrice(a) | Amount::Additional(a) => {
            is_measure(a)
        }
        Amount::Plus(a, _) => is_measure(a),
        _ => true,
    }
}

fn amount_num(a: &Amount) -> Num {
    match a {
        Amount::N(1) => Num::Sg,
        Amount::UpTo(a) | Amount::Additional(a) => amount_num(a),
        _ => Num::Pl,
    }
}

fn amount(o: &mut Out, a: &Amount) {
    match a {
        Amount::AnyOf(v) => {
            for (i, x) in v.iter().enumerate() {
                if i > 0 && v.len() > 2 {
                    o.w(",");
                }
                if i + 1 == v.len() {
                    o.w("or");
                }
                amount(o, x);
            }
        }
        Amount::N(n) => syn::number(o, *n),
        Amount::X => o.w("X"),
        Amount::ThatMany => syn::that_many(o),
        Amount::UpTo(a) => {
            o.w("up to");
            amount(o, a);
        }
        Amount::Twice(a) => {
            o.w("twice");
            amount(o, a);
        }
        Amount::Thrice(a) => {
            o.w("three times");
            amount(o, a);
        }
        Amount::Plus(a, k) => {
            amount(o, a);
            o.w("plus");
            syn::number(o, *k);
        }
        // "one or two", "one, two, or three".
        Amount::Between(a, b) => {
            for k in *a..=*b {
                if k > *a {
                    o.w(if k == *b {
                        if b - a > 1 {
                            ", or"
                        } else {
                            "or"
                        }
                    } else {
                        ","
                    });
                }
                syn::number(o, k);
            }
        }
        Amount::Additional(a) => {
            amount(o, a);
            o.w("additional");
        }
        Amount::AnyAmount => o.w("any amount of"),
        _ => measure(o, a),
    }
}

/// An amount where "a" means one.
fn amount_a(o: &mut Out, a: &Amount) {
    match a {
        Amount::N(1) => syn::one(o),
        Amount::Additional(n) if **n == Amount::N(1) => {
            syn::one(o);
            o.w("additional");
        }
        _ => amount(o, a),
    }
}

fn stat_name(s: Stat) -> &'static str {
    match s {
        Stat::Power => "power",
        Stat::Toughness => "toughness",
        Stat::ManaValue => "mana value",
    }
}

/// What follows "equal to".
fn measure(o: &mut Out, a: &Amount) {
    match a {
        Amount::N(_)
        | Amount::AnyOf(_)
        | Amount::X
        | Amount::ThatMany
        | Amount::UpTo(_)
        | Amount::Twice(_)
        | Amount::Thrice(_)
        | Amount::Between(..)
        | Amount::Additional(_) => amount(o, a),
        Amount::Plus(m, k) => {
            let a = out(|o| measure(o, m));
            let b = out(|o| syn::number(o, *k));
            syn::sum(o, a, b);
        }
        Amount::CountOf(x) => {
            o.w("the number of");
            objects(o, x);
        }
        Amount::PowerOf(x) | Amount::ToughnessOf(x) | Amount::ManaValueOf(x) => {
            let s = match a {
                Amount::PowerOf(_) => Stat::Power,
                Amount::ToughnessOf(_) => Stat::Toughness,
                _ => Stat::ManaValue,
            };
            if is_bare_antecedent(x) {
                possessive_of(o, x);
                o.w(stat_name(s));
            } else {
                // "the mana value of the exiled card", "the exiled card's
                // mana value".
                let of_form = out(|o| {
                    o.w("the");
                    o.w(stat_name(s));
                    o.w("of");
                    objects(o, x);
                });
                let s_form = out(|o| {
                    possessive_of(o, x);
                    o.w(stat_name(s));
                });
                syn::genitive(o, of_form, s_form);
            }
        }
        Amount::LifeTotal(p) => {
            whose(o, p);
            o.w("life total");
        }
        Amount::StartingLifeTotal(p) => {
            whose(o, p);
            o.w("starting life total");
        }
        Amount::NumberOfBasicLandTypes(x) => {
            o.w("the number of basic land types among");
            objects(o, x);
        }
        Amount::ColorsOfManaSpentToCast(x) => {
            o.w("the number of colors of mana spent to cast");
            objects(o, x);
        }
        Amount::LifeLost => o.w("the life lost this way"),
        Amount::Damage => o.w("the damage dealt this way"),
        Amount::ManaSpentToCast(x) => {
            o.w("the amount of mana spent to cast");
            objects(o, x);
        }
        Amount::TimesCastFrom { what, zone: z } => {
            o.w("the number of times");
            pronoun_or(
                o,
                what,
                |o| o.w("it's"),
                |o| {
                    objects(o, what);
                    o.w("has");
                },
            );
            o.w("been cast from");
            zone(o, z);
            o.w("this game");
        }
        // The grammar reads these only in a condition, which says them its
        // own way ("an opponent lost 2 or more life this turn").
        Amount::DifferentlyNamed(x) => {
            o.w("the number of differently named");
            objects(o, x);
        }
        Amount::CardsDrawnThisTurn(p) => {
            o.w("the number of cards");
            match p {
                Player::You => o.w("you've"),
                p => {
                    player(o, p);
                    o.w("has");
                }
            }
            o.w("drawn this turn");
        }
        Amount::LifeGainedThisTurn(p) | Amount::LifeLostThisTurn(p) => {
            syn::the_life(o);
            let verb = if matches!(a, Amount::LifeGainedThisTurn(_)) {
                "gained"
            } else {
                "lost"
            };
            life_subject(o, p, verb);
            o.w("this turn");
        }
        Amount::DamageDealtThisTurn(p) => {
            o.w("the damage");
            player(o, p);
            o.w("was dealt this turn");
        }
        Amount::DamagePrevented => o.w("1 damage prevented this way"),
        Amount::TimesAttackedThisTurn(x) => {
            o.w("time");
            pronoun_or(o, x, syn::it_has, |o| {
                objects(o, x);
                o.w("has");
            });
            o.w("attacked this turn");
        }
        Amount::Greatest { stat, among } => {
            o.w("the greatest");
            o.w(stat_name(*stat));
            o.w("among");
            objects(o, among);
        }
        Amount::Devotion { who, to } => {
            whose(o, who);
            o.w("devotion to");
            palette_named(o, to);
        }
        Amount::NumberOfColors(pal) => {
            o.w("the number of colors");
            match &**pal {
                Palette::CommanderIdentity => {
                    o.w("in");
                    commander_identity(o);
                }
                Palette::ColorsOf(x) => {
                    o.w("among");
                    objects(o, x);
                }
                p => palette_named(o, p),
            }
        }
        Amount::GreatestDiscardedThisWay => {
            o.w("the greatest number of cards a player discarded this way")
        }
        Amount::TimesKicked => o.w("time it was kicked"),
        Amount::Total { stat, of } => {
            o.w("the total");
            o.w(stat_name(*stat));
            o.w("of");
            objects(o, of);
        }
        Amount::BasePowerOf(_) => {
            syn::its(o);
            o.w("base power");
        }
        Amount::NumberOfCardTypes(x) => {
            o.w("the number of card types among");
            objects(o, x);
        }
        Amount::DieResult => o.w("the result"),
        Amount::ThatDamage => o.w("that damage"),
        Amount::Half { of, round_up } => {
            o.w("half");
            measure(o, of);
            rounded(o, *round_up);
        }
        Amount::AnyAmount => o.w("any amount of"),
        Amount::EnergyPaidThisWay => {
            o.w("the amount of");
            o.tok(Tok::Sym("E".into()));
            o.w("paid this way");
        }
        Amount::ThatNumber => o.w("that number"),
        Amount::Minus(a, b) => {
            measure(o, a);
            o.w("minus");
            measure(o, b);
        }
        Amount::Every { n, of } => {
            syn::number(o, *n);
            objects(o, of);
        }
        Amount::Speed(p) => {
            whose(o, p);
            o.w("speed");
        }
    }
}

/// ", rounded up", ", rounded down".
fn rounded(o: &mut Out, up: bool) {
    o.w(if up { ", rounded up" } else { ", rounded down" });
}

/// "~'s", "target creature's", or "its" for an earlier noun.
fn possessive_of(o: &mut Out, x: &Objects) {
    pronoun_or(o, x, syn::its, |o| {
        objects(o, x);
        o.tok(Tok::Possessive);
    });
}

fn is_bare_antecedent(x: &Objects) -> bool {
    x.quant == Quant::Antecedent && x.filter == Filter::default()
}

/// The card itself with nothing more said, which the text may name or
/// refer back to.
fn is_bare_self(x: &Objects) -> bool {
    x.quant == Quant::SelfRef && x.filter == Filter::default()
}

/// Where English has a pronoun form: an earlier noun by pronoun, the card
/// itself by name or pronoun (`syn::the_card`), anything else by name.
fn pronoun_or(
    o: &mut Out,
    x: &Objects,
    pronoun: impl FnOnce(&mut Out),
    named: impl FnOnce(&mut Out),
) {
    if is_bare_antecedent(x) {
        pronoun(o);
    } else if is_bare_self(x) {
        let named = out(named);
        let pronoun = out(pronoun);
        syn::the_card(o, named, pronoun);
    } else {
        named(o);
    }
}

/// "from among the nonland permanents they control": a set its description
/// names in full, said with "the" or "all".
fn from_among(o: &mut Out, x: &Objects) {
    o.w("from among");
    if x.quant == Quant::All {
        let all = out(|o| objects(o, x));
        let the = out(|o| {
            o.w("the");
            filter(o, &x.filter, obj_num(x));
        });
        syn::all_or_the(o, all, the);
    } else {
        objects(o, x);
    }
}

/// The card itself, said by name or by a character's pronoun.
fn self_named() -> Objects {
    Objects {
        quant: Quant::SelfRef,
        filter: Filter::default(),
    }
}

// ---------------------------------------------------------------- players

fn player_num(p: &Player) -> Num {
    match p {
        Player::You => Num::Pl,
        Player::That | Player::EachOpponent => Num::Any,
        Player::EachOf(_) | Player::Those(_) | Player::AnyNumberOfTarget => Num::Pl,
        _ => Num::Sg,
    }
}

fn player(o: &mut Out, p: &Player) {
    match p {
        Player::You => o.w("you"),
        Player::Target => o.w("target player"),
        Player::TargetOpponent => o.w("target opponent"),
        Player::UpToOneTarget => o.w("up to one target player"),
        Player::AnyNumberOfTarget => o.w("any number of target players"),
        Player::Each => syn::each_player(o),
        Player::EachOpponent => syn::each_opponent(o),
        Player::EachOtherPlayer => o.w("each other player"),
        Player::AnOpponent => syn::an_opponent(o),
        Player::APlayer => o.w("a player"),
        Player::That => syn::that_player(o),
        Player::DefendingPlayer => o.w("defending player"),
        Player::ControllerOf(x) => pronoun_or(
            o,
            x,
            |o| {
                syn::its(o);
                o.w("controller");
            },
            |o| genitive(o, "controller", x),
        ),
        Player::OwnerOf(x) => pronoun_or(
            o,
            x,
            |o| {
                syn::its(o);
                o.w("owner");
            },
            |o| genitive(o, "owner", x),
        ),
        Player::Chosen => o.w("the chosen player"),
        Player::Active => o.w("the active player"),
        Player::EachOf(v) => {
            for (i, q) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                player(o, q);
            }
            o.w("each");
        }
        Player::WhoCant(w) => {
            player(o, w);
            o.w("who can't");
        }
        Player::Enchanted => o.w("enchanted player"),
        Player::Attacking { who, attacked } => {
            player(o, who);
            o.w("attacking");
            player(o, attacked);
        }
        Player::DidThisWay { who, did: e } => {
            player(o, who);
            o.w("who");
            did(o, e);
            o.w("this way");
        }
        Player::OneOf(v) => {
            for (i, q) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                player(o, q);
            }
        }
        Player::Those(_) => o.w("those players"),
    }
}

/// "the controller of that creature", "that creature's controller".
fn genitive(o: &mut Out, noun: &str, x: &Objects) {
    let of_form = out(|o| {
        o.w(&format!("the {noun} of"));
        objects(o, x);
    });
    let s_form = out(|o| {
        objects(o, x);
        o.tok(Tok::Possessive);
        o.w(noun);
    });
    syn::genitive(o, of_form, s_form);
}

/// A player as a possessive: "your", "their", "target player's".
fn whose(o: &mut Out, p: &Player) {
    match p {
        Player::You => o.w("your"),
        Player::That => syn::their(o),
        Player::OwnerOf(x) => pronoun_or(o, x, syn::its_owners, |o| {
            player(o, p);
            o.tok(Tok::Possessive);
        }),
        p => {
            player(o, p);
            o.tok(Tok::Possessive);
        }
    }
}

fn did(o: &mut Out, e: &Event) {
    match e {
        Event::Draws(_) => {
            syn::did_tense(o, "drew", out(|o| o.verb("draw", Num::Any)));
            o.w("a card");
        }
        Event::Searches { zone: z, .. } => {
            syn::did_tense(o, "searched", out(|o| o.verb("search", Num::Any)));
            zone(o, z);
        }
        e => event(o, e),
    }
}

/// A subject and its verb. Where the subject is the one the clause
/// inherits, it may go unsaid.
fn subj(o: &mut Out, who: &Player, verb: &str, cx: &Cx) {
    let inherited = cx.implied.clone().unwrap_or(Player::You);
    let n = if cx.modal { Num::Pl } else { player_num(who) };
    let said = out(|o| {
        player(o, who);
        // "Any number of target players each mill two cards".
        if *who == Player::AnyNumberOfTarget {
            syn::each_after(o);
        }
    });
    let after = out(|o| o.verb(verb, n));
    // "you may have that player lose 2 life": after "may", another
    // player's verb is said with "have".
    if cx.modal && *who != inherited {
        o.w("have");
        o.add(said);
        o.add(after);
        return;
    }
    if *who == inherited {
        let alone_n = if cx.modal || cx.implied.is_none() {
            Num::Pl
        } else {
            n
        };
        let alone = out(|o| o.verb(verb, alone_n));
        syn::subject(o, said, alone, after);
    } else if cx.continues.as_ref() == Some(who) && (n != Num::Pl || *who != Player::You) {
        // "that player shuffles", "those players …, then shuffle": the
        // verb agrees with however the subject was said.
        let alone = out(|o| o.verb(verb, n));
        syn::subject(o, said, alone, after);
    } else {
        o.add(said);
        o.add(after);
    }
}

// ---------------------------------------------------------------- objects

fn obj_num(x: &Objects) -> Num {
    match &x.quant {
        Quant::Target { n, .. }
        | Quant::OtherTarget { n, .. }
        | Quant::N(n)
        | Quant::NFromEach(n)
        | Quant::UpTo(n)
        | Quant::AnyTarget { n } => amount_num(n),
        // "one or more creatures … deals": the verb may agree with "one".
        Quant::AtLeast(Amount::N(1)) => Num::Any,
        Quant::Each
        | Quant::One
        | Quant::Another
        | Quant::AnotherTarget
        | Quant::AnyOtherTarget
        | Quant::SecondTarget
        | Quant::SelfOrAnother(_)
        | Quant::SelfOrA(_)
        | Quant::NthEachTurn { .. }
        | Quant::NthOfGame { .. }
        | Quant::Triggering
        | Quant::Next => Num::Sg,
        Quant::WithAllOthers(_) | Quant::Both => Num::Pl,
        // "your commander", "your commanders": a partner pair is two.
        Quant::Possessive(_) => Num::Any,
        Quant::All
        | Quant::Plural
        | Quant::AnyNumber
        | Quant::AnyNumberOfTarget
        | Quant::AtLeast(_)
        | Quant::AtMost(_)
        | Quant::Rest
        | Quant::Additional
        | Quant::WithOthers(_)
        | Quant::SelfAnd(_)
        | Quant::OriginalAndCopies => Num::Pl,
        // "one or more creatures … deals": the verb may agree with "one".
        Quant::Antecedent
        | Quant::BeforeEach
        | Quant::ClauseObject
        | Quant::Listed
        | Quant::Itself
        | Quant::Created
        | Quant::ThisAbility
        | Quant::SourceOf(_)
        | Quant::Linked
        | Quant::Other
        | Quant::OneOrMore => Num::Any,
        // "Krang & Shredder enter", "equipped creatures you control": a
        // name or an attached-to creature may be one or several.
        Quant::SelfRef if x.filter == Filter::default() => Num::Any,
        Quant::SelfRef => Num::Sg,
        Quant::Enchanted | Quant::Equipped => Num::Any,
    }
}

/// "two or more of those creatures": some of an earlier noun, else the
/// filter as usual.
fn of_those(o: &mut Out, f: &Filter, n: Num) {
    if let ([], None, [Post::FromAmong(among)], None) = (&f.alts[..], &f.kind, &f.posts[..], &f.or)
    {
        if among.quant == Quant::Antecedent {
            o.w("of");
            objects(o, among);
            return;
        }
    }
    filter(o, f, n);
}

/// An object phrase after "each" or a keyword that takes one, where a bare
/// noun is singular: "for each creature you control", "enchant creature",
/// "for each of those creatures".
fn each_objects(o: &mut Out, x: &Objects) {
    match x.quant {
        Quant::Plural => filter(o, &x.filter, Num::Sg),
        Quant::Antecedent => {
            o.w("of");
            objects(o, x);
        }
        _ => objects(o, x),
    }
}

fn objects(o: &mut Out, x: &Objects) {
    let f = &x.filter;
    // "an exiled card": a card in exile, said by an adjective.
    let quant_word_only = matches!(
        x.quant,
        Quant::One
            | Quant::Plural
            | Quant::Each
            | Quant::All
            | Quant::Another
            | Quant::AnyNumber
            | Quant::OneOrMore
            | Quant::Target { .. }
    );
    if quant_word_only && f.posts.first() == Some(&Post::In(Zone::Exile)) {
        let rest = Objects {
            quant: x.quant.clone(),
            filter: Filter {
                posts: f.posts[1..].to_vec(),
                ..f.clone()
            },
        };
        let bare = Objects {
            quant: x.quant.clone(),
            filter: Filter::default(),
        };
        let q = out(|o| objects_plain(o, &bare)).0.len();
        let mut adjective = out(|o| objects_plain(o, &rest));
        adjective.0.insert(q, Piece::Toks(lex::lex("exiled")));
        let phrase = out(|o| objects_plain(o, x));
        syn::exiled(o, adjective, phrase);
        return;
    }
    objects_plain(o, x);
}

fn objects_plain(o: &mut Out, x: &Objects) {
    let n = obj_num(x);
    let f = &x.filter;
    match &x.quant {
        Quant::Target { n: k, up_to } => {
            if *k != Amount::N(1) {
                syn::each_of(o);
            }
            match (k, up_to) {
                (Amount::N(1), false) => {}
                (k @ Amount::Between(..), false) => amount(o, k),
                (k, true) => {
                    o.w("up to");
                    amount(o, k);
                }
                (k, false) => amount(o, k),
            }
            o.w("target");
            filter(o, f, n);
        }
        Quant::AnyNumberOfTarget => {
            syn::each_of(o);
            o.w("any number of target");
            filter(o, f, n);
        }
        Quant::AnyTarget { n: Amount::N(1) } => o.w("any target"),
        Quant::AnyOtherTarget => o.w("any other target"),
        Quant::AnyTarget {
            n: k @ Amount::Between(..),
        } => {
            amount(o, k);
            o.w("targets");
        }
        Quant::AnyTarget { n: k } => {
            amount(o, k);
            o.w("target targets");
        }
        Quant::Each => {
            o.w("each");
            filter(o, f, n);
        }
        Quant::All => {
            syn::all_the(o);
            filter(o, f, n);
        }
        Quant::OtherTarget { n: k, up_to } => {
            if *up_to {
                o.w("up to");
            }
            amount(o, k);
            o.w("other target");
            filter(o, f, n);
        }
        // "another creature you control" said against an object the
        // sentence named: one, other than it.
        Quant::One
            if !f.alts.is_empty()
                && f.alts
                    .iter()
                    .all(|g| matches!(g.first(), Some(Adj::OtherThan(_)))) =>
        {
            o.w("another");
            let rest = Filter {
                alts: f
                    .alts
                    .iter()
                    .map(|g| g[1..].to_vec())
                    .filter(|g| !g.is_empty())
                    .collect(),
                ..f.clone()
            };
            filter(o, &rest, n);
        }
        Quant::One => {
            // "the artifact with the greatest mana value": one, picked out.
            let greatest = f.posts.iter().any(|q| matches!(q, Post::Greatest { .. }));
            if greatest {
                syn::the_greatest(o);
            } else {
                syn::a_any(o);
            }
            filter_q(o, f, n, true);
        }
        Quant::N(k) => {
            if f.kind.is_none() && f.alts.is_empty() && f.posts == [Post::FromAmongThem] {
                amount(o, k);
                o.w("of them");
                return;
            }
            // "put one onto the battlefield": a bare number names the cards
            // just found.
            if *f == Filter::default() {
                amount(o, k);
                return;
            }
            // "the top half of their library, rounded up".
            if let (Amount::Half { round_up, .. }, [Post::In(Zone::TopOfLibrary(w))]) =
                (k, &f.posts[..])
            {
                o.w("the top half of");
                whose(o, w);
                o.w("library");
                rounded(o, *round_up);
                return;
            }
            // "the top three cards of your library", "the top card".
            if f.kind == Some(Noun::Card) && f.alts.is_empty() && f.or.is_none() {
                let whose_library = match &f.posts[..] {
                    [Post::In(Zone::TopOfLibrary(w))] => Some(Some(w)),
                    [Post::In(Zone::OwnersLibraryTop)] => Some(None),
                    _ => None,
                };
                if let Some(w) = whose_library {
                    let the_top = out(|o| {
                        o.w("the top");
                        if *k != Amount::N(1) {
                            amount(o, k);
                        }
                        o.noun("card", n);
                        if let Some(w) = w {
                            o.w("of");
                            whose(o, w);
                            o.w("library");
                        }
                    });
                    match w {
                        // "exile that many cards from the top of your library".
                        Some(w) => {
                            let from_the_top = out(|o| {
                                amount_a(o, k);
                                o.noun("card", n);
                                o.w("from the top of");
                                whose(o, w);
                                o.w("library");
                            });
                            syn::top_cards(o, the_top, from_the_top);
                        }
                        None => o.add(the_top),
                    }
                    return;
                }
            }
            amount_a(o, k);
            of_those(o, f, n);
        }
        Quant::OneOrMore => {
            o.w("one or more");
            filter(o, f, n);
        }
        Quant::AnyNumber => {
            o.w("any number of");
            filter(o, f, n);
        }
        Quant::UpTo(k) => {
            o.w("up to");
            amount(o, k);
            if f.kind.is_none() && f.alts.is_empty() && f.posts == [Post::FromAmongThem] {
                o.w("of them");
                return;
            }
            filter(o, f, n);
        }
        Quant::Another => {
            o.w("another");
            filter(o, f, n);
        }
        Quant::AnotherTarget => {
            o.w("another target");
            filter(o, f, n);
        }
        Quant::SecondTarget => {
            o.w("a second target");
            filter(o, f, n);
        }
        // Said only in a comparison ("lesser mana value") or as "other",
        // which render no object; elsewhere it is "that one".
        Quant::Triggering => {
            syn::that(o);
            filter(o, f, n);
        }
        Quant::SelfRef if *f == Filter::default() => {
            let named = out(syn::self_named);
            let pronoun = out(syn::it);
            syn::the_card(o, named, pronoun);
        }
        Quant::SelfRef => {
            let named = out(|o| {
                o.w("this");
                filter(o, f, n);
            });
            let back = out(|o| {
                syn::that(o);
                filter(o, f, n);
            });
            syn::the_card(o, named, back);
        }
        Quant::Both => {
            o.w("both");
            filter(o, f, Num::Pl);
        }
        Quant::NFromEach(k) => objects_plain(
            o,
            &Objects {
                quant: Quant::N(k.clone()),
                filter: f.clone(),
            },
        ),
        // Said as any reference back is.
        Quant::BeforeEach
        | Quant::ClauseObject
        | Quant::Listed
        | Quant::Itself
        | Quant::Created
        | Quant::ThisAbility
        | Quant::SourceOf(_)
        | Quant::Linked
        | Quant::OriginalAndCopies => objects_plain(
            o,
            &Objects {
                quant: Quant::Antecedent,
                filter: f.clone(),
            },
        ),
        Quant::Antecedent if *f == Filter::default() => {
            syn::each_of(o);
            syn::it(o);
        }
        Quant::Antecedent
            if *f
                == (Filter {
                    kind: Some(Noun::Counter(None)),
                    ..Filter::default()
                }) =>
        {
            syn::those_counters(o)
        }
        Quant::Antecedent => match f.posts.first() {
            // "the exiled card", "the nonland cards exiled this way".
            Some(Post::Marked(m)) => {
                let before = out(|o| {
                    syn::that(o);
                    o.w(marked_word(*m));
                    let rest = Filter {
                        posts: f.posts[1..].to_vec(),
                        ..f.clone()
                    };
                    filter(o, &rest, n);
                });
                let after = out(|o| {
                    syn::that(o);
                    filter(o, f, n);
                });
                syn::position(o, before, after);
            }
            // Dance of Many's "the token": the one its linked ability
            // created.
            Some(Post::CreatedWith(x)) if x.quant == Quant::SelfRef => {
                let rest = Filter {
                    posts: f.posts[1..].to_vec(),
                    ..f.clone()
                };
                syn::that(o);
                filter(o, &rest, n);
            }
            // Isochron Scepter's "the exiled card": the card its linked
            // ability exiled with it.
            Some(Post::ExiledWith(x)) if x.quant == Quant::SelfRef => {
                let rest = Filter {
                    posts: f.posts[1..].to_vec(),
                    ..f.clone()
                };
                let adjective = out(|o| {
                    syn::that(o);
                    o.w("exiled");
                    filter(o, &rest, n);
                });
                let phrase = out(|o| {
                    syn::that(o);
                    filter(o, f, n);
                });
                syn::exiled(o, adjective, phrase);
            }
            _ => {
                syn::each_of(o);
                syn::that(o);
                filter(o, f, n);
            }
        },
        Quant::Enchanted => {
            o.w("enchanted");
            filter(o, f, n);
        }
        Quant::Equipped => {
            o.w("equipped");
            filter(o, f, n);
        }
        Quant::Plural => filter(o, f, n),
        Quant::Other => {
            o.w("other");
            filter(o, f, n);
        }
        Quant::AtLeast(k) => {
            let or_more = out(|o| {
                amount(o, k);
                o.w("or more");
            });
            let at_least = out(|o| {
                o.w("at least");
                amount(o, k);
            });
            syn::at_least(o, or_more, at_least);
            of_those(o, f, n);
        }
        Quant::AtMost(k) => {
            amount(o, k);
            o.w("or fewer");
            of_those(o, f, n);
        }
        // "the rest of the exiled cards".
        Quant::Rest
            if matches!(
                (&f.alts[..], &f.kind, &f.posts[..]),
                ([], None, [Post::FromAmong(_)])
            ) =>
        {
            if let [Post::FromAmong(x)] = &f.posts[..] {
                o.w("the rest of");
                objects(o, x);
            }
        }
        Quant::Rest => {
            syn::the_rest(o);
            filter(o, f, n);
        }
        Quant::SelfOrAnother(me) => {
            objects(o, me);
            o.w("or another");
            filter(o, f, Num::Sg);
        }
        Quant::WithOthers(one) => {
            objects(o, one);
            o.w("and other");
            filter(o, f, Num::Pl);
        }
        Quant::SelfOrA(me) => {
            objects(o, me);
            o.w("or a");
            filter(o, f, Num::Sg);
        }
        Quant::SelfAnd(me) => {
            objects(o, me);
            o.w("and");
            filter(o, f, Num::Pl);
        }
        Quant::Possessive(p) => {
            whose(o, p);
            filter(o, f, n);
        }
        Quant::NthEachTurn { n: k, of } => {
            let nth = ["first", "second", "third"][*k as usize - 1];
            let yours = out(|o| {
                whose(o, of);
                o.w(nth);
                filter(o, f, Num::Sg);
                o.w("each turn");
            });
            // "the first creature spell you cast each turn".
            let cast = out(|o| {
                o.w("the");
                o.w(nth);
                let mut g = f.clone();
                g.posts.insert(0, Post::Cast(of.clone()));
                filter(o, &g, Num::Sg);
                o.w("each turn");
            });
            if f.kind == Some(Noun::Spell) {
                syn::nth_spell(o, yours, cast);
            } else {
                o.add(yours);
            }
        }

        Quant::NthOfGame { n: k, of } => {
            whose(o, of);
            o.w(ordinal_word(*k));
            filter(o, f, Num::Sg);
            o.w("of the game");
        }
        Quant::WithAllOthers(one) => {
            objects(o, one);
            o.w("and all other");
            filter(o, f, Num::Pl);
        }
        Quant::Next => {
            o.w("the next");
            filter(o, f, n);
        }
        Quant::Additional => {
            o.w("additional");
            filter(o, f, n);
        }
    }
}

fn marked_word(m: Marked) -> &'static str {
    match m {
        Marked::Exiled => "exiled",
        Marked::Chosen => "chosen",
        Marked::Revealed => "revealed",
        Marked::Sacrificed => "sacrificed",
        Marked::Copied => "copied",
        Marked::Returned => "returned",
        Marked::Destroyed => "destroyed",
        Marked::Found => "found",
        Marked::Tapped => "tapped",
        Marked::Milled => "milled",
        Marked::Discarded => "discarded",
    }
}

fn is_typed(a: &Adj) -> bool {
    matches!(a, Adj::Type(_) | Adj::Subtype(_))
}

/// The groups as printed: undoes the sharing `filter` did, where "red
/// instant or sorcery" gave "red" to both and "white or black creature"
/// gave "creature" to both.
fn printed_groups(alts: &[Vec<Adj>]) -> Vec<Vec<Adj>> {
    let mut g: Vec<Vec<Adj>> = alts.to_vec();
    // "other activated and triggered abilities", as `filter` reads it.
    let ability = |x: &[Adj]| matches!(x.last(), Some(Adj::Ability(_)));
    if g.len() > 1 && g.iter().all(|x| ability(x)) {
        let lead = &g[0][..g[0].len() - 1];
        if g[1..]
            .iter()
            .all(|x| x.len() == lead.len() + 1 && x.starts_with(lead))
        {
            let lead = lead.len();
            for x in g.iter_mut().skip(1) {
                x.drain(..lead);
            }
        }
    }
    if g.len() > 1 && g[0].iter().any(is_typed) {
        let lead: Vec<Adj> = g[0].iter().take_while(|a| !is_typed(a)).cloned().collect();
        if !lead.is_empty()
            && g[1..].iter().all(|x| {
                x.starts_with(&lead) && x[lead.len()..].iter().all(is_typed) && x.len() > lead.len()
            })
        {
            for x in g.iter_mut().skip(1) {
                x.drain(..lead.len());
            }
        }
    }
    if let Some(last) = g.last().cloned() {
        let shared: Vec<Adj> = last.iter().filter(|a| is_typed(a)).cloned().collect();
        let n = g.len();
        if !shared.is_empty() {
            for x in g.iter_mut().take(n - 1) {
                if x.len() > shared.len()
                    && x.ends_with(&shared)
                    && !x[..x.len() - shared.len()].iter().any(is_typed)
                {
                    x.truncate(x.len() - shared.len());
                }
            }
        }
        // "Beast, Camel, Horse, Elephant, and Wolf creature": the card types
        // after the last group's subtypes were given to each group of
        // subtypes alone.
        let lead = last
            .iter()
            .take_while(|a| matches!(a, Adj::Subtype(_)))
            .count();
        let types = &last[lead..];
        // The first group keeps the words said before the list ("tapped
        // Merfolk or Kithkin creature"), given back to it above.
        let subtypes_then = |x: &[Adj]| {
            let untyped = x.iter().take_while(|a| !is_typed(a)).count();
            x.len() > types.len() + untyped
                && x.ends_with(types)
                && x[untyped..x.len() - types.len()]
                    .iter()
                    .all(|a| matches!(a, Adj::Subtype(_)))
        };
        if lead > 0
            && !types.is_empty()
            && types.iter().all(|a| matches!(a, Adj::Type(_)))
            && g[..n - 1].iter().all(|x| subtypes_then(x))
        {
            for x in g.iter_mut().take(n - 1) {
                x.truncate(x.len() - types.len());
            }
        }
    }
    g
}

fn adj(o: &mut Out, a: &Adj, noun: Option<Num>) {
    let word = match a {
        Adj::Color(c) => color_name(*c).to_string(),
        Adj::Non(s) if crate::vocab::vocab().subtypes.contains(s) => format!("non-{s}"),
        Adj::Non(s) => format!("non{s}"),
        Adj::NonColor(c) => format!("non{}", color_name(*c)),
        Adj::Type(s) | Adj::Supertype(s) | Adj::Subtype(s) => s.clone(),
        Adj::State(s) => state_word(*s).to_string(),
        Adj::Ability(AbilityKind::Activated) => "activated".into(),
        Adj::Ability(AbilityKind::Triggered) => "triggered".into(),
        Adj::Ability(AbilityKind::Loyalty) => "loyalty".into(),
        Adj::Ability(AbilityKind::Exhaust) => "exhaust".into(),
        Adj::Ability(AbilityKind::Cycling) => "cycling".into(),
        Adj::Ability(AbilityKind::Mana) => "mana".into(),
        Adj::OtherThan(_) => "other".into(),
    };
    match noun {
        Some(n) => o.noun(&word, n),
        None => o.w(&word),
    }
}

fn state_word(s: State) -> &'static str {
    match s {
        State::Tapped => "tapped",
        State::Untapped => "untapped",
        State::Attacking => "attacking",
        State::Blocking => "blocking",
        State::AttackingOrBlocking => "attacking or blocking",
        State::Blocked => "blocked",
        State::Unblocked => "unblocked",
        State::Token => "token",
        State::Nontoken => "nontoken",
        State::FaceDown => "face-down",
        State::Other => "other",
        State::Modified => "modified",
        State::Historic => "historic",
        State::Commander => "commander",
        State::FaceUp => "face-up",
        State::Prepared => "prepared",
        State::Saddled => "saddled",
        State::Enchanted => "enchanted",
        State::Equipped => "equipped",
    }
}

fn color_name(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Blue => "blue",
        Color::Black => "black",
        Color::Red => "red",
        Color::Green => "green",
        Color::Colorless => "colorless",
        Color::Multicolored => "multicolored",
        Color::Monocolored => "monocolored",
    }
}

fn kind_noun(o: &mut Out, k: &Noun, n: Num) {
    match k {
        Noun::SpellOrAbility => {
            o.noun("spell", n);
            syn::list_conj(o, false);
            o.noun("ability", n);
        }
        Noun::PermanentCard => o.noun("permanent card", n),
        Noun::Permanent => o.noun("permanent", n),
        Noun::Spell => o.noun("spell", n),
        Noun::Card => o.noun("card", n),
        Noun::Token => o.noun("token", n),
        Noun::Player => o.noun("player", n),
        Noun::Opponent => o.noun("opponent", n),
        Noun::Ability => o.noun("ability", n),
        Noun::Source => o.noun("source", n),
        Noun::Commander => o.noun("commander", n),
        Noun::Copy => o.noun("copy", n),
        Noun::Counter(None) => o.noun("counter", n),
        Noun::Counter(Some(kind)) => counter_noun(o, kind, n),
        Noun::Pile => o.noun("pile", n),
    }
}

/// A list's separators: "a or b", "a, b, or c".
fn list_sep(o: &mut Out, i: usize, len: usize, conj: &str) {
    if i == 0 {
        return;
    }
    if i + 1 < len {
        o.w(",");
    } else if len > 2 {
        o.w(&format!(", {conj}"));
    } else {
        o.w(conj);
    }
}

/// The separators of a filter's list of kinds, which is a union whatever
/// joins it.
fn kinds_sep(o: &mut Out, i: usize, len: usize) {
    if i == 0 {
        return;
    }
    if i + 1 < len {
        o.w(",");
    } else {
        syn::list_conj(o, len > 2);
    }
}

fn filter(o: &mut Out, f: &Filter, n: Num) {
    filter_q(o, f, n, false);
}

/// A filter, where `another` says its quantifier was "a", so a later
/// alternative with "other" reads "or another": "a Shaman or another
/// Wizard you control".
fn filter_q(o: &mut Out, f: &Filter, n: Num, another: bool) {
    // "an artifact, enchantment, or token you control": the kind is the
    // list's last alternative, and the posts hold of all of it.
    let or_kind = f.kind.is_none()
        && f.or.as_ref().is_some_and(|x| {
            x.alts.is_empty() && x.kind.is_some() && x.or.is_none() && x.posts == f.posts
        });
    let printed = printed_groups(&f.alts);
    // "artifact, enchantment, or creature with flying": the last kind holds
    // its keyword alone (`keyword_on_last_kind`), and the list is still one.
    let kinds_after = f.kind.is_none()
        && !f.alts.is_empty()
        && f.or
            .as_ref()
            .is_some_and(|x| x.kind.is_none() && x.alts.len() == 1 && x.or.is_none());
    let extra = usize::from(or_kind) + usize::from(kinds_after);
    let render = |o: &mut Out, groups: &[Vec<Adj>]| {
        groups_words(o, groups, f.kind.is_none(), n, extra, another);
        if or_kind {
            kinds_sep(o, groups.len(), groups.len() + extra);
            kind_noun(o, f.or.as_ref().unwrap().kind.as_ref().unwrap(), n);
        }
    };
    if printed == f.alts {
        render(o, &printed);
    } else {
        let once = out(|o| render(o, &printed));
        let again = out(|o| render(o, &f.alts));
        syn::distributed(o, once, again);
    }
    if let Some(k) = &f.kind {
        kind_noun(o, k, n);
    }
    let mut chain = Vec::new();
    if let (Some(_), false) = (&f.or, or_kind) {
        let mut next = f.or.as_deref();
        while let Some(x) = next {
            chain.push(x);
            next = x.or.as_deref();
        }
    }
    // "a creature card or a planeswalker card from your graveyard", "target
    // spell or nonland permanent an opponent controls": what holds of each
    // kind, said once after the last (as `filter` shares it).
    let shared: &[Post] = match chain.last() {
        Some(l) if !l.posts.is_empty() && f.posts.ends_with(&l.posts) => &l.posts,
        Some(l) => match l.posts.last() {
            Some(q) if f.posts.last() == Some(q) => std::slice::from_ref(q),
            _ => &[],
        },
        None => &[],
    };
    let without_zone = |ps: &[Post]| -> Vec<Post> {
        match ps.strip_suffix(shared) {
            Some(rest) if !shared.is_empty() => rest.to_vec(),
            _ => ps.to_vec(),
        }
    };
    posts(o, &without_zone(&f.posts), n);
    // "instant spell, sorcery spell, activated ability, or triggered
    // ability": a chain of kinds, listed.
    let len = chain.len() + 1;
    for (i, x) in chain.iter().enumerate() {
        if kinds_after {
            kinds_sep(o, f.alts.len(), f.alts.len() + 1);
        } else {
            kinds_sep(o, i + 1, len);
        }
        let last = i + 1 == chain.len();
        let alone = Filter {
            or: None,
            posts: if last {
                x.posts.clone()
            } else {
                without_zone(&x.posts)
            },
            ..(*x).clone()
        };
        // "a black or red permanent, spell, or card": the colors the first
        // kind was said with belong to each kind after it.
        if !f.alts.is_empty() && alone.alts == f.alts {
            let once = Filter {
                alts: vec![],
                ..alone.clone()
            };
            let once = out(|o| filter(o, &once, n));
            let again = out(|o| filter(o, &alone, n));
            syn::distributed(o, once, again);
        } else {
            filter(o, &alone, n);
        }
    }
}

fn groups_words(
    o: &mut Out,
    groups: &[Vec<Adj>],
    nouns: bool,
    n: Num,
    extra: usize,
    another: bool,
) {
    let len = groups.len() + extra;
    for (i, g) in groups.iter().enumerate() {
        let mut g = &g[..];
        if another && i > 0 && g.first() == Some(&Adj::State(State::Other)) {
            o.w("or another");
            g = &g[1..];
        } else {
            kinds_sep(o, i, len);
        }
        for (j, a) in g.iter().enumerate() {
            if j > 0 && is_non(&g[j - 1]) && is_non(a) {
                syn::adj_comma(o);
            }
            let is_noun = nouns && j + 1 == g.len() && is_typed(a);
            adj(o, a, is_noun.then_some(n));
        }
    }
}

fn is_non(a: &Adj) -> bool {
    matches!(
        a,
        Adj::Non(_) | Adj::NonColor(_) | Adj::State(State::Nontoken)
    )
}

fn posts(o: &mut Out, ps: &[Post], n: Num) {
    let mut i = 0;
    while i < ps.len() {
        // "that each have a different mana value 3 or less": the bound
        // follows the post that says the values differ.
        if let (Post::DifferentValues(s), Some(Post::With(c))) = (&ps[i], ps.get(i + 1)) {
            if c.stat == *s {
                o.w("that each have a different");
                o.w(stat_name(*s));
                comparison(o, c.op, &c.value);
                i += 2;
                continue;
            }
        }
        post(o, &ps[i], n);
        i += 1;
    }
}

fn that_is(o: &mut Out, n: Num) {
    match n {
        Num::Sg => o.w("that's"),
        Num::Pl => o.w("that are"),
        Num::Any => syn::that_is(o),
    }
}

fn post(o: &mut Out, q: &Post, n: Num) {
    match q {
        Post::FromSource(g) => {
            o.w("from a");
            for a in g {
                adj(o, a, None);
            }
            o.w("source");
        }
        Post::Controller(Player::You) => o.w("you control"),
        Post::Controller(p) => {
            player(o, p);
            o.verb("control", player_num(p));
        }
        Post::NotControlledBy(Player::You) => o.w("you don't control"),
        Post::NotControlledBy(p) => {
            player(o, p);
            o.verb("don't control", player_num(p));
        }
        Post::Owner(Player::You) => o.w("you own"),
        Post::NotOwnedBy(p) => {
            player(o, p);
            o.verb("don't own", player_num(p));
        }
        Post::NamedCard(name) => {
            o.w("named");
            o.w(name);
        }
        Post::Owner(p) => {
            player(o, p);
            o.verb("own", player_num(p));
        }
        Post::In(Zone::Battlefield) => {
            syn::on_from(o);
            o.w("the battlefield");
        }
        Post::In(z) => {
            syn::in_from(o);
            zone(o, z);
        }
        Post::CastFrom(z) => {
            o.w("from");
            zone(o, z);
        }
        Post::With(c) => {
            syn::each_with(o);
            compare_words(o, c);
        }
        Post::Total(c) => {
            o.w("with total");
            compare_words(o, c);
        }
        Post::WithKeyword(k) => {
            o.w("with");
            o.w(k);
        }
        Post::WithoutKeyword(k) => {
            o.w("without");
            o.w(k);
        }
        Post::WithCounter(kind) => {
            syn::with_counter(o);
            if let Some(k) = kind {
                counter_kind(o, k);
            }
            o.noun("counter", Num::Any);
            o.w("on");
            syn::it(o);
        }
        Post::Named(x) => {
            o.w("named");
            objects(o, x);
        }
        Post::Targets(x) => {
            o.w("that");
            o.verb("target", n);
            objects(o, x);
        }
        // "that's a land card", "that are black or red": the verb agrees
        // with the objects.
        Post::ThatIs(f) if n == Num::Pl => {
            o.w("that are");
            filter(o, f, Num::Pl);
        }
        Post::ThatIs(f) => {
            o.w("that's");
            if has_noun(f) {
                o.w("a");
            }
            filter(o, f, Num::Sg);
        }
        Post::Attacking => {
            that_is(o, n);
            o.w("attacking");
        }
        Post::Blocking => {
            that_is(o, n);
            o.w("blocking");
        }
        Post::AttachedToIt => o.w("attached to it"),
        Post::OfChosenParity(s) => {
            o.w("with");
            o.w(stat_name(*s));
            syn::chosen_quality(o);
        }
        Post::AttachedTo(x) => {
            that_is(o, n);
            o.w("attached to");
            objects(o, x);
        }
        Post::OfChosenType => o.w("of the chosen type"),
        Post::OfChosenColor => o.w("of the chosen color"),
        Post::OfThatType => o.w("of that type"),
        Post::OfThatColor => o.w("of that color"),
        Post::Not(q) if matches!(&**q, Post::ThatIs(_)) => {
            o.w(if n == Num::Sg {
                "that isn't"
            } else {
                "that aren't"
            });
            if let Post::ThatIs(f) = &**q {
                // "that isn't a Dragon", "that aren't enchanted".
                let state = f.alts.iter().flatten().all(|a| matches!(a, Adj::State(_)));
                if !state {
                    o.w("a");
                }
                filter(o, f, if state { n } else { Num::Sg });
            }
        }
        Post::Not(q) => {
            o.w(if n == Num::Sg {
                "that isn't"
            } else {
                "that aren't"
            });
            post(o, q, n);
        }
        Post::EnteredThisTurn => o.w("that entered this turn"),
        Post::EnteredUnderThisTurn(p) => {
            o.w("that entered");
            syn::the_battlefield(o);
            o.w("under");
            whose(o, p);
            o.w("control this turn");
        }
        Post::FromAmong(x) => from_among(o, x),
        Post::Greatest { stat, or_tied } => {
            o.w("with the greatest");
            o.w(stat_name(*stat));
            if *or_tied {
                o.w("or tied for the greatest");
                o.w(stat_name(*stat));
            }
        }
        Post::DidThisWay(e) => match &**e {
            Event::CountersPut { .. } => {
                o.w("that had a counter put on");
                syn::it(o);
                o.w("this way");
            }
            e => {
                o.w("who");
                did(o, e);
                o.w("this way");
            }
        },
        Post::SameName => syn::same_name(o),
        Post::DifferentValues(s) => syn::different_values(o, *s),
        Post::OneOrMoreColors => {
            that_is(o, n);
            o.w("one or more colors");
        }
        Post::AttackingAnyOf(v) => {
            that_is(o, n);
            o.w("attacking");
            for (i, r) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and/or");
                }
                recipient(o, r);
            }
        }
        Post::WithChosenName => o.w("with the chosen name"),
        Post::OfEachPermanentType => o.w("of each permanent type"),
        Post::OtherThanSelf(f) => {
            o.w("other than");
            objects(
                o,
                &Objects {
                    quant: Quant::SelfRef,
                    filter: f.clone(),
                },
            );
        }
        Post::Cast(Player::You) => o.w("you cast"),
        Post::Cast(p) => {
            player(o, p);
            o.verb("cast", player_num(p));
        }
        Post::FromAmongThem => syn::from_among_them(o),
        Post::ExiledWith(x) if x.quant == Quant::SelfRef => {
            o.w("exiled with");
            let named = out(|o| objects(o, x));
            syn::exiled_with_it(o, named);
        }
        Post::ExiledWith(x) => {
            o.w("exiled with");
            objects(o, x);
        }
        Post::SharesType { card_type, with } => {
            o.w("that");
            o.verb("share", n);
            syn::shares_a(o);
            o.w(card_type);
            o.w("type");
            if let Some(x) = with {
                o.w("with");
                objects(o, x);
            }
        }
        // Only said as "the token" (`objects`), where it is the first post.
        Post::CreatedWith(x) => {
            o.w("created with");
            objects(o, x);
        }
        Post::WithBasicLandType => o.w("with a basic land type"),
        Post::WithSingleTarget => o.w("with a single target"),
        Post::ChosenBy(Player::You) => o.w("of your choice"),
        Post::ChosenBy(Player::That) => o.w("of their choice"),
        Post::ChosenBy(p) => {
            o.w("of");
            whose(o, p);
            o.w("choice");
        }
        Post::PowerOrToughness { op, value } => {
            o.w("with power or toughness");
            comparison(o, *op, value);
        }
        Post::DiedThisTurn => o.w("that died this turn"),
        Post::From(x) => {
            o.w("from");
            objects(o, x);
        }
        Post::FromHand(_) => o.w("from it"),
        Post::GreatestAmong { stat, among } => {
            o.w("with the greatest");
            o.w(stat_name(*stat));
            o.w("among");
            objects(o, among);
        }
        Post::DealtDamageThisWay => o.w("dealt damage this way"),
        Post::WasCastThisTurn => o.w("that were cast this turn"),
        Post::LostLifeThisTurn => o.w("who lost life this turn"),
        Post::CouldProduce(m) => {
            o.w("that could produce");
            mana(o, m);
        }
        Post::Either(v) => {
            o.w("that is");
            for (i, q) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                match q {
                    Post::ThatIs(f) => filter(o, f, Num::Sg),
                    Post::CouldProduce(m) => {
                        o.w("could produce");
                        mana(o, m);
                    }
                    q => post(o, q, n),
                }
            }
        }
        Post::CardTypesAmongAtLeast(k) => {
            o.w("with");
            syn::number(o, *k);
            o.w("or more card types among them");
        }
        Post::SameNameAs(x) if is_bare_antecedent(x) => {
            let said = out(|o| {
                o.w("with the same name as");
                objects(o, x);
            });
            syn::same_name_as_it(o, said);
        }
        Post::SameNameAs(x) => {
            o.w("with the same name as");
            objects(o, x);
        }
        Post::MarkedFrom { how, from } => {
            o.w(marked_word(*how));
            o.w("from");
            zone(o, from);
            o.w("this way");
        }
        Post::WithThatName => o.w("with that name"),
        Post::Cleave(v) => {
            o.tok(Tok::Other('['));
            posts(o, v, n);
            o.tok(Tok::Other(']'));
        }
        Post::OtherThan(f) => {
            o.w("other than a");
            filter(o, f, Num::Sg);
        }
        Post::DoneThisTurn { who, verbs } => {
            match who {
                Player::You => o.w("you've"),
                p => {
                    player(o, p);
                    o.w("has");
                }
            }
            for (i, v) in verbs.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                o.w(match v {
                    PastVerb::Cycled => "cycled",
                    PastVerb::Discarded => "discarded",
                });
            }
            o.w("this turn");
        }
        Post::CastThisTurn(Player::That) => {
            syn::they_have(o);
            o.w("cast this turn");
        }
        Post::CastThisTurn(_) => o.w("you've cast this turn"),
        Post::WillCastThisTurn(_) => o.w("you cast this turn"),
        Post::WithAbilityOf(k) => {
            let kind = out(|o| adj(o, &Adj::Ability(*k), None));
            syn::with_ability(o, kind, n);
        }
        Post::XInManaCost | Post::NoXInManaCost => {
            o.w(if *q == Post::XInManaCost {
                "with"
            } else {
                "without"
            });
            o.w("{X} in");
            syn::its(o);
            o.w("mana");
            o.noun("cost", n);
        }
        Post::ManaCost(v) => {
            o.w("with mana cost");
            for (i, m) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                mana(o, m);
            }
        }
        Post::AttackingPlayer(p) => {
            that_is(o, n);
            o.w("attacking");
            player(o, p);
        }
        Post::On(x) => {
            o.w("on");
            objects(o, x);
        }
        Post::Marked(m) => {
            o.w(marked_word(*m));
            o.w("this way");
        }
        Post::PutOntoBattlefieldWith(x) => {
            o.w("put onto the battlefield with");
            objects(o, x);
        }
        Post::MarkedBy {
            how: Marked::Found,
            by,
        } => {
            player(o, by);
            o.verb("find", player_num(by));
        }
        Post::MarkedBy { how, by } => {
            player(o, by);
            o.w(marked_word(*how));
            o.w("this way");
        }
        Post::DrawnThisTurn => o.w("drawn this turn"),
        Post::WhoDoesnt => {
            o.w("who");
            o.verb("don't", n);
        }
        Post::WhoCant => o.w("who can't"),
        Post::NotIn(z) => {
            let prep = if *z == Zone::Battlefield { "on" } else { "in" };
            let that_isnt = out(|o| {
                o.w(if n == Num::Sg {
                    "that isn't"
                } else {
                    "that aren't"
                });
                o.w(prep);
                zone(o, z);
            });
            let not = out(|o| {
                o.w("not");
                o.w(prep);
                zone(o, z);
            });
            syn::not_in(o, that_isnt, not);
        }
        Post::DifferentNames => o.w("with different names"),
    }
}

fn compare_words(o: &mut Out, c: &Compare) {
    // "with lesser mana value": than the object the ability is about.
    let of = match (&c.value, c.stat) {
        (Amount::PowerOf(x), Stat::Power)
        | (Amount::ToughnessOf(x), Stat::Toughness)
        | (Amount::ManaValueOf(x), Stat::ManaValue) => Some(x),
        _ => None,
    };
    let relative = |o: &mut Out| {
        o.w(match c.op {
            Op::Le => "equal or lesser",
            Op::Lt => "lesser",
            Op::Ge => "equal or greater",
            Op::Gt => "greater",
            Op::Eq => "equal",
        });
        o.w(stat_name(c.stat));
    };
    let stated = |o: &mut Out| {
        o.w(stat_name(c.stat));
        comparison(o, c.op, &c.value);
    };
    match of {
        Some(x) => pronoun_or(o, x, relative, stated),
        None => stated(o),
    }
}

fn comparison(o: &mut Out, op: Op, value: &Amount) {
    if !is_measure(value) && !matches!(op, Op::Lt | Op::Gt) {
        amount(o, value);
        match op {
            Op::Le => o.w("or less"),
            Op::Ge => syn::or_more(o),
            _ => {}
        }
        return;
    }
    o.w(match op {
        Op::Le => "less than or equal to",
        Op::Ge => "greater than or equal to",
        Op::Lt => "less than",
        Op::Gt => "greater than",
        Op::Eq => "equal to",
    });
    measure(o, value);
}

// ---------------------------------------------------------------- zones

fn zone(o: &mut Out, z: &Zone) {
    match z {
        Zone::Hand(w) => {
            whose(o, w);
            o.noun("hand", possessor_num(w));
        }
        Zone::Graveyard(Player::Each) => syn::all_graveyards(o),
        Zone::Graveyard(w) => {
            whose(o, w);
            o.noun("graveyard", possessor_num(w));
        }
        Zone::AGraveyard => syn::a_graveyard(o),
        Zone::SingleGraveyard => o.w("a single graveyard"),
        Zone::ALibrary => syn::a_library(o),
        Zone::Library(w) => {
            whose(o, w);
            o.noun("library", possessor_num(w));
        }
        Zone::TopOfLibrary(w) => {
            o.w("the top of");
            whose(o, w);
            o.w("library");
        }
        Zone::BottomOfLibrary(w) => {
            o.w("the bottom of");
            whose(o, w);
            o.w("library");
        }
        Zone::OwnersHand | Zone::OwnersGraveyard => {
            syn::its(o);
            o.noun("owner", Num::Any);
            o.tok(Tok::Possessive);
            o.noun(
                if *z == Zone::OwnersHand {
                    "hand"
                } else {
                    "graveyard"
                },
                Num::Any,
            );
        }
        Zone::OwnersLibraryTop => o.w("the top of its owner's library"),
        Zone::OwnersLibraryBottom => o.w("the bottom of its owner's library"),
        Zone::OwnersLibraryNthFromTop(n) => {
            o.w("its owner's library");
            o.w(ordinal_word(*n));
            o.w("from the top");
        }
        Zone::Battlefield => o.w("the battlefield"),
        Zone::Exile => o.w("exile"),
        Zone::Stack => o.w("the stack"),
        Zone::CommandZone => o.w("the command zone"),
        Zone::Anywhere => o.w("anywhere"),
        Zone::AnywhereOtherThan(z) => {
            o.w("anywhere other than");
            zone(o, z);
        }
        Zone::OutsideTheGame => o.w("outside the game"),
        Zone::AnyOf(v) => {
            for (i, z) in v.iter().enumerate() {
                if i > 0 {
                    syn::list_conj(o, false);
                }
                match z {
                    Zone::AGraveyard => o.w("graveyards"),
                    Zone::ALibrary => o.w("libraries"),
                    z => zone(o, z),
                }
            }
        }
        Zone::TopOrBottomOfLibrary(w) => {
            o.w("their choice of the top or bottom of");
            whose(o, w);
            o.w("library");
        }
        Zone::StartingDeck(w) => {
            whose(o, w);
            o.w("starting deck");
        }
    }
}

/// "their hands": a zone of several players is plural.
fn possessor_num(w: &Player) -> Num {
    match w {
        Player::That | Player::EachOpponent | Player::Each | Player::AnyNumberOfTarget => Num::Any,
        _ => Num::Sg,
    }
}

fn ordinal_word(n: u32) -> &'static str {
    [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth", "eleventh", "twelfth",
    ][n as usize - 1]
}

/// Where "put" sends something: "onto the battlefield", "into your hand",
/// "on top of your library".
fn put_to(o: &mut Out, z: &Zone, tapped: bool, transformed: bool, under: &Option<Player>) {
    match z {
        Zone::TopOfLibrary(w) => {
            syn::back(o);
            o.w("on top of");
            whose(o, w);
            o.w("library");
        }
        Zone::TopOrBottomOfLibrary(_) => {
            o.w("on");
            zone(o, z);
        }
        Zone::BottomOfLibrary(w) => {
            o.w("on the bottom of");
            whose(o, w);
            o.w("library");
        }
        Zone::OwnersLibraryTop | Zone::OwnersLibraryBottom => {
            o.w(if *z == Zone::OwnersLibraryTop {
                "on top"
            } else {
                "on the bottom"
            });
            let said = out(|o| {
                o.w("of");
                syn::its(o);
                o.noun("owner", Num::Any);
                o.tok(Tok::Possessive);
                o.noun("library", Num::Any);
            });
            syn::of_library(o, said);
        }
        Zone::Battlefield => {
            o.w("onto");
            battlefield(o, tapped, transformed, under);
        }
        z => {
            o.w("into");
            zone(o, z);
        }
    }
}

fn battlefield(o: &mut Out, tapped: bool, transformed: bool, under: &Option<Player>) {
    o.w("the battlefield");
    match (tapped, transformed) {
        (true, true) => o.w("tapped and transformed"),
        (true, false) => o.w("tapped"),
        (false, true) => o.w("transformed"),
        _ => {}
    }
    match under {
        Some(Player::You) => o.w("under your control"),
        Some(_) => {
            o.w("under");
            syn::its(o);
            o.noun("owner", Num::Any);
            o.tok(Tok::Possessive);
            o.w("control");
        }
        None => {}
    }
}

/// Where "return … to" sends something.
fn destination(o: &mut Out, z: &Zone, tapped: bool, transformed: bool, under: &Option<Player>) {
    match z {
        Zone::Battlefield => battlefield(o, tapped, transformed, under),
        Zone::OwnersLibraryTop => o.w("the top of its owner's library"),
        z => zone(o, z),
    }
}

// ---------------------------------------------------------------- effects

/// Clauses in order, each joined to the last, ending in a period.
/// The sentence after a destroy: "They can't be regenerated."
fn cant_be_regenerated(o: &mut Out, what: &[&Objects]) {
    let types: Vec<String> = what
        .iter()
        .flat_map(|x| {
            x.filter.alts.iter().filter_map(|g| match g.last() {
                Some(Adj::Type(t)) => Some(t.clone()),
                _ => None,
            })
        })
        .collect();
    o.tok(Tok::Period);
    syn::destroyed(o, &types);
    o.w("can't be regenerated");
}

fn sentences(o: &mut Out, es: &[Effect], cx: &Cx) {
    clauses(o, es, cx);
    // A modal effect's modes end their own sentences.
    if !matches!(es.last(), Some(Effect::Modal { .. })) {
        o.period();
    }
}

/// Clauses joined, with no period after the last.
fn clauses(o: &mut Out, es: &[Effect], cx: &Cx) {
    let mut implied = cx.continues.clone();
    let mut i = 0;
    let mut last = Out::default();
    while i < es.len() {
        let e = &es[i];
        let cx = Cx {
            continues: implied.clone(),
            ..cx.clone()
        };
        let full = match e {
            // "Destroy target nonblack creature and target land. They can't
            // be regenerated.": one sentence after the last destroy of the
            // list says it of each.
            Effect::Destroy {
                what,
                no_regen: true,
            } => {
                let run: Vec<&Objects> = es[..=i]
                    .iter()
                    .rev()
                    .map_while(|x| match x {
                        Effect::Destroy {
                            what,
                            no_regen: true,
                        } => Some(what),
                        _ => None,
                    })
                    .collect();
                let more = matches!(es.get(i + 1), Some(Effect::Destroy { no_regen: true, .. }));
                out(|o| {
                    let bare = Effect::Destroy {
                        what: what.clone(),
                        no_regen: false,
                    };
                    effect(o, &bare, &cx);
                    if !more {
                        cant_be_regenerated(o, &run);
                    }
                })
            }
            _ => out(|o| effect(o, e, &cx)),
        };
        let prev = i.checked_sub(1).map(|j| &es[j]);
        // "Tyvar gains indestructible until end of turn. Tap it.": the card,
        // named just before as the only object, again by pronoun.
        let named_or_it = match (prev.cloned(), e.clone()) {
            (Some(p), mut it)
                if p.sole_object().is_some_and(|x| x.quant == Quant::SelfRef)
                    && p.sole_object() == it.sole_object() =>
            {
                if let Some(x) = it.sole_object_mut() {
                    *x = Objects {
                        quant: Quant::Antecedent,
                        filter: Filter::default(),
                    };
                }
                let pronoun = out(|o| effect(o, &it, &cx));
                out(|o| syn::it_self(o, full.clone(), pronoun))
            }
            _ => full.clone(),
        };
        if i > 0 {
            syn::joiner(o, ends_in_quote(&last));
        }
        let start = o.0.len();
        // "You and permanents you control gain hexproof until end of turn":
        // one verb for a player and for objects, two clauses in the tree.
        if let (
            Effect::PlayerGains {
                who: Player::You,
                keywords,
                dur: d1,
            },
            Some(Effect::Modify {
                what,
                pt: None,
                gains,
                dur: d2,
            }),
        ) = (e, es.get(i + 1))
        {
            if keywords == gains && d1 == d2 {
                let next = out(|o| effect(o, &es[i + 1], &cx));
                let apart = out(|o| {
                    o.add(full.clone());
                    syn::joiner(o, false);
                    o.add(next);
                });
                let joined = out(|o| {
                    o.w("you and");
                    objects(o, what);
                    o.w("gain");
                    keyword_list(o, gains);
                    dur(o, d2);
                });
                syn::gapped(o, apart, joined);
                last = Out(o.0[start..].to_vec());
                i += 2;
                continue;
            }
        }
        // "gets +1/+0 and gains reach, hexproof, and "Whenever …" until end
        // of turn": one "gains" and one duration for the keywords and the
        // abilities, each ability its own effect in the tree.
        let head = match e {
            Effect::Modify {
                what,
                pt,
                gains,
                dur,
            } if what.quant != Quant::Next => Some((what, pt, &gains[..], dur, 1)),
            Effect::GrantAbility { what, dur, .. } => Some((what, &None, &[][..], dur, 0)),
            _ => None,
        };
        let granted: Vec<&Ability> = match head {
            Some((what, _, _, d, skip)) => es[i + skip..]
                .iter()
                .map_while(|x| match x {
                    Effect::GrantAbility {
                        what: w,
                        ability,
                        dur: d2,
                    } if w == what && d2 == d => Some(&**ability),
                    _ => None,
                })
                .collect(),
            None => vec![],
        };
        if let Some((what, pt, gains, d, skip)) = head.filter(|h| granted.len() + h.4 > 1) {
            let taken = granted.len() + skip;
            let apart = out(|o| {
                for (j, x) in es[i..i + taken].iter().enumerate() {
                    if j > 0 {
                        syn::joiner(o, true);
                    }
                    effect(o, x, &cx);
                }
            });
            let joined = out(|o| {
                objects(o, what);
                let n = obj_num(what);
                if n != Num::Sg {
                    syn::each_after(o);
                }
                if let Some(pt) = pt {
                    o.verb("get", n);
                    o.tok(Tok::Pt(*pt));
                    o.w("and");
                }
                o.verb("gain", n);
                let len = gains.len() + granted.len();
                for (j, k) in gains.iter().enumerate() {
                    list_sep(o, j, len, "and");
                    keyword(o, k);
                }
                for (j, a) in granted.iter().enumerate() {
                    let at = gains.len() + j;
                    syn::quote_list_sep(o, at, len, j > 0);
                    quoted(o, a);
                }
                dur(o, d);
            });
            syn::gapped(o, apart, joined);
            last = Out(o.0[start..].to_vec());
            i += taken;
            continue;
        }
        // "This land becomes a 2/2 creature with all creature types until end
        // of turn": the new types and all creature types, one clause.
        if let (
            Effect::Becomes {
                what,
                pt,
                types,
                gains,
                keeps: k,
                dur: d,
            },
            Some(Effect::AlsoIs {
                what: w2,
                types: TypeSet::EveryCreatureType,
                dur: d2,
            }),
        ) = (e, es.get(i + 1))
        {
            if what == w2 && d == d2 && !types.is_empty() && gains.is_empty() {
                obj_subj(o, what, "become");
                o.w("a");
                if let Some(pt) = pt {
                    o.tok(Tok::Pt(*pt));
                }
                for a in types {
                    adj(o, a, None);
                }
                o.w("with all creature types");
                keeps_then_dur(o, k, types, d);
                last = Out(o.0[start..].to_vec());
                i += 2;
                continue;
            }
        }
        match (prev, e) {
            // "~ deals 1 damage to target player and 1 damage to target
            // creature": one source, its verb said once.
            (
                Some(Effect::Damage {
                    source: Some(s1), ..
                }),
                Effect::Damage {
                    source: Some(s2), ..
                },
            ) if s1 == s2 => {
                let subject = out(|o| objects(o, s2));
                let n = subject.0.len();
                let pred = out(|o| o.0.extend(full.0.iter().skip(n).cloned()));
                let no_verb = out(|o| o.0.extend(full.0.iter().skip(n + 1).cloned()));
                let alone = out(|o| syn::gapped(o, pred.clone(), no_verb));
                syn::subject(o, subject, alone, pred);
            }
            // "gets +2/+2 and gains trample and "Whenever …"": "gains" said
            // once for the keyword and the ability.
            (
                Some(Effect::Modify { what: w1, .. } | Effect::GrantAbility { what: w1, .. }),
                Effect::GrantAbility { what: w2, .. },
            ) if w1 == w2
                && !matches!(prev, Some(Effect::Modify { gains, .. }) if gains.is_empty()) =>
            {
                let subject = out(|o| objects(o, w2));
                let n = subject.0.len();
                let pred = out(|o| o.0.extend(full.0.iter().skip(n).cloned()));
                let no_verb = out(|o| o.0.extend(full.0.iter().skip(n + 1).cloned()));
                let alone = out(|o| syn::gapped(o, pred.clone(), no_verb));
                syn::subject(o, subject, alone, pred);
            }
            // "put two into your hand and the rest on the bottom".
            (Some(Effect::Move { .. }), Effect::Move { .. })
            | (Some(Effect::Destroy { .. }), Effect::Destroy { .. })
            | (Some(Effect::Exile(_)), Effect::Exile(_))
            | (Some(Effect::Tap(_)), Effect::Tap(_))
            | (Some(Effect::Untap(_)), Effect::Untap(_))
            | (
                Some(Effect::Create {
                    who: Player::You, ..
                }),
                Effect::Create {
                    who: Player::You, ..
                },
            ) => {
                let without = match e {
                    Effect::Move { .. } => out(|o| move_put_rest(o, e)),
                    _ => out(|o| o.0.extend(full.0.iter().skip(1).cloned())),
                };
                syn::gapped(o, full, without);
            }
            // "This creature gets +1/+0 until end of turn and deals 1 damage
            // to you": one object subject for both clauses.
            (Some(p), e) if obj_subject(p).is_some() && obj_subject(p) == obj_subject(e) => {
                let subject = out(|o| objects(o, obj_subject(e).unwrap()));
                let n = subject.0.len();
                let pred = out(|o| o.0.extend(full.0.iter().skip(n).cloned()));
                syn::subject(o, subject, pred.clone(), pred);
            }
            // "and you untap all lands you control": an instruction with
            // its "you" said.
            (_, e) if says_no_player(e) => {
                let you = out(|o| o.w("you"));
                syn::subject(o, you, named_or_it.clone(), named_or_it);
            }
            _ => o.add(named_or_it),
        }
        last = Out(o.0[start..].to_vec());
        if let Some(s) = subject_of(e) {
            implied = Some(s);
        }
        i += 1;
    }
}

/// Whether rendered words end at a closing quote.
fn ends_in_quote(o: &Out) -> bool {
    fn ends(ps: &[Piece]) -> bool {
        match ps.last() {
            Some(Piece::Toks(ts)) => ts.last() == Some(&Tok::Quote),
            Some(Piece::Alt(alts)) => alts.iter().any(|a| ends(a)),
            _ => false,
        }
    }
    ends(&o.0)
}

/// Instructions whose verb takes no player, which a "you" may still lead.
fn says_no_player(e: &Effect) -> bool {
    matches!(
        e,
        Effect::Untap(_)
            | Effect::Tap(_)
            | Effect::Exile(_)
            | Effect::Destroy { .. }
            | Effect::Attach { .. }
            | Effect::Move { .. }
            | Effect::Choose(_)
    )
}

/// The object an effect's verb is said of, rendered first.
fn obj_subject(e: &Effect) -> Option<&Objects> {
    match e {
        Effect::Modify { what, .. } if what.quant != Quant::Next => Some(what),
        Effect::GrantAbility { what, .. }
        | Effect::LoseAbility { what, .. }
        | Effect::LoseKeywords { what, .. }
        | Effect::Restrict { what, .. }
        | Effect::Becomes { what, .. }
        | Effect::AlsoIs { what, .. } => Some(what),
        Effect::Damage { source, .. } => source.as_ref(),
        _ => None,
    }
}

fn subject_of(e: &Effect) -> Option<Player> {
    Some(match e {
        Effect::Draw { who, .. }
        | Effect::Discard { who, .. }
        | Effect::DiscardHand(who)
        | Effect::GainLife { who, .. }
        | Effect::LoseLife { who, .. }
        | Effect::Search { who, .. }
        | Effect::Shuffle(who)
        | Effect::ShuffleInto { who, .. }
        | Effect::ShuffleZones { who, .. }
        | Effect::Mill { who, .. }
        | Effect::Reveal { who, .. }
        | Effect::RevealTop { who, .. }
        | Effect::LookAtTop { who, .. }
        | Effect::Sacrifice { who, .. }
        | Effect::Create { who, .. }
        | Effect::Pay { who, .. }
        | Effect::May { who, .. }
        | Effect::RevealHand(who)
        | Effect::UntilFound { who, .. }
        | Effect::DigUntilTotal { who, .. }
        | Effect::Performed { by: who, .. } => who.clone(),
        // "Counter target spell unless its controller pays {2}. If they do,
        // …": the "do" is the paying, so "they" is the payer.
        Effect::Unless { who, .. }
        | Effect::Counter {
            unless: Some((who, _)),
            ..
        } => who.clone(),
        _ => return None,
    })
}

fn dur(o: &mut Out, d: &Duration) {
    match d {
        Duration::UntilEndOfTurn => o.w("until end of turn"),
        Duration::ThisTurn => o.w("this turn"),
        Duration::UntilYourNextTurn => o.w("until your next turn"),
        Duration::WhileSelfOnBattlefield => o.w("for as long as you control ~"),
        Duration::Permanent => {}
        Duration::RestOfGame => o.w("for the rest of the game"),
        Duration::ForAsLongAs(c) => {
            o.w("for as long as");
            condition(o, c);
        }
        Duration::UntilEndOfYourNextTurn => o.w("until the end of your next turn"),
        Duration::UntilEvent(e) => {
            o.w("until");
            event(o, e);
        }
    }
}

/// "draw two cards", "draw cards equal to …".
fn cards(o: &mut Out, n: &Amount) {
    match n {
        // "mills half their library, rounded down".
        Amount::Half { of, round_up } if matches!(&**of, Amount::CountOf(x) if matches!(&x.filter.posts[..], [Post::In(Zone::Library(_))])) =>
        {
            let Amount::CountOf(x) = &**of else {
                unreachable!()
            };
            let [Post::In(Zone::Library(w))] = &x.filter.posts[..] else {
                unreachable!()
            };
            let short = out(|o| {
                o.w("half");
                whose(o, w);
                o.w("library");
                rounded(o, *round_up);
            });
            let long = out(|o| {
                o.w("cards equal to");
                measure(o, n);
            });
            syn::half_library(o, short, long);
        }
        // "draw half X cards, rounded down".
        Amount::Half { of, round_up } if !is_measure(of) => {
            o.w("half");
            amount(o, of);
            o.noun("card", Num::Pl);
            rounded(o, *round_up);
        }
        n if is_measure(n) => {
            o.w("cards equal to");
            measure(o, n);
        }
        Amount::Plus(a, k) => {
            amount_a(o, a);
            o.noun("card", Num::Pl);
            o.w("plus");
            syn::number(o, *k);
        }
        Amount::UpTo(a) => {
            o.w("up to");
            amount(o, a);
            o.noun("card", amount_num(a));
        }
        n => {
            amount_a(o, n);
            o.noun("card", amount_num(n));
        }
    }
}

fn recipients(o: &mut Out, rs: &[Recipient]) {
    for (i, r) in rs.iter().enumerate() {
        list_sep(o, i, rs.len(), "and");
        recipient(o, r);
    }
}

fn recipient(o: &mut Out, r: &Recipient) {
    match r {
        Recipient::Objects(x) => objects(o, x),
        Recipient::Player(Player::That) => syn::them(o),
        Recipient::Player(p) => player(o, p),
        Recipient::Or(v) => match &v[..] {
            [Recipient::Player(p @ (Player::Target | Player::TargetOpponent)), Recipient::Objects(x)]
                if matches!(x.quant, Quant::Target { .. }) =>
            {
                player(o, p);
                o.w("or");
                filter(o, &x.filter, Num::Sg);
            }
            [Recipient::Player(Player::That), Recipient::Objects(x)]
                if x.quant == Quant::Antecedent =>
            {
                o.w("that player or");
                filter(o, &x.filter, Num::Sg);
            }
            // "a player or planeswalker": "a" said once for both.
            [Recipient::Player(Player::APlayer), Recipient::Objects(x)]
                if x.quant == Quant::One =>
            {
                o.w("a player or");
                filter(o, &x.filter, Num::Sg);
            }
            v => {
                for (i, r) in v.iter().enumerate() {
                    if i > 0 {
                        o.w("or");
                    }
                    recipient(o, r);
                }
            }
        },
        Recipient::Same | Recipient::ControllerChooses => {}
        Recipient::AttackedBy(x) => {
            o.w("the player or planeswalker");
            pronoun_or(
                o,
                x,
                |o| o.w("it's"),
                |o| {
                    objects(o, x);
                    o.w("is");
                },
            );
            o.w("attacking");
        }
    }
}

fn token(o: &mut Out, n: &Amount, t: &Token) {
    let counted = is_measure(n) || matches!(n, Amount::DieResult);
    if counted {
        o.w("a number of");
    } else {
        amount_a(o, n);
    }
    let attacking_says_tapped = t.tapped && t.attacking.is_some();
    if t.tapped && !attacking_says_tapped {
        o.w("tapped");
    }
    if let Some(pt) = &t.pt {
        o.tok(Tok::Pt(*pt));
    }
    for (i, c) in t.colors.iter().enumerate() {
        if i > 0 && i + 1 == t.colors.len() {
            o.w("and");
        }
        o.w(color_name(*c));
    }
    match (&t.named, &t.types[..]) {
        // "a Monster Role token": the Role's name says its subtype.
        (Some(name), [ty]) if ty == "role" => o.w(name),
        _ => {
            for ty in &t.types {
                o.w(ty);
            }
        }
    }
    o.noun("token", amount_num(n));
    token_has(o, t);
    if counted {
        o.w("equal to");
        measure(o, n);
    }
    token_attacking(o, n, t);
}

/// "that's tapped and attacking": said after everything else the token is.
fn token_attacking(o: &mut Out, n: &Amount, t: &Token) {
    if let Some(r) = &t.attacking {
        that_is(o, amount_num(n));
        if t.tapped {
            o.w("tapped and");
        }
        o.w("attacking");
        recipient(o, r);
    }
}

/// "with flying and "…"": what a token has, keywords and quoted abilities
/// in either order, since the tree keeps them apart.
fn token_has(o: &mut Out, t: &Token) {
    let items = t.keywords.len() + t.abilities.len();
    if items == 0 {
        return;
    }
    let list = |o: &mut Out, keywords_first: bool| {
        o.w("with");
        let mut i = 0;
        let ks = |o: &mut Out, i: &mut usize| {
            for k in &t.keywords {
                list_sep(o, *i, items, "and");
                keyword(o, k);
                *i += 1;
            }
        };
        let abilities = |o: &mut Out, i: &mut usize| {
            for a in &t.abilities {
                list_sep(o, *i, items, "and");
                quoted(o, a);
                *i += 1;
            }
        };
        if keywords_first {
            ks(o, &mut i);
            abilities(o, &mut i);
        } else {
            abilities(o, &mut i);
            ks(o, &mut i);
        }
    };
    if t.keywords.is_empty() || t.abilities.is_empty() {
        list(o, true);
    } else {
        let keywords_first = out(|o| list(o, true));
        let abilities_first = out(|o| list(o, false));
        syn::position(o, keywords_first, abilities_first);
    }
}

fn restriction(o: &mut Out, r: &Restriction, n: Num) {
    match r {
        Restriction::AttacksDespiteDefender => {
            o.w("can attack as though");
            syn::it(o);
            o.w("didn't have defender");
        }
        Restriction::CantBlock => o.w("can't block"),
        Restriction::CantAttack => o.w("can't attack"),
        Restriction::CantAttackOrBlock => o.w("can't attack or block"),
        Restriction::CantBeBlocked => o.w("can't be blocked"),
        Restriction::CantBeBlockedExceptBy(f) => {
            o.w("can't be blocked except by");
            filter(o, f, Num::Pl);
        }
        Restriction::CantBeCountered => o.w("can't be countered"),
        Restriction::CantBeSacrificed => o.w("can't be sacrificed"),
        Restriction::DoesntUntap(p) => {
            o.verb("don't", n);
            o.w("untap during");
            if *p == Player::You {
                o.w("your");
            } else {
                player(o, p);
                o.tok(Tok::Possessive);
            }
            o.w("untap step");
        }
        Restriction::AttacksEachCombat => {
            o.verb("attack", n);
            o.w("each combat if able");
        }
        Restriction::AttacksThisTurn => {
            o.verb("attack", n);
            o.w("this turn if able");
        }
        Restriction::BlocksEachCombat => {
            o.verb("block", n);
            o.w("each combat if able");
        }
        Restriction::CantBeTheTargetOf(f) => {
            o.w("can't be the target of");
            filter(o, f, Num::Pl);
        }
        Restriction::CantBeRegenerated => o.w("can't be regenerated"),
        Restriction::CanBlockOnly(f) => {
            o.w("can block only");
            filter(o, f, Num::Pl);
        }
        Restriction::CantBeBlockedBy(f) => {
            o.w("can't be blocked by");
            filter(o, f, Num::Pl);
        }
        Restriction::CantBeBlockedByMoreThanOne => {
            o.w("can't be blocked by more than one creature")
        }
        Restriction::CanBlockAdditional(Amount::X) => o.w("can block any number of creatures"),
        Restriction::CanBlockAdditional(_) => o.w("can block an additional creature each combat"),
        Restriction::Unless(r, c) => {
            restriction(o, r, n);
            o.w("unless");
            condition(o, c);
        }
        Restriction::UnlessPays {
            rule,
            who,
            cost: c,
            per,
        } => {
            restriction(o, rule, n);
            o.w("unless");
            player(o, who);
            o.verb("pay", player_num(who));
            costs(o, c);
            if let Some(per) = per {
                o.w("for each");
                each_objects(o, per);
            }
        }
        Restriction::CantAttackPlayer(p) => {
            o.w("can't attack");
            player(o, p);
        }
        Restriction::NoActivatedAbilities => o.w("its activated abilities can't be activated"),
        Restriction::MayNotUntap => o.w("may not untap"),
        Restriction::DoesntUntapNext => {
            o.verb("don't", n);
            o.w("untap during");
            syn::its(o);
            o.noun("controller", Num::Any);
            o.tok(Tok::Possessive);
            o.w("next untap");
            o.noun("step", Num::Any);
        }
        Restriction::CantBlockAny(f) => {
            o.w("can't block");
            filter(o, f, Num::Pl);
        }
        Restriction::CantAttackAnyOf(v) => {
            o.w("can't attack");
            for (i, r) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                recipient(o, r);
            }
        }
        Restriction::AttacksPlayerOtherThan(p) => {
            o.verb("attack", n);
            o.w("a player other than");
            player(o, p);
            o.w("if able");
        }
        Restriction::NoNonManaActivatedAbilities => {
            o.w("its activated abilities can't be activated unless they're mana abilities")
        }
        Restriction::CantEnterBattlefield => o.w("can't enter the battlefield"),
        Restriction::CantBeTurnedFaceUp => o.w("can't be turned face up"),
        Restriction::CantHaveCountersPut => o.w("can't have counters put on it"),
        Restriction::MustBeBlocked => o.w("must be blocked if able"),
    }
}

fn player_rule(o: &mut Out, r: &PlayerRule) {
    match r {
        // Said after "can't"; the grammar reads it only as "your life total
        // can't change", which `Effect::PlayerRestrict` renders.
        PlayerRule::LifeTotalCantChange => o.w("have their life total change"),
        PlayerRule::CantCast(x) => {
            o.w("cast");
            objects(o, x);
        }
        PlayerRule::CantActivateAbilitiesOf(x) => {
            o.w("activate abilities of");
            objects(o, x);
        }
        PlayerRule::CantDrawMoreThanEachTurn(n) => {
            o.w("draw more than");
            syn::number(o, *n);
            o.noun("card", if *n == 1 { Num::Sg } else { Num::Pl });
            o.w("each turn");
        }
        PlayerRule::CantCastMoreThanEachTurn { n, what } => {
            o.w("cast more than");
            syn::number(o, *n);
            filter(o, what, if *n == 1 { Num::Sg } else { Num::Pl });
            o.w("each turn");
        }
        PlayerRule::CantGainLife => o.w("gain life"),
        PlayerRule::CantSearchLibraries => o.w("search libraries"),
        PlayerRule::CantGetCounters => o.w("get counters"),
    }
}

/// An effect's verb with an object subject: "~ deals", "target creature gets".
fn obj_subj(o: &mut Out, x: &Objects, verb: &str) {
    objects(o, x);
    o.verb(verb, obj_num(x));
}

fn effect(o: &mut Out, e: &Effect, cx: &Cx) {
    match e {
        Effect::Draw { who, n } => {
            subj(o, who, "draw", cx);
            cards(o, n);
        }
        Effect::Discard { who, what, random } => {
            subj(o, who, "discard", cx);
            objects(o, what);
            if *random {
                o.w("at random");
            }
        }
        Effect::DiscardHand(who) => {
            subj(o, who, "discard", cx);
            o.w(if *who == Player::You {
                "your hand"
            } else {
                "their hand"
            });
        }
        Effect::Damage { source, n, to } => {
            match source {
                // "you may have this land deal 3 damage": after "may", an
                // object's verb is said with "have".
                Some(s) if cx.modal => {
                    o.w("have");
                    objects(o, s);
                    o.verb("deal", Num::Pl);
                }
                Some(s) => obj_subj(o, s, "deal"),
                None => o.verb("deal", Num::Any),
            }
            if to[..] == [Recipient::Same] {
                amount(o, n);
                o.w("damage");
            } else if is_measure(n) {
                // "deals damage to any target equal to …": either order.
                let amount_first = out(|o| {
                    o.w("damage equal to");
                    measure(o, n);
                    o.w("to");
                    recipients(o, to);
                });
                let target_first = out(|o| {
                    o.w("damage to");
                    recipients(o, to);
                    o.w("equal to");
                    measure(o, n);
                });
                syn::position(o, amount_first, target_first);
            } else {
                amount(o, n);
                o.w("damage to");
                recipients(o, to);
            }
        }
        Effect::Destroy { what, no_regen } => {
            o.w("destroy");
            objects(o, what);
            if *no_regen {
                cant_be_regenerated(o, &[what]);
            }
        }
        Effect::Exile(x) => {
            o.w("exile");
            objects(o, x);
        }
        Effect::Move { .. } => move_effect(o, e, Num::Pl),
        Effect::Counter { what, unless } => {
            o.w("counter");
            objects(o, what);
            if let Some((who, c)) = unless {
                o.w("unless");
                player(o, who);
                o.verb("pay", player_num(who));
                costs(o, c);
            }
        }
        Effect::Create { who, n, token: t } => {
            subj(o, who, "create", cx);
            match &t.abilities[..] {
                // "Create a … token. It has "Sacrifice this token: Add {C}."".
                [a] if t.keywords.is_empty() && t.attacking.is_none() => {
                    let with = out(|o| token(o, n, t));
                    let it_has = out(|o| {
                        let bare = Token {
                            abilities: vec![],
                            ..t.clone()
                        };
                        token(o, n, &bare);
                        o.tok(Tok::Period);
                        syn::it(o);
                        o.verb("have", amount_num(n));
                        quoted(o, a);
                    });
                    syn::token_has(o, with, it_has);
                }
                _ => token(o, n, t),
            }
        }
        // "put a number of +1/+1 counters on it equal to its power", the
        // amount before or after the object.
        Effect::PutCounters { n, kind, on } if is_measure(n) => {
            o.w("put a number of");
            counter_noun(o, kind, Num::Pl);
            let on_first = out(|o| {
                o.w("on");
                objects(o, on);
                o.w("equal to");
                measure(o, n);
            });
            let equal_first = out(|o| {
                o.w("equal to");
                measure(o, n);
                o.w("on");
                objects(o, on);
            });
            syn::position(o, on_first, equal_first);
        }
        Effect::PutCounters { n, kind, on } => {
            o.w("put");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("on");
            objects(o, on);
        }
        Effect::RemoveAnyCounters { n, from } => {
            o.w("remove");
            match n {
                Amount::AnyAmount => o.w("any number of"),
                n => amount_a(o, n),
            }
            o.noun("counter", amount_num(n));
            o.w("from");
            objects(o, from);
        }
        Effect::RemoveCounters { n, kind, from } => {
            o.w("remove");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("from");
            objects(o, from);
        }
        Effect::Modify {
            what,
            pt,
            gains,
            dur: d,
        } => {
            if what.quant == Quant::Next {
                obj_subj(o, what, "have");
                keyword_list(o, gains);
                return;
            }
            objects(o, what);
            let n = obj_num(what);
            syn::also(o);
            if n != Num::Sg {
                syn::each_after(o);
            }
            let get = |o: &mut Out, pt: &Pt| {
                o.verb("get", n);
                syn::additional(o);
                o.tok(Tok::Pt(*pt));
            };
            let gain = |o: &mut Out| {
                o.verb("gain", n);
                keyword_list(o, gains);
            };
            match pt {
                Some(pt) if !gains.is_empty() => {
                    // "get +X/+X and gain trample", "gain trample and get
                    // +X/+X": two predicates in either order.
                    let get_first = out(|o| {
                        get(o, pt);
                        o.w("and");
                        gain(o);
                    });
                    let gain_first = out(|o| {
                        gain(o);
                        o.w("and");
                        get(o, pt);
                    });
                    syn::position(o, get_first, gain_first);
                }
                Some(pt) => get(o, pt),
                None => gain(o),
            }
            dur(o, d);
        }
        Effect::GainLife { who, n } => {
            subj(o, who, "gain", cx);
            life(o, n);
        }
        Effect::LoseLife { who, n } => {
            subj(o, who, "lose", cx);
            life(o, n);
        }
        Effect::AddMana(m) => {
            o.w("add");
            mana_add(o, m);
        }
        Effect::Search { who, zones, what } => {
            subj(o, who, "search", cx);
            match &zones[..] {
                [Zone::Library(w), Zone::Graveyard(_)] => {
                    whose(o, w);
                    o.w("library and/or graveyard");
                }
                [Zone::Graveyard(w), Zone::Hand(_), Zone::Library(_)] => {
                    whose(o, w);
                    o.w("graveyard, hand, and library");
                }
                [Zone::Library(w)] => {
                    whose(o, w);
                    o.w("library");
                }
                zs => {
                    for z in zs {
                        zone(o, z);
                    }
                }
            }
            o.w("for");
            objects(o, what);
        }
        Effect::Scry(n) => {
            o.w("scry");
            amount(o, n);
        }
        Effect::Surveil(n) => {
            o.w("surveil");
            amount(o, n);
        }
        Effect::Mill { who, n } => {
            subj(o, who, "mill", cx);
            cards(o, n);
        }
        Effect::Action { by, name, n } => {
            match by {
                Some(x) => obj_subj(o, x, name),
                None => o.w(name),
            }
            if let Some(n) = n {
                amount(o, n);
            }
        }
        Effect::Tap(x) => {
            o.w("tap");
            objects(o, x);
        }
        Effect::Untap(x) => {
            o.w("untap");
            objects(o, x);
        }
        Effect::Sacrifice { who, what } => {
            subj(o, who, "sacrifice", cx);
            match who {
                Player::ControllerOf(x) | Player::OwnerOf(x)
                    if **x == *what && x.quant == Quant::Antecedent =>
                {
                    let named = out(|o| objects(o, what));
                    syn::again_it(o, named);
                }
                _ => objects(o, what),
            }
        }
        Effect::FightEachOther(a) => {
            objects(o, a);
            o.verb("fight each other", Num::Pl);
        }
        Effect::Fight { a, b } => {
            if cx.modal {
                o.w("have");
                objects(o, a);
                o.verb("fight", Num::Pl);
            } else {
                obj_subj(o, a, "fight");
            }
            objects(o, b);
        }
        Effect::Shuffle(who) => {
            subj(o, who, "shuffle", cx);
            syn::shuffle_library(o);
        }
        Effect::Reveal { who, what } => {
            subj(o, who, "reveal", cx);
            objects(o, what);
        }
        Effect::LookAtTop { who, whose: w, n } => {
            subj(o, who, "look at", cx);
            top_of(o, n, w);
        }
        // "must be blocked this turn if able": the duration before "if able".
        Effect::Restrict {
            what,
            rule: Restriction::AttacksDespiteDefender,
            dur: Duration::ThisTurn,
        } => {
            objects(o, what);
            o.w("can attack this turn as though");
            syn::it(o);
            o.w("didn't have defender");
        }
        Effect::Restrict {
            what,
            rule: Restriction::MustBeBlocked,
            dur: Duration::ThisTurn,
        } => {
            objects(o, what);
            o.w("must be blocked this turn if able");
        }
        Effect::Restrict { what, rule, dur: d } => {
            objects(o, what);
            restriction(o, rule, obj_num(what));
            dur(o, d);
        }
        Effect::GainControl { what, dur: d } => {
            o.w("gain control of");
            objects(o, what);
            dur(o, d);
        }
        Effect::Transform(x) => {
            o.w("transform");
            objects(o, x);
        }
        Effect::Regenerate(x) => {
            o.w("regenerate");
            objects(o, x);
        }
        Effect::Attach { what, to } => {
            o.w("attach");
            objects(o, what);
            o.w("to");
            objects(o, to);
        }
        Effect::Copy {
            what,
            may_choose_new_targets,
        } => {
            o.w("copy");
            objects(o, what);
            if *may_choose_new_targets {
                o.w("and may choose");
                syn::new_targets(o);
                o.w("for");
                syn::that(o);
                o.w("copy");
            }
        }
        Effect::ExtraTurn => o.w("take an extra turn after this one"),
        Effect::RevealTop { who, whose: w, n } => {
            subj(o, who, "reveal", cx);
            top_of(o, n, w);
        }
        Effect::CreateCopy { n: _, of, except } => {
            o.w("create a token that's a copy of");
            objects(o, of);
            exceptions(o, except);
        }
        Effect::ChooseNewTargets(x) => {
            o.w("choose new targets for");
            objects(o, x);
        }
        Effect::ClassLevel(n) => {
            o.w("level");
            syn::number(o, *n);
        }
        Effect::May { who, then } => {
            player(o, who);
            o.w("may");
            let inner = Cx {
                implied: Some(who.clone()),
                continues: None,
                modal: true,
            };
            // "You may put a creature card … onto the battlefield. That
            // creature gains haste.": the steps after the option, said in
            // its sentence or in sentences of their own.
            let forms = (1..=then.len())
                .map(|k| {
                    out(|o| {
                        clauses(o, &then[..k], &inner);
                        if k < then.len() {
                            o.w(".");
                            clauses(o, &then[k..], cx);
                        }
                    })
                })
                .collect();
            syn::option_steps(o, forms);
        }
        Effect::IfYouDont(v) => {
            o.w("if");
            did_option(o, cx, "don't");
            o.w(",");
            clauses(o, v, cx);
        }
        Effect::Pay { who, cost: c } => {
            subj(o, who, "pay", cx);
            match &c[..] {
                [Cost::PayLife(n)] => life(o, n),
                [Cost::PayEnergy(n)] => {
                    amount(o, n);
                    o.tok(Tok::Sym("E".into()));
                }
                [Cost::Mana(m), Cost::PayLife(n)] => {
                    mana(o, m);
                    o.w("and");
                    life(o, n);
                }
                c => costs(o, c),
            }
        }
        // "Whenever you attack this turn, …", "When that creature dies this
        // turn, …", "If that creature would die this turn, …": a delayed
        // trigger or replacement that lasts the turn says so after its event.
        Effect::During {
            dur: Duration::ThisTurn,
            then,
        } if matches!(
            &then[..],
            [Effect::DelayedWhen { .. }
                | Effect::DelayedWhenever { .. }
                | Effect::Replacement { .. }]
        ) =>
        {
            match &then[0] {
                d @ (Effect::DelayedWhen { event: ev, then }
                | Effect::DelayedWhenever { event: ev, then }) => {
                    o.w(if matches!(d, Effect::DelayedWhen { .. }) {
                        "when"
                    } else {
                        "whenever"
                    });
                    event(o, ev);
                    o.w("this turn,");
                    clauses(o, then, cx);
                }
                Effect::Replacement { event: ev, instead } => {
                    let w = out(|o| {
                        would(o, ev);
                        o.w("this turn");
                    });
                    replacement(o, w, instead, cx);
                }
                _ => {}
            }
        }
        Effect::During { dur: d, then } => {
            dur(o, d);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::ExileUntilLeaves { what, until } => {
            o.w("exile");
            objects(o, what);
            o.w("until");
            objects(o, until);
            o.w("leaves the battlefield");
        }
        Effect::Prevent {
            combat_only,
            n,
            to,
            by,
            dur: d,
        } => {
            o.w("prevent");
            match n {
                None => o.w("all"),
                Some(n) => {
                    o.w("the next");
                    amount(o, n);
                }
            }
            if *combat_only {
                o.w("combat");
            }
            // "damage that black and/or red sources would deal to creatures
            // you control": the same damage, said actively.
            if let (Some(t), Some(b)) = (to, by) {
                if !(t.len() == 1 && t[0] == Recipient::Objects(b.clone())) {
                    let passive = out(|o| {
                        o.w("damage that would be dealt to");
                        recipients(o, t);
                        o.w("by");
                        objects(o, b);
                    });
                    let active = out(|o| {
                        o.w("damage that");
                        objects(o, b);
                        o.w("would deal to");
                        recipients(o, t);
                    });
                    syn::voice(o, passive, active);
                    dur(o, d);
                    return;
                }
            }
            o.w("damage that would be dealt");
            match (to, by) {
                (Some(t), Some(b)) if t.len() == 1 && t[0] == Recipient::Objects(b.clone()) => {
                    o.w("to and dealt by");
                    objects(o, b);
                }
                _ => {
                    if let Some(t) = to {
                        o.w("to");
                        recipients(o, t);
                    }
                    if let Some(b) = by {
                        o.w("by");
                        objects(o, b);
                    }
                }
            }
            dur(o, d);
        }
        Effect::Becomes {
            what,
            pt,
            types,
            gains,
            keeps: k,
            dur: d,
        } => {
            let becomes = out(|o| {
                obj_subj(o, what, "become");
                // "becomes a 2/2 creature", "becomes blue": an article only
                // before a noun, and only for one object: "Artifacts you
                // control become artifact creatures".
                let n = obj_num(what);
                let noun = types.last().is_some_and(is_typed);
                if (pt.is_some() || noun) && n != Num::Pl {
                    o.w("a");
                }
                let type_words = |o: &mut Out| {
                    for (i, a) in types.iter().enumerate() {
                        let last = i + 1 == types.len();
                        adj(o, a, (last && noun && n == Num::Pl).then_some(Num::Pl));
                    }
                };
                let inline = out(|o| {
                    if let Some(pt) = pt {
                        o.tok(Tok::Pt(*pt));
                    }
                    type_words(o);
                });
                match pt {
                    // "becomes a Kithkin Scout with base power and
                    // toughness 2/3".
                    Some(pt) if !types.is_empty() => {
                        let with = out(|o| {
                            type_words(o);
                            o.w("with base power and toughness");
                            o.tok(Tok::Pt(*pt));
                        });
                        syn::base_pt(o, inline, with);
                    }
                    _ => o.add(inline),
                }
                if !gains.is_empty() {
                    let with = out(|o| {
                        o.w("with");
                        keyword_list(o, gains);
                    });
                    let and_gains = out(|o| {
                        o.w("and");
                        o.verb("gain", obj_num(what));
                        keyword_list(o, gains);
                    });
                    syn::becomes_with(o, with, and_gains);
                }
                keeps_then_dur(o, k, types, d);
            });
            match pt {
                // "creatures you control have base power and toughness X/X".
                Some(pt)
                    if types.is_empty() && gains.is_empty() && *k == Keeps::implied_by(&[]) =>
                {
                    let base = out(|o| {
                        obj_subj(o, what, "have");
                        o.w("base power and toughness");
                        o.tok(Tok::Pt(*pt));
                        dur(o, d);
                    });
                    syn::base_pt(o, becomes, base);
                }
                _ => o.add(becomes),
            }
        }
        Effect::BecomesCopy {
            what,
            of,
            dur: d,
            except,
        } => {
            // "you may have it become a copy of …".
            if cx.modal {
                o.w("have");
                objects(o, what);
                o.verb("become", Num::Pl);
            } else {
                obj_subj(o, what, "become");
            }
            o.w("a copy of");
            objects(o, of);
            dur(o, d);
            exceptions(o, except);
        }
        Effect::Designation(s) => o.w(match s.as_str() {
            "monarch" => "you become the monarch",
            "initiative" => "you take the initiative",
            _ => "the ring tempts you",
        }),
        Effect::Delayed { step: s, then } => {
            let before = out(|o| {
                o.w("at the beginning of");
                step(o, s);
                o.w(",");
                clauses(o, then, cx);
            });
            if then.len() == 1 {
                let after = out(|o| {
                    clauses(o, then, cx);
                    o.w("at the beginning of");
                    step(o, s);
                });
                syn::position(o, before, after);
            } else {
                o.add(before);
            }
        }
        Effect::EntersTapped(x) => {
            obj_subj(o, x, "enter");
            o.w("tapped");
        }
        Effect::Choose(c) => {
            o.w("choose");
            choice(o, c);
        }
        Effect::GetEnergy(n) => {
            subj(o, &Player::You, "get", cx);
            for _ in 0..*n {
                o.tok(Tok::Sym("E".into()));
            }
        }
        Effect::Emblem(v) => {
            o.w("you get an emblem with");
            for (i, a) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                quoted(o, a);
            }
        }
        Effect::Where { then, x } => {
            let forms = (1..=then.len())
                .map(|k| {
                    out(|o| {
                        clauses(o, &then[..k], cx);
                        o.w(", where X is");
                        measure(o, x);
                        if k < then.len() {
                            syn::joiner(o, false);
                            clauses(o, &then[k..], cx);
                        }
                    })
                })
                .collect();
            syn::where_x_after(o, forms);
        }
        Effect::IfYouDo(v) => {
            o.w("if");
            did_option(o, cx, "do");
            o.w(",");
            clauses(o, v, cx);
        }
        Effect::BecomesPrepared(what) => {
            obj_subj(o, what, "become");
            o.w("prepared");
        }
        Effect::OnSpend { of, on, then } => {
            o.w(match of {
                SpentOf::Any => "if any of that mana is spent on",
                SpentOf::All => "if you spend this mana to cast",
            });
            objects(o, on);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::If { cond, then } => {
            let before = out(|o| {
                o.w("if");
                condition(o, cond);
                o.w(",");
                clauses(o, then, cx);
            });
            if then.len() == 1 {
                let after = out(|o| {
                    clauses(o, then, cx);
                    o.w("if");
                    condition(o, cond);
                });
                let if_forms = out(|o| syn::position(o, before, after));
                match cond {
                    // "put that card into your hand unless it has the same
                    // name as another card exiled this way".
                    Condition::Not(c) => {
                        let unless = out(|o| {
                            clauses(o, then, cx);
                            o.w("unless");
                            condition(o, c);
                        });
                        syn::unless(o, if_forms, unless);
                    }
                    _ => o.add(if_forms),
                }
            } else {
                // "exile it if it's an instant or sorcery card. You may cast
                // that card …": the sentences after the first are the
                // branch's too (SEMANTICS [branch-owns-new-object]), and the
                // "if" goes after the first as it may after a lone clause.
                let after = out(|o| {
                    clauses(o, &then[..1], cx);
                    o.w("if");
                    condition(o, cond);
                    syn::joiner(o, false);
                    clauses(o, &then[1..], cx);
                });
                syn::position(o, before, after);
            }
        }
        // The rules are said as an "if" after the permission.
        Effect::PermissionRules {
            permission,
            spell,
            rules,
        } => {
            effect(o, permission, cx);
            syn::joiner(o, false);
            let cond = Condition::ThisWay(Box::new(Event::Casts {
                who: Player::You,
                what: spell.clone(),
            }));
            effect(
                o,
                &Effect::If {
                    cond,
                    then: rules.clone(),
                },
                cx,
            );
        }
        Effect::Unless {
            effect: inner,
            who,
            cost: c,
        } => {
            effect(o, inner, cx);
            o.w("unless");
            player(o, who);
            unless_costs(o, c, player_num(who));
        }
        // "when each opponent casts their first spell of the game, …": one
        // delayed trigger per opponent, "each" said inside its event.
        Effect::ForEach { each, then }
            if each.filter.kind == Some(Noun::Opponent)
                && matches!(
                    &then[..],
                    [Effect::DelayedWhen {
                        event: Event::Casts {
                            who: Player::That,
                            ..
                        },
                        ..
                    }]
                ) =>
        {
            let [Effect::DelayedWhen {
                event: Event::Casts { what, .. },
                then: body,
            }] = &then[..]
            else {
                return;
            };
            let per = out(|o| {
                o.w("for each");
                each_objects(o, each);
                o.w(",");
                clauses(o, then, cx);
            });
            let inside = out(|o| {
                o.w("when");
                event(
                    o,
                    &Event::Casts {
                        who: Player::EachOpponent,
                        what: what.clone(),
                    },
                );
                o.w(",");
                clauses(o, body, cx);
            });
            syn::each_in_trigger(o, per, inside);
        }
        Effect::ForEach { each, then } => {
            let before = out(|o| {
                o.w("for each");
                each_objects(o, each);
                o.w(",");
                clauses(o, then, cx);
            });
            if then.len() == 1 {
                let after = out(|o| {
                    clauses(o, then, cx);
                    o.w("for each");
                    each_objects(o, each);
                });
                syn::position(o, before, after);
            } else {
                o.add(before);
            }
        }
        Effect::ForEachColor { among, then } => {
            let colors = |o: &mut Out| {
                o.w("for each color among");
                if let Palette::ColorsOf(x) = among {
                    objects(o, x);
                }
            };
            let before = out(|o| {
                colors(o);
                o.w(",");
                clauses(o, then, cx);
            });
            // "draw a card for each color among permanents you control".
            if then.len() == 1 {
                let after = out(|o| {
                    clauses(o, then, cx);
                    colors(o);
                });
                syn::position(o, before, after);
            } else {
                o.add(before);
            }
        }
        Effect::Modal { choose, modes } => modal(o, choose, modes, cx),
        Effect::GrantAbility {
            what,
            ability,
            dur: d,
        } => {
            obj_subj(o, what, "gain");
            quoted(o, ability);
            dur(o, d);
        }
        Effect::LoseAbility {
            what,
            ability,
            dur: d,
        } => {
            obj_subj(o, what, "lose");
            quoted(o, ability);
            dur(o, d);
        }
        Effect::ShuffleInto { who, what } => {
            subj(o, who, "shuffle", cx);
            objects(o, what);
            o.w("into");
            syn::owners_library(o);
        }
        Effect::Performed { by, effect: inner } => {
            let n = player_num(by);
            let said = out(|o| player(o, by));
            let rest = |n: Num| out(|o| performed(o, inner, n));
            if cx.implied.as_ref() == Some(by) || cx.continues.as_ref() == Some(by) {
                syn::subject(o, said, rest(n), rest(n));
            } else {
                o.add(said);
                o.add(rest(n));
            }
        }
        Effect::ExileZone(z) => {
            o.w("exile");
            zone(o, z);
        }
        Effect::Cast {
            verb,
            what,
            payment,
            dur: d,
        } => {
            o.w(match verb {
                CastVerb::Play => "play",
                CastVerb::Cast => "cast",
            });
            objects(o, what);
            // "play it this turn without paying its mana cost": either
            // order.
            let free = out(|o| {
                if *payment == Payment::Free {
                    o.w("without paying");
                    syn::its(o);
                    o.w("mana");
                    o.noun("cost", Num::Any);
                }
            });
            let how_long = out(|o| dur(o, d));
            let free_first = out(|o| {
                o.add(free.clone());
                o.add(how_long.clone());
            });
            let dur_first = out(|o| {
                o.add(how_long);
                o.add(free);
            });
            syn::position(o, free_first, dur_first);
        }
        Effect::PlayerGains {
            who,
            keywords,
            dur: d,
        } => {
            player(o, who);
            o.verb("gain", player_num(who));
            keyword_list(o, keywords);
            dur(o, d);
        }
        Effect::PlayerRestrict { who, rule, dur: d } => {
            if *rule == PlayerRule::LifeTotalCantChange {
                whose(o, who);
                o.w("life total can't change");
            } else {
                player(o, who);
                o.w("can't");
                player_rule(o, rule);
            }
            dur(o, d);
        }
        Effect::PhaseOut(x) => obj_subj(o, x, "phase out"),
        Effect::Instead { cond, then } => {
            let head = out(|o| {
                o.w("if");
                condition(o, cond);
                o.w(",");
            });
            let lead = out(|o| {
                o.add(head.clone());
                o.w("instead");
                clauses(o, then, cx);
            });
            let trail = out(|o| {
                o.add(head.clone());
                clauses(o, then, cx);
                o.w("instead");
            });
            let mut forms = vec![lead, trail];
            // "Add {B}{B}{B}{B}{B} instead if there are seven or more cards
            // in your graveyard."
            if then.len() == 1 {
                forms.push(out(|o| {
                    clauses(o, then, cx);
                    o.w("instead if");
                    condition(o, cond);
                }));
            }
            syn::instead(o, forms);
        }
        Effect::Or(v) => {
            let creates: Option<Vec<(&Player, &Amount, &Token)>> = v
                .iter()
                .map(|e| match e {
                    Effect::Create { who, n, token } => Some((who, n, token)),
                    _ => None,
                })
                .collect();
            match creates {
                Some(cs) => {
                    subj(o, cs[0].0, "create", cx);
                    for (i, (_, n, t)) in cs.iter().enumerate() {
                        if i > 0 {
                            o.w("or");
                        }
                        token(o, n, t);
                    }
                }
                None => {
                    for (i, e) in v.iter().enumerate() {
                        if i > 0 {
                            o.w("or");
                        }
                        effect(o, e, cx);
                    }
                }
            }
        }
        Effect::CreateThose { by, n, those } => match by {
            Actor::Passive => {
                amount(o, n);
                o.w("of");
                objects(o, those);
                o.w("are created");
            }
            by => {
                actor_subj(o, by, "create", cx);
                amount(o, n);
                o.w("of");
                objects(o, those);
            }
        },
        Effect::PutThose { by, n, those, on } => {
            actor_subj(o, by, "put", cx);
            amount(o, n);
            o.w("of");
            objects(o, those);
            o.w("on");
            objects(o, on);
        }
        Effect::DelayedWhen { event: ev, then } => {
            o.w("when");
            event(o, ev);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::DelayedNext {
            event: ev,
            dur: d,
            then,
        } => {
            o.w("when you next");
            match ev {
                Event::Casts { what, .. } => {
                    o.w("cast");
                    objects(o, what);
                }
                Event::Activates { what, .. } => {
                    o.w("activate");
                    objects(o, what);
                }
                e => event(o, e),
            }
            dur(o, d);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::WhenYouDo(v) => {
            o.w("when you do,");
            clauses(o, v, cx);
        }
        Effect::Process(v) => clauses(o, v, cx),
        Effect::WhenYouDoIf { cond, then } => {
            o.w("when you do, if");
            condition(o, cond);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::Labelled { label, then } => {
            o.w(label);
            o.tok(Tok::Dash);
            sentences_in_mode(o, then, cx);
        }
        Effect::ChangeTarget(x) => {
            o.w("change the target of");
            objects(o, x);
        }
        Effect::ChangeTargetTo { what, to } => {
            o.w("change the target of");
            objects(o, what);
            o.w("to");
            objects(o, to);
        }
        Effect::ExileFaceDown(x) => {
            o.w("exile");
            objects(o, x);
            o.w("face down");
        }
        Effect::LookAtHand { who, whose: w } => {
            subj(o, who, "look at", cx);
            whose(o, w);
            o.w("hand");
        }
        Effect::CastAsThoughFlash { what, dur: d } => {
            o.w("cast");
            objects(o, what);
            dur(o, d);
            o.w("as though they had flash");
        }
        Effect::LoseKeywords {
            what,
            keywords,
            dur: d,
        } => {
            obj_subj(o, what, "lose");
            keyword_list(o, keywords);
            dur(o, d);
        }
        Effect::LoseGame(who) => {
            subj(o, who, "lose", cx);
            o.w("the game");
        }
        Effect::WinGame(who) => {
            subj(o, who, "win", cx);
            o.w("the game");
        }
        Effect::ExtraLandPlays { n, dur: d } => {
            o.w("play");
            amount_a(o, n);
            o.w("additional");
            o.noun("land", amount_num(n));
            dur(o, d);
        }
        Effect::Amass { of, n } => {
            o.w("amass");
            o.noun(of, Num::Pl);
            amount(o, n);
        }
        Effect::CreateOfEach { who, n } => {
            subj(o, who, "create", cx);
            amount_a(o, n);
            o.w("of each");
        }
        Effect::LifeTotalBecomes { who, n } => {
            whose(o, who);
            o.w("life total becomes");
            if is_measure(n) {
                syn::becomes_equal_to(o);
                measure(o, n);
            } else {
                amount(o, n);
            }
        }
        Effect::InsteadOf { then, of } => {
            effect(o, then, cx);
            // "instead of into that player's graveyard": the verb and its
            // object said once, for the same object.
            if let (
                Effect::Move { what: w1, .. },
                Effect::Move {
                    what: w2, to: z, ..
                },
            ) = (&**then, &**of)
            {
                if w1 == w2 && !matches!(z, Zone::AnywhereOtherThan(_)) {
                    let full = out(|o| {
                        o.w("instead of putting");
                        move_put_rest(o, of);
                    });
                    let elided = out(|o| {
                        o.w("instead of into");
                        zone(o, z);
                    });
                    syn::gapped(o, full, elided);
                    return;
                }
            }
            o.w("instead of putting");
            match &**of {
                Effect::Move {
                    what,
                    to: Zone::AnywhereOtherThan(_),
                    ..
                } => {
                    objects(o, what);
                    o.w("anywhere else");
                }
                m @ Effect::Move { .. } => move_put_rest(o, m),
                e => effect(o, e, cx),
            }
        }
        Effect::ActionOn { who, name, what } => {
            subj(o, who, name, cx);
            objects(o, what);
        }
        Effect::MoveCounters { counters, from, to } => {
            o.w("move");
            objects(o, counters);
            o.w("from");
            objects(o, from);
            o.w("onto");
            objects(o, to);
        }
        // "put its counters on target creature you control".
        Effect::PutThoseCounters {
            those:
                Objects {
                    quant: Quant::All,
                    filter:
                        Filter {
                            kind: Some(Noun::Counter(kind)),
                            posts,
                            ..
                        },
                },
            on,
        } if matches!(&posts[..], [Post::On(x)] if is_bare_antecedent(x) || is_bare_self(x)) => {
            o.w("put");
            if let [Post::On(x)] = &posts[..] {
                if is_bare_antecedent(x) {
                    syn::its_counters(o);
                } else {
                    possessive_of(o, x);
                }
            }
            if let Some(k) = kind {
                counter_kind(o, k);
            }
            o.w("counters on");
            objects(o, on);
        }
        Effect::PutThoseCounters { those, on } => {
            o.w("put");
            objects(o, those);
            o.w("on");
            objects(o, on);
        }
        Effect::ExileWithCounters { what, n, kind } => {
            o.w("exile");
            objects(o, what);
            o.w("with");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("on");
            syn::it(o);
        }
        Effect::Double { stats, of, dur: d } => {
            o.w("double");
            let stat = if stats.len() == 2 {
                "power and toughness"
            } else {
                stat_name(stats[0])
            };
            let of_form = out(|o| {
                o.w("the");
                o.w(stat);
                o.w("of");
                objects(o, of);
            });
            let s_form = out(|o| {
                objects(o, of);
                o.tok(Tok::Possessive);
                o.w(stat);
            });
            syn::genitive(o, of_form, s_form);
            dur(o, d);
        }
        Effect::RollDie(n) => o.w(&format!("roll a d{n}")),
        Effect::KeepsMana { who, mana: m } => {
            player(o, who);
            o.verb("don't lose", player_num(who));
            match m {
                KeptMana::This => o.w("this mana"),
                KeptMana::Unspent(c) => {
                    o.w("unspent");
                    if let Some(c) = c {
                        o.w(color_name(*c));
                    }
                    o.w("mana");
                }
            }
            o.w("as steps and phases end");
        }
        Effect::Repeat { times, then } => match times {
            Amount::DamagePrevented => {
                o.w("for each 1 damage prevented this way,");
                clauses(o, then, cx);
            }
            t => {
                o.w("repeat the following process");
                amount(o, t);
                o.w("times.");
                clauses(o, then, cx);
            }
        },
        Effect::RepeatUntil(e) => {
            o.w("repeat this process until");
            event(o, e);
            syn::whichever_first(o);
        }
        Effect::ShuffleZones { who, zones } => {
            subj(o, who, "shuffle", cx);
            let w = match &zones[0] {
                Zone::Hand(w) | Zone::Graveyard(w) => w.clone(),
                _ => Player::You,
            };
            whose(o, &w);
            for (i, z) in zones.iter().enumerate() {
                list_sep(o, i, zones.len(), "and");
                o.w(match z {
                    Zone::Hand(_) => "hand",
                    _ => "graveyard",
                });
            }
            o.w("into");
            whose(o, &w);
            o.w("library");
        }
        Effect::NoMaxHandSize { who, dur: d } => {
            subj(o, who, "have", cx);
            o.w("no maximum hand size");
            dur(o, d);
        }
        Effect::EntersWithCounters { what, n, kind } => {
            obj_subj(o, what, "enter");
            o.w("with");
            enters_counters(o, n, kind);
        }
        Effect::DistributeCounters { n, kind, among } => {
            o.w("distribute");
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w("among");
            objects(o, among);
        }
        Effect::AdditionalPhases { after, phases } => {
            let this = out(|o| {
                o.w("after this");
                if let Some(k) = after {
                    o.w(phase_word(*k));
                }
                o.w("phase");
            });
            let extra = out(|o| {
                syn::there_is(o);
                for (i, k) in phases.iter().enumerate() {
                    if i > 0 {
                        o.w("followed by");
                    }
                    o.w("an additional");
                    o.w(phase_word(*k));
                    o.w("phase");
                }
            });
            let before = out(|o| {
                o.add(this.clone());
                o.w(",");
                o.add(extra.clone());
            });
            let after = out(|o| {
                o.add(extra);
                o.add(this);
            });
            syn::position(o, before, after);
        }
        Effect::DoesTheSame(who) => {
            player(o, who);
            o.verb("do the same", player_num(who));
        }
        Effect::SeparatePiles { who, what, n } => {
            subj(o, who, "separate", cx);
            objects(o, what);
            o.w("into");
            syn::number(o, *n);
            o.w("piles");
        }
        Effect::Replacement { event: ev, instead } => {
            let w = out(|o| would(o, ev));
            replacement(o, w, instead, cx);
        }
        Effect::DoubleCounters { kind, on } => {
            o.w("double the number of");
            if let Some(k) = kind {
                counter_kind(o, k);
            }
            o.w("counters on");
            objects(o, on);
        }
        Effect::ThatDamage { source, n, to } => {
            obj_subj(o, source, "deal");
            match n {
                // "that much damage plus 2".
                Amount::Plus(_, k) => {
                    o.w("that much damage plus");
                    syn::number(o, *k);
                }
                Amount::Thrice(_) => o.w("triple that damage"),
                _ => o.w("double that damage"),
            }
            if let Some(t) = to {
                o.w("to");
                recipients(o, t);
            }
        }
        Effect::UntilFound {
            who,
            verb,
            zone: z,
            until,
        } => {
            let v = match verb {
                DigVerb::Exile => "exile",
                DigVerb::Reveal => "reveal",
            };
            subj(o, who, v, cx);
            o.w("cards from");
            zone(o, z);
            o.w("until");
            let active = out(|o| {
                o.w(if *who == Player::You { "you" } else { "they" });
                o.verb(v, Num::Pl);
                objects(o, until);
            });
            // "until an artifact or creature card is revealed".
            let passive = out(|o| {
                objects(o, until);
                o.w("is");
                o.w(if *verb == DigVerb::Exile {
                    "exiled"
                } else {
                    "revealed"
                });
            });
            syn::voice(o, active, passive);
        }
        Effect::GetCounters { who, n, kind } => {
            player(o, who);
            o.verb("get", player_num(who));
            amount_a(o, n);
            counter_noun(o, kind, amount_num(n));
        }
        Effect::DelayedWhenever { event: ev, then } => {
            o.w("whenever");
            event(o, ev);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::CreateThosePlus {
            by,
            those,
            n,
            token: t,
        } => match by {
            Actor::Passive => {
                objects(o, those);
                o.w("plus");
                token(o, n, t);
                o.w("are created");
            }
            by => {
                actor_subj(o, by, "create", cx);
                objects(o, those);
                o.w("plus");
                token(o, n, t);
            }
        },
        Effect::RemoveFromCombat(x) => {
            o.w("remove");
            objects(o, x);
            o.w("from combat");
        }
        Effect::SkipTurn(who) => {
            subj(o, who, "skip", cx);
            o.w("that turn");
        }
        Effect::AlsoIs { what, dur: d, .. } => {
            obj_subj(o, what, "gain");
            o.w("all creature types");
            dur(o, d);
        }
        // Its duration is the play permission's, said with it.
        Effect::SpendAsAnyColor { uses, .. } => {
            o.w("spend mana as though it were mana of any color");
            spend_uses(o, uses);
        }
        Effect::SpendAsAnyType { uses, .. } => {
            syn::any_type_spent(o);
            spend_uses(o, uses);
        }
        Effect::IsNow {
            what,
            types,
            keeps: k,
        } => {
            pronoun_or(o, what, |o| o.w("it's"), |o| obj_subj(o, what, "are"));
            type_words(o, types, Num::Sg);
            keeps(o, k);
        }

        Effect::Otherwise(v) => {
            o.w("otherwise,");
            clauses(o, v, cx);
        }
        // "that many -1/-1 counters minus one are put on it".
        Effect::CountersArePut {
            n: Amount::Minus(a, b),
            kind,
            on,
        } => {
            amount(o, a);
            counter_noun(o, kind, Num::Pl);
            o.w("minus");
            amount(o, b);
            o.w("are put on");
            objects(o, on);
        }
        Effect::CountersArePut { n, kind, on } => {
            amount(o, n);
            counter_noun(o, kind, amount_num(n));
            o.w(if amount_num(n) == Num::Sg {
                "is put on"
            } else {
                "are put on"
            });
            objects(o, on);
        }
        Effect::EntersWith {
            effect: inner,
            n,
            kind,
        } => {
            let with = out(|o| {
                effect(o, inner, cx);
                o.w("with");
                enters_counters(o, n, kind);
            });
            let after = out(|o| {
                effect(o, inner, cx);
                syn::joiner(o, false);
                let what = Objects {
                    quant: Quant::Antecedent,
                    filter: Filter::default(),
                };
                effect(
                    o,
                    &Effect::EntersWithCounters {
                        what,
                        n: n.clone(),
                        kind: kind.clone(),
                    },
                    cx,
                );
            });
            syn::enters_with_after(o, with, after);
        }
        Effect::DamageCantBePrevented(d) => {
            o.w("damage can't be prevented");
            dur(o, d);
        }
        Effect::RevealHand(who) => {
            subj(o, who, "reveal", cx);
            o.w(if *who == Player::You {
                "your hand"
            } else {
                "their hand"
            });
        }
        Effect::Loses { what, lost, dur: d } => {
            obj_subj(o, what, "lose");
            lost_words(o, lost);
            dur(o, d);
        }
        Effect::GainsChoiceOf {
            what,
            keywords,
            dur: d,
        } => {
            obj_subj(o, what, "gain");
            o.w("your choice of");
            for (i, k) in keywords.iter().enumerate() {
                list_sep(o, i, keywords.len(), "or");
                keyword(o, k);
            }
            dur(o, d);
        }
        Effect::ShuffleAllInto { who, what, zones } => {
            subj(o, who, "shuffle", cx);
            let mut first = true;
            for x in what {
                if !first {
                    o.w("and");
                }
                first = false;
                objects(o, x);
            }
            for z in zones {
                if !first {
                    o.w("and");
                }
                first = false;
                zone(o, z);
            }
            o.w("into");
            syn::owners_library(o);
        }
        Effect::GetEnergyAmount(n) => {
            subj(o, &Player::You, "get", cx);
            amount(o, n);
            o.tok(Tok::Sym("E".into()));
        }
        Effect::RepeatProcess => o.w("repeat this process"),
        Effect::ControlPlayerNextTurn(who) => {
            subj(o, &Player::You, "gain", cx);
            o.w("control of");
            player(o, who);
            o.w("during that player's next turn");
        }
        Effect::ExtraTurnAfterThat(who) => {
            o.w("after that turn,");
            player(o, who);
            o.verb("take", player_num(who));
            o.w("an extra turn");
        }
        Effect::BecomesSized {
            what,
            types,
            keeps: _,
            each_equal_to,
            dur: d,
        } => {
            obj_subj(o, what, "become");
            o.w("a");
            for a in types {
                adj(o, a, None);
            }
            o.w("with power and toughness each equal to");
            measure(o, each_equal_to);
            dur(o, d);
        }
        Effect::KeywordCostIs { keyword: k, .. } => {
            o.w("the");
            o.w(k);
            o.w("cost is equal to its mana cost");
        }
        Effect::CreateNamed {
            who,
            n,
            token: t,
            name,
        } => {
            subj(o, who, "create", cx);
            // The name sits between "token" and the list of what it has.
            let with = Token {
                keywords: vec![],
                abilities: vec![],
                // "named Boulder that's tapped and attacking".
                attacking: None,
                tapped: t.tapped && t.attacking.is_none(),
                ..t.clone()
            };
            // "named Everywhere that is every basic land type": the five
            // basic land types, said after the name.
            let every_basic = BASIC_LAND_TYPES
                .iter()
                .all(|b| t.types.iter().any(|x| x == b));
            let with = Token {
                types: if every_basic {
                    with.types
                        .iter()
                        .filter(|x| !BASIC_LAND_TYPES.contains(&x.as_str()))
                        .cloned()
                        .collect()
                } else {
                    with.types
                },
                ..with
            };
            token(o, n, &with);
            o.w("named");
            o.w(name);
            if every_basic {
                o.w("that is every basic land type");
            }
            token_has(o, t);
            token_attacking(o, n, t);
        }
        Effect::CreateAttached {
            who,
            n,
            token: t,
            to,
        } => {
            subj(o, who, "create", cx);
            token(o, n, t);
            o.w("attached to");
            objects(o, to);
        }
        Effect::EmblemFor { who, abilities } => {
            player(o, who);
            o.verb("get", player_num(who));
            o.w("an emblem with");
            for (i, a) in abilities.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                quoted(o, a);
            }
        }
        Effect::ForEachCardType(then) => {
            o.w("for each card type,");
            clauses(o, then, cx);
        }
        Effect::FlipCoin => o.w("flip a coin"),
        Effect::CostChange {
            what,
            by,
            less,
            dur: d,
        } => {
            obj_subj(o, what, "cost");
            mana(o, by);
            o.w(if *less { "less" } else { "more" });
            o.w("to cast");
            dur(o, d);
        }
        Effect::DamageDivided { source, n, among } => {
            match source {
                Some(s) => obj_subj(o, s, "deal"),
                None => o.verb("deal", Num::Any),
            }
            amount(o, n);
            o.w("damage divided as you choose among");
            objects(o, among);
        }
        Effect::EndTheTurn => o.w("end the turn"),
        Effect::DigUntilTotal {
            who,
            verb,
            zone: z,
            total,
        } => {
            let v = match verb {
                DigVerb::Exile => "exile",
                DigVerb::Reveal => "reveal",
            };
            subj(o, who, v, cx);
            o.w("cards from");
            zone(o, z);
            o.w("until");
            if *who == Player::You {
                o.w("you have");
            } else {
                syn::they_have(o);
            }
            o.w(if *verb == DigVerb::Exile {
                "exiled"
            } else {
                "revealed"
            });
            o.w("cards with total");
            compare_words(o, total);
        }
        Effect::PutCountersEach { counters, on } => {
            o.w("put");
            for (i, (n, k)) in counters.iter().enumerate() {
                list_sep(o, i, counters.len(), "and");
                amount_a(o, n);
                counter_noun(o, k, amount_num(n));
            }
            o.w("on");
            objects(o, on);
        }
        Effect::WhenThisWay { event: ev, then } => {
            o.w("when");
            event(o, ev);
            o.w("this way,");
            clauses(o, then, cx);
        }
        Effect::RemoveAllCounters(x) => {
            o.w("remove all counters from");
            objects(o, x);
        }
        Effect::LosesAllCounters(who) => {
            player(o, who);
            o.verb("lose", player_num(who));
            o.w("all counters");
        }
        Effect::NextTime {
            event: ev,
            dur: d,
            then,
        } => {
            o.w("the next time");
            would(o, ev);
            dur(o, d);
            o.w(",");
            clauses(o, then, cx);
        }
        Effect::PreventThatDamage => o.w("prevent that damage"),
        Effect::Meld { what, into } => {
            o.w("meld");
            objects(o, what);
            o.w("into");
            o.w(into);
        }
        Effect::ProducesInstead { what, mana: m } => {
            obj_subj(o, what, "produce");
            mana(o, m);
            o.w("instead of any other type and amount");
        }
    }
}

/// A move between zones, with "put", "return" or "put … back": no rule
/// reads the verb (see `syn::move_verb`).
fn move_effect(o: &mut Out, e: &Effect, n: Num) {
    let Effect::Move {
        what,
        from,
        to,
        order,
        ..
    } = e
    else {
        return;
    };
    let put = out(|o| {
        o.verb("put", n);
        move_put_rest(o, e);
    });
    let ret = out(|o| {
        o.verb("return", n);
        move_return_rest(o, e);
    });
    let mut forms = vec![put, ret];
    // "put them back in any order": to the top of the library they were
    // looked at in.
    if *to == Zone::OwnersLibraryTop && from.is_none() {
        forms.push(out(|o| {
            o.verb("put", n);
            objects(o, what);
            o.w("back");
            order_words(o, order);
        }));
    }
    syn::move_verb(o, forms);
}

/// A move after "put": what, from where, to where.
fn move_put_rest(o: &mut Out, e: &Effect) {
    let Effect::Move {
        what,
        from,
        to,
        tapped,
        transformed,
        under,
        order,
    } = e
    else {
        return;
    };
    objects(o, what);
    // "Put your commander into your hand from the command zone".
    let place = out(|o| put_to(o, to, *tapped, *transformed, under));
    let from_words = out(|o| {
        if let Some(z) = from {
            o.w("from");
            zone(o, z);
        }
    });
    let before = out(|o| {
        o.add(from_words.clone());
        o.add(place.clone());
    });
    let after = out(|o| {
        o.add(place);
        o.add(from_words);
    });
    syn::position(o, before, after);
    order_words(o, order);
}

/// A move after "return": what, from where, and "to" where, the object
/// before or after the place.
fn move_return_rest(o: &mut Out, e: &Effect) {
    let Effect::Move {
        what,
        from,
        to,
        tapped,
        transformed,
        under,
        order,
    } = e
    else {
        return;
    };
    let place = out(|o| {
        if let Some(z) = from {
            o.w("from");
            zone(o, z);
        }
        o.w("to");
        destination(o, to, *tapped, *transformed, under);
    });
    let before = out(|o| {
        objects(o, what);
        o.add(place.clone());
    });
    let after = out(|o| {
        o.add(place);
        objects(o, what);
    });
    syn::position(o, before, after);
    order_words(o, order);
}

/// "you do", "they do", "that player does": the player an earlier "may"
/// offered the choice to.
fn did_option(o: &mut Out, cx: &Cx, verb: &str) {
    match cx.continues.as_ref().or(cx.implied.as_ref()) {
        Some(Player::You) | None => {
            o.w("you");
            o.verb(verb, Num::Pl);
        }
        // "if they do", "if that player does", "if the player does".
        Some(p) => {
            let said = out(|o| player(o, p));
            let pronoun = out(syn::that_player);
            syn::anaphor_player(o, said, pronoun);
            o.verb(verb, Num::Any);
        }
    }
}

/// "If X would happen, Y instead": "instead" before or after Y, or unsaid
/// where Y says "instead of"; an "If you do" after Y is still Y's.
fn replacement(o: &mut Out, would_words: Out, instead: &[Effect], cx: &Cx) {
    let split = instead
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, e)| matches!(e, Effect::IfYouDo(_) | Effect::IfYouDont(_)))
        .map_or(instead.len(), |(i, _)| i);
    let (head, tail) = instead.split_at(split);
    let opening = out(|o| {
        o.w("if");
        o.add(would_words);
        o.w(",");
    });
    let rest = |o: &mut Out| {
        if !tail.is_empty() {
            o.tok(Tok::Period);
            clauses(o, tail, cx);
        }
    };
    let lead = out(|o| {
        o.add(opening.clone());
        o.w("instead");
        clauses(o, head, cx);
        rest(o);
    });
    let trail = out(|o| {
        o.add(opening.clone());
        clauses(o, head, cx);
        o.w("instead");
        rest(o);
    });
    let mut forms = vec![lead, trail];
    // "prevent that damage and …", "it produces {C} instead of any other
    // type": the clauses say they replace the event themselves.
    if matches!(
        head.last(),
        Some(Effect::InsteadOf { .. }) | Some(Effect::ProducesInstead { .. })
    ) || matches!(head.first(), Some(Effect::PreventThatDamage))
    {
        forms.push(out(|o| {
            o.add(opening.clone());
            clauses(o, head, cx);
            rest(o);
        }));
    }
    syn::instead(o, forms);
}

/// The verbs a player is said to do for an effect that takes none.
fn performed(o: &mut Out, e: &Effect, n: Num) {
    match e {
        Effect::Move { .. } => move_effect(o, e, n),
        Effect::PutCounters { n: k, kind, on } => {
            o.verb("put", n);
            amount_a(o, k);
            counter_noun(o, kind, amount_num(k));
            o.w("on");
            objects(o, on);
        }
        Effect::Choose(c) => {
            o.verb("choose", n);
            choice(o, c);
        }
        Effect::AddMana(m) => {
            o.verb("add", n);
            mana_add(o, m);
        }
        Effect::Exile(x) => {
            o.verb("exile", n);
            objects(o, x);
        }
        Effect::GainControl { what, dur: d } => {
            o.verb("gain", n);
            o.w("control of");
            objects(o, what);
            dur(o, d);
        }
        Effect::Scry(k) | Effect::Surveil(k) => {
            o.verb(
                if matches!(e, Effect::Scry(_)) {
                    "scry"
                } else {
                    "surveil"
                },
                n,
            );
            amount(o, k);
        }
        Effect::EntersWith {
            effect: inner,
            n: k,
            kind,
        } => {
            performed(o, inner, n);
            o.w("with");
            enters_counters(o, k, kind);
        }
        e => effect(o, e, &Cx::default()),
    }
}

fn actor_subj(o: &mut Out, a: &Actor, verb: &str, cx: &Cx) {
    match a {
        Actor::Player(p) => subj(o, p, verb, cx),
        Actor::AnEffect => {
            o.w("an effect");
            o.verb(verb, Num::Sg);
        }
        Actor::ThatEffect => {
            o.w("it");
            o.verb(verb, Num::Sg);
        }
        Actor::Passive => o.verb(verb, Num::Pl),
    }
}

fn phase_word(k: PhaseKind) -> &'static str {
    match k {
        PhaseKind::Combat => "combat",
        PhaseKind::Main => "main",
    }
}

fn enters_counters(o: &mut Out, n: &Amount, kind: &str) {
    let (base, extra) = match n {
        Amount::Additional(b) => (&**b, true),
        b => (b, false),
    };
    // "a +1/+1 counter on it for each color of mana spent to cast it".
    if let (Amount::ColorsOfManaSpentToCast(x), false) = (base, extra) {
        let for_each = out(|o| {
            o.w("a");
            counter_noun(o, kind, Num::Sg);
            o.w("on");
            syn::it(o);
            o.w("for each color of mana spent to cast");
            objects(o, x);
        });
        let a_number = out(|o| {
            o.w("a number of");
            counter_noun(o, kind, Num::Pl);
            o.w("on");
            syn::it(o);
            o.w("equal to");
            measure(o, base);
        });
        syn::for_each_count(o, for_each, a_number);
        return;
    }
    if is_measure(base) && !matches!(base, Amount::TimesKicked | Amount::CountOf(_)) {
        let a_number = out(|o| {
            o.w("a number of");
            if extra {
                o.w("additional");
            }
            counter_noun(o, kind, Num::Pl);
            o.w("on");
            syn::it(o);
            o.w("equal to");
            measure(o, base);
        });
        if extra {
            o.add(a_number);
            return;
        }
        let where_x = out(|o| {
            o.w("x");
            counter_noun(o, kind, Num::Pl);
            o.w("on");
            syn::it(o);
            o.w(", where x is");
            measure(o, base);
        });
        syn::where_x(o, a_number, where_x);
        return;
    }
    match base {
        Amount::TimesKicked => {
            o.w("a");
            counter_noun(o, kind, Num::Sg);
            o.w("on it for each time it was kicked");
        }
        Amount::CountOf(x) => {
            let for_each = out(|o| {
                o.w("a");
                if extra {
                    o.w("additional");
                }
                counter_noun(o, kind, Num::Sg);
                o.w("on");
                syn::it(o);
                o.w("for each");
                each_objects(o, x);
            });
            let a_number = out(|o| {
                o.w("a number of");
                if extra {
                    o.w("additional");
                }
                counter_noun(o, kind, Num::Pl);
                o.w("on");
                syn::it(o);
                o.w("equal to");
                measure(o, base);
            });
            if extra {
                syn::for_each_count(o, for_each, a_number);
                return;
            }
            let where_x = out(|o| {
                o.w("x");
                counter_noun(o, kind, Num::Pl);
                o.w("on");
                syn::it(o);
                o.w(", where x is");
                measure(o, base);
            });
            let counted = out(|o| syn::where_x(o, a_number, where_x));
            syn::for_each_count(o, for_each, counted);
        }
        b => {
            amount_a(o, b);
            if extra {
                o.w("additional");
            }
            counter_noun(o, kind, amount_num(b));
            o.w("on");
            syn::it(o);
        }
    }
}

fn unless_costs(o: &mut Out, cs: &[Cost], n: Num) {
    match cs {
        [Cost::Sacrifice(x)] => {
            o.verb("sacrifice", n);
            objects(o, x);
        }
        [Cost::Discard(x)] => {
            o.verb("discard", n);
            objects(o, x);
        }
        [Cost::OneOf(v)]
            if v.iter()
                .all(|c| matches!(c, Cost::Sacrifice(_) | Cost::Discard(_))) =>
        {
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                unless_costs(o, std::slice::from_ref(c), n);
            }
        }
        cs => {
            o.verb("pay", n);
            costs(o, cs);
        }
    }
}

fn life(o: &mut Out, n: &Amount) {
    // "loses half their life, rounded up".
    if let Amount::Half { of, round_up } = n {
        if let Amount::LifeTotal(w) = &**of {
            o.w("half");
            whose(o, w);
            o.w("life");
            rounded(o, *round_up);
            return;
        }
    }
    if is_measure(n) {
        o.w("life equal to");
        measure(o, n);
    } else {
        amount(o, n);
        o.w("life");
    }
}

fn top_of(o: &mut Out, n: &Amount, w: &Player) {
    o.w("the top");
    if *n != Amount::N(1) {
        amount(o, n);
    }
    o.noun("card", amount_num(n));
    o.w("of");
    whose(o, w);
    o.w("library");
}

fn order_words(o: &mut Out, order: &Option<Order>) {
    match order {
        Some(Order::Any) => o.w("in any order"),
        Some(Order::Random) => o.w("in a random order"),
        None => {}
    }
}

/// What a "becomes" keeps and how long: "that's still a land until end of
/// turn", or "until end of turn. It's still a land."
fn keeps_then_dur(o: &mut Out, k: &Keeps, types: &[Adj], d: &Duration) {
    match k {
        // "becomes an artifact creature": what CR 205.1b keeps whether
        // "in addition to its other types" is said or not.
        k if *k == Keeps::implied_by(types) => {
            let said = out(|o| keeps(o, k));
            syn::keeps_implied(o, said);
            dur(o, d);
        }
        Keeps::Listed(v) => {
            let clause = out(|o| {
                keeps(o, k);
                dur(o, d);
            });
            let sentence = out(|o| {
                dur(o, d);
                o.w(". it's still a");
                for a in v {
                    adj(o, a, None);
                }
            });
            syn::still_a(o, clause, sentence);
        }
        _ => {
            keeps(o, k);
            dur(o, d);
        }
    }
}

fn keeps(o: &mut Out, k: &Keeps) {
    match k {
        Keeps::Supertypes | Keeps::AllButSubtypesOfSet => {}
        Keeps::OtherTypes => o.w("in addition to its other types"),
        Keeps::Listed(v) => {
            o.w("that's still a");
            for a in v {
                adj(o, a, None);
            }
        }
    }
}

fn exceptions(o: &mut Out, es: &[Exception]) {
    if es.is_empty() {
        return;
    }
    o.w(", except");
    for (i, e) in es.iter().enumerate() {
        list_sep(o, i, es.len(), "and");
        match e {
            Exception::AlsoIs(v) => {
                o.w("it's a");
                for a in v {
                    adj(o, a, None);
                }
                o.w("in addition to its other types");
            }
            Exception::IsNot(v) => {
                syn::anaphor(o, "token");
                o.w("isn't");
                for a in v {
                    adj(o, a, None);
                }
            }
            Exception::Has {
                keywords,
                abilities,
            } => {
                syn::anaphor(o, "token");
                o.w("has");
                keyword_list(o, keywords);
                for (i, a) in abilities.iter().enumerate() {
                    if !keywords.is_empty() || i > 0 {
                        list_sep(
                            o,
                            keywords.len().min(1) + i,
                            keywords.len().min(1) + abilities.len(),
                            "and",
                        );
                    }
                    quoted(o, a);
                }
            }
            Exception::HasThisAbility => o.w("it has this ability"),
            Exception::EntersWithCounters { n, kind, cond } => {
                o.w("it enters with");
                let (b, extra) = match n {
                    Amount::Additional(b) => (&**b, true),
                    b => (b, false),
                };
                amount_a(o, b);
                if extra {
                    o.w("additional");
                }
                counter_noun(o, kind, amount_num(b));
                o.w("on it");
                if let Some(c) = cond {
                    o.w("if");
                    condition(o, c);
                }
            }
            Exception::NameIsSelf => {
                syn::its(o);
                o.w("name is ~");
            }
            Exception::AlsoIsSized { pt, types } => {
                syn::it_is(o);
                o.w("a");
                o.tok(Tok::Pt(*pt));
                for a in types {
                    adj(o, a, None);
                }
                o.w("in addition to");
                syn::its(o);
                o.w("other types");
            }
        }
    }
}

fn choice(o: &mut Out, c: &Choice) {
    match c {
        Choice::Color => o.w("a color"),
        Choice::OddOrEven => o.w("odd or even"),
        Choice::CreatureType => o.w("a creature type"),
        Choice::CardType => o.w("a card type"),
        Choice::CardName => o.w("a card name"),
        Choice::NonlandCardName => o.w("a nonland card name"),
        Choice::Number => o.w("a number"),
        Choice::Player => o.w("a player"),
        Choice::Opponent => o.w("an opponent"),
        Choice::Objects(x) => objects(o, x),
        Choice::CardTypeAmong(v) => {
            for (i, t) in v.iter().enumerate() {
                list_sep(o, i, v.len(), "or");
                o.w(t);
            }
        }
        Choice::LandCardName => o.w("a land card name"),
        Choice::TargetOpponent => o.w("target opponent"),
        Choice::TargetPlayer => o.w("target player"),
        Choice::OneOfEach { picks, among } => {
            for (i, f) in picks.iter().enumerate() {
                list_sep(o, i, picks.len(), "and");
                o.w("a");
                filter(o, f, Num::Sg);
            }
            from_among(o, among);
        }
    }
}

fn modal(o: &mut Out, choose: &Choose, modes: &[Vec<Effect>], cx: &Cx) {
    o.w("choose");
    choose_words(o, choose, modes.len());
    if !matches!(choose, Choose::Upgrade { .. }) {
        o.tok(Tok::Dash);
    }
    for m in modes {
        o.tok(Tok::Bullet);
        sentences_in_mode(o, m, cx);
    }
}

fn sentences_in_mode(o: &mut Out, m: &[Effect], cx: &Cx) {
    if let [Effect::Labelled { label, then }] = m {
        o.w(label);
        o.tok(Tok::Dash);
        sentences(o, then, cx);
    } else {
        sentences(o, m, cx);
    }
}

/// How many modes are chosen; `modes` says how many there are, since
/// "choose both" is two of two and "choose two" two of more.
fn choose_words(o: &mut Out, c: &Choose, modes: usize) {
    match c {
        Choose::One => o.w("one"),
        Choose::N(n) => syn::number(o, *n),
        Choose::OneOrBoth => o.w("one or both"),
        Choose::OneOrMore => o.w("one or more"),
        Choose::AnyNumber => o.w("any number"),
        Choose::UpTo(n) => {
            o.w("up to");
            syn::number(o, *n);
        }
        Choose::Upgrade {
            base,
            cond,
            to,
            may,
        } => {
            choose_words(o, base, modes);
            o.w(". if");
            condition(o, cond);
            o.w(",");
            if *may {
                o.w("you may");
            }
            o.w("choose");
            match **to {
                Choose::N(2) if modes == 2 => o.w("both"),
                ref t => choose_words(o, t, modes),
            }
            o.w("instead.");
        }
        Choose::NotChosenThisTurn(c) => {
            choose_words(o, c, modes);
            o.w("that hasn't been chosen this turn");
        }
    }
}

// ---------------------------------------------------------------- mana

fn mana_add(o: &mut Out, m: &ManaAdd) {
    match m {
        ManaAdd::Symbols(m) => mana(o, m),
        ManaAdd::Or(v) => {
            for (i, x) in v.iter().enumerate() {
                list_sep(o, i, v.len(), "or");
                mana(o, x);
            }
        }
        ManaAdd::Of { n, pick, within } if is_measure(n) => {
            o.w("an amount of mana");
            mana_pick(o, *pick, within);
            o.w("equal to");
            measure(o, n);
        }
        ManaAdd::Of { n, pick, within } => {
            amount_a(o, n);
            o.w("mana");
            mana_pick(o, *pick, within);
        }
        ManaAdd::AmountOf { mana: m, n } if is_measure(n) => {
            o.w("an amount of");
            mana(o, m);
            o.w("equal to");
            measure(o, n);
        }
        ManaAdd::AmountOf { mana: m, n } => {
            amount(o, n);
            mana(o, m);
        }
        ManaAdd::Additional(m) => {
            o.w("an additional");
            mana_add(o, m);
        }
    }
}

fn mana_pick(o: &mut Out, pick: Pick, within: &Option<Palette>) {
    match (pick, within) {
        (Pick::Stated, Some(p)) => {
            o.w("of");
            palette_named(o, p);
            return;
        }
        (Pick::ThatType, _) => {
            o.w("of that type");
            return;
        }
        (Pick::AnyColor, Some(Palette::ColorsOf(x))) => {
            let possessive = out(|o| {
                o.w("of any of");
                objects(o, x);
                o.w("'s colors");
            });
            let among = out(|o| {
                o.w("of any color among");
                objects(o, x);
            });
            syn::colors_of(o, possessive, among);
            return;
        }
        (Pick::AnyOneColor, _) => o.w("of any one color"),
        (Pick::AnyColor, _) => o.w("of any color"),
        (Pick::AnyType, _) => o.w("of any type"),
        (Pick::AnyCombination, _) => o.w("in any combination of colors"),
        (Pick::Stated, None) => {}
    }
    match within {
        Some(Palette::CommanderIdentity) => {
            o.w("in");
            commander_identity(o);
        }
        Some(Palette::CouldProduce(x)) => {
            o.w("that");
            objects(o, x);
            o.w("could produce");
        }
        Some(Palette::Produced(x)) => {
            objects(o, x);
            o.w("produced");
        }
        Some(Palette::ColorsOf(x)) => {
            o.w("among");
            objects(o, x);
        }
        Some(p) => palette_named(o, p),
        None => {}
    }
}

/// "your commander's color identity"; a partner pair is "your
/// commanders'", which is the same identity said of both.
fn commander_identity(o: &mut Out) {
    o.w("your");
    o.noun("commander", Num::Any);
    o.tok(Tok::Possessive);
    o.w("color identity");
}

fn palette_named(o: &mut Out, p: &Palette) {
    match p {
        Palette::Listed(cs) => {
            for (i, c) in cs.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                o.w(color_name(*c));
            }
        }
        Palette::Chosen => o.w("the chosen color"),
        Palette::That => o.w("that color"),
        Palette::CommanderIdentity => commander_identity(o),
        Palette::ColorsOf(x) => {
            objects(o, x);
            o.w("'s colors");
        }
        Palette::CouldProduce(x) => {
            objects(o, x);
            o.w("could produce");
        }
        Palette::ChoiceOf(_) => o.w("the color of your choice"),
        Palette::Produced(x) => {
            objects(o, x);
            o.w("produced");
        }
    }
}

// ---------------------------------------------------------------- triggers

fn trigger(o: &mut Out, t: &Trigger) {
    match t {
        Trigger::WhenAndAt(e, s) => {
            syn::when(o);
            event(o, e);
            o.w("and at the beginning of");
            step(o, s);
        }
        Trigger::At(s) => {
            o.w("at the beginning of");
            step(o, s);
        }
        Trigger::When(e) => {
            let one = out(|o| {
                syn::when(o);
                event(o, e);
            });
            // "When this creature enters and whenever an opponent draws …".
            match &**e {
                Event::Or(v) if v.len() == 2 => {
                    let two = out(|o| {
                        syn::when(o);
                        event(o, &v[0]);
                        o.w("and");
                        syn::when(o);
                        event(o, &v[1]);
                    });
                    syn::trigger_conditions(o, one, two);
                }
                _ => o.add(one),
            }
        }
        Trigger::InterveningIf(t, c) => {
            trigger(o, t);
            o.w(", if");
            condition(o, c);
        }
    }
}

fn step(o: &mut Out, s: &Step) {
    let (w, name) = match s {
        Step::Upkeep(w) => (w, "upkeep"),
        Step::DrawStep(w) => (w, "draw step"),
        Step::PrecombatMain(w) => (w, "precombat main phase"),
        Step::Combat(w) => (w, "combat"),
        Step::EndStep(w) => (w, "end step"),
        Step::EndOfCombat(w) => (w, "end of combat"),
        Step::SecondMain(w) => (w, "second main phase"),
        Step::PostcombatMain(w) => (w, "postcombat main phase"),
        Step::Main(w) => (w, "main phase"),
        Step::Untap(w) => (w, "untap step"),
    };
    if matches!(s, Step::Combat(Whose::Yours)) {
        o.w("combat on your turn");
        return;
    }
    if matches!(s, Step::Combat(Whose::EachOpponent)) {
        o.w("combat on each opponent's turn");
        return;
    }
    if matches!(s, Step::Main(Whose::Yours)) {
        syn::your_main_phase(o);
        return;
    }
    if *w == Whose::Each {
        syn::each_step(o);
        if matches!(s, Step::PrecombatMain(_)) {
            syn::precombat_main(o);
        } else {
            o.w(name);
        }
        return;
    }
    o.w(match w {
        Whose::Yours => "your",
        Whose::Each => "each",
        Whose::EachOpponent => "each opponent's",
        Whose::The => "the",
        Whose::Next => "the next",
        Whose::NextTurns => "the next turn's",
        Whose::ThatPlayers => "that player's",
        Whose::YourNext => "your next",
        Whose::YourFirst => "your first",
        Whose::EachOther => "each other player's",

        Whose::That => "that",
    });
    if matches!(s, Step::PrecombatMain(_)) {
        syn::precombat_main(o);
    } else {
        o.w(name);
    }
}

fn event(o: &mut Out, e: &Event) {
    match e {
        Event::Enters { what, under, state } => {
            obj_subj(o, what, "enter");
            syn::the_battlefield(o);
            enters_rest(o, under, state);
        }
        Event::Dies(x) => obj_subj(o, x, "die"),
        Event::AttacksAlone(x) => {
            obj_subj(o, x, "attack");
            o.w("alone");
        }
        Event::AttacksUnblocked { what, at } => {
            obj_subj(o, what, "attack");
            if let Some(r) = at {
                recipient(o, r);
            }
            o.w("and isn't blocked");
        }
        Event::TransformsInto { what, into } => {
            obj_subj(o, what, "transform");
            o.w("into");
            objects(o, into);
        }
        Event::Performs { who, action } => {
            player(o, who);
            o.verb(action, player_num(who));
        }
        Event::CastUsing { what, keyword } if keyword == "warp" => {
            objects(o, what);
            o.verb("is", obj_num(what));
            o.w("warped");
        }
        Event::CastUsing { what, keyword } => {
            objects(o, what);
            o.verb("is", obj_num(what));
            o.w("cast using");
            o.w(keyword);
        }
        Event::Leaves { what, from } => {
            obj_subj(o, what, "leave");
            zone(o, from);
        }
        Event::Attacks(x) => obj_subj(o, x, "attack"),
        Event::Blocks(x) => obj_subj(o, x, "block"),
        Event::AttacksOrBlocks(x) => obj_subj(o, x, "attack or block"),
        Event::BecomesBlocked(x) => {
            obj_subj(o, x, "become");
            o.w("blocked");
        }
        Event::DealsCombatDamageToPlayer(x) => {
            obj_subj(o, x, "deal");
            o.w("combat damage to a player");
        }
        Event::DealsCombatDamageTo { what, to } => {
            obj_subj(o, what, "deal");
            o.w("combat damage to");
            recipient(o, to);
        }
        Event::DealsDamage(x) => {
            obj_subj(o, x, "deal");
            o.w("damage");
        }
        Event::IsDealtDamage(x) => {
            obj_subj(o, x, "are");
            o.w("dealt damage");
        }
        Event::Casts { who, what } => {
            player(o, who);
            o.verb("cast", player_num(who));
            objects(o, what);
        }
        Event::GainsLife(p) => {
            player(o, p);
            o.verb("gain", player_num(p));
            o.w("life");
        }
        Event::LosesLife(p) => {
            player(o, p);
            o.verb("lose", player_num(p));
            o.w("life");
        }
        Event::Draws(p) => {
            player(o, p);
            o.verb("draw", player_num(p));
            o.w("a card");
        }
        Event::Discards { who, what } => {
            player(o, who);
            o.verb("discard", player_num(who));
            objects(o, what);
        }
        Event::Sacrifices { who, what } => {
            player(o, who);
            o.verb("sacrifice", player_num(who));
            objects(o, what);
        }
        Event::YouAttack => o.w("you attack"),
        Event::BecomesTapped(x) => {
            obj_subj(o, x, "become");
            o.w("tapped");
        }
        Event::BecomesTarget { what, of } => {
            obj_subj(o, what, "become");
            o.w("the target of");
            if let Some(of) = of {
                objects(o, of);
            }
        }
        Event::PutInto { what, to, from } => {
            obj_subj(o, what, "are");
            o.w("put into");
            zone(o, to);
            let from_words = out(|o| {
                o.w("from");
                zone(o, from);
            });
            if *from == Zone::Anywhere {
                syn::from_anywhere(o, from_words);
            } else {
                o.add(from_words);
            }
        }
        Event::Cycles { who, what } => {
            player(o, who);
            o.verb("cycle", player_num(who));
            if let Some(x) = what {
                objects(o, x);
            }
        }
        Event::CounterPutOn(x) => {
            o.w("a counter is put on");
            objects(o, x);
        }
        Event::PlaysLand(p) => {
            player(o, p);
            o.verb("play", player_num(p));
            o.w("a land");
        }
        Event::Or(v) => event_list(o, v),
        Event::Copies { who, what } => {
            player(o, who);
            o.verb("copy", player_num(who));
            objects(o, what);
        }
        Event::Creates { by, what, .. } => {
            match by {
                Actor::Player(p) => {
                    player(o, p);
                    o.verb("create", player_num(p));
                }
                Actor::AnEffect => o.w("an effect creates"),
                Actor::ThatEffect => o.w("it creates"),
                Actor::Passive => {
                    objects(o, what);
                    o.w("are created");
                    return;
                }
            }
            objects(o, what);
        }
        Event::CountersPut { by, n, kind, on } => {
            match by {
                Some(Actor::Player(p)) => {
                    player(o, p);
                    o.verb("put", player_num(p));
                    counters_phrase(o, n, kind);
                    o.w("on");
                }
                _ => {
                    counters_phrase(o, n, kind);
                    o.w(if quant_is_one(n) {
                        "is put on"
                    } else {
                        "are put on"
                    });
                }
            }
            objects(o, on);
        }
        Event::TappedForMana(x) => {
            obj_subj(o, x, "are");
            o.w("tapped for mana");
        }
        Event::DrawsNth { who, n } => {
            player(o, who);
            o.verb("draw", player_num(who));
            o.w(if *who == Player::You { "your" } else { "their" });
            o.w(ordinal_word(*n));
            o.w("card");
            syn::each_turn(o);
        }
        Event::ManaSpentToCast(x) => {
            o.w("that mana is spent to cast");
            objects(o, x);
        }
        Event::DrawsExceptFirstInDrawStep(p) => {
            player(o, p);
            o.verb("draw", player_num(p));
            o.w("a card except the first one");
            o.w(if *p == Player::You {
                "you draw in each of your draw steps"
            } else {
                "they draw in each of their draw steps"
            });
        }
        Event::Countered(x) => {
            obj_subj(o, x, "are");
            o.w("countered");
        }
        Event::Searches { who, zone: z } => {
            player(o, who);
            o.verb("search", player_num(who));
            zone(o, z);
        }
        Event::TapsForMana { who, what } => {
            player(o, who);
            o.verb("tap", player_num(who));
            objects(o, what);
            o.w("for mana");
        }
        Event::While { event: e, cond } => {
            event(o, e);
            match &**cond {
                Condition::ItsYourTurn => o.w("during your turn"),
                Condition::OpponentsTurn => o.w("during an opponent's turn"),
                // "attacks while saddled": the attacker, said once.
                Condition::Is { what, filter: f }
                    if matches!(&**e, Event::Attacks(x) if x == what)
                        && f.alts == [vec![Adj::State(State::Saddled)]] =>
                {
                    o.w("while saddled")
                }
                c => {
                    o.w("while");
                    condition(o, c);
                }
            }
        }
        Event::DealsDamageTo { what, to } => {
            obj_subj(o, what, "deal");
            o.w("damage to");
            recipients(o, to);
        }
        Event::BecomesLevel { what, n } => {
            obj_subj(o, what, "become");
            o.w("level");
            syn::number(o, *n);
        }
        Event::AttacksWith { who, what, at } => {
            player(o, who);
            o.verb("attack", player_num(who));
            if let Some(a) = at {
                player(o, a);
            }
            o.w("with");
            objects(o, what);
        }
        Event::PlayerAttacks { who, at } => {
            player(o, who);
            o.verb("attack", player_num(who));
            if let Some(a) = at {
                player(o, a);
            }
        }
        Event::BeginsExtraTurn(p) => {
            player(o, p);
            o.verb("begin", player_num(p));
            o.w("an extra turn");
        }
        Event::NthCounterPutOn { n, kind, on } => {
            o.w("the");
            o.w(ordinal_word(*n));
            counter_kind(o, kind);
            o.w("counter is put on");
            objects(o, on);
        }
        Event::IsAttacked(_) => o.w("enchanted player is attacked"),
        Event::Puts { who, what, to } => {
            player(o, who);
            o.verb("put", player_num(who));
            objects(o, what);
            put_to(o, to, false, false, &None);
        }
        Event::Exiles { who, what } => {
            player(o, who);
            o.verb("exile", player_num(who));
            objects(o, what);
        }
        Event::DealsNoncombatDamageTo { what, to } => {
            obj_subj(o, what, "deal");
            o.w("noncombat damage to");
            recipients(o, to);
        }
        Event::FirstTimeEachTurn(e) => {
            event(o, e);
            o.w("for the first time each turn");
        }
        Event::TapsFor { who, what, mana: m } => {
            player(o, who);
            o.verb("tap", player_num(who));
            objects(o, what);
            o.w("for");
            mana(o, m);
        }
        Event::AddsMana {
            who,
            by_ability_of,
            of,
        } => {
            objects(o, by_ability_of);
            o.w("'s ability causes");
            player(o, who);
            o.w("to add one or more mana of");
            palette_named(o, of);
        }
        Event::WasCast(x) => {
            objects(o, x);
            o.w("was cast");
        }
        // Read only in "… this turn", so said in the past.
        Event::LeftUnderControlOf { what, who } => {
            objects(o, what);
            o.w("left the battlefield under");
            whose(o, who);
            o.w("control");
        }
        Event::DealsCombatDamage(x) => {
            obj_subj(o, x, "deal");
            o.w("combat damage");
        }
        Event::Exiled(x) => {
            obj_subj(o, x, "are");
            o.w("exiled");
        }
        Event::ReturnedToHand(x) => {
            obj_subj(o, x, "are");
            o.w("returned to hand");
        }
        Event::AttacksAnyOf { what, to } => {
            obj_subj(o, what, "attack");
            for (i, r) in to.iter().enumerate() {
                if i > 0 {
                    syn::list_conj(o, false);
                }
                recipient(o, r);
            }
        }
        Event::Activates { who, what } => {
            player(o, who);
            o.verb("activate", player_num(who));
            objects(o, what);
        }
        Event::TappedForAtLeast { what, n } => {
            obj_subj(o, what, "are");
            o.w("tapped for");
            syn::number(o, *n);
            o.w("or more mana");
        }
        Event::AndIf { event: e, cond } => {
            would(o, e);
            o.w("and");
            condition(o, cond);
        }
        Event::DrawsOneOrMore(p) => {
            player(o, p);
            o.verb("draw", player_num(p));
            o.w("one or more cards");
        }
        Event::CastFrom { what, zone: z } => {
            obj_subj(o, what, "are");
            o.w("cast from");
            zone(o, z);
        }
    }
}

/// Whom or what an event is about, rendered first.
#[derive(PartialEq)]
enum Subject<'a> {
    Obj(&'a Objects),
    Player(&'a Player),
}

fn event_subject(e: &Event) -> Option<Subject<'_>> {
    Some(match e {
        Event::Enters { what, .. }
        | Event::Leaves { what, .. }
        | Event::BecomesTarget { what, .. }
        | Event::PutInto { what, .. }
        | Event::DealsDamageTo { what, .. }
        | Event::DealsNoncombatDamageTo { what, .. }
        | Event::BecomesLevel { what, .. }
        | Event::AttacksUnblocked { what, .. }
        | Event::TransformsInto { what, .. } => Subject::Obj(what),
        Event::Dies(x)
        | Event::Attacks(x)
        | Event::AttacksAlone(x)
        | Event::Blocks(x)
        | Event::AttacksOrBlocks(x)
        | Event::BecomesBlocked(x)
        | Event::DealsCombatDamageToPlayer(x)
        | Event::DealsCombatDamageTo { what: x, .. }
        | Event::DealsDamage(x)
        | Event::IsDealtDamage(x)
        | Event::BecomesTapped(x)
        | Event::TappedForMana(x)
        | Event::Countered(x) => Subject::Obj(x),
        Event::Casts { who, .. }
        | Event::Copies { who, .. }
        | Event::Sacrifices { who, .. }
        | Event::Discards { who, .. }
        | Event::Cycles { who, .. }
        | Event::DrawsNth { who, .. }
        | Event::Searches { who, .. }
        | Event::TapsForMana { who, .. }
        | Event::TapsFor { who, .. }
        | Event::Puts { who, .. }
        | Event::Exiles { who, .. }
        | Event::AttacksWith { who, .. }
        | Event::Performs { who, .. }
        | Event::Creates {
            by: Actor::Player(who),
            ..
        }
        | Event::CountersPut {
            by: Some(Actor::Player(who)),
            ..
        } => Subject::Player(who),
        Event::GainsLife(p)
        | Event::LosesLife(p)
        | Event::Draws(p)
        | Event::PlaysLand(p)
        | Event::DrawsExceptFirstInDrawStep(p) => Subject::Player(p),
        _ => return None,
    })
}

/// A player's verb that takes an object, for "cast or copy an instant or
/// sorcery spell": the verbs listed, the object said once.
fn verb_on(e: &Event) -> Option<(&Player, &'static str, &Objects)> {
    Some(match e {
        Event::Casts { who, what } => (who, "cast", what),
        Event::Copies { who, what } => (who, "copy", what),
        Event::Sacrifices { who, what } => (who, "sacrifice", what),
        Event::Discards { who, what } => (who, "discard", what),
        Event::Cycles {
            who,
            what: Some(what),
        } => (who, "cycle", what),
        Event::Activates { who, what } => (who, "activate", what),
        Event::Creates {
            by: Actor::Player(who),
            what,
            under: None,
        } => (who, "create", what),
        _ => return None,
    })
}

/// "enters or attacks", "attacks you with two or more creatures, draws
/// their second card each turn, or casts their second spell each turn":
/// a subject said once for several verbs, or several events in full.
fn event_list(o: &mut Out, v: &[Event]) {
    let verbs: Option<Vec<_>> = v.iter().map(verb_on).collect();
    if let Some(vs) = verbs {
        if vs.iter().all(|(w, _, x)| *w == vs[0].0 && *x == vs[0].2) {
            let (who, _, what) = vs[0];
            player(o, who);
            for (i, (_, verb, _)) in vs.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                o.verb(verb, player_num(who));
            }
            objects(o, what);
            return;
        }
    }
    let first = event_subject(&v[0]);
    let shared = first.is_some() && v.iter().all(|e| event_subject(e) == first);
    for (i, e) in v.iter().enumerate() {
        if i > 0 {
            syn::or_sep(o, i, v.len());
        }
        let full = out(|o| event(o, e));
        match (shared, i, &first) {
            (true, 1.., Some(s)) => {
                let said = out(|o| match s {
                    Subject::Obj(x) => objects(o, x),
                    Subject::Player(p) => player(o, p),
                });
                let pred = Out(full.0[said.0.len()..].to_vec());
                syn::subject(o, said, pred.clone(), pred);
            }
            _ => o.add(full),
        }
    }
}

fn enters_rest(o: &mut Out, under: &Option<Player>, state: &Option<State>) {
    if let Some(w) = under {
        o.w("under");
        whose(o, w);
        o.w("control");
    }
    match state {
        Some(State::Untapped) => o.w("untapped"),
        Some(State::Tapped) => o.w("tapped"),
        _ => {}
    }
}

fn quant_is_one(q: &Quant) -> bool {
    matches!(q, Quant::One | Quant::N(Amount::N(1)))
}

fn counters_phrase(o: &mut Out, n: &Quant, kind: &Option<String>) {
    let x = Objects {
        quant: n.clone(),
        filter: Filter::default(),
    };
    let num = obj_num(&x);
    match n {
        Quant::One => o.w("a"),
        Quant::OneOrMore => o.w("one or more"),
        q => objects(
            o,
            &Objects {
                quant: q.clone(),
                filter: Filter::default(),
            },
        ),
    }
    if let Some(k) = kind {
        counter_kind(o, k);
    }
    o.noun("counter", num);
}

/// What a replacement effect watches for, said with "would".
fn would(o: &mut Out, e: &Event) {
    match e {
        Event::Creates { by, what, under } => {
            match by {
                Actor::Passive => {
                    objects(o, what);
                    o.w("would be created");
                }
                by => {
                    match by {
                        Actor::Player(p) => player(o, p),
                        Actor::AnEffect => o.w("an effect"),
                        _ => o.w("it"),
                    }
                    o.w("would create");
                    objects(o, what);
                }
            }
            if let Some(w) = under {
                o.w("under");
                whose(o, w);
                o.w("control");
            }
        }
        Event::CountersPut { by, n, kind, on } => match by {
            Some(by) => {
                match by {
                    Actor::Player(p) => player(o, p),
                    Actor::AnEffect => o.w("an effect"),
                    _ => o.w("it"),
                }
                o.w("would put");
                counters_phrase(o, n, kind);
                o.w("on");
                objects(o, on);
            }
            None => {
                counters_phrase(o, n, kind);
                o.w("would be put on");
                objects(o, on);
            }
        },
        Event::Draws(p) => {
            player(o, p);
            o.w("would draw a card");
        }
        Event::DrawsOneOrMore(p) => {
            player(o, p);
            o.w("would draw one or more cards");
        }
        Event::DrawsExceptFirstInDrawStep(p) => {
            player(o, p);
            o.w("would draw a card except the first one");
            o.w(if *p == Player::You {
                "you draw in each of your draw steps"
            } else {
                "they draw in each of their draw steps"
            });
        }
        Event::GainsLife(p) => {
            player(o, p);
            o.w("would gain life");
        }
        Event::LosesLife(p) => {
            player(o, p);
            o.w("would lose life");
        }
        Event::BeginsExtraTurn(p) => {
            player(o, p);
            o.w("would begin an extra turn");
        }
        Event::Dies(x) => {
            objects(o, x);
            o.w("would die");
        }
        Event::PutInto { what, to, from } => {
            objects(o, what);
            o.w("would be put into");
            zone(o, to);
            let from_words = out(|o| {
                o.w("from");
                zone(o, from);
            });
            if *from == Zone::Anywhere {
                syn::from_anywhere(o, from_words);
            } else {
                o.add(from_words);
            }
        }
        Event::Leaves { what, .. } => {
            objects(o, what);
            o.w("would leave the battlefield");
        }
        Event::DealsDamageTo { what, to } => {
            objects(o, what);
            o.w("would deal damage to");
            recipients(o, to);
        }
        Event::DealsNoncombatDamageTo { what, to } => {
            objects(o, what);
            o.w("would deal noncombat damage to");
            recipients(o, to);
        }
        Event::DealsDamage(x) => {
            objects(o, x);
            o.w("would deal damage");
        }
        Event::IsDealtDamage(x) => {
            objects(o, x);
            o.w("would be dealt damage");
        }
        Event::Enters { what, .. } => {
            objects(o, what);
            o.w("would enter");
            syn::the_battlefield(o);
        }
        Event::While { event: e, cond } => {
            would(o, e);
            match &**cond {
                Condition::ItsYourTurn => o.w("during your turn"),
                c => {
                    o.w("while");
                    condition(o, c);
                }
            }
        }
        e => event(o, e),
    }
}

// ---------------------------------------------------------------- conditions

fn condition(o: &mut Out, c: &Condition) {
    match c {
        Condition::YouControl(x) => {
            o.w("you control");
            objects(o, x);
        }
        Condition::YouDontControl(x) => {
            let no = out(|o| {
                o.w("you control no");
                objects(
                    o,
                    &Objects {
                        quant: Quant::Plural,
                        ..x.clone()
                    },
                );
            });
            let dont = out(|o| {
                o.w("you don't control");
                objects(o, x);
            });
            syn::control_no(o, no, dont);
        }
        Condition::ItsYourTurn => o.w("it's your turn"),
        Condition::DoneThisWay { what, how } => {
            objects(o, what);
            syn::passive_be(o, obj_num(what));
            o.w(marked_word(*how));
            o.w("this way");
        }
        Condition::FirstCombatPhase { your_turn } => {
            o.w("it's the first combat phase of");
            o.w(if *your_turn { "your turn" } else { "the turn" });
        }
        Condition::NotYourTurn => o.w("it's not your turn"),
        Condition::OpponentsTurn => o.w("it's an opponent's turn"),
        Condition::LifeAtLeast(n) => {
            o.w("you have");
            syn::number(o, *n);
            o.w("or more life");
        }
        Condition::LifeAtMost(n) => {
            o.w("you have");
            syn::number(o, *n);
            o.w("or less life");
        }
        Condition::CardsInHandAtMost(n) => {
            o.w("you have");
            syn::number(o, *n);
            o.w("or fewer cards in hand");
        }
        Condition::Is { what, filter: f } => {
            pronoun_or(
                o,
                what,
                |o| o.w("it's"),
                |o| {
                    objects(o, what);
                    o.verb("are", obj_num(what));
                },
            );
            is_predicate(o, f);
        }
        Condition::WasKicked => {
            syn::kicked(o);
            o.w("was kicked");
        }
        Condition::Threshold(n, x) => {
            amount(o, n);
            o.w("or more");
            objects(o, x);
        }
        Condition::YouAttackedThisTurn => o.w("you attacked this turn"),
        Condition::Opponent(c) => match &**c {
            Condition::YouControl(x) => {
                o.w("an opponent controls");
                objects(o, x);
            }
            c => condition(o, c),
        },
        Condition::OpponentsAtLeast(n) => {
            o.w("you have");
            syn::number(o, *n);
            o.w("or more opponents");
        }
        Condition::PlayerControls(p, x) => {
            player(o, p);
            o.verb("control", player_num(p));
            objects(o, x);
        }
        Condition::PlayerHas {
            who,
            at_least,
            what,
        } => {
            player(o, who);
            o.verb("have", player_num(who));
            syn::number(o, *at_least);
            o.w("or more");
            // "cards in their graveyard": the zone is the subject's own.
            match what {
                Holding::Life => o.w("life"),
                Holding::CardsInHand => {
                    o.w("cards in");
                    syn::own(o, who);
                    o.w("hand");
                }
                Holding::CardsInGraveyard => {
                    o.w("cards in");
                    o.w(if *who == Player::You { "your" } else { "their" });
                    o.w("graveyard");
                }
                Holding::Opponents => o.w("opponents"),
            }
        }
        Condition::YouCastIt(what) => {
            o.w("you cast");
            pronoun_or(o, what, syn::it, |o| objects(o, what));
        }
        Condition::YouCastItFrom { what, zone: z } => {
            o.w("you cast");
            pronoun_or(o, what, syn::it, |o| objects(o, what));
            o.w("from");
            zone(o, z);
        }
        Condition::NoSpellsCastLastTurn => o.w("no spells were cast last turn"),
        Condition::TwoSpellsCastByAPlayerLastTurn => {
            o.w("a player cast two or more spells last turn")
        }
        Condition::Not(c) => not_condition(o, c),
        Condition::And(v) => {
            if v.iter().all(|c| matches!(c, Condition::YouControl(_))) {
                o.w("you control");
                for (i, c) in v.iter().enumerate() {
                    if i > 0 {
                        o.w("and");
                    }
                    if let Condition::YouControl(x) = c {
                        objects(o, x);
                    }
                }
                return;
            }
            // "enchanted creature is white and untapped": one subject said
            // once for both predicates.
            if let [Condition::Is { what: a, filter: f }, Condition::Is { what: b, filter: g }] =
                &v[..]
            {
                if a == b {
                    let said = out(|o| {
                        objects(o, a);
                        o.verb("are", obj_num(a));
                        filter(o, f, Num::Sg);
                        o.w("and");
                        filter(o, g, Num::Sg);
                    });
                    let full = out(|o| {
                        condition(o, &v[0]);
                        o.w("and");
                        condition(o, &v[1]);
                    });
                    syn::said_again(o, said, full);
                    return;
                }
            }
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                condition(o, c);
            }
        }
        Condition::HasCounter { what, kind } => {
            objects(o, what);
            o.verb("have", obj_num(what));
            match kind {
                Some(k) => {
                    // "has a loyalty counter on it", "has one or more
                    // loyalty counters on him".
                    let a = out(|o| {
                        o.w("a");
                        counter_kind(o, k);
                        o.w("counter");
                    });
                    let more = out(|o| {
                        o.w("one or more");
                        counter_kind(o, k);
                        o.w("counters");
                    });
                    syn::a_counter(o, a, more);
                    o.w("on");
                    syn::it(o);
                }
                None => o.w("counters on it"),
            }
        }
        Condition::In {
            what,
            zone: Zone::Exile,
        } => {
            objects(o, what);
            o.verb("remain exiled", obj_num(what));
        }
        Condition::In {
            what,
            zone: Zone::Battlefield,
        } if !is_bare_antecedent(what) => {
            // "this creature remains on the battlefield": a permanent that
            // leaves is a new object (CR 400.7), so being on it is remaining.
            pronoun_or(
                o,
                what,
                |o| o.w("it's"),
                |o| {
                    objects(o, what);
                    let is = out(|o| o.verb("are", obj_num(what)));
                    let remains = out(|o| o.verb("remain", obj_num(what)));
                    syn::remains(o, is, remains);
                },
            );
            o.w("on the battlefield");
        }
        Condition::In { what, zone: z } => {
            pronoun_or(
                o,
                what,
                |o| o.w("it's"),
                |o| {
                    objects(o, what);
                    o.verb("are", obj_num(what));
                },
            );
            o.w(if *z == Zone::Battlefield { "on" } else { "in" });
            zone(o, z);
        }
        Condition::InOpeningHand(x) => {
            objects(o, x);
            o.verb("are", obj_num(x));
            o.w("in your opening hand");
        }
        Condition::YouAreStartingPlayer => o.w("you're the starting player"),
        Condition::DidThisTurn(e) => match &**e {
            // "if you cycled two or more cards this turn".
            Event::Cycles {
                who,
                what: Some(what),
            } => {
                player(o, who);
                syn::did_tense(o, "cycled", out(|o| o.verb("cycle", player_num(who))));
                objects(o, what);
                o.w("this turn");
            }
            Event::Dies(x) => {
                let said = out(|o| {
                    objects(o, x);
                    o.w("died this turn");
                });
                match x.filter.posts.split_last() {
                    Some((Post::Controller(Player::You), rest)) => {
                        let mut without = x.clone();
                        without.filter.posts = rest.to_vec();
                        let under = out(|o| {
                            objects(o, &without);
                            o.w("died under your control this turn");
                        });
                        syn::died_under(o, said, under);
                    }
                    _ => o.add(said),
                }
            }
            Event::Leaves {
                what,
                from: Zone::Battlefield,
            } => {
                objects(o, what);
                o.w("left the battlefield this turn");
            }
            Event::Sacrifices { who, what } => {
                player(o, who);
                syn::did_tense(
                    o,
                    "sacrificed",
                    out(|o| o.verb("sacrifice", player_num(who))),
                );
                objects(o, what);
                o.w("this turn");
            }
            Event::Leaves { what, from } => {
                objects(o, what);
                o.w("left");
                zone(o, from);
                o.w("this turn");
            }
            Event::CastUsing { what, keyword } if keyword == "warp" => {
                objects(o, what);
                o.w("was warped this turn");
            }
            Event::Creates { what, .. } => {
                o.w("you created");
                objects(o, what);
                o.w("this turn");
            }
            Event::Casts { who, what } => {
                if *who == Player::You {
                    syn::you_have_cast(o);
                } else {
                    player(o, who);
                    syn::has_cast(o);
                }
                objects(o, what);
                o.w("this turn");
            }
            // "you had a land enter the battlefield under your control this
            // turn".
            Event::Enters {
                what,
                under: Some(Player::You),
                ..
            } => {
                let had = out(|o| {
                    o.w("you had");
                    objects(o, what);
                    o.w("enter the battlefield under your control this turn");
                });
                let entered = out(|o| {
                    objects(o, what);
                    o.w("entered the battlefield under your control this turn");
                });
                syn::had_enter(o, had, entered);
            }
            // "this land entered this turn", "a creature entered the
            // battlefield under an opponent's control this turn".
            Event::Enters { what, under, .. } => {
                objects(o, what);
                o.w("entered");
                match under {
                    Some(w) => {
                        o.w("the battlefield under");
                        whose(o, w);
                        o.w("control");
                    }
                    None => syn::the_battlefield(o),
                }
                o.w("this turn");
            }
            // "you gained life this turn", "an opponent searched their
            // library this turn".
            Event::GainsLife(who) => {
                player(o, who);
                syn::did_tense(o, "gained", out(|o| o.verb("gain", player_num(who))));
                o.w("life this turn");
            }
            Event::LosesLife(who) => {
                player(o, who);
                syn::did_tense(o, "lost", out(|o| o.verb("lose", player_num(who))));
                o.w("life this turn");
            }
            Event::Searches { who, zone: z } => {
                player(o, who);
                syn::did_tense(o, "searched", out(|o| o.verb("search", player_num(who))));
                zone(o, z);
                o.w("this turn");
            }
            e => {
                event(o, e);
                o.w("this turn");
            }
        },
        Condition::Total { what, cmp } => {
            objects(o, what);
            o.w("have total");
            compare_words(o, cmp);
        }
        Condition::ControlsMore { who, what, than } => {
            player(o, who);
            o.verb("control", player_num(who));
            o.w("more");
            filter(o, what, Num::Pl);
            o.w("than");
            player(o, than);
        }
        Condition::AsYouCastIt(c) => {
            condition(o, c);
            o.w("as you cast this spell");
        }
        Condition::ThisWay(e) => match &**e {
            Event::WasCast(x) => {
                objects(o, x);
                o.w("was cast this way");
            }
            Event::Sacrifices { who, what } => {
                player(o, who);
                syn::did_tense(
                    o,
                    "sacrificed",
                    out(|o| o.verb("sacrifice", player_num(who))),
                );
                objects(o, what);
                o.w("this way");
            }
            // "you exiled four or more cards this way".
            Event::Exiles { who, what } => {
                player(o, who);
                syn::did_tense(o, "exiled", out(|o| o.verb("exile", player_num(who))));
                objects(o, what);
                o.w("this way");
            }
            e => {
                event(o, e);
                o.w("this way");
            }
        },
        Condition::AmountIs { n, op, value } => amount_is(o, n, *op, value),
        Condition::SameName { what, as_any_of } => {
            objects(o, what);
            o.verb("have", obj_num(what));
            o.w("the same name as");
            for (i, x) in as_any_of.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                objects(o, x);
            }
        }
        Condition::CastFrom { what, zone: z } => {
            objects(o, what);
            o.w("was cast from");
            zone(o, z);
        }
        Condition::AttackedWithThisTurn(x) => {
            o.w("you attacked with");
            objects(o, x);
            o.w("this turn");
        }
        Condition::GiftPromised => o.w("the gift was promised"),
        Condition::CastDuring(s) => {
            o.w("you cast this spell during");
            step(o, s);
        }
        Condition::Searching { who, zone: z } => {
            if *who == Player::That {
                o.w("they're");
            } else {
                player(o, who);
                o.verb("are", player_num(who));
            }
            o.w("searching");
            if let Zone::Library(w) = z {
                whose(o, w);
            }
            o.noun("library", player_num(who));
        }
        Condition::AttachedTo { what, to } => {
            objects(o, what);
            o.verb("are", obj_num(what));
            o.w("attached to");
            objects(o, to);
        }
        Condition::YourNthTurn(v) => {
            o.w("it's your");
            for (i, n) in v.iter().enumerate() {
                list_sep(o, i, v.len(), "or");
                o.w(ordinal_word(*n));
            }
            o.w("turn of the game");
        }
        Condition::InStep(s) => {
            o.w("it's");
            step(o, s);
        }
        Condition::NoManaSpentToCast(x) => {
            o.w("no mana was spent to cast");
            objects(o, x);
        }
        Condition::WasBargained => o.w("this spell was bargained"),
        Condition::Past(c) => past(o, c),
        Condition::PlayerHasAtMost { who, at_most, what } => {
            player(o, who);
            o.verb("have", player_num(who));
            syn::number(o, *at_most);
            // Life is a mass noun, cards are counted.
            match what {
                Holding::Life => o.w("or less life"),
                Holding::CardsInHand => {
                    o.w("or fewer cards in");
                    syn::own(o, who);
                    o.w("hand");
                }
                Holding::CardsInGraveyard => o.w("or fewer cards in their graveyard"),
                Holding::Opponents => o.w("or fewer opponents"),
            }
        }
        Condition::Has { what, cmp } => {
            objects(o, what);
            o.verb("have", obj_num(what));
            compare_words(o, cmp);
        }
        Condition::ManaSpent { mana: m, what } => {
            mana(o, m);
            o.w("was spent to cast");
            objects(o, what);
        }
        Condition::CitysBlessing(p) => {
            player(o, p);
            o.verb("have", player_num(p));
            o.w("the city's blessing");
        }
        Condition::Beheld(x) => {
            objects(o, x);
            o.w("was beheld");
        }
        Condition::EnduringStory(p) => {
            player(o, p);
            o.verb("have", player_num(p));
            o.w("an enduring story");
        }
        Condition::CastUsing { what, keyword } => {
            objects(o, what);
            o.w("was cast using");
            o.w(keyword);
        }
        Condition::MaxSpeed(p) => {
            player(o, p);
            o.verb("have", player_num(p));
            o.w("max speed");
        }
        Condition::ZoneHasCards { zone: z, op, n } => {
            zone(o, z);
            o.w("has");
            syn::number(o, *n);
            o.w(if *op == Op::Ge { "or more" } else { "or fewer" });
            o.w("cards in it");
        }
        Condition::NthSpellThisTurn {
            what,
            n,
            spell,
            who,
        } => {
            pronoun_or(o, what, |o| o.w("it's"), |o| obj_subj(o, what, "are"));
            o.w("the");
            o.w(ordinal_word(*n));
            filter(o, spell, Num::Sg);
            player(o, who);
            o.verb("cast", player_num(who));
            o.w("this turn");
        }
        Condition::Targets { what, target } => {
            obj_subj(o, what, "target");
            objects(o, target);
        }
        Condition::NthResolution(n) => {
            let full = out(|o| {
                o.w("this is the");
                o.w(ordinal_word(*n));
                o.w("time this ability has resolved this turn");
            });
            let short = out(|o| {
                o.w("it's the");
                o.w(ordinal_word(*n));
                o.w("time");
            });
            syn::resolved_again(o, full, short);
        }
        Condition::CostPaid(m) => {
            o.w("the");
            mana(o, m);
            o.w("cost was paid");
        }
        Condition::AdditionalCostPaid => o.w("this spell's additional cost was paid"),
        Condition::FaceDown(x) => {
            obj_subj(o, x, "are");
            o.w("face down");
        }
        Condition::WonFlip(won) => o.w(if *won {
            "you win the flip"
        } else {
            "you lose the flip"
        }),
        Condition::KickedWith(m) => {
            o.w("it was kicked with its");
            mana(o, m);
            o.w("kicker");
        }
        Condition::Or(v) => {
            // "it entered from your library or was cast from your library":
            // one subject, said once.
            for (i, c) in v.iter().enumerate() {
                if i > 0 {
                    syn::or_if(o);
                }
                let full = out(|o| condition(o, c));
                match (i, c) {
                    (1.., Condition::CastFrom { what, zone: z }) => {
                        let said = out(|o| objects(o, what));
                        let pred = out(|o| {
                            o.w("was cast from");
                            zone(o, z);
                        });
                        syn::subject(o, said, pred.clone(), pred);
                    }
                    _ => o.add(full),
                }
            }
        }
        Condition::Entered { what, from, under } => {
            objects(o, what);
            o.w("entered");
            if let Some(z) = from {
                o.w("from");
                zone(o, z);
            }
            if let Some(w) = under {
                o.w("under");
                whose(o, w);
                o.w("control");
            }
        }
        Condition::WasCast(x) => {
            objects(o, x);
            o.w("was cast");
        }
        Condition::ThereAre(v) => {
            // The zone each is in is said once, after the last.
            syn::there_is(o);
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                if i + 1 < v.len() {
                    let mut bare = x.clone();
                    bare.filter.posts.pop();
                    objects(o, &bare);
                } else {
                    objects(o, x);
                }
            }
        }
        Condition::OwnAndControl(v) => {
            o.w("you both own and control");
            for (i, x) in v.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                objects(o, x);
            }
        }
        Condition::DamagePreventedThisWay => o.w("damage is prevented this way"),
        Condition::HasMore { who, what, than } => {
            player(o, who);
            o.verb("have", player_num(who));
            o.w("more");
            match what {
                Holding::Life => o.w("life"),
                Holding::CardsInHand => o.w("cards in hand"),
                Holding::CardsInGraveyard => o.w("cards in their graveyard"),
                Holding::Opponents => o.w("opponents"),
            }
            o.w("than");
            player(o, than);
        }
        Condition::DidThisGame(e) => {
            if let Event::Casts { what, .. } = &**e {
                o.w("you've cast");
                objects(o, what);
            }
            o.w("this game");
        }
    }
}

/// What follows "is": "a land card", "tapped", "attacking you and/or
/// planeswalkers you control". An article only before a noun.
fn is_predicate(o: &mut Out, f: &Filter) {
    if let ([], None, [Post::AttackingAnyOf(v)]) = (&f.alts[..], &f.kind, &f.posts[..]) {
        o.w("attacking");
        for (i, r) in v.iter().enumerate() {
            if i > 0 {
                o.w("and/or");
            }
            recipient(o, r);
        }
        return;
    }
    if f.alts.is_empty() && f.kind.is_none() {
        // "it wasn't put onto the battlefield with this ability".
        for q in &f.posts {
            match q {
                Post::PutOntoBattlefieldWith(x) => {
                    o.w("put onto the battlefield with");
                    objects(o, x);
                }
                q => post(o, q, Num::Sg),
            }
        }
        return;
    }
    if has_noun(f) {
        o.w("a");
    }
    filter(o, f, Num::Sg);
}

/// Whether a filter ends in a noun, which takes an article: "a land card",
/// not "a tapped".
fn has_noun(f: &Filter) -> bool {
    f.kind.is_some() || f.alts.last().and_then(|g| g.last()).is_some_and(is_typed)
}

/// Types said as a predicate: "a Forest land", "Forest lands", "legendary".
fn type_words(o: &mut Out, types: &[Adj], n: Num) {
    let noun = types.last().is_some_and(is_typed);
    if noun && n != Num::Pl {
        o.w("a");
    }
    for (i, a) in types.iter().enumerate() {
        let last = i + 1 == types.len();
        adj(o, a, (last && noun).then_some(n));
    }
}

/// A condition as it held when its event happened: "if it was a
/// creature", "if it had counters on it".
fn past(o: &mut Out, c: &Condition) {
    match c {
        Condition::Is { what, filter: f } => {
            objects(o, what);
            o.verb("were", obj_num(what));
            is_predicate(o, f);
        }
        Condition::Not(inner) => match &**inner {
            Condition::Is { what, filter: f } => {
                objects(o, what);
                o.verb("weren't", obj_num(what));
                is_predicate(o, f);
            }
            c => condition(o, c),
        },
        Condition::HasCounter { what, kind } => {
            objects(o, what);
            o.w("had");
            match kind {
                Some(k) => {
                    o.w("a");
                    counter_kind(o, k);
                    o.w("counter on it");
                }
                None => {
                    syn::any_counters(o);
                    o.w("on it");
                }
            }
        }
        Condition::YouControl(x) => {
            o.w("you controlled");
            objects(o, x);
        }
        // "if its mana value was 2 or less".
        Condition::AmountIs { n, op, value } => {
            measure(o, n);
            o.w("was");
            comparison(o, *op, value);
        }
        c => condition(o, c),
    }
}

fn not_condition(o: &mut Out, c: &Condition) {
    match c {
        // "this enchantment has no charge counters on it", "there are no
        // charge counters on this Vehicle".
        Condition::HasCounter { what, kind } => {
            let kind_word = |o: &mut Out| {
                if let Some(k) = kind {
                    counter_kind(o, k);
                }
            };
            let has = out(|o| {
                objects(o, what);
                o.verb("have", obj_num(what));
                o.w("no");
                kind_word(o);
                o.w("counters on");
                syn::it(o);
            });
            let there = out(|o| {
                o.w("there are no");
                kind_word(o);
                o.w("counters on");
                objects(o, what);
            });
            syn::there_are_on(o, has, there);
        }
        Condition::ThisWay(e) => match &**e {
            Event::WasCast(x) => {
                objects(o, x);
                o.w("wasn't cast this way");
            }
            Event::Casts { who, what } | Event::Copies { who, what } => {
                player(o, who);
                o.w("didn't");
                o.w(if matches!(**e, Event::Casts { .. }) {
                    "cast"
                } else {
                    "copy"
                });
                objects(o, what);
                o.w("this way");
            }
            Event::Creates {
                by: Actor::Player(p),
                what,
                ..
            } => {
                player(o, p);
                o.w("didn't create");
                objects(o, what);
                o.w("this way");
            }
            e => {
                o.w("not");
                event(o, e);
            }
        },
        Condition::Is { what, filter: f } => {
            objects(o, what);
            o.verb("aren't", obj_num(what));
            is_predicate(o, f);
        }
        Condition::YouAreStartingPlayer => o.w("you're not the starting player"),
        Condition::CastFrom { what, zone: z } => {
            objects(o, what);
            o.w("wasn't cast from");
            zone(o, z);
        }
        Condition::WasCast(x) => {
            objects(o, x);
            o.w("wasn't cast");
        }
        Condition::DidThisTurn(e) => {
            if let Event::Casts { who, what } = &**e {
                player(o, who);
                syn::hasnt_cast(o);
                objects(o, what);
                o.w("this turn");
            }
        }
        Condition::SameName { what, as_any_of } => {
            objects(o, what);
            o.w("doesn't have the same name as");
            for (i, x) in as_any_of.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                objects(o, x);
            }
        }
        c => {
            o.w("not");
            condition(o, c);
        }
    }
}

fn amount_is(o: &mut Out, n: &Amount, op: Op, value: &Amount) {
    match n {
        Amount::CardsDrawnThisTurn(Player::You) if op == Op::Ge => {
            o.w("you've drawn");
            amount(o, value);
            o.w("or more cards this turn");
        }
        Amount::LifeLostThisTurn(p)
        | Amount::LifeGainedThisTurn(p)
        | Amount::DamageDealtThisTurn(p) => {
            match n {
                Amount::LifeLostThisTurn(_) => life_subject(o, p, "lost"),
                Amount::LifeGainedThisTurn(_) => life_subject(o, p, "gained"),
                _ => {
                    player(o, p);
                    o.w("was dealt");
                }
            }
            amount(o, value);
            o.w(if op == Op::Ge { "or more" } else { "or less" });
            o.w(if matches!(n, Amount::DamageDealtThisTurn(_)) {
                "damage"
            } else {
                "life"
            });
            o.w("this turn");
        }
        Amount::CountOf(x) if op == Op::Eq && *value == Amount::N(0) => {
            if let Some(Post::In(Zone::Library(w))) = x.filter.posts.first() {
                whose(o, w);
            }
            o.w("library has no cards in it");
        }
        Amount::CountOf(x) if op == Op::Ge && matches!(x.filter.kind, Some(Noun::Counter(_))) => {
            if let (Some(Post::On(on)), Some(Noun::Counter(Some(k)))) =
                (x.filter.posts.first(), &x.filter.kind)
            {
                let has = out(|o| {
                    objects(o, on);
                    o.verb("have", obj_num(on));
                    amount(o, value);
                    o.w("or more");
                    counter_kind(o, k);
                    o.w("counters on it");
                });
                let there_are = out(|o| {
                    o.w("there are");
                    amount(o, value);
                    o.w("or more");
                    filter(o, &x.filter, Num::Pl);
                });
                syn::counters_on(o, has, there_are);
            } else {
                // "there are twenty or more counters on it": of any kind.
                o.w("there are");
                amount(o, value);
                o.w("or more");
                filter(o, &x.filter, Num::Pl);
            }
        }
        Amount::CountOf(x) if op == Op::Ge => {
            o.w("there are");
            amount(o, value);
            o.w("or more");
            filter(o, &x.filter, Num::Pl);
        }
        Amount::NumberOfCardTypes(x) => {
            o.w("there are");
            amount(o, value);
            o.w("or more card types among");
            objects(o, x);
        }
        // "five or more mana was spent to cast that spell", "at least four
        // mana was spent to cast it".
        Amount::ManaSpentToCast(x) if op == Op::Ge => {
            let or_more = out(|o| {
                amount(o, value);
                o.w("or more");
            });
            let at_least = out(|o| {
                o.w("at least");
                amount(o, value);
            });
            syn::at_least(o, or_more, at_least);
            o.w("mana was spent to cast");
            objects(o, x);
        }
        n => {
            measure(o, n);
            o.w("is");
            comparison(o, op, value);
        }
    }
}

// ---------------------------------------------------------------- statics

/// The subject a static is said of, where one predicate could follow
/// another's: "Equipped creature has ward {2}, is an Assassin …, and can't
/// be blocked."
fn static_subject(s: &Static) -> Option<&Objects> {
    match s {
        Static::Modify { what, .. }
        | Static::HasAbility { what, .. }
        | Static::Restrict { what, .. }
        | Static::EntersTappedUnless { what, .. }
        | Static::EntersWithCounters { what, .. }
        | Static::EscapesWithCounters { what, .. }
        | Static::AlsoIs { what, .. }
        | Static::Is { what, .. }
        | Static::Loses { what, .. }
        | Static::BasePt { what, .. }
        | Static::AdditionalCostFor { what, .. } => Some(what),
        Static::EntersTapped(what)
        | Static::EntersUntapped(what)
        | Static::IsAllColors(what)
        | Static::Goaded(what)
        | Static::EntersPrepared(what) => Some(what),
        Static::HasAbilitiesOf { what, .. } | Static::HasIf { what, .. } => Some(what),
        Static::CostChange { what, .. } | Static::CostChangePer { what, .. } => Some(what),
        // "Equipped creature has menace and gets +X/+0, where X is …".
        Static::Where { inner, .. } => static_subject(inner),
        _ => None,
    }
}

fn statics(o: &mut Out, ss: &[Static]) {
    // Permissions with rules for what is cast through them: the
    // permissions, then an "if" about each spell, in the same list.
    if ss
        .iter()
        .any(|s| matches!(s, Static::PermissionRules { .. }))
    {
        let mut said = Vec::new();
        for s in ss {
            match s {
                Static::PermissionRules {
                    permissions,
                    spell,
                    rules,
                } => {
                    said.extend(permissions.iter().cloned());
                    let cond = Condition::ThisWay(Box::new(Event::Casts {
                        who: Player::You,
                        what: spell.clone(),
                    }));
                    said.push(Static::If {
                        cond,
                        then: rules.clone(),
                    });
                }
                s => said.push(s.clone()),
            }
        }
        return statics(o, &said);
    }
    let mut i = 0;
    let mut start = 0;
    while i < ss.len() {
        let s = &ss[i];
        // "can't attack or block, and its activated abilities can't be
        // activated": statics with different subjects may share a sentence.
        if i > 0 {
            let last = Out(o.0[start..].to_vec());
            syn::joiner(o, ends_in_quote(&last));
        }
        start = o.0.len();
        // A run of PlayFrom from one zone is one sentence.
        if let Static::PlayFrom { zone: z, .. } = s {
            let mut j = i;
            while matches!(ss.get(j), Some(Static::PlayFrom { zone: z2, .. }) if z2 == z) {
                j += 1;
            }
            o.w("you may");
            let mut last_verb = None;
            for (k, p) in ss[i..j].iter().enumerate() {
                if k > 0 {
                    o.w("and");
                }
                if let Static::PlayFrom { verb, what, .. } = p {
                    let word = out(|o| {
                        o.w(match verb {
                            CastVerb::Play => "play",
                            CastVerb::Cast => "cast",
                        })
                    });
                    // "cast artifact spells and colorless spells".
                    if last_verb == Some(*verb) {
                        syn::gapped(o, word, Out::default());
                    } else {
                        o.add(word);
                    }
                    last_verb = Some(*verb);
                    objects(o, what);
                }
            }
            o.w("from");
            zone(o, z);
            i = j;
            continue;
        }
        // "can't block or be blocked by non-Spirit creatures": two rules,
        // one object said once.
        if let (
            Static::Restrict {
                what,
                rule: Restriction::CantBlockAny(f1),
            },
            Some(
                next @ Static::Restrict {
                    what: w2,
                    rule: Restriction::CantBeBlockedBy(f2),
                },
            ),
        ) = (s, ss.get(i + 1))
        {
            if what == w2 && f1 == f2 {
                let apart = out(|o| {
                    static_full(o, s);
                    syn::joiner(o, false);
                    static_full(o, next);
                });
                let joined = out(|o| {
                    objects(o, what);
                    o.w("can't block or be blocked by");
                    filter(o, f1, Num::Pl);
                });
                syn::gapped(o, apart, joined);
                i += 2;
                continue;
            }
        }
        // An Is followed by what the new object has: "is an Insect artifact
        // creature with base power and toughness 0/1"; the pairs a size
        // said with the types makes are read by `sized`.
        let mut j = i + sized(o, ss, i);
        while let Some(next) = ss.get(j) {
            let same = static_subject(s).is_some() && static_subject(next) == static_subject(s);
            if !same {
                break;
            }
            // "is a Toy artifact creature with base power and toughness 0/2
            // in addition to its other types".
            if let (Static::AlsoIs { types, .. }, Some(Static::BasePt { pt, .. })) =
                (next, ss.get(j + 1))
            {
                let n = static_subject(next).map(obj_num).unwrap_or(Num::Sg);
                syn::joiner(o, false);
                o.verb("are", n);
                type_set(o, types, n);
                o.w("with base power and toughness");
                o.tok(Tok::Pt(*pt));
                o.w("in addition to");
                syn::its(o);
                o.w("other types");
                j += 2;
                continue;
            }
            let pred = out(|o| static_predicate(o, next));
            let after_is = matches!(ss[j - 1], Static::Is { .. })
                && matches!(next, Static::BasePt { .. } | Static::HasAbility { .. });
            if after_is {
                o.w("with");
                if let Static::BasePt { pt, .. } = next {
                    o.w("base power and toughness");
                    o.tok(Tok::Pt(*pt));
                } else if let Static::HasAbility { ability, .. } = next {
                    quoted(o, ability);
                }
            } else {
                // "gets +1/+1 and has trample and "Whenever …"": "has"
                // said once for both.
                let pred = if matches!(ss[j - 1], Static::Modify { .. })
                    && matches!(next, Static::HasAbility { .. })
                {
                    let without = out(|o| o.0.extend(pred.0.iter().skip(1).cloned()));
                    out(|o| syn::gapped(o, pred, without))
                } else {
                    pred
                };
                let once = out(|o| {
                    syn::joiner(o, false);
                    let it = out(syn::it);
                    syn::subject(o, it, pred.clone(), pred);
                });
                let again = out(|o| {
                    o.tok(Tok::Period);
                    static_full(o, next);
                });
                syn::said_again(o, once, again);
            }
            j += 1;
        }
        i = j;
    }
    o.period();
}

/// The static at `i` in full, with the base size after it when the two are
/// said as one: "he's a 3/4 Ninja creature" (a size said before the types
/// sets base power and toughness, CR 613.4b), "is a Toy artifact creature
/// with base power and toughness 0/2 in addition to its other types".
/// Returns how many statics it said.
fn sized(o: &mut Out, ss: &[Static], i: usize) -> usize {
    let s = &ss[i];
    match (s, ss.get(i + 1)) {
        (Static::Is { what, types }, Some(Static::BasePt { what: w2, pt }))
            if what == w2 && (is_bare_antecedent(what) || *what == self_named()) =>
        {
            let inline = out(|o| {
                syn::it_is(o);
                o.w("a");
                o.tok(Tok::Pt(*pt));
                match types {
                    TypeSet::Listed(v) => {
                        for a in v {
                            adj(o, a, None);
                        }
                    }
                    t => type_set(o, t, Num::Sg),
                }
            });
            let with = out(|o| {
                static_full(o, s);
                o.w("with base power and toughness");
                o.tok(Tok::Pt(*pt));
            });
            syn::base_pt(o, inline, with);
            2
        }
        (Static::AlsoIs { what, types }, Some(Static::BasePt { what: w2, pt })) if what == w2 => {
            let n = obj_num(what);
            objects(o, what);
            o.verb("are", n);
            type_set(o, types, n);
            o.w("with base power and toughness");
            o.tok(Tok::Pt(*pt));
            o.w("in addition to");
            syn::its(o);
            o.w("other types");
            2
        }
        _ => {
            static_full(o, s);
            1
        }
    }
}

fn static_full(o: &mut Out, s: &Static) {
    match s {
        // "it's a creature in addition to its other types": the pronoun and
        // its verb contracted.
        Static::AlsoIs { what, .. } if is_bare_antecedent(what) || is_bare_self(what) => {
            let full = out(|o| {
                objects(o, what);
                static_predicate(o, s);
            });
            let pred = out(|o| static_predicate(o, s));
            let mut contracted = out(syn::it_is);
            contracted.0.extend(pred.0.into_iter().skip(1));
            syn::contraction(o, full, contracted);
            return;
        }
        // "its activated abilities can't be activated": the subject is a
        // possessive inside the rule.
        Static::Restrict {
            rule: Restriction::NoActivatedAbilities,
            what,
        } if is_bare_antecedent(what) || is_bare_self(what) => {
            pronoun_or(
                o,
                what,
                |o| o.w("its activated abilities can't be activated"),
                |o| {
                    o.w("activated abilities of");
                    objects(o, what);
                    o.w("can't be activated");
                },
            );
            return;
        }
        // "Activated abilities of artifacts can't be activated", perhaps
        // "unless they're mana abilities".
        Static::Restrict {
            rule:
                rule @ (Restriction::NoActivatedAbilities | Restriction::NoNonManaActivatedAbilities),
            what,
        } => {
            o.w("activated abilities of");
            objects(o, what);
            o.w("can't be activated");
            if *rule == Restriction::NoNonManaActivatedAbilities {
                o.w("unless they're mana abilities");
            }
            return;
        }
        // "You may choose not to untap this artifact during your untap step."
        Static::Restrict {
            rule: Restriction::MayNotUntap,
            what,
        } => {
            o.w("you may choose not to untap");
            objects(o, what);
            o.w("during your untap step");
            return;
        }
        _ => {}
    }
    match static_subject(s) {
        Some(what) => {
            objects(o, what);
            static_predicate(o, s);
        }
        None => static_other(o, s),
    }
}

/// A static's predicate after its subject.
fn static_predicate(o: &mut Out, s: &Static) {
    let n = static_subject(s).map(obj_num).unwrap_or(Num::Sg);
    match s {
        Static::Modify { pt, has, per, .. } => match pt {
            Some(pt) => {
                o.verb("get", n);
                o.tok(Tok::Pt(*pt));
                if let Some(per) = per {
                    o.w(if matches!(per, Amount::Every { .. }) {
                        "for every"
                    } else {
                        "for each"
                    });
                    match per {
                        Amount::NumberOfColors(p) => {
                            o.w("color among");
                            if let Palette::ColorsOf(x) = &**p {
                                objects(o, x);
                            }
                        }
                        Amount::TimesAttackedThisTurn(_) => measure(o, per),
                        Amount::CountOf(x) => each_objects(o, x),
                        a => measure(o, a),
                    }
                }
                if !has.is_empty() {
                    o.w("and");
                    o.verb("have", n);
                    keyword_list(o, has);
                }
            }
            None => {
                o.verb("have", n);
                keyword_list(o, has);
            }
        },
        Static::HasAbility { ability, .. } => {
            o.verb("have", n);
            quoted(o, ability);
        }
        Static::Restrict { rule, .. } => restriction(o, rule, n),
        Static::EntersTappedUnless { unless, .. } => {
            o.verb("enter", n);
            syn::the_battlefield(o);
            o.w("tapped unless");
            condition(o, unless);
        }
        Static::EntersWithCounters { n: k, kind, .. } => {
            o.verb("enter", n);
            syn::the_battlefield(o);
            o.w("with");
            enters_counters(o, k, kind);
        }
        Static::EscapesWithCounters { n: k, kind, .. } => {
            o.verb("escape", n);
            o.w("with");
            enters_counters(o, k, kind);
        }
        Static::AlsoIs { types, .. } => {
            o.verb("are", n);
            type_set(o, types, n);
            let land_types = matches!(types, TypeSet::Listed(v) if v.iter().all(is_land_type))
                || *types == TypeSet::EveryBasicLandType;
            let creature_types = matches!(types, TypeSet::Listed(v) if v.iter().all(|a| matches!(a, Adj::Subtype(_)) && !is_land_type(a)));
            if land_types {
                syn::other_land_types(o);
            } else if creature_types {
                syn::other_creature_types(o);
            } else {
                o.w("in addition to");
                syn::its(o);
                o.w("other types");
            }
        }
        Static::Is { types, .. } => {
            o.verb("are", n);
            type_set(o, types, n);
        }
        Static::Loses { lost, .. } => {
            o.verb("lose", n);
            lost_words(o, lost);
        }
        Static::BasePt { pt, .. } => {
            let with = out(|o| {
                o.w("with base power and toughness");
                o.tok(Tok::Pt(*pt));
            });
            let have = out(|o| {
                o.verb("have", n);
                o.w("base power and toughness");
                o.tok(Tok::Pt(*pt));
            });
            syn::with_base_pt(o, with, have);
        }
        Static::AdditionalCostFor { cost: c, .. } => {
            o.verb("cost", n);
            o.w("an additional");
            match &c[..] {
                [Cost::PayLife(k)] => {
                    amount(o, k);
                    o.w("life");
                }
                [Cost::Mana(m)] => mana(o, m),
                c => costs(o, c),
            }
            o.w("to cast");
        }
        Static::EntersTapped(_) => {
            o.verb("enter", n);
            syn::the_battlefield(o);
            o.w("tapped");
        }
        Static::EntersUntapped(_) => {
            o.verb("enter", n);
            syn::the_battlefield(o);
            o.w("untapped");
        }
        Static::CostChange { by, less, per, .. } => {
            o.verb("cost", n);
            mana(o, by);
            o.w(if *less { "less" } else { "more" });
            o.w("to cast");
            if let Some(per) = per {
                o.w("for each");
                each_objects(o, per);
            }
        }
        Static::CostChangePer { by, less, per, .. } => {
            o.verb("cost", n);
            mana(o, by);
            o.w(if *less { "less" } else { "more" });
            o.w("to cast for each");
            match per {
                Amount::NumberOfCardTypes(x) => {
                    o.w("card type among");
                    objects(o, x);
                }
                Amount::NumberOfBasicLandTypes(x) => {
                    o.w("basic land type among");
                    objects(o, x);
                }
                a => measure(o, a),
            }
        }
        Static::IsAllColors(_) => {
            o.verb("are", n);
            o.w("all colors");
        }
        Static::Goaded(_) => {
            o.verb("are", n);
            o.w("goaded");
        }
        Static::Where { inner, x } if static_subject(inner).is_some() => {
            static_predicate(o, inner);
            o.w(", where X is");
            measure(o, x);
        }
        Static::HasAbilitiesOf { kind, of, .. } => {
            o.verb("have", n);
            o.w("all");
            adj(o, &Adj::Ability(*kind), None);
            o.w("abilities of");
            objects(o, of);
        }
        Static::HasIf { pairs, .. } => {
            o.verb("have", n);
            for (i, (ks, c)) in pairs.iter().enumerate() {
                list_sep(o, i, pairs.len(), "and");
                keyword_list(o, ks);
                o.w("if");
                condition(o, c);
            }
        }
        Static::EntersPrepared(_) => {
            o.verb("enter", n);
            o.w("prepared");
        }
        // Every caller asks `static_subject` first, so a static here has a
        // predicate above. Handing it to `static_other` instead, as this arm
        // did, could come back here through `static_full` and never end.
        s => unsaid(o, s),
    }
}

fn is_land_type(a: &Adj) -> bool {
    matches!(a, Adj::Subtype(s) if crate::vocab::vocab().land_types.contains(s))
}

fn type_set(o: &mut Out, t: &TypeSet, n: Num) {
    match t {
        TypeSet::Listed(v) | TypeSet::Subtypes(v) => type_words(o, v, n),
        TypeSet::Chosen => o.w("the chosen type"),
        TypeSet::EveryBasicLandType => o.w("every basic land type"),
        TypeSet::EveryCreatureType => o.w("every creature type"),
    }
}

fn lost_words(o: &mut Out, lost: &[Lost]) {
    if let [Lost::Keywords(k)] = lost {
        keyword_list(o, k);
        return;
    }
    o.w("all");
    if lost
        .iter()
        .any(|l| matches!(l, Lost::All { other: true, .. }))
    {
        o.w("other");
    }
    for (i, l) in lost.iter().enumerate() {
        list_sep(o, i, lost.len(), "and");
        if let Lost::All { of, .. } = l {
            o.w(match of {
                Characteristic::Abilities => "abilities",
                Characteristic::CardTypes => "card types",
                Characteristic::CreatureTypes => "creature types",
            });
        }
    }
}

fn static_other(o: &mut Out, s: &Static) {
    match s {
        Static::PreventDamage {
            combat_only,
            to,
            by,
        } => {
            let as_effect = Effect::Prevent {
                combat_only: *combat_only,
                n: None,
                to: to.clone(),
                by: by.clone(),
                dur: Duration::Permanent,
            };
            effect(o, &as_effect, &Cx::default());
        }
        Static::PermissionRules {
            permissions,
            spell,
            rules,
        } => {
            // `statics` says it as its parts; alone, the same.
            statics(
                o,
                &[Static::PermissionRules {
                    permissions: permissions.clone(),
                    spell: spell.clone(),
                    rules: rules.clone(),
                }],
            );
        }
        Static::AbilityCostChange { kind, of, by, less } => {
            adj(o, &Adj::Ability(*kind), None);
            o.w("abilities of");
            objects(o, of);
            o.w("cost");
            mana(o, by);
            o.w(if *less { "less" } else { "more" });
            o.w("to activate");
        }
        Static::ExtraLand(n) => {
            o.w("you may play");
            amount_a(o, n);
            o.w("additional");
            o.noun("land", amount_num(n));
            o.w("on each of your turns");
        }
        Static::PlayerRestrict { who, rules } => {
            player(o, who);
            o.w("can't");
            for (i, r) in rules.iter().enumerate() {
                if i > 0 {
                    o.w("or");
                }
                player_rule(o, r);
            }
        }
        Static::UntapDuring { what, step: s } => {
            o.w("untap");
            objects(o, what);
            o.w("during");
            step(o, s);
        }
        Static::AsLongAs { cond, then } => {
            if let (
                [Static::ControlPlayer(Player::That)],
                Condition::Searching {
                    who: Player::AnOpponent,
                    zone: Zone::Library(Player::That),
                },
            ) = (&then[..], cond)
            {
                let each = out(|o| {
                    o.w("you control");
                    player(o, &Player::EachOpponent);
                    o.w("while");
                    condition(
                        o,
                        &Condition::Searching {
                            who: Player::That,
                            zone: Zone::Library(Player::That),
                        },
                    );
                });
                let one = out(|o| {
                    o.w("while");
                    condition(o, cond);
                    o.w(", you control");
                    player(o, &Player::That);
                });
                syn::each_while_they(o, each, one);
                return;
            }
            if let [Static::ControlPlayer(p)] = &then[..] {
                o.w("you control");
                player(o, p);
                o.w("while");
                condition(o, cond);
                return;
            }
            let body = |o: &mut Out| statics_joined(o, then);
            if *cond == Condition::ItsYourTurn {
                let before = out(|o| {
                    o.w("during your turn,");
                    body(o);
                });
                let after = out(|o| {
                    body(o);
                    o.w("during your turn");
                });
                syn::position(o, before, after);
                return;
            }
            let before = out(|o| {
                o.w("as long as");
                condition(o, cond);
                o.w(",");
                body(o);
            });
            let after = out(|o| {
                body(o);
                o.w("as long as");
                condition(o, cond);
            });
            syn::position(o, before, after);
        }
        Static::SetPt {
            what,
            each_equal_to,
        } => {
            possessive_of(o, what);
            o.w("power and toughness are each equal to");
            measure(o, each_equal_to);
        }
        Static::MaxHandSize => o.w("you have no maximum hand size"),
        Static::AdditionalCost(c) => {
            o.w("as an additional cost to cast this spell,");
            costs_said(o, c);
        }
        Static::AlternativeCost(c) => {
            o.w("you may");
            costs_said(o, c);
            o.w("rather than pay this spell's mana cost");
        }
        Static::CanBeCommander => o.w("~ can be your commander"),
        Static::Control(x) => {
            o.w("you control");
            objects(o, x);
        }
        Static::AsEnters { what, then } => {
            o.w("as");
            obj_subj(o, what, "enter");
            syn::the_battlefield(o);
            o.w(",");
            clauses(o, then, &Cx::default());
        }
        Static::LookAtTopAnyTime => o.w("you may look at the top card of your library any time"),
        Static::YouHave(k) => {
            o.w("you have");
            keyword_list(o, k);
        }
        Static::SetPower { what, equal_to } => {
            possessive_of(o, what);
            o.w("power is equal to");
            measure(o, equal_to);
        }
        Static::If { cond, then } => {
            let before = out(|o| {
                o.w("if");
                condition(o, cond);
                o.w(",");
                statics_joined(o, then);
            });
            let after = out(|o| {
                statics_joined(o, then);
                o.w("if");
                condition(o, cond);
            });
            syn::position(o, before, after);
        }
        Static::PlayFrom {
            verb,
            what,
            zone: z,
        } => {
            o.w("you may");
            o.w(match verb {
                CastVerb::Play => "play",
                CastVerb::Cast => "cast",
            });
            objects(o, what);
            o.w("from");
            zone(o, z);
        }
        Static::Replacement { event, instead } => {
            let w = out(|o| would(o, event));
            replacement(o, w, instead, &Cx::default());
        }
        Static::CastWithoutPaying => o.w("you may cast this spell without paying its mana cost"),
        Static::AdditionalTrigger { of, cause } => match cause {
            None => {
                o.w("if a triggered ability of");
                objects(o, of);
                o.w("triggers,");
                syn::anaphor(o, "ability");
                o.w("triggers an additional time");
            }
            Some(Event::Enters { what, .. }) => {
                o.w("if");
                objects(o, what);
                o.w("entering");
                syn::the_battlefield(o);
                o.w("causes a triggered ability of");
                objects(o, of);
                o.w("to trigger, that ability triggers an additional time");
            }
            Some(Event::Attacks(what)) => {
                o.w("if");
                objects(o, what);
                o.w("attacking causes a triggered ability of");
                objects(o, of);
                o.w("to trigger, that ability triggers an additional time");
            }
            Some(e) => {
                o.w("if");
                let (who, what, verbs) = match e {
                    Event::Casts { who, what } => (who, what, vec!["casting"]),
                    Event::Copies { who, what } => (who, what, vec!["copying"]),
                    Event::Or(v) => match &v[0] {
                        Event::Casts { who, what } | Event::Copies { who, what } => (
                            who,
                            what,
                            v.iter()
                                .map(|e| {
                                    if matches!(e, Event::Casts { .. }) {
                                        "casting"
                                    } else {
                                        "copying"
                                    }
                                })
                                .collect(),
                        ),
                        _ => return,
                    },
                    _ => return,
                };
                player(o, who);
                o.w(&verbs.join(" or "));
                objects(o, what);
                o.w("causes a triggered ability of");
                objects(o, of);
                o.w("to trigger, that ability triggers an additional time");
            }
        },
        Static::Where { inner, x } => {
            static_full(o, inner);
            o.w(", where X is");
            measure(o, x);
        }
        Static::EntersAsCopy {
            may,
            tapped,
            what,
            of,
            except,
        } => {
            if *may {
                o.w("you may have");
                objects(o, what);
                o.verb("enter", Num::Pl);
            } else {
                obj_subj(o, what, "enter");
            }
            if *tapped {
                o.w("tapped");
            }
            o.w("as a copy of");
            objects(o, of);
            exceptions(o, except);
        }
        Static::SameFor(v) => {
            o.w("the same is true for");
            for (i, x) in v.iter().enumerate() {
                list_sep(o, i, v.len(), "and");
                objects(o, x);
            }
        }
        Static::KeywordCost { keyword, cost: c } => {
            o.w("the");
            o.w(keyword);
            o.w("cost is equal to");
            if let Some(Cost::ManaCostOf(x)) = c.first() {
                objects(o, x);
            }
            o.w("'s mana cost");
            if c.len() > 1 {
                o.w("plus");
                costs(o, &c[1..]);
            }
        }
        Static::PaysRather { cost: c, .. } => {
            costs(o, c);
            o.w("rather than pay its mana cost");
        }
        Static::PlayWithTopRevealed(Player::That) => {
            o.w("players play with the top card of their libraries revealed")
        }
        Static::PlayWithTopRevealed(_) => o.w("play with the top card of your library revealed"),
        Static::SkipStep(s) => {
            o.w("skip");
            step(o, s);
        }
        Static::ActivationsPerTurn {
            what,
            keyword,
            times,
            during,
        } => {
            objects(o, what);
            o.w("can");
            o.w(keyword);
            if *times == 2 {
                o.w("twice");
            } else {
                syn::number(o, *times);
                o.w("times");
            }
            o.w("during each of");
            o.w(if *during == Whose::Yours {
                "your"
            } else {
                "their"
            });
            o.w("turns rather than once");
        }
        Static::BeginOnBattlefield {
            what,
            counters,
            then,
        } => {
            o.w("you may begin the game with");
            objects(o, what);
            o.w("on the battlefield");
            if let Some((n, kind)) = counters {
                o.w("with");
                amount_a(o, n);
                counter_noun(o, kind, amount_num(n));
                o.w("on it");
            }
            if !then.is_empty() {
                o.tok(Tok::Period);
                clauses(o, then, &Cx::default());
            }
        }
        Static::ControlPlayer(p) => {
            o.w("you control");
            player(o, p);
        }
        Static::While { cond, then } => {
            o.w("while");
            condition(o, cond);
            o.w(",");
            clauses(o, then, &Cx::default());
        }
        Static::IsNot { what, types } => {
            objects(o, what);
            o.verb("aren't", obj_num(what));
            type_words(o, types, obj_num(what));
        }
        Static::CastAsThoughFlashFor { what, cost } => {
            o.w("you may cast");
            objects(o, what);
            o.w("as though it had flash if you");
            match &cost[..] {
                [Cost::Mana(m)] => {
                    o.w("pay");
                    mana(o, m);
                    o.w("more to cast it");
                }
                c => {
                    costs(o, c);
                    o.w("as an additional cost to cast it");
                }
            }
        }
        Static::CastAsThoughFlash(x) => {
            o.w("you may cast");
            objects(o, x);
            o.w("as though");
            syn::it(o);
            o.w("had flash");
        }
        Static::ActivateAsThoughHaste(x) => {
            o.w("you may activate abilities of");
            objects(o, x);
            o.w("as though those creatures had haste");
        }
        Static::EnteringDoesntTrigger { what, of } => {
            objects(o, what);
            o.w("entering don't cause abilities");
            if let Some(x) = of {
                o.w("of");
                objects(o, x);
            }
            o.w("to trigger");
        }
        Static::CastOnlyAsSorcery { who, what } => {
            player(o, who);
            o.w("can cast");
            objects(o, what);
            o.w("only any time they could cast a sorcery");
        }
        Static::SpendAsAnyColor(uses) => {
            o.w("you may spend mana as though it were mana of any color");
            spend_uses(o, uses);
        }
        Static::SpendAsAnyType(uses) => {
            let you_may = out(|o| {
                o.w("you may");
                syn::any_type_spent(o);
            });
            let passive = out(syn::any_type_spent);
            syn::may_or_passive(o, you_may, passive);
            spend_uses(o, uses);
        }
        Static::PreventNoncombat(to) => {
            o.w("prevent all noncombat damage that would be dealt to");
            recipients(o, to);
        }
        Static::PowerGreaterFor { what, keywords, by } => {
            objects(o, what);
            for (i, (k, x)) in keywords.iter().enumerate() {
                if i > 0 {
                    o.w("and");
                }
                o.verb(k, obj_num(what));
                objects(o, x);
            }
            o.w("as though its power were");
            amount(o, by);
            o.w("greater");
        }
        Static::CantCauseSearch(x) => {
            objects(o, x);
            o.w("can't cause their controller to search their library");
        }
        Static::MaxHandSizeIs { who, n } => {
            whose(o, who);
            o.w("maximum hand size is");
            syn::number(o, *n);
        }
        Static::AsTransforms { what, into, then } => {
            o.w("as");
            obj_subj(o, what, "transform");
            o.w("into");
            objects(o, into);
            o.w(",");
            clauses(o, then, &Cx::default());
        }
        Static::RevealFromOpeningHand(then) => {
            o.w("you may reveal this card from your opening hand. if you do,");
            clauses(o, then, &Cx::default());
        }
        Static::OptionalAdditionalCost(c) => {
            o.w("as an additional cost to cast this spell, you may");
            costs_said(o, c);
        }
        Static::CastFromUsing { zone: z, keyword } => {
            o.w("you may cast this card from");
            zone(o, z);
            o.w("using its");
            o.w(keyword);
            o.w("ability");
        }
        Static::SetToughness { what, equal_to } => {
            possessive_of(o, what);
            o.w("toughness is equal to");
            measure(o, equal_to);
        }
        Static::Reflexive { inner, then } => {
            static_full(o, inner);
            o.w(". when you do,");
            clauses(o, then, &Cx::default());
        }
        // Said as a subject and a predicate. Listed rather than caught by a
        // wildcard that hands the static back to `static_full`: a variant
        // with no subject that reached such a wildcard went round
        // `static_full` and here forever, and a new variant now fails to
        // compile until it is said somewhere. Every other call here is on a
        // strictly smaller static, so rendering a static ends.
        Static::Modify { .. }
        | Static::HasAbility { .. }
        | Static::Restrict { .. }
        | Static::EntersTappedUnless { .. }
        | Static::EntersWithCounters { .. }
        | Static::EscapesWithCounters { .. }
        | Static::AlsoIs { .. }
        | Static::Is { .. }
        | Static::Loses { .. }
        | Static::BasePt { .. }
        | Static::AdditionalCostFor { .. }
        | Static::EntersTapped(_)
        | Static::EntersUntapped(_)
        | Static::IsAllColors(_)
        | Static::Goaded(_)
        | Static::EntersPrepared(_)
        | Static::HasAbilitiesOf { .. }
        | Static::HasIf { .. }
        | Static::CostChange { .. }
        | Static::CostChangePer { .. } => match static_subject(s) {
            Some(what) => {
                objects(o, what);
                static_predicate(o, s);
            }
            None => unsaid(o, s),
        },
    }
}

/// A static no render function says, which the round trip reports as a
/// mismatch instead of the renderer recursing on it.
fn unsaid(o: &mut Out, s: &Static) {
    o.w("[unrendered static");
    o.w(&format!("{s:?}"));
    o.w("]");
}

/// Statics inside "as long as …," or "if …,": one sentence, joined.
fn statics_joined(o: &mut Out, ss: &[Static]) {
    let mut i = 0;
    while i < ss.len() {
        let s = &ss[i];
        if i > 0 {
            syn::joiner(o, false);
            if static_subject(s).is_some() && static_subject(s) == static_subject(&ss[i - 1]) {
                // "… can attack as though it didn't have defender and it
                // can't be blocked": the subject again by pronoun, or not.
                let pred = out(|o| static_predicate(o, s));
                let it = out(syn::it);
                syn::subject(o, it, pred.clone(), pred);
                i += 1;
                continue;
            }
        }
        i += sized(o, ss, i);
    }
}

/// "you gained", "you've gained", "that player has gained": who gained or
/// lost life this turn, in the past or the present perfect.
fn life_subject(o: &mut Out, p: &Player, verb: &str) {
    let simple = out(|o| {
        player(o, p);
        o.w(verb);
    });
    let perfect = out(|o| match p {
        Player::You => o.w("you've"),
        p => {
            player(o, p);
            o.verb("have", player_num(p));
        }
    });
    let mut perfect = perfect;
    perfect.w(verb);
    syn::perfect(o, simple, perfect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_face, Face};

    /// The one ability a line reads into, with the text it came from.
    fn read(type_line: &str, text: &str) -> (Ability, String) {
        let face = Face {
            name: "Some Card",
            type_line,
            text,
        };
        let line = read_face(&face, "Some Card").remove(0);
        let a = line
            .parsed
            .unwrap_or_else(|b| panic!("{text}: broke at {}", b.at));
        (a, line.text)
    }

    /// A static with no subject handed to the predicate renderer is
    /// reported, not passed back to `static_full`, which once looped forever.
    #[test]
    fn a_static_with_no_subject_as_a_predicate_ends() {
        let s = Static::MaxHandSize;
        assert!(static_subject(&s).is_none());
        let said = out(|o| static_predicate(o, &s));
        assert!(format!("{said:?}").contains("unrendered"), "{said:?}");
    }

    fn round_trips(a: &Ability, text: &str) -> bool {
        compare(&ability(a), &line_tokens(text)).is_ok()
    }

    fn spell_effects(a: &mut Ability) -> &mut Vec<Effect> {
        match a {
            Ability::Spell(e) => e,
            a => panic!("{a:?}"),
        }
    }

    #[test]
    fn a_tree_that_parsed_renders_back_into_its_own_words() {
        let (a, text) = read(
            "Sorcery",
            "Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom of your library in a random order.",
        );
        assert!(round_trips(&a, &text), "{}", ability(&a).text());
    }

    #[test]
    fn the_bottom_of_a_library_said_as_the_top_does_not_round_trip() {
        let (mut a, text) = read(
            "Sorcery",
            "Look at the top three cards of your library. Put one of them into your hand and the rest on the bottom of your library in a random order.",
        );
        let Some(Effect::Move { to, .. }) = spell_effects(&mut a).last_mut() else {
            panic!()
        };
        *to = Zone::TopOfLibrary(Player::You);
        assert!(!round_trips(&a, &text));
    }

    #[test]
    fn a_target_player_read_as_you_does_not_round_trip() {
        let (mut a, text) = read("Sorcery", "Target player draws two cards.");
        let Effect::Draw { who, .. } = &mut spell_effects(&mut a)[0] else {
            panic!()
        };
        *who = Player::You;
        assert!(!round_trips(&a, &text));
    }

    #[test]
    fn a_dropped_duration_does_not_round_trip() {
        let (mut a, text) = read("Instant", "Target creature gets +3/+3 until end of turn.");
        let Effect::Modify { dur, .. } = &mut spell_effects(&mut a)[0] else {
            panic!()
        };
        *dur = Duration::Permanent;
        assert!(!round_trips(&a, &text));
    }

    #[test]
    fn a_wrong_number_or_zone_does_not_round_trip() {
        let (a, text) = read(
            "Sorcery",
            "Return target creature card from your graveyard to your hand.",
        );
        let mut fewer = a.clone();
        let Effect::Move { to, .. } = &mut spell_effects(&mut fewer)[0] else {
            panic!()
        };
        *to = Zone::Battlefield;
        assert!(!round_trips(&fewer, &text));
        let (mut b, text) = read("Sorcery", "Draw three cards.");
        let Effect::Draw { n, .. } = &mut spell_effects(&mut b)[0] else {
            panic!()
        };
        *n = Amount::N(2);
        assert!(!round_trips(&b, &text));
    }

    #[test]
    fn a_synonym_is_accepted_only_where_it_is_listed() {
        // "When" and "Whenever" are one trigger word (CR 603.1) …
        let (a, _) = read("Creature — Bear", "When this creature enters, draw a card.");
        assert!(round_trips(
            &a,
            "Whenever this creature enters, draw a card."
        ));
        // … and so are "put" and "return", a wording with no rules meaning
        // (CR 400.6) …
        let (b, _) = read(
            "Sorcery",
            "Return target creature card from your graveyard to your hand.",
        );
        assert!(round_trips(
            &b,
            "Put target creature card from your graveyard into your hand."
        ));
        // … but "a second target" may choose the first target again and
        // "another target" may not (CR 115.3), so the tree keeps them apart.
        let (c, _) = read(
            "Sorcery",
            "Move a counter from target permanent onto a second target permanent.",
        );
        assert!(!round_trips(
            &c,
            "Move a counter from target permanent onto another target permanent."
        ));
    }
}
