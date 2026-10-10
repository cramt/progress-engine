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
        self.attempt(|p| {
            for w in ws.split(' ') {
                match w {
                    "'s" | "'" => p.tok(&Tok::Possessive)?,
                    "," => p.tok(&Tok::Comma)?,
                    "." => p.tok(&Tok::Period)?,
                    ":" => p.tok(&Tok::Colon)?,
                    "—" => p.tok(&Tok::Dash)?,
                    "~" => p.tok(&Tok::SelfRef)?,
                    _ => p.word(w)?,
                }
            }
            Some(())
        })
    }

    /// The first of `options` that matches, longest written first by the caller.
    fn one_of(&mut self, options: &[&'static str]) -> Option<&'static str> {
        options.iter().copied().find(|o| self.words(o).is_some())
    }

    /// A verb in either number: `gain` or `gains`.
    fn verb(&mut self, base: &str) -> Option<()> {
        let third = match base {
            "have" => "has".to_string(),
            "do" => "does".to_string(),
            b if b.ends_with('s') || b.ends_with("sh") || b.ends_with('x') => format!("{b}es"),
            b if b.ends_with('y') && !b.ends_with("ay") => format!("{}ies", &b[..b.len() - 1]),
            b => format!("{b}s"),
        };
        let first = base.split(' ').next().unwrap_or(base);
        let rest: Vec<&str> = base.split(' ').skip(1).collect();
        let third_first = if rest.is_empty() {
            third
        } else {
            let f = first;
            match f {
                "have" => "has".into(),
                f if f.ends_with('s') => format!("{f}es"),
                f => format!("{f}s"),
            }
        };
        self.attempt(|p| {
            if p.word(first).is_none() {
                p.word(&third_first)?;
            }
            for w in &rest {
                p.word(w)?;
            }
            Some(())
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

// ---------------------------------------------------------------- amounts

fn number(p: &mut P) -> Option<u32> {
    match p.peek()? {
        Tok::Num(n) => {
            let n = *n;
            p.bump();
            Some(n)
        }
        Tok::Word(w) => {
            let n = NUMBERS.iter().position(|x| x == w)?;
            p.bump();
            Some(n as u32)
        }
        _ => None,
    }
}

fn amount(p: &mut P) -> Option<Amount> {
    alt!(p;
        |p| number(p).map(Amount::N),
        |p| p.word("x").map(|_| Amount::X),
        |p| p.words("that many").map(|_| Amount::ThatMany),
        |p| p.words("that much").map(|_| Amount::ThatMany),
    )
}

/// An amount where "a" and "an" mean one.
fn amount_a(p: &mut P) -> Option<Amount> {
    alt!(p;
        amount,
        |p| p.one_of(&["a", "an"]).map(|_| Amount::N(1)),
    )
}

fn stat(p: &mut P) -> Option<Stat> {
    alt!(p;
        |p| p.word("power").map(|_| Stat::Power),
        |p| p.word("toughness").map(|_| Stat::Toughness),
        |p| p.words("mana value").map(|_| Stat::ManaValue),
        |p| p.words("converted mana cost").map(|_| Stat::ManaValue),
    )
}

fn stat_of(s: Stat, o: Objects) -> Amount {
    match s {
        Stat::Power => Amount::PowerOf(Box::new(o)),
        Stat::Toughness => Amount::ToughnessOf(Box::new(o)),
        Stat::ManaValue => Amount::ManaValueOf(Box::new(o)),
    }
}

/// What follows "equal to".
fn measure(p: &mut P) -> Option<Amount> {
    alt!(p;
        |p| { p.words("the number of")?; objects(p).map(|o| Amount::CountOf(Box::new(o))) },
        |p| { p.one_of(&["its", "their"])?; let s = stat(p)?; Some(stat_of(s, antecedent())) },
        |p| { let o = objects(p)?; p.tok(&Tok::Possessive)?; let s = stat(p)?; Some(stat_of(s, o)) },
        |p| { p.words("the life lost this way").map(|_| Amount::LifeLost) },
        |p| { p.words("the damage dealt this way").map(|_| Amount::Damage) },
        |p| { p.words("the amount of damage dealt this way").map(|_| Amount::Damage) },
        amount,
    )
}

fn antecedent() -> Objects {
    Objects {
        quant: Quant::Antecedent,
        filter: Filter::default(),
    }
}

fn self_ref() -> Objects {
    Objects {
        quant: Quant::SelfRef,
        filter: Filter::default(),
    }
}

// ---------------------------------------------------------------- mana and costs

fn mana(p: &mut P) -> Option<Mana> {
    let mut syms = Vec::new();
    while let Some(Tok::Sym(s)) = p.peek() {
        if matches!(s.as_str(), "T" | "Q" | "E") {
            break;
        }
        syms.push(s.clone());
        p.bump();
    }
    (!syms.is_empty()).then_some(Mana(syms))
}

fn sym(p: &mut P, s: &str) -> Option<()> {
    match p.peek()? {
        Tok::Sym(x) if x == s => {
            p.bump();
            Some(())
        }
        _ => None,
    }
}

fn cost(p: &mut P) -> Option<Cost> {
    alt!(p;
        |p| mana(p).map(Cost::Mana),
        |p| sym(p, "T").map(|_| Cost::Tap),
        |p| sym(p, "Q").map(|_| Cost::Untap),
        |p| {
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Cost::Energy(n))
        },
        |p| { p.word("sacrifice")?; objects(p).map(Cost::Sacrifice) },
        |p| { p.word("discard")?; objects(p).map(Cost::Discard) },
        |p| { p.word("pay")?; let n = amount(p)?; p.word("life")?; Some(Cost::PayLife(n)) },
        |p| { p.word("exile")?; objects(p).map(Cost::Exile) },
        |p| {
            p.word("remove")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("from")?;
            let from = objects(p)?;
            Some(Cost::RemoveCounters { n, kind, from })
        },
        |p| {
            p.word("put")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("on")?;
            let on = objects(p)?;
            Some(Cost::PutCounters { n, kind, on })
        },
        |p| { p.word("tap")?; objects(p).map(Cost::TapUntapped) },
        |p| { p.word("return")?; let o = objects(p)?; p.word("to")?; owners_hand(p)?; Some(Cost::ReturnToHand(o)) },
        |p| { p.word("reveal")?; objects(p).map(Cost::Reveal) },
        |p| { p.word("mill")?; let n = amount_a(p)?; p.one_of(&["cards", "card"])?; Some(Cost::Mill(n)) },
    )
}

fn costs(p: &mut P) -> Option<Vec<Cost>> {
    let mut v = vec![cost(p)?];
    while let Some(c) = p.attempt(|p| {
        p.tok(&Tok::Comma)?;
        p.attempt(|p| p.word("and"));
        cost(p)
    }) {
        v.push(c);
    }
    // "{2} and sacrifice a creature" reads the same as a comma.
    if let Some(c) = p.attempt(|p| {
        p.word("and")?;
        cost(p)
    }) {
        v.push(c);
    }
    Some(v)
}

fn counter_kind(p: &mut P) -> Option<String> {
    match p.peek()? {
        Tok::Pt(pt) if pt.signed => {
            let s = format!("{pt:?}");
            p.bump();
            Some(s)
        }
        Tok::Word(w) if !matches!(w.as_str(), "on" | "from" | "counter" | "counters") => {
            let w = w.clone();
            p.bump();
            Some(w)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- players

fn player(p: &mut P) -> Option<Player> {
    alt!(p;
        |p| p.words("target player").map(|_| Player::Target),
        |p| p.words("target opponent").map(|_| Player::TargetOpponent),
        |p| p.words("each other player").map(|_| Player::EachOtherPlayer),
        |p| p.words("each player").map(|_| Player::Each),
        |p| p.words("each opponent").map(|_| Player::EachOpponent),
        |p| p.words("your opponents").map(|_| Player::EachOpponent),
        |p| p.words("an opponent").map(|_| Player::AnOpponent),
        |p| p.words("a player").map(|_| Player::APlayer),
        |p| p.words("that player").map(|_| Player::That),
        |p| p.words("the chosen player").map(|_| Player::Chosen),
        |p| p.words("defending player").map(|_| Player::DefendingPlayer),
        |p| p.words("the active player").map(|_| Player::Active),
        |p| p.words("its controller").map(|_| Player::ControllerOf(Box::new(antecedent()))),
        |p| p.words("their controller").map(|_| Player::ControllerOf(Box::new(antecedent()))),
        |p| p.words("its owner").map(|_| Player::OwnerOf(Box::new(antecedent()))),
        |p| p.words("their owner").map(|_| Player::OwnerOf(Box::new(antecedent()))),
        |p| p.words("their owners").map(|_| Player::OwnerOf(Box::new(antecedent()))),
        |p| p.word("they").map(|_| Player::That),
        |p| p.word("you").map(|_| Player::You),
        |p| {
            let o = objects_no_player(p)?;
            p.tok(&Tok::Possessive)?;
            alt!(p;
                |p| p.word("controller").map(|_| Player::ControllerOf(Box::new(o.clone()))),
                |p| p.word("owner").map(|_| Player::OwnerOf(Box::new(o.clone()))),
            )
        },
    )
}

/// A player followed by `'s`, or "your" and "their".
fn whose(p: &mut P) -> Option<Player> {
    alt!(p;
        |p| p.word("your").map(|_| Player::You),
        |p| p.word("their").map(|_| Player::That),
        |p| p.word("its").map(|_| Player::ControllerOf(Box::new(antecedent()))),
        |p| { p.words("its owner")?; p.tok(&Tok::Possessive)?; Some(Player::OwnerOf(Box::new(antecedent()))) },
        |p| { p.words("their owner")?; p.tok(&Tok::Possessive)?; Some(Player::OwnerOf(Box::new(antecedent()))) },
        |p| { let pl = player(p)?; p.tok(&Tok::Possessive)?; Some(pl) },
    )
}

// ---------------------------------------------------------------- zones

fn zone(p: &mut P) -> Option<Zone> {
    alt!(p;
        |p| p.words("a graveyard").map(|_| Zone::AGraveyard),
        |p| p.word("anywhere").map(|_| Zone::Anywhere),
        |p| p.words("exile").map(|_| Zone::Exile),
        |p| p.words("the battlefield").map(|_| Zone::Battlefield),
        |p| p.words("the command zone").map(|_| Zone::CommandZone),
        |p| p.words("the stack").map(|_| Zone::Stack),
        |p| { p.words("the top of")?; let w = whose(p)?; p.word("library")?; Some(Zone::TopOfLibrary(w)) },
        |p| { p.words("the bottom of")?; let w = whose(p)?; p.word("library")?; Some(Zone::BottomOfLibrary(w)) },
        |p| {
            let w = whose(p)?;
            alt!(p;
                |p| p.word("graveyard").map(|_| Zone::Graveyard(w.clone())),
                |p| p.word("graveyards").map(|_| Zone::Graveyard(w.clone())),
                |p| p.word("hand").map(|_| Zone::Hand(w.clone())),
                |p| p.word("hands").map(|_| Zone::Hand(w.clone())),
                |p| p.word("library").map(|_| Zone::Library(w.clone())),
            )
        },
    )
}

fn owners_hand(p: &mut P) -> Option<Zone> {
    alt!(p;
        |p| p.words("its owner 's hand").map(|_| Zone::OwnersHand),
        |p| p.words("their owner 's hand").map(|_| Zone::OwnersHand),
        |p| p.words("their owners ' hands").map(|_| Zone::OwnersHand),
        |p| p.words("their owners 's hands").map(|_| Zone::OwnersHand),
        |p| p.words("their owners hands").map(|_| Zone::OwnersHand),
        |p| p.words("your hand").map(|_| Zone::Hand(Player::You)),
    )
}

/// Where "put"/"return" sends something.
fn destination(p: &mut P) -> Option<(Zone, bool, bool)> {
    alt!(p;
        |p| {
            p.words("the battlefield")?;
            let tapped = p.attempt(|p| p.word("tapped")).is_some();
            p.attempt(|p| p.word("transformed"));
            let mine = p.attempt(|p| p.words("under your control")).is_some();
            if !mine {
                p.attempt(|p| p.one_of(&["under its owner 's control", "under their owners ' control", "under their owner 's control"]));
            }
            let tapped = tapped || p.attempt(|p| p.word("tapped")).is_some();
            Some((Zone::Battlefield, tapped, mine))
        },
        |p| owners_hand(p).map(|z| (z, false, false)),
        |p| p.words("its owner 's graveyard").map(|_| (Zone::OwnersGraveyard, false, false)),
        |p| p.words("their owners ' graveyards").map(|_| (Zone::OwnersGraveyard, false, false)),
        |p| p.words("the top of its owner 's library").map(|_| (Zone::OwnersLibraryTop, false, false)),
        |p| p.words("the bottom of its owner 's library").map(|_| (Zone::OwnersLibraryBottom, false, false)),
        |p| p.words("its owner 's library").map(|_| (Zone::OwnersLibraryTop, false, false)),
        |p| zone(p).map(|z| (z, false, false)),
    )
}

// ---------------------------------------------------------------- objects

fn color_word(w: &str) -> Option<Color> {
    Some(match w {
        "white" => Color::White,
        "blue" => Color::Blue,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "colorless" => Color::Colorless,
        "multicolored" => Color::Multicolored,
        "monocolored" => Color::Monocolored,
        _ => return None,
    })
}

fn state_word(w: &str) -> Option<State> {
    Some(match w {
        "tapped" => State::Tapped,
        "untapped" => State::Untapped,
        "attacking" => State::Attacking,
        "blocking" => State::Blocking,
        "blocked" => State::Blocked,
        "unblocked" => State::Unblocked,
        "nontoken" => State::Nontoken,
        "face-down" => State::FaceDown,
        "other" => State::Other,
        _ => return None,
    })
}

/// A type or subtype named by a word that may be plural.
fn type_word(w: &str) -> Option<Adj> {
    let v = vocab();
    for s in singulars(w) {
        if v.card_types.contains(&s) || s == "tribal" {
            return Some(Adj::Type(s));
        }
        if v.subtypes.contains(&s) {
            return Some(Adj::Subtype(s));
        }
    }
    None
}

fn descriptor(p: &mut P) -> Option<Adj> {
    if p.words("attacking or blocking").is_some() {
        return Some(Adj::State(State::AttackingOrBlocking));
    }
    let w = p.peek_word()?;
    let adj = if let Some(c) = color_word(w) {
        Adj::Color(c)
    } else if let Some(s) = state_word(w) {
        Adj::State(s)
    } else if vocab().supertypes.contains(w) {
        Adj::Supertype(w.to_string())
    } else if let Some(t) = type_word(w) {
        t
    } else {
        let rest = w.strip_prefix("non")?.trim_start_matches('-');
        if let Some(c) = color_word(rest) {
            Adj::NonColor(c)
        } else if type_word(rest).is_some() || vocab().supertypes.contains(rest) {
            Adj::Non(rest.to_string())
        } else {
            return None;
        }
    };
    p.bump();
    Some(adj)
}

fn kind(p: &mut P) -> Option<Noun> {
    alt!(p;
        |p| p.one_of(&["spells or abilities", "spell or ability"]).map(|_| Noun::SpellOrAbility),
        |p| p.one_of(&["permanent cards", "permanent card"]).map(|_| Noun::PermanentCard),
        |p| p.one_of(&["permanents", "permanent"]).map(|_| Noun::Permanent),
        |p| p.one_of(&["spells", "spell"]).map(|_| Noun::Spell),
        |p| p.one_of(&["cards", "card"]).map(|_| Noun::Card),
        |p| p.one_of(&["tokens", "token"]).map(|_| Noun::Token),
        |p| p.one_of(&["players", "player"]).map(|_| Noun::Player),
        |p| p.one_of(&["opponents", "opponent"]).map(|_| Noun::Opponent),
        |p| p.one_of(&["abilities", "ability"]).map(|_| Noun::Ability),
        |p| p.one_of(&["sources", "source"]).map(|_| Noun::Source),
    )
}

fn group(p: &mut P) -> Option<Vec<Adj>> {
    let mut v = Vec::new();
    while let Some(a) = p.attempt(descriptor) {
        v.push(a);
    }
    (!v.is_empty()).then_some(v)
}

fn is_typed(a: &Adj) -> bool {
    matches!(a, Adj::Type(_) | Adj::Subtype(_))
}

fn filter(p: &mut P) -> Option<Filter> {
    let mut groups = Vec::new();
    if let Some(g) = p.attempt(group) {
        groups.push(g);
        // A comma list ending in "or" is an or; without the "or" it is an and.
        let mut commas = Vec::new();
        while let Some(g) = p.attempt(|p| {
            p.tok(&Tok::Comma)?;
            group(p)
        }) {
            commas.push(g);
        }
        let last = p.attempt(|p| {
            if !commas.is_empty() {
                p.tok(&Tok::Comma)?;
            }
            p.one_of(&["and/or", "or", "and"])?;
            group(p)
        });
        match last {
            Some(l) => {
                groups.extend(commas);
                groups.push(l);
            }
            None => {
                for c in commas {
                    groups[0].extend(c);
                }
            }
        }
    }
    let kind = p.attempt(kind);
    if groups.is_empty() && kind.is_none() {
        return None;
    }
    // "white or black creature": the last group's type belongs to every group.
    if let Some(last) = groups.last() {
        let shared: Vec<Adj> = last.iter().filter(|a| is_typed(a)).cloned().collect();
        let n = groups.len();
        for g in groups.iter_mut().take(n - 1) {
            if !g.iter().any(is_typed) {
                g.extend(shared.iter().cloned());
            }
        }
    }
    let mut posts = Vec::new();
    while let Some(post) = p.attempt(post) {
        posts.push(post);
    }
    Some(Filter {
        alts: groups,
        kind,
        posts,
    })
}

fn compare(p: &mut P) -> Option<Compare> {
    let stat = stat(p)?;
    alt!(p;
        |p| {
            let value = amount(p)?;
            let op = alt!(p;
                |p| p.words("or less").map(|_| Op::Le),
                |p| p.words("or greater").map(|_| Op::Ge),
                |p| p.words("or more").map(|_| Op::Ge),
            ).unwrap_or(Op::Eq);
            Some(Compare { stat, op, value })
        },
        |p| {
            let op = alt!(p;
                |p| p.words("less than or equal to").map(|_| Op::Le),
                |p| p.words("greater than or equal to").map(|_| Op::Ge),
                |p| p.words("less than").map(|_| Op::Lt),
                |p| p.words("greater than").map(|_| Op::Gt),
                |p| p.words("equal to").map(|_| Op::Eq),
            )?;
            let value = measure(p)?;
            Some(Compare { stat, op, value })
        },
    )
}

fn post(p: &mut P) -> Option<Post> {
    alt!(p;
        |p| p.words("you control").map(|_| Post::Controller(Player::You)),
        |p| p.words("you don't control").map(|_| Post::NotControlledBy(Player::You)),
        |p| p.words("you own").map(|_| Post::Owner(Player::You)),
        |p| p.words("you don't own").map(|_| Post::Owner(Player::EachOpponent)),
        |p| { let pl = player_simple(p)?; p.one_of(&["controls", "control"])?; Some(Post::Controller(pl)) },
        |p| { let pl = player_simple(p)?; p.one_of(&["doesn't control", "don't control"])?; Some(Post::NotControlledBy(pl)) },
        |p| { p.word("with")?; compare(p).map(Post::With) },
        |p| {
            p.word("with")?;
            p.one_of(&["a", "one or more"])?;
            let k = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.one_of(&["on it", "on them"])?;
            Some(Post::WithCounter(k))
        },
        |p| { p.word("with")?; let k = keyword_name(p)?; Some(Post::WithKeyword(k)) },
        |p| { p.word("without")?; let k = keyword_name(p)?; Some(Post::WithoutKeyword(k)) },
        |p| { p.one_of(&["in", "from"])?; zone(p).map(Post::In) },
        |p| p.one_of(&["that's attacking", "that are attacking"]).map(|_| Post::Attacking),
        |p| p.one_of(&["that's blocking", "that are blocking"]).map(|_| Post::Blocking),
        |p| p.words("other than ~").map(|_| Post::OtherThanSelf),
        |p| p.words("of the chosen type").map(|_| Post::OfChosenType),
        |p| p.words("of the chosen color").map(|_| Post::OfChosenType),
        |p| p.words("attached to it").map(|_| Post::AttachedToIt),
        |p| p.words("that shares a creature type with it").map(|_| Post::SharesType),
        |p| p.one_of(&["from among them", "revealed this way", "from among those cards"]).map(|_| Post::FromAmongThem),
        |p| { p.words("exiled with")?; objects(p).map(|o| Post::ExiledWith(Box::new(o))) },
        |p| p.words("you cast").map(|_| Post::Cast(Player::You)),
        |p| { let pl = player_simple(p)?; p.one_of(&["casts", "cast"])?; Some(Post::Cast(pl)) },
    )
}

/// Players that can stand before "controls" without eating an object noun.
fn player_simple(p: &mut P) -> Option<Player> {
    alt!(p;
        |p| p.words("target player").map(|_| Player::Target),
        |p| p.words("target opponent").map(|_| Player::TargetOpponent),
        |p| p.words("an opponent").map(|_| Player::AnOpponent),
        |p| p.words("your opponents").map(|_| Player::EachOpponent),
        |p| p.words("that player").map(|_| Player::That),
        |p| p.words("defending player").map(|_| Player::DefendingPlayer),
        |p| p.words("the chosen player").map(|_| Player::Chosen),
        |p| p.words("a player").map(|_| Player::APlayer),
        |p| p.words("its controller").map(|_| Player::ControllerOf(Box::new(antecedent()))),
        |p| p.word("they").map(|_| Player::That),
    )
}

fn quant(p: &mut P) -> Option<Quant> {
    alt!(p;
        |p| { p.words("up to")?; let n = amount(p)?; p.word("target")?; Some(Quant::Target { n, up_to: true }) },
        |p| { p.words("up to")?; amount(p).map(Quant::UpTo) },
        |p| p.words("another target").map(|_| Quant::AnotherTarget),
        |p| { p.words("any number of target").map(|_| Quant::Target { n: Amount::X, up_to: true }) },
        |p| { let n = amount(p)?; p.word("target")?; Some(Quant::Target { n, up_to: false }) },
        |p| { let n = amount(p)?; p.words("or more")?; Some(Quant::AtLeast(n)) },
        |p| { let n = amount(p)?; p.words("or fewer").map(|_| Quant::AtMost(n)) },
        |p| p.word("target").map(|_| Quant::Target { n: Amount::N(1), up_to: false }),
        |p| p.words("each other").map(|_| Quant::Each),
        |p| p.word("each").map(|_| Quant::Each),
        |p| p.words("all other").map(|_| Quant::All),
        |p| p.word("all").map(|_| Quant::All),
        |p| p.word("another").map(|_| Quant::Another),
        |p| p.words("one or more").map(|_| Quant::OneOrMore),
        |p| p.words("any number of").map(|_| Quant::AnyNumber),
        |p| p.one_of(&["a", "an"]).map(|_| Quant::One),
        |p| amount(p).map(Quant::N),
    )
}

fn pronoun(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| p.tok(&Tok::SelfRef).map(|_| self_ref()),
        |p| { p.word("this")?; let f = filter(p)?; Some(Objects { quant: Quant::SelfRef, filter: f }) },
        |p| p.one_of(&["each of them", "it", "them", "they"]).map(|_| antecedent()),
        |p| p.words("the rest").map(|_| Objects { quant: Quant::Rest, filter: Filter::default() }),
        |p| p.one_of(&["the other", "the others"]).map(|_| antecedent()),
        |p| {
            let n = amount_a(p)?;
            let posts = if p.attempt(|p| p.words("of them")).is_some() { vec![Post::FromAmongThem] } else { vec![] };
            // A bare number names the cards just found: "put one onto the battlefield".
            if posts.is_empty() && !matches!(p.peek_word(), Some("onto" | "into" | "on")) { return None; }
            Some(Objects { quant: Quant::N(n), filter: Filter { alts: vec![], kind: None, posts } })
        },
        |p| {
            p.one_of(&["that", "those", "the"])?;
            // "the exiled card", "the token", "those creatures".
            p.attempt(|p| p.one_of(&["exiled", "chosen", "revealed", "sacrificed", "copied", "returned"]));
            let f = filter(p)?;
            Some(Objects { quant: Quant::Antecedent, filter: f })
        },
        |p| { p.word("enchanted")?; let f = filter(p)?; Some(Objects { quant: Quant::Enchanted, filter: f }) },
        |p| { p.word("equipped")?; let f = filter(p)?; Some(Objects { quant: Quant::Equipped, filter: f }) },
        |p| {
            p.words("the top")?;
            let n = p.attempt(amount).unwrap_or(Amount::N(1));
            p.one_of(&["cards", "card"])?;
            p.word("of")?;
            let w = whose(p)?;
            p.word("library")?;
            Some(Objects {
                quant: Quant::N(n),
                filter: Filter { alts: vec![], kind: Some(Noun::Card), posts: vec![Post::In(Zone::TopOfLibrary(w))] },
            })
        },
    )
}

fn objects_no_player(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| {
            let first = pronoun(p)?;
            if first.quant != Quant::SelfRef { return None; }
            p.words("or another")?;
            let filter = filter(p)?;
            Some(Objects { quant: Quant::SelfOrAnother, filter })
        },
        pronoun,
        |p| p.words("any target").map(|_| Objects { quant: Quant::AnyTarget { n: Amount::N(1) }, filter: Filter::default() }),
        |p| { let n = amount(p)?; p.words("target")?; p.word("targets").map(|_| Objects { quant: Quant::AnyTarget { n }, filter: Filter::default() }) },
        |p| {
            let quant = p.attempt(quant).unwrap_or(Quant::Plural);
            let filter = filter(p)?;
            Some(Objects { quant, filter })
        },
    )
}

pub fn objects(p: &mut P) -> Option<Objects> {
    objects_no_player(p)
}

// ---------------------------------------------------------------- keywords

fn keyword_name(p: &mut P) -> Option<String> {
    let candidates = vocab().keyword_abilities.get(p.peek_word()?)?;
    candidates.iter().find(|k| p.words(k).is_some()).cloned()
}

fn keyword(p: &mut P) -> Option<Keyword> {
    alt!(p;
        |p| {
            p.words("protection from")?;
            let mut cs = Vec::new();
            while let Some(c) = p.peek_word().and_then(color_word) {
                p.bump();
                cs.push(c);
                if p.attempt(|p| p.one_of(&["and from", ", from", "from"])).is_none() { break; }
            }
            if !cs.is_empty() {
                return Some(Keyword { name: "protection".into(), param: Some(KeywordParam::Colors(cs)) });
            }
            let o = alt!(p;
                |p| p.word("everything").map(|_| Objects { quant: Quant::All, filter: Filter::default() }),
                objects,
            )?;
            Some(Keyword { name: "protection".into(), param: Some(KeywordParam::Objects(Box::new(o))) })
        },
        |p| {
            let w = p.peek_word()?;
            let land = w.strip_suffix("walk")?;
            let ok = type_word(land).is_some() || land == "nonbasic " || land.is_empty();
            if !ok { return None; }
            let w = w.to_string();
            p.bump();
            Some(Keyword { name: w, param: None })
        },
        |p| {
            let name = keyword_name(p)?;
            let param = alt!(p;
                |p| { p.tok(&Tok::Dash)?; let c = costs(p)?; p.attempt(|p| p.tok(&Tok::Period)); Some(KeywordParam::Cost(c)) },
                |p| mana(p).map(|m| KeywordParam::Cost(vec![Cost::Mana(m)])),
                |p| number(p).map(|n| KeywordParam::N(Amount::N(n))),
                |p| p.word("x").map(|_| KeywordParam::N(Amount::X)),
                |p| { p.word("for")?; objects(p).map(|o| KeywordParam::Objects(Box::new(o))) },
                |p| { p.word("from")?; objects(p).map(|o| KeywordParam::Objects(Box::new(o))) },
                |p| {
                    // "Enchant creature you control", "Equip legendary creature {3}".
                    if !matches!(name.as_str(), "enchant" | "equip") { return None; }
                    let o = objects(p)?;
                    if let Some(m) = p.attempt(mana) {
                        return Some(KeywordParam::Cost(vec![Cost::Mana(m)]));
                    }
                    Some(KeywordParam::Objects(Box::new(o)))
                },
            );
            Some(Keyword { name, param })
        },
    )
}

fn keyword_line(p: &mut P) -> Option<Vec<Keyword>> {
    let mut v = vec![keyword(p)?];
    while let Some(k) = p.attempt(|p| {
        p.one_of(&[",", ";"])
            .or_else(|| p.tok(&Tok::Comma).map(|_| ","))?;
        keyword(p)
    }) {
        v.push(k);
    }
    Some(v)
}

/// "flying", "flying and trample", "first strike, vigilance, and lifelink".
fn keyword_list(p: &mut P) -> Option<Vec<Keyword>> {
    let mut v = vec![keyword(p)?];
    loop {
        let next = p.attempt(|p| {
            p.tok(&Tok::Comma)?;
            p.attempt(|p| p.word("and"));
            keyword(p)
        });
        match next {
            Some(k) => v.push(k),
            None => break,
        }
    }
    if let Some(k) = p.attempt(|p| {
        p.word("and")?;
        keyword(p)
    }) {
        v.push(k);
    }
    Some(v)
}

/// A quoted ability: `"{T}: Add {G}."`
fn quoted(p: &mut P) -> Option<Ability> {
    p.tok(&Tok::Quote)?;
    let a = ability(p, false)?;
    p.tok(&Tok::Quote)?;
    Some(a)
}

// ---------------------------------------------------------------- effects

fn duration(p: &mut P) -> Option<Duration> {
    alt!(p;
        |p| p.words("until end of turn").map(|_| Duration::UntilEndOfTurn),
        |p| p.words("this turn").map(|_| Duration::ThisTurn),
        |p| p.words("until your next turn").map(|_| Duration::UntilYourNextTurn),
        |p| p.words("for as long as you control ~").map(|_| Duration::WhileSelfOnBattlefield),
        |p| p.words("until ~ leaves the battlefield").map(|_| Duration::WhileSelfOnBattlefield),
    )
}

fn cards(p: &mut P) -> Option<Amount> {
    let n = amount_a(p)?;
    p.one_of(&["cards", "card"])?;
    Some(n)
}

/// The subject of a sentence whose verb takes a player.
fn subject_player(p: &mut P) -> Player {
    p.attempt(player).unwrap_or(Player::You)
}

fn token(p: &mut P) -> Option<(Amount, Token)> {
    let n = amount_a(p)?;
    let tapped = p.attempt(|p| p.word("tapped")).is_some();
    let pt = p.attempt(|p| match p.peek()? {
        Tok::Pt(pt) if !pt.signed => {
            let pt = *pt;
            p.bump();
            Some(pt)
        }
        _ => None,
    });
    let mut colors = Vec::new();
    while let Some(c) = p.attempt(|p| {
        p.attempt(|p| p.word("and"));
        let c = color_word(p.peek_word()?)?;
        p.bump();
        Some(c)
    }) {
        colors.push(c);
    }
    let mut types = Vec::new();
    let mut named = None;
    while let Some(a) = p.attempt(|p| {
        let w = p.peek_word()?;
        if w == "token" || w == "tokens" {
            return None;
        }
        let a = type_word(w).or_else(|| {
            vocab()
                .supertypes
                .contains(w)
                .then(|| Adj::Supertype(w.to_string()))
        })?;
        p.bump();
        Some(a)
    }) {
        match a {
            Adj::Type(t) | Adj::Supertype(t) => types.push(t),
            Adj::Subtype(s) => {
                if pt.is_none() && named.is_none() {
                    named = Some(s.clone());
                }
                types.push(s)
            }
            _ => {}
        }
    }
    if types.is_empty() {
        return None;
    }
    p.one_of(&["tokens", "token"])?;
    if pt.is_some() {
        named = None;
    }
    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    if p.attempt(|p| p.word("with")).is_some() {
        loop {
            if let Some(a) = p.attempt(quoted) {
                abilities.push(a);
            } else {
                keywords.push(p.attempt(keyword)?);
            }
            if p.attempt(|p| p.one_of(&[", and", "and", ","])).is_none() {
                break;
            }
        }
    }
    Some((
        n,
        Token {
            tapped,
            pt,
            colors,
            types,
            keywords,
            abilities,
            named,
        },
    ))
}

fn pt_signed(p: &mut P) -> Option<Pt> {
    match p.peek()? {
        Tok::Pt(pt) if pt.signed => {
            let pt = *pt;
            p.bump();
            Some(pt)
        }
        _ => None,
    }
}

fn restriction(p: &mut P) -> Option<Restriction> {
    let r = restriction_bare(p)?;
    Some(
        match p.attempt(|p| {
            p.word("unless")?;
            condition(p)
        }) {
            Some(c) => Restriction::Unless(Box::new(r), c),
            None => r,
        },
    )
}

fn restriction_bare(p: &mut P) -> Option<Restriction> {
    alt!(p;
        |p| { p.words("can block only")?; filter(p).map(Restriction::CanBlockOnly) },
        |p| p.words("can't be blocked by more than one creature").map(|_| Restriction::CantBeBlockedByMoreThanOne),
        |p| { p.words("can't be blocked by")?; filter(p).map(Restriction::CantBeBlockedBy) },
        |p| { p.words("can block an additional")?; p.attempt(|p| p.word("creature")); p.words("each combat").map(|_| Restriction::CanBlockAdditional(Amount::N(1))) },
        |p| p.words("can block any number of creatures").map(|_| Restriction::CanBlockAdditional(Amount::X)),
        |p| p.words("can't attack or block").map(|_| Restriction::CantAttackOrBlock),
        |p| p.words("can't block").map(|_| Restriction::CantBlock),
        |p| p.words("can't attack").map(|_| Restriction::CantAttack),
        |p| { p.words("can't be blocked except by")?; filter(p).map(Restriction::CantBeBlockedExceptBy) },
        |p| p.words("can't be blocked").map(|_| Restriction::CantBeBlocked),
        |p| p.words("can't be countered").map(|_| Restriction::CantBeCountered),
        |p| p.words("can't be regenerated").map(|_| Restriction::CantBeRegenerated),
        |p| p.one_of(&["attacks each combat if able", "attack each combat if able"]).map(|_| Restriction::AttacksEachCombat),
        |p| p.one_of(&["blocks each combat if able", "block each combat if able"]).map(|_| Restriction::BlocksEachCombat),
        |p| p.one_of(&["doesn't untap during your untap step", "don't untap during your untap step", "doesn't untap during its controller 's untap step"]).map(|_| Restriction::DoesntUntap),
        |p| p.one_of(&["doesn't untap during its controller 's next untap step", "don't untap during their controllers ' next untap steps", "don't untap during their controller 's next untap step"]).map(|_| Restriction::DoesntUntapNext),
    )
}

fn mana_add(p: &mut P) -> Option<ManaAdd> {
    alt!(p;
        |p| {
            let first = mana(p)?;
            let mut rest = Vec::new();
            while let Some(m) = p.attempt(|p| { p.tok(&Tok::Comma)?; mana(p) }) { rest.push(m); }
            let last = p.attempt(|p| { p.attempt(|p| p.tok(&Tok::Comma)); p.word("or")?; mana(p) });
            match last {
                Some(l) => { let mut v = vec![first]; v.extend(rest); v.push(l); Some(ManaAdd::Or(v)) }
                None if rest.is_empty() => Some(ManaAdd::Symbols(first)),
                None => None,
            }
        },
        |p| { let n = amount_a(p)?; p.one_of(&["mana of any color", "mana of any one color"]).map(|w| if w.contains("one") { ManaAdd::AnyOneColor(n.clone()) } else { ManaAdd::AnyColor(n.clone()) }) },
        |p| { let n = amount_a(p)?; p.words("mana in any combination of colors")?; Some(ManaAdd::AnyCombination(n)) },
        |p| { let n = amount_a(p)?; p.words("mana of the chosen color")?; Some(ManaAdd::ChosenColor(n)) },
        |p| { let n = amount_a(p)?; p.words("mana of any color in your commander 's color identity")?; Some(ManaAdd::CommanderIdentity(n)) },
        |p| { let n = amount_a(p)?; p.one_of(&["mana of any color that a land an opponent controls could produce", "mana of any type that a land an opponent controls could produce"])?; Some(ManaAdd::OpponentLandsCouldProduce(n)) },
        |p| { p.words("an amount of")?; let m = mana(p)?; p.words("equal to")?; let n = measure(p)?; Some(ManaAdd::AmountOf { mana: m, n }) },
    )
}

fn effect(p: &mut P) -> Option<Effect> {
    alt!(p;
        // draw
        |p| { let who = subject_player(p); p.verb("draw")?; let n = cards(p)?; Some(Effect::Draw { who, n }) },
        |p| { let who = subject_player(p); p.verb("draw")?; p.words("cards equal to")?; let n = measure(p)?; Some(Effect::Draw { who, n }) },
        // discard
        |p| { let who = subject_player(p); p.verb("discard")?; p.one_of(&["their hand", "your hand"])?; Some(Effect::DiscardHand(who)) },
        |p| {
            let who = subject_player(p);
            p.verb("discard")?;
            let what = objects(p)?;
            let random = p.attempt(|p| p.words("at random")).is_some();
            Some(Effect::Discard { who, what, random })
        },
        // damage
        |p| {
            let source = p.attempt(objects);
            p.verb("deal")?;
            let (n, to) = alt!(p;
                |p| { let n = amount(p)?; p.word("damage")?; p.word("to")?; Some((n, recipients(p)?)) },
                |p| { p.words("damage equal to")?; let n = measure(p)?; p.word("to")?; Some((n, recipients(p)?)) },
                |p| { p.words("damage to")?; let r = recipients(p)?; p.words("equal to")?; Some((measure(p)?, r)) },
            )?;
            Some(Effect::Damage { source, n, to })
        },
        // destroy / exile
        |p| { p.word("destroy")?; let what = objects(p)?; let no_regen = p.attempt(|p| p.words(". it can't be regenerated")).is_some(); Some(Effect::Destroy { what, no_regen }) },
        |p| {
            p.word("exile")?;
            let what = objects(p)?;
            p.word("until")?;
            let until = objects(p)?;
            p.words("leaves the battlefield")?;
            Some(Effect::ExileUntilLeaves { what, until })
        },
        |p| { p.word("exile")?; objects(p).map(Effect::Exile) },
        |p| {
            let who = subject_player(p);
            p.verb("pay")?;
            let cost = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(vec![Cost::PayLife(n)]) },
                costs,
            )?;
            Some(Effect::Pay { who, cost })
        },
        |p| {
            p.word("prevent")?;
            let n = alt!(p;
                |p| p.word("all").map(|_| None),
                |p| { p.words("the next")?; amount(p).map(Some) },
            )?;
            let combat_only = p.attempt(|p| p.word("combat")).is_some();
            p.words("damage that would be dealt")?;
            let to = p.attempt(|p| { p.word("to")?; recipients(p) });
            let by = p.attempt(|p| { p.word("by")?; objects(p) });
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Prevent { combat_only, n, to, by, dur })
        },
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.words("a copy of")?;
            let of = objects(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::BecomesCopy { what, of, dur })
        },
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let pt = p.attempt(|p| match p.peek()? { Tok::Pt(pt) if !pt.signed => { let pt = *pt; p.bump(); Some(pt) } _ => None });
            p.attempt(|p| p.words("base power and toughness"));
            let mut types = Vec::new();
            while let Some(a) = p.attempt(descriptor) {
                types.push(a);
            }
            if types.is_empty() && pt.is_none() { return None; }
            let gains = p.attempt(|p| { p.word("with")?; keyword_list(p) }).unwrap_or_default();
            p.attempt(|p| p.words("in addition to its other types"));
            p.attempt(|p| p.words("that's still a land"));
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Becomes { what, pt, types, gains, dur })
        },
        |p| p.words("you become the monarch").map(|_| Effect::Designation("monarch".into())),
        |p| p.words("you take the initiative").map(|_| Effect::Designation("initiative".into())),
        |p| p.words("the ring tempts you").map(|_| Effect::Designation("ring".into())),
        |p| {
            p.word("choose")?;
            let c = alt!(p;
                |p| p.words("a color").map(|_| Choice::Color),
                |p| p.words("a creature type").map(|_| Choice::CreatureType),
                |p| p.words("a card type").map(|_| Choice::CardType),
                |p| p.one_of(&["a card name", "a nonland card name"]).map(|_| Choice::CardName),
                |p| p.words("a number").map(|_| Choice::Number),
                |p| p.one_of(&["an opponent", "target opponent"]).map(|_| Choice::Opponent),
                |p| p.one_of(&["a player", "target player"]).map(|_| Choice::Player),
                |p| objects(p).map(Choice::Objects),
            )?;
            Some(Effect::Choose(c))
        },
        |p| {
            p.words("you get")?;
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Effect::GetEnergy(n))
        },
        |p| {
            p.words("you get an emblem with")?;
            let mut v = vec![quoted(p)?];
            while let Some(a) = p.attempt(|p| { p.word("and")?; quoted(p) }) { v.push(a); }
            Some(Effect::Emblem(v))
        },
        |p| { let what = objects(p)?; p.verb("enter")?; p.word("tapped")?; Some(Effect::EntersTapped(what)) },
        // "… into your hand and the rest on the bottom of your library": the
        // verb was said once for both.
        |p| {
            let what = objects(p)?;
            p.one_of(&["onto", "into", "on top of", "on the bottom of"])?;
            let (to, tapped, under_your_control) = destination(p)?;
            p.attempt(|p| p.one_of(&["in any order", "in a random order"]));
            Some(Effect::Move { what, from: None, to, tapped, under_your_control })
        },
        // moves
        |p| {
            p.word("return")?;
            let what = objects(p)?;
            let from = p.attempt(|p| { p.word("from")?; zone(p) });
            p.word("to")?;
            let (to, tapped, under_your_control) = destination(p)?;
            Some(Effect::Move { what, from, to, tapped, under_your_control })
        },
        |p| {
            p.word("put")?;
            let what = objects(p)?;
            let from = p.attempt(|p| { p.word("from")?; zone(p) });
            p.one_of(&["onto", "into", "on top of", "on the bottom of", "on"])?;
            let (to, tapped, under_your_control) = destination(p)?;
            p.attempt(|p| p.one_of(&["in any order", "in a random order"]));
            Some(Effect::Move { what, from, to, tapped, under_your_control })
        },
        // counter
        |p| {
            p.word("counter")?;
            let what = objects(p)?;
            let unless = p.attempt(|p| { p.word("unless")?; let who = player(p)?; p.verb("pay")?; Some((who, costs(p)?)) });
            Some(Effect::Counter { what, unless })
        },
        // tokens
        |p| { p.word("create")?; p.words("a token that's a copy of")?; let of = objects(p)?; Some(Effect::CreateCopy { n: Amount::N(1), of }) },
        |p| { let who = subject_player(p); p.verb("create")?; let (n, token) = token(p)?; Some(Effect::Create { who, n, token }) },
        // counters
        |p| {
            p.word("put")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("on")?;
            let on = objects(p)?;
            Some(Effect::PutCounters { n, kind, on })
        },
        |p| {
            p.word("remove")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("from")?;
            let from = objects(p)?;
            Some(Effect::RemoveCounters { n, kind, from })
        },
        // life
        |p| {
            let who = subject_player(p);
            p.verb("gain")?;
            let n = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(n) },
                |p| { p.words("life equal to")?; measure(p) },
            )?;
            Some(Effect::GainLife { who, n })
        },
        |p| {
            let who = subject_player(p);
            p.verb("lose")?;
            let n = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(n) },
                |p| { p.words("life equal to")?; measure(p) },
            )?;
            Some(Effect::LoseLife { who, n })
        },
        // mana
        |p| { p.word("add")?; mana_add(p).map(Effect::AddMana) },
        // library
        |p| {
            let who = subject_player(p);
            p.verb("search")?;
            let whose = whose(p)?;
            let zones = alt!(p;
                |p| p.words("library and/or graveyard").map(|_| vec![Zone::Library(whose.clone()), Zone::Graveyard(whose.clone())]),
                |p| p.words("graveyard , hand , and library").map(|_| vec![Zone::Graveyard(whose.clone()), Zone::Hand(whose.clone()), Zone::Library(whose.clone())]),
                |p| p.word("library").map(|_| vec![Zone::Library(whose.clone())]),
            )?;
            p.word("for")?;
            let what = objects(p)?;
            Some(Effect::Search { who, zones, what })
        },
        |p| { let who = subject_player(p); p.verb("shuffle")?; p.attempt(|p| p.one_of(&["your library", "their library", "it into their library"])); Some(Effect::Shuffle(who)) },
        |p| { p.word("scry")?; amount(p).map(Effect::Scry) },
        |p| { p.word("surveil")?; amount(p).map(Effect::Surveil) },
        |p| { let who = subject_player(p); p.verb("mill")?; let n = cards(p)?; Some(Effect::Mill { who, n }) },
        |p| { let who = subject_player(p); p.words("look at the top")?; let n = cards(p)?; p.word("of")?; let whose = whose(p)?; p.word("library")?; Some(Effect::LookAtTop { who, whose, n }) },
        |p| { let who = subject_player(p); p.verb("reveal")?; p.words("the top")?; let n = cards(p)?; p.word("of")?; let whose = whose(p)?; p.word("library")?; Some(Effect::RevealTop { who, whose, n }) },
        |p| { let who = subject_player(p); p.verb("reveal")?; let what = objects(p)?; Some(Effect::Reveal { who, what }) },
        // permanents
        |p| { p.word("tap")?; objects(p).map(Effect::Tap) },
        |p| { p.word("untap")?; objects(p).map(Effect::Untap) },
        |p| { let who = subject_player(p); p.verb("sacrifice")?; let what = objects(p)?; Some(Effect::Sacrifice { who, what }) },
        |p| { let a = objects(p)?; p.verb("fight")?; let b = objects(p)?; Some(Effect::Fight { a, b }) },
        |p| { p.words("gain control of")?; let what = objects(p)?; let dur = p.attempt(duration).unwrap_or(Duration::Permanent); Some(Effect::GainControl { what, dur }) },
        |p| { p.word("transform")?; objects(p).map(Effect::Transform) },
        |p| { p.word("regenerate")?; objects(p).map(Effect::Regenerate) },
        |p| { p.word("attach")?; let what = objects(p)?; p.word("to")?; let to = objects(p)?; Some(Effect::Attach { what, to }) },
        |p| { p.word("copy")?; let what = objects(p)?; Some(Effect::Copy { what, may_choose_new_targets: false }) },
        |p| p.words("you may choose new targets for the copy").map(|_| Effect::ChooseNewTargets),
        |p| p.words("take an extra turn after this one").map(|_| Effect::ExtraTurn),
        // modify
        |p| {
            let what = objects(p)?;
            let (pt, gains) = alt!(p;
                |p| {
                    p.verb("get")?;
                    let pt = pt_signed(p)?;
                    let gains = p.attempt(|p| { p.word("and")?; p.verb("gain")?; keyword_list(p) }).unwrap_or_default();
                    Some((Some(pt), gains))
                },
                |p| { p.verb("gain")?; Some((None, keyword_list(p)?)) },
            )?;
            let dur = duration(p)?;
            Some(Effect::Modify { what, pt, gains, dur })
        },
        |p| {
            let what = objects(p)?;
            p.verb("gain")?;
            let ability = quoted(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::GrantAbility { what, ability: Box::new(ability), dur })
        },
        |p| {
            let what = objects(p)?;
            let rule = restriction(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Restrict { what, rule, dur })
        },
        // keyword actions: "proliferate", "investigate", "~ explores", "amass Zombies 2"
        |p| {
            let by = p.attempt(objects);
            let w = p.peek_word()?;
            let base = w.strip_suffix("es").filter(|b| vocab().keyword_actions.contains(*b))
                .or_else(|| w.strip_suffix('s').filter(|b| vocab().keyword_actions.contains(*b)))
                .unwrap_or(w);
            if !vocab().keyword_actions.contains(base) { return None; }
            let base = base.to_string();
            p.bump();
            if let Some(n) = p.attempt(amount) {
                return Some(Effect::Action { by, name: base, n: Some(n) });
            }
            p.end_of_clause().then_some(Effect::Action { by, name: base, n: None })
        },
        |p| p.words("venture into the dungeon").map(|_| Effect::Action { by: None, name: "venture into the dungeon".into(), n: None }),
    )
}

