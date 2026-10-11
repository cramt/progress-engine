//! Numbers and amounts: "three", "that many", "the number of …", devotion.

use super::*;

pub(super) fn number(p: &mut P) -> Option<u32> {
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

pub(super) fn amount(p: &mut P) -> Option<Amount> {
    let n = alt!(p;
        |p| { p.word("twice")?; amount(p).map(|n| Amount::Twice(Box::new(n))) },
        amount_bare,
    )?;
    Some(
        match p.attempt(|p| {
            p.word("plus")?;
            number(p)
        }) {
            Some(k) => Amount::Plus(Box::new(n), k),
            None => n,
        },
    )
}

pub(super) fn amount_bare(p: &mut P) -> Option<Amount> {
    alt!(p;
        |p| number(p).map(Amount::N),
        |p| p.word("x").map(|_| Amount::X),
        |p| p.words("that many").map(|_| Amount::ThatMany),
        |p| p.words("that much").map(|_| Amount::ThatMany),
    )
}

/// An amount where "a" and "an" mean one.
pub(super) fn amount_a(p: &mut P) -> Option<Amount> {
    alt!(p;
        amount,
        |p| p.one_of(&["a", "an"]).map(|_| Amount::N(1)),
    )
}

pub(super) fn stat(p: &mut P) -> Option<Stat> {
    alt!(p;
        |p| p.word("power").map(|_| Stat::Power),
        |p| p.word("toughness").map(|_| Stat::Toughness),
        |p| p.words("mana value").map(|_| Stat::ManaValue),
        |p| p.words("converted mana cost").map(|_| Stat::ManaValue),
    )
}

pub(super) fn stat_of(s: Stat, o: Objects) -> Amount {
    match s {
        Stat::Power => Amount::PowerOf(Box::new(o)),
        Stat::Toughness => Amount::ToughnessOf(Box::new(o)),
        Stat::ManaValue => Amount::ManaValueOf(Box::new(o)),
    }
}

/// What follows "equal to".
pub(super) fn measure(p: &mut P) -> Option<Amount> {
    alt!(p;
        |p| { p.words("the number of")?; objects(p).map(|o| Amount::CountOf(Box::new(o))) },
        |p| { p.one_of(&["its", "their"])?; let s = stat(p)?; Some(stat_of(s, antecedent())) },
        |p| { let o = objects(p)?; p.tok(&Tok::Possessive)?; let s = stat(p)?; Some(stat_of(s, o)) },
        |p| { p.words("the life lost this way").map(|_| Amount::LifeLost) },
        |p| { p.words("the damage dealt this way").map(|_| Amount::Damage) },
        |p| { p.words("the amount of damage dealt this way").map(|_| Amount::Damage) },
        |p| p.words("the greatest number of cards a player discarded this way").map(|_| Amount::GreatestDiscardedThisWay),
        |p| {
            p.words("the total")?;
            let stat = stat(p)?;
            p.word("of")?;
            let of = objects(p)?;
            Some(Amount::Total { stat, of: Box::new(of) })
        },
        |p| {
            p.words("the greatest")?;
            let stat = stat(p)?;
            p.word("among")?;
            let among = objects(p)?;
            Some(Amount::Greatest { stat, among: Box::new(among) })
        },
        |p| {
            let who = whose(p)?;
            p.words("devotion to")?;
            let to = palette_named(p)?;
            Some(Amount::Devotion { who, to })
        },
        |p| {
            p.words("the number of colors")?;
            let pal = alt!(p;
                |p| { p.word("in")?; commander_identity(p) },
                |p| { p.word("among")?; objects(p).map(|o| Palette::ColorsOf(Box::new(o))) },
            )?;
            Some(Amount::NumberOfColors(Box::new(pal)))
        },
        amount,
    )
}

/// Colors named outright, or by a choice made before: "black", "that color".
pub(super) fn palette_named(p: &mut P) -> Option<Palette> {
    alt!(p;
        |p| p.one_of(&["that color", "the chosen color"]).map(|_| Palette::Chosen),
        |p| {
            let mut cs = vec![color_word(p.peek_word()?)?];
            p.bump();
            while let Some(c) = p.attempt(|p| { p.word("and")?; let c = color_word(p.peek_word()?)?; p.bump(); Some(c) }) {
                cs.push(c);
            }
            Some(Palette::Listed(cs))
        },
    )
}

/// "your commander's color identity"; a partner pair is "your commanders'".
pub(super) fn commander_identity(p: &mut P) -> Option<Palette> {
    p.word("your")?;
    p.one_of(&["commander", "commanders"])?;
    p.tok(&Tok::Possessive)?;
    p.words("color identity")
        .map(|_| Palette::CommanderIdentity)
}

pub(super) fn antecedent() -> Objects {
    Objects {
        quant: Quant::Antecedent,
        filter: Filter::default(),
    }
}

pub(super) fn self_ref() -> Objects {
    Objects {
        quant: Quant::SelfRef,
        filter: Filter::default(),
    }
}
