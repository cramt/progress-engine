//! Mana symbols, costs, and the mana an ability adds.

use super::*;

pub(super) fn mana(p: &mut P) -> Option<Mana> {
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

pub(super) fn sym(p: &mut P, s: &str) -> Option<()> {
    match p.peek()? {
        Tok::Sym(x) if x == s => {
            p.bump();
            Some(())
        }
        _ => None,
    }
}

pub(super) fn cost(p: &mut P) -> Option<Cost> {
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
        |p| { p.words("pay life equal to")?; measure(p).map(Cost::PayLife) },
        |p| { p.word("pay")?; mana(p).map(Cost::Mana) },
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
        |p| { p.word("exert")?; objects(p).map(Cost::Exert) },
        |p| { p.word("mill")?; let n = amount_a(p)?; p.one_of(&["cards", "card"])?; Some(Cost::Mill(n)) },
    )
}

pub(super) fn costs(p: &mut P) -> Option<Vec<Cost>> {
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

pub(super) fn counter_kind(p: &mut P) -> Option<String> {
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

pub(super) fn mana_add(p: &mut P) -> Option<ManaAdd> {
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
        |p| {
            let n = amount_a(p)?;
            p.word("mana")?;
            let (pick, within) = mana_pick(p)?;
            Some(ManaAdd::Of { n, pick, within })
        },
        |p| { p.words("an amount of")?; let m = mana(p)?; p.words("equal to")?; let n = measure(p)?; Some(ManaAdd::AmountOf { mana: m, n }) },
        |p| {
            p.words("an amount of mana")?;
            let (pick, within) = mana_pick(p)?;
            p.words("equal to")?;
            let n = measure(p)?;
            Some(ManaAdd::Of { n, pick, within })
        },
        |p| {
            p.words("an additional")?;
            alt!(p;
                |p| mana(p).map(ManaAdd::Symbols),
                mana_add,
            ).map(|m| ManaAdd::Additional(Box::new(m)))
        },
    )
}

/// What follows "mana": how its colors are picked, and from what.
pub(super) fn mana_pick(p: &mut P) -> Option<(Pick, Option<Palette>)> {
    alt!(p;
        |p| { p.word("of")?; palette_named(p).map(|c| (Pick::Stated, Some(c))) },
        |p| p.words("of that type").map(|_| (Pick::ThatType, None)),
        |p| {
            p.words("of any of")?;
            let o = objects_no_player(p)?;
            p.tok(&Tok::Possessive)?;
            p.word("colors")?;
            Some((Pick::AnyColor, Some(Palette::ColorsOf(Box::new(o)))))
        },
        |p| {
            let pick = alt!(p;
                |p| p.words("of any one color").map(|_| Pick::AnyOneColor),
                |p| p.words("of any color").map(|_| Pick::AnyColor),
                |p| p.words("of any type").map(|_| Pick::AnyType),
                |p| p.words("in any combination of colors").map(|_| Pick::AnyCombination),
            )?;
            let within = p.attempt(|p| alt!(p;
                |p| { p.word("in")?; commander_identity(p) },
                |p| { p.word("that")?; let o = objects(p)?; p.words("could produce")?; Some(Palette::CouldProduce(Box::new(o))) },
                |p| { p.word("among")?; objects(p).map(|o| Palette::ColorsOf(Box::new(o))) },
            ));
            Some((pick, within))
        },
    )
}