fn recipients(p: &mut P) -> Option<Vec<Recipient>> {
    let one = |p: &mut P| {
        alt!(p;
            |p| player_simple(p).or_else(|| p.words("each player").map(|_| Player::Each)).or_else(|| p.words("each opponent").map(|_| Player::EachOpponent)).or_else(|| p.word("you").map(|_| Player::You)).map(Recipient::Player),
            |p| objects(p).map(Recipient::Objects),
        )
    };
    let mut v = vec![one(p)?];
    while let Some(r) = p.attempt(|p| {
        p.attempt(|p| p.tok(&Tok::Comma));
        p.word("and")?;
        let r = one(p)?;
        // "any target and you gain 3 life": a recipient followed by a verb was
        // the subject of the next clause, not one more recipient.
        let ok = match p.peek_word() {
            None => true,
            Some(w) => matches!(
                w,
                "and" | "then" | "equal" | "where" | "for" | "this" | "instead" | "unless" | "if"
            ),
        };
        ok.then_some(r)
    }) {
        v.push(r);
    }
    Some(v)
}

fn holding(p: &mut P) -> Option<Holding> {
    alt!(p;
        |p| p.word("life").map(|_| Holding::Life),
        |p| p.one_of(&["cards in hand", "cards in their hand", "cards in your hand"]).map(|_| Holding::CardsInHand),
        |p| p.one_of(&["cards in your graveyard", "cards in their graveyard"]).map(|_| Holding::CardsInGraveyard),
        |p| p.word("opponents").map(|_| Holding::Opponents),
    )
}

