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

/// "first", "seventh", "twelfth".
pub(super) fn ordinal(p: &mut P) -> Option<u32> {
    const ORDINALS: [&str; 12] = [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth", "eleventh", "twelfth",
    ];
    let n = ORDINALS.iter().position(|w| p.peek_word() == Some(w))?;
    p.bump();
    Some(n as u32 + 1)
}

pub(super) fn amount(p: &mut P) -> Option<Amount> {
    let n = alt!(p;
        |p| { p.word("twice")?; amount(p).map(|n| Amount::Twice(Box::new(n))) },
        |p| { p.words("three times")?; amount(p).map(|n| Amount::Thrice(Box::new(n))) },
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

/// What follows "equal to", perhaps with a number added on either side:
/// "the number of cards in your hand plus one", "2 plus the sacrificed
/// creature's mana value".
pub(super) fn measure(p: &mut P) -> Option<Amount> {
    if let Some(m) = p.attempt(|p| {
        let k = number(p)?;
        p.word("plus")?;
        measure_bare(p).map(|m| Amount::Plus(Box::new(m), k))
    }) {
        return Some(m);
    }
    let m = measure_bare(p)?;
    Some(
        match p.attempt(|p| {
            p.word("plus")?;
            number(p)
        }) {
            Some(k) => Amount::Plus(Box::new(m), k),
            None => m,
        },
    )
}

/// ", rounded up" or ", rounded down": whether a half rounds up.
pub(super) fn rounded(p: &mut P) -> Option<bool> {
    p.tok(&Tok::Comma)?;
    p.word("rounded")?;
    Some(p.one_of(&["up", "down"])? == "up")
}

fn measure_bare(p: &mut P) -> Option<Amount> {
    alt!(p;
        // "half the number of cards in your library, rounded up".
        |p| {
            p.word("half")?;
            let of = measure_bare(p)?;
            let round_up = rounded(p)?;
            Some(Amount::Half { of: Box::new(of), round_up })
        },
        |p| { p.words("the amount of")?; sym(p, "E")?; p.words("paid this way").map(|_| Amount::EnergyPaidThisWay) },
        |p| p.words("that number").map(|_| Amount::ThatNumber),
        // "three minus X".
        |p| {
            let a = number(p)?;
            p.word("minus")?;
            let b = amount(p)?;
            Some(Amount::Minus(Box::new(Amount::N(a)), Box::new(b)))
        },
        |p| p.words("that damage").map(|_| Amount::ThatDamage),
        |p| { p.words("the amount of mana spent to cast")?; objects(p).map(|o| Amount::ManaSpentToCast(Box::new(o))) },
        |p| {
            p.words("the number of times")?;
            let what = alt!(p;
                |p| p.word("it's").map(|_| antecedent()),
                |p| { let o = objects(p)?; p.word("has")?; Some(o) },
            )?;
            p.words("been cast from")?;
            let zone = zone(p)?;
            p.words("this game")?;
            Some(Amount::TimesCastFrom { what: Box::new(what), zone })
        },
        |p| { p.words("the number of basic land types among")?; objects(p).map(|o| Amount::NumberOfBasicLandTypes(Box::new(o))) },
        |p| { p.words("the number of card types among")?; objects(p).map(|o| Amount::NumberOfCardTypes(Box::new(o))) },
        |p| { p.words("the number of colors of mana spent to cast")?; objects(p).map(|o| Amount::ColorsOfManaSpentToCast(Box::new(o))) },
        |p| { p.words("the number of")?; objects(p).map(|o| Amount::CountOf(Box::new(o))) },
        // "twice the number of creatures you control".
        |p| { p.word("twice")?; measure_bare(p).map(|m| Amount::Twice(Box::new(m))) },
        // "the mana value of the exiled card".
        |p| { p.word("the")?; let s = stat(p)?; p.word("of")?; let o = objects(p)?; Some(stat_of(s, o)) },
        |p| { let who = whose(p)?; p.words("life total")?; Some(Amount::LifeTotal(who)) },
        |p| { let who = whose(p)?; p.word("speed")?; Some(Amount::Speed(who)) },
        |p| { p.one_of(&["its", "their"])?; p.words("base power")?; Some(Amount::BasePowerOf(Box::new(antecedent()))) },
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
        |p| p.words("that color").map(|_| Palette::That),
        |p| p.words("the chosen color").map(|_| Palette::Chosen),
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