fn condition(p: &mut P) -> Option<Condition> {
    alt!(p;
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.one_of(&["have", "has"])?;
            let at_least = number(p)?;
            p.words("or more")?;
            let what = holding(p)?;
            Some(Condition::PlayerHas { who, at_least, what })
        },
        |p| { let who = player_simple(p)?; p.one_of(&["controls", "control"])?; let o = objects(p)?; Some(Condition::PlayerControls(who, o)) },
        |p| p.words("you cast it").map(|_| Condition::YouCastIt),
        |p| p.words("no spells were cast last turn").map(|_| Condition::NoSpellsCastLastTurn),
        |p| p.words("a player cast two or more spells last turn").map(|_| Condition::TwoSpellsCastByAPlayerLastTurn),
        |p| { p.words("you control")?; objects(p).map(Condition::YouControl) },
        |p| { p.words("you don't control")?; objects(p).map(Condition::YouDontControl) },
        |p| { p.words("an opponent controls")?; objects(p).map(|o| Condition::Opponent(Box::new(Condition::YouControl(o)))) },
        |p| p.words("it's your turn").map(|_| Condition::ItsYourTurn),
        |p| p.words("it's not your turn").map(|_| Condition::NotYourTurn),
        |p| { p.words("you have")?; let n = number(p)?; p.words("or more life")?; Some(Condition::LifeAtLeast(n)) },
        |p| { p.words("you have")?; let n = number(p)?; p.words("or less life")?; Some(Condition::LifeAtMost(n)) },
        |p| { p.one_of(&["this spell was kicked", "~ was kicked", "it was kicked"]).map(|_| Condition::WasKicked) },
        |p| p.words("you attacked this turn").map(|_| Condition::YouAttackedThisTurn),
        |p| {
            p.words("there are")?;
            let n = number(p)?;
            p.words("or more")?;
            let f = filter(p)?;
            Some(Condition::Graveyard(Amount::N(n), f))
        },
        |p| {
            let what = objects(p)?;
            p.one_of(&["is", "was", "are"])?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let f = filter(p)?;
            Some(Condition::Is { what, filter: f })
        },
        |p| { p.word("it's")?; p.attempt(|p| p.one_of(&["a", "an"])); let f = filter(p)?; Some(Condition::Is { what: antecedent(), filter: f }) },
    )
}

/// What can trail an effect and qualify it: "unless that player pays {1}",
/// "for each creature you control", "where X is …", "if …", a delayed step.
fn suffixed(p: &mut P, mut e: Effect) -> Effect {
    loop {
        if let Some((who, cost)) = p.attempt(|p| {
            p.word("unless")?;
            let who = player(p)?;
            p.verb("pay")?;
            Some((who, costs(p)?))
        }) {
            e = Effect::Unless {
                effect: Box::new(e),
                who,
                cost,
            };
        } else if let Some(each) = p.attempt(|p| {
            p.words("for each")?;
            objects(p)
        }) {
            e = Effect::ForEach {
                each,
                effect: Box::new(e),
            };
        } else if let Some(x) = p.attempt(|p| {
            p.attempt(|p| p.tok(&Tok::Comma));
            p.words("where x is")?;
            measure(p)
        }) {
            e = Effect::Where {
                effect: Box::new(e),
                x,
            };
        } else if let Some(cond) = p.attempt(|p| {
            p.word("if")?;
            condition(p)
        }) {
            e = Effect::If {
                cond,
                then: vec![e],
            };
        } else if let Some(s) = p.attempt(|p| {
            p.words("at the beginning of")?;
            step(p)
        }) {
            e = Effect::Delayed {
                step: s,
                then: vec![e],
            };
        } else {
            return e;
        }
    }
}

fn clause(p: &mut P) -> Option<Vec<Effect>> {
    alt!(p;
        |p| {
            p.one_of(&["if you do", "if they do", "if that player does", "if the player does", "when you do"])?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::IfYouDo(clauses(p)?)])
        },
        |p| {
            p.one_of(&["if you don't", "if they don't", "if that player doesn't", "if the player doesn't", "if no one does", "otherwise"])?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::IfYouDont(clauses(p)?)])
        },
        |p| { p.word("if")?; let cond = condition(p)?; p.tok(&Tok::Comma)?; let then = clauses(p)?; Some(vec![Effect::If { cond, then }]) },
        |p| { p.words("at the beginning of")?; let step = step(p)?; p.tok(&Tok::Comma)?; let then = clauses(p)?; Some(vec![Effect::Delayed { step, then }]) },
        |p| { let dur = duration(p)?; p.tok(&Tok::Comma)?; let then = clauses(p)?; Some(vec![Effect::During { dur, then }]) },
        |p| { p.word("then")?; clause(p) },
        |p| {
            let who = subject_player(p);
            p.word("may")?;
            let e = effect(p)?;
            Some(vec![Effect::May { who, then: vec![suffixed(p, e)] }])
        },
        |p| { let e = effect(p)?; Some(vec![suffixed(p, e)]) },
    )
}

fn clauses(p: &mut P) -> Option<Vec<Effect>> {
    let mut v = clause(p)?;
    while let Some(more) = p.attempt(|p| {
        p.one_of(&[
            ", and then",
            ", then",
            ", and",
            "and then",
            "and",
            "then",
            ",",
        ])?;
        clause(p)
    }) {
        v.extend(more);
    }
    Some(v)
}

fn modal(p: &mut P) -> Option<Effect> {
    p.word("choose")?;
    let choose = alt!(p;
        |p| p.words("one or both").map(|_| Choose::OneOrBoth),
        |p| p.words("one or more").map(|_| Choose::OneOrMore),
        |p| p.words("any number").map(|_| Choose::AnyNumber),
        |p| { p.words("up to")?; number(p).map(Choose::UpTo) },
        |p| p.word("one").map(|_| Choose::One),
        |p| number(p).map(Choose::N),
    )?;
    p.tok(&Tok::Dash)?;
    let mut modes = Vec::new();
    while p.attempt(|p| p.tok(&Tok::Bullet)).is_some() {
        let mut mode = Vec::new();
        while !matches!(p.peek(), None | Some(Tok::Bullet)) {
            mode.extend(sentence(p)?);
        }
        modes.push(mode);
    }
    (!modes.is_empty()).then_some(Effect::Modal { choose, modes })
}

fn sentence(p: &mut P) -> Option<Vec<Effect>> {
    alt!(p;
        |p| modal(p).map(|m| vec![m]),
        |p| { let e = clauses(p)?; p.tok(&Tok::Period)?; Some(e) },
    )
}

pub fn effects(p: &mut P) -> Option<Vec<Effect>> {
    let mut v = Vec::new();
    while let Some(s) = p.attempt(sentence) {
        v.extend(s);
    }
    (!v.is_empty()).then_some(v)
}

// ---------------------------------------------------------------- triggers

fn whose_step(p: &mut P) -> Option<Whose> {
    alt!(p;
        |p| p.word("your").map(|_| Whose::Yours),
        |p| p.words("each opponent 's").map(|_| Whose::EachOpponent),
        |p| p.words("each player 's").map(|_| Whose::Each),
        |p| p.word("each").map(|_| Whose::Each),
        |p| p.words("that player 's").map(|_| Whose::ThatPlayers),
        |p| p.words("the next turn 's").map(|_| Whose::The),
        |p| p.words("the next").map(|_| Whose::The),
        |p| p.word("the").map(|_| Whose::The),
    )
}

fn step(p: &mut P) -> Option<Step> {
    let w = whose_step(p)?;
    alt!(p;
        |p| p.word("upkeep").map(|_| Step::Upkeep(w.clone())),
        |p| p.words("draw step").map(|_| Step::DrawStep(w.clone())),
        |p| p.words("precombat main phase").map(|_| Step::PrecombatMain(w.clone())),
        |p| p.words("postcombat main phase").map(|_| Step::SecondMain(w.clone())),
        |p| p.words("end step").map(|_| Step::EndStep(w.clone())),
        |p| p.words("end of combat").map(|_| Step::EndOfCombat(w.clone())),
        |p| p.word("combat").map(|_| Step::Combat(w.clone())),
    )
    .map(|s| {
        p.attempt(|p| p.words("on your turn"));
        s
    })
}

fn single_event(p: &mut P) -> Option<Event> {
    alt!(p;
        |p| p.words("you attack").map(|_| Event::YouAttack),
        |p| { let who = player(p)?; p.verb("cast")?; let what = objects(p)?; Some(Event::Casts { who, what }) },
        |p| { let who = player(p)?; p.verb("gain")?; p.word("life")?; Some(Event::GainsLife(who)) },
        |p| { let who = player(p)?; p.verb("lose")?; p.word("life")?; Some(Event::LosesLife(who)) },
        |p| { let who = player(p)?; p.verb("draw")?; p.words("a card")?; Some(Event::Draws(who)) },
        |p| { let who = player(p)?; p.verb("discard")?; let what = objects(p)?; Some(Event::Discards { who, what }) },
        |p| { let who = player(p)?; p.verb("cycle")?; let what = p.attempt(objects); Some(Event::Cycles { who, what }) },
        |p| { let who = player(p)?; p.verb("sacrifice")?; let what = objects(p)?; Some(Event::Sacrifices { who, what }) },
        |p| {
            // "enters or attacks": one subject, several predicates.
            let what = objects(p)?;
            let first = predicate(p, &what)?;
            let mut v = vec![first];
            while let Some(e) = p.attempt(|p| { p.word("or")?; predicate(p, &what) }) {
                v.push(e);
            }
            Some(if v.len() == 1 { v.remove(0) } else { Event::Or(v) })
        },
    )
}

fn predicate(p: &mut P, what: &Objects) -> Option<Event> {
    alt!(p;
        |p| {
            p.verb("enter")?;
            p.attempt(|p| p.words("the battlefield"));
            let under = p.attempt(|p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(w) });
            Some(Event::Enters { what: what.clone(), under })
        },
        |p| p.verb("die").map(|_| Event::Dies(what.clone())),
        |p| p.words("leaves the battlefield").map(|_| Event::LeavesBattlefield(what.clone())),
        |p| p.verb("attack or block").map(|_| Event::AttacksOrBlocks(what.clone())),
        |p| p.verb("attack").map(|_| Event::Attacks(what.clone())),
        |p| p.verb("block").map(|_| Event::Blocks(what.clone())),
        |p| p.one_of(&["becomes blocked", "become blocked"]).map(|_| Event::BecomesBlocked(what.clone())),
        |p| { p.verb("deal")?; p.words("combat damage to")?; p.one_of(&["a player", "an opponent", "one or more players", "a player or planeswalker"])?; Some(Event::DealsCombatDamageToPlayer(what.clone())) },
        |p| { p.verb("deal")?; p.word("damage")?; Some(Event::DealsDamage(what.clone())) },
        |p| p.one_of(&["is dealt damage", "are dealt damage"]).map(|_| Event::IsDealtDamage(what.clone())),
        |p| p.one_of(&["becomes tapped", "become tapped"]).map(|_| Event::BecomesTapped(what.clone())),
        |p| { p.one_of(&["is put into", "are put into"])?; let to = zone(p)?; p.word("from")?; let from = zone(p)?; Some(Event::PutInto { what: what.clone(), to, from }) },
        |p| { p.one_of(&["becomes the target of", "become the target of"])?; let of = p.attempt(objects); Some(Event::BecomesTarget { what: what.clone(), of }) },
    )
}

fn event(p: &mut P) -> Option<Event> {
    let first = single_event(p)?;
    let mut rest = Vec::new();
    while let Some(e) = p.attempt(|p| {
        p.word("or")?;
        single_event(p)
    }) {
        rest.push(e);
    }
    if rest.is_empty() {
        Some(first)
    } else {
        let mut v = vec![first];
        v.extend(rest);
        Some(Event::Or(v))
    }
}

fn trigger(p: &mut P) -> Option<Trigger> {
    alt!(p;
        |p| { p.words("at the beginning of")?; step(p).map(Trigger::At) },
        |p| { p.one_of(&["whenever", "when"])?; event(p).map(Trigger::When) },
    )
}

// ---------------------------------------------------------------- statics

fn static_sentence(p: &mut P) -> Option<Vec<Static>> {
    // "As this land enters, you may pay 2 life. If you don't, it enters
    // tapped." runs over several sentences, so it reads its own periods.
    if let Some(s) = p.attempt(|p| {
        p.word("as")?;
        let what = objects(p)?;
        p.verb("enter")?;
        p.attempt(|p| p.words("the battlefield"));
        p.tok(&Tok::Comma)?;
        effects(p).map(|then| vec![Static::AsEnters { what, then }])
    }) {
        return Some(s);
    }
    let s = alt!(p;
        |p| {
            p.words("as long as")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            let then = static_joined(p)?;
            Some(vec![Static::AsLongAs { cond, then }])
        },
        |p| {
            p.words("during your turn ,")?;
            let then = static_joined(p)?;
            Some(vec![Static::AsLongAs { cond: Condition::ItsYourTurn, then }])
        },
        |p| {
            p.word("if")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            let then = static_joined(p)?;
            Some(vec![Static::If { cond, then }])
        },
        |p| {
            let then = static_joined(p)?;
            p.words("as long as")?;
            let cond = condition(p)?;
            Some(vec![Static::AsLongAs { cond, then }])
        },
        replacement,
        static_joined,
    )?;
    p.tok(&Tok::Period)?;
    Some(s)
}

/// "can't attack or block, and its activated abilities can't be activated".
fn static_joined(p: &mut P) -> Option<Vec<Static>> {
    let mut v = static_body(p)?;
    while let Some(more) = p.attempt(|p| {
        p.one_of(&[", and", "and"])?;
        static_body(p)
    }) {
        v.extend(more);
    }
    Some(v)
}

/// What a replacement effect watches for, after "would".
fn would(p: &mut P, what: &Objects) -> Option<Event> {
    alt!(p;
        |p| p.verb("die").map(|_| Event::Dies(what.clone())),
        |p| { p.words("be put into")?; let to = zone(p)?; p.word("from")?; let from = zone(p)?; Some(Event::PutInto { what: what.clone(), to, from }) },
        |p| { p.words("deal damage")?; p.attempt(|p| { p.word("to")?; recipients(p) }); Some(Event::DealsDamage(what.clone())) },
        |p| p.words("be dealt damage").map(|_| Event::IsDealtDamage(what.clone())),
        |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); Some(Event::Enters { what: what.clone(), under: None }) },
    )
}

fn replacement(p: &mut P) -> Option<Vec<Static>> {
    p.word("if")?;
    let event = alt!(p;
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("would")?;
            alt!(p;
                |p| p.words("draw a card").map(|_| Event::Draws(who.clone())),
                |p| p.words("gain life").map(|_| Event::GainsLife(who.clone())),
                |p| p.words("lose life").map(|_| Event::LosesLife(who.clone())),
            )
        },
        |p| { let what = objects(p)?; p.word("would")?; would(p, &what) },
    )?;
    p.tok(&Tok::Comma)?;
    let instead = clauses(p)?;
    p.attempt(|p| p.word("instead"));
    Some(vec![Static::Replacement {
        event: Box::new(event),
        instead,
    }])
}

fn static_body(p: &mut P) -> Option<Vec<Static>> {
    alt!(p;
        |p| p.words("you may play an additional land on each of your turns").map(|_| vec![Static::ExtraLand]),
        |p| p.words("you have no maximum hand size").map(|_| vec![Static::MaxHandSize]),
        |p| { p.words("you have")?; keyword_list(p).map(|k| vec![Static::YouHave(k)]) },
        |p| p.words("~ can be your commander").map(|_| vec![Static::CanBeCommander]),
        |p| p.words("you may look at the top card of your library any time").map(|_| vec![Static::LookAtTopAnyTime]),
        |p| { p.words("as an additional cost to cast this spell ,")?; costs(p).map(|c| vec![Static::AdditionalCost(c)]) },
        |p| {
            p.words("you may")?;
            let c = costs(p)?;
            p.words("rather than pay this spell 's mana cost")?;
            Some(vec![Static::AlternativeCost(c)])
        },
        |p| { p.words("you control")?; objects(p).map(|o| vec![Static::Control(o)]) },
        |p| {
            p.words("you may choose not to untap")?;
            let what = objects(p)?;
            p.words("during your untap step")?;
            Some(vec![Static::Restrict { what, rule: Restriction::MayNotUntap }])
        },
        |p| {
            p.words("its activated abilities can't be activated")
                .map(|_| vec![Static::Restrict { what: antecedent(), rule: Restriction::NoActivatedAbilities }])
        },
        |p| {
            let what = objects(p)?;
            p.tok(&Tok::Possessive)?;
            p.words("power is equal to")?;
            let equal_to = measure(p)?;
            Some(vec![Static::SetPower { what, equal_to }])
        },
        |p| {
            p.words("you may")?;
            let what = alt!(p;
                |p| { p.word("play")?; objects(p) },
                |p| { p.word("cast")?; objects(p) },
            )?;
            let mut what = what;
            // The object's own "from your graveyard" got there first.
            let zone = match what.filter.posts.last() {
                Some(Post::In(z)) => { let z = z.clone(); what.filter.posts.pop(); z }
                _ => { p.word("from")?; zone(p)? }
            };
            Some(vec![Static::PlayFrom { what, zone }])
        },
        |p| {
            p.words("spend this mana only to cast")?;
            objects(p).map(|o| vec![Static::SpendOnly(o)])
        },
        |p| {
            let what = objects(p)?;
            let r = alt!(p;
                |p| {
                    p.verb("get")?;
                    let pt = pt_signed(p)?;
                    let per = p.attempt(|p| { p.words("for each")?; objects(p) });
                    let has = p.attempt(|p| { p.word("and")?; p.verb("have")?; keyword_list(p) }).unwrap_or_default();
                    Some(vec![Static::Modify { what: what.clone(), pt: Some(pt), has, per }])
                },
                |p| { p.verb("have")?; let k = keyword_list(p)?; Some(vec![Static::Modify { what: what.clone(), pt: None, has: k, per: None }]) },
                |p| {
                    p.verb("enter")?;
                    p.attempt(|p| p.words("the battlefield"));
                    p.words("tapped unless")?;
                    let unless = condition(p)?;
                    Some(vec![Static::EntersTappedUnless { what: what.clone(), unless }])
                },
                |p| { p.verb("have")?; let a = quoted(p)?; Some(vec![Static::HasAbility { what: what.clone(), ability: Box::new(a) }]) },
                |p| restriction(p).map(|rule| vec![Static::Restrict { what: what.clone(), rule }]),
                |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); p.word("tapped")?; Some(vec![Static::EntersTapped(what.clone())]) },
                |p| {
                    p.verb("enter")?;
                    p.attempt(|p| p.words("the battlefield"));
                    p.word("with")?;
                    let n = amount_a(p)?;
                    let kind = counter_kind(p)?;
                    p.one_of(&["counters", "counter"])?;
                    p.one_of(&["on it", "on them"])?;
                    Some(vec![Static::EntersWithCounters { what: what.clone(), n, kind }])
                },
                |p| {
                    p.verb("cost")?;
                    let by = mana(p)?;
                    let less = p.one_of(&["less", "more"])? == "less";
                    p.one_of(&["to cast", "to activate"])?;
                    let per = p.attempt(|p| { p.words("for each")?; objects(p) });
                    Some(vec![Static::CostChange { what: what.clone(), by, less, per }])
                },
            )?;
            Some(r)
        },
        |p| {
            let what = objects(p)?;
            p.tok(&Tok::Possessive)?;
            p.words("power and toughness are each equal to")?;
            let n = measure(p)?;
            Some(vec![Static::SetPt { what, each_equal_to: n }])
        },
    )
}

fn statics(p: &mut P) -> Option<Vec<Static>> {
    let mut v = Vec::new();
    while let Some(s) = p.attempt(static_sentence) {
        v.extend(s);
    }
    (!v.is_empty()).then_some(v)
}

// ---------------------------------------------------------------- abilities

fn limits(p: &mut P) -> Vec<Limit> {
    let mut v = Vec::new();
    while let Some(l) = p.attempt(|p| {
        let l = alt!(p;
            |p| p.words("activate only as a sorcery").map(|_| Limit::SorcerySpeed),
            |p| p.words("activate only once each turn").map(|_| Limit::OncePerTurn),
            |p| p.words("activate only during your turn").map(|_| Limit::OnlyDuringYourTurn),
            |p| { p.words("activate only if")?; condition(p).map(Limit::OnlyIf) },
        )?;
        p.tok(&Tok::Period)?;
        Some(l)
    }) {
        v.push(l);
    }
    v
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
            let mut limits = limits(p);
            if let Some(Static::SpendOnly(o)) = p.attempt(|p| { let s = static_sentence(p)?; s.into_iter().next() }) {
                limits.push(Limit::OnlyIf(Condition::Is { what: o, filter: Filter::default() }));
            }
            finished(p).then_some(Ability::Activated { cost, effect, limits })
        },
        |p| {
            let trigger = trigger(p)?;
            p.tok(&Tok::Comma)?;
            let effect = effects(p)?;
            finished(p).then_some(Ability::Triggered { trigger, effect })
        },
        |p| { if !spell { return None; } let e = effects(p)?; finished(p).then_some(Ability::Spell(e)) },
        |p| { let s = statics(p)?; finished(p).then_some(Ability::Static(s)) },
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
