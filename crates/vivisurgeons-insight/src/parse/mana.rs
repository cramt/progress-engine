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
        // "unless its controller pays twice {X}" (Thassa's Intervention): {X}{X}
        // ([twice-mana]).
        |p| { p.word("twice")?; let Mana(s) = mana(p)?; (s.len() == 1).then(|| Cost::Mana(Mana([s.clone(), s].concat()))) },
        |p| sym(p, "T").map(|_| Cost::Tap),
        |p| sym(p, "Q").map(|_| Cost::Untap),
        |p| {
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Cost::Energy(n))
        },
        |p| { p.word("sacrifice")?; objects(p).map(Cost::Sacrifice) },
        |p| p.words("discard your hand").map(|_| Cost::DiscardHand),
        |p| { p.word("discard")?; let o = objects(p)?; p.words("at random")?; Some(Cost::DiscardAtRandom(o)) },
        |p| { p.word("discard")?; objects(p).map(Cost::Discard) },
        |p| { p.word("pay")?; let n = amount(p)?; p.word("life")?; Some(Cost::PayLife(n)) },
        |p| { p.words("pay any amount of life")?; Some(Cost::PayLife(Amount::AnyAmount)) },
        // "Pay X {E}", "pay any amount of {E}".
        |p| {
            p.word("pay")?;
            let n = alt!(p; |p| p.words("any amount of").map(|_| Amount::AnyAmount), amount)?;
            sym(p, "E")?;
            Some(Cost::PayEnergy(n))
        },
        // "Pay six {C}".
        |p| { p.word("pay")?; let n = amount(p)?; let m = mana(p)?; Some(Cost::ManaTimes { n, mana: m }) },
        |p| {
            p.word("remove")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.words("from among")?;
            let among = objects(p)?;
            Some(Cost::RemoveCountersAmong { n, kind, among })
        },
        |p| { p.words("pay life equal to")?; measure(p).map(Cost::PayLife) },
        |p| { p.word("pay")?; mana(p).map(Cost::Mana) },
        // "Replicate—Pay {E}{E}{E}": paying {E} is removing an energy
        // counter (CR 107.14), the cost "{E}{E}{E}" says before a colon.
        |p| {
            p.word("pay")?;
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Cost::Energy(n))
        },
        |p| { p.word("exile")?; objects(p).map(Cost::Exile) },
        // "Remove a counter from this creature": of any kind, the payer's pick.
        |p| {
            p.word("remove")?;
            let n = amount_a(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("from")?;
            let from = objects(p)?;
            Some(Cost::RemoveAnyCounters { n, from })
        },
        // "behold a Goblin": reveal one from hand or choose one you control
        // (CR 701.4a).
        |p| { p.word("behold")?; objects(p).map(|what| Cost::ActionOn { name: "behold".into(), what }) },
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
        // "Collect evidence 6", "blight 1", "Waterbend {8}": a keyword action
        // paid as a cost. Last, and not for the verbs the rules above read,
        // which say more after their number.
        |p| {
            if matches!(p.peek_word()?, "mill" | "exile" | "sacrifice" | "discard" | "tap" | "untap" | "reveal" | "exert" | "remove" | "put" | "return" | "pay") { return None; }
            let name = keyword_action_phrase(p)?;
            alt!(p;
                |p| amount(p).map(|n| Cost::Action { name: name.clone(), n }),
                |p| mana(p).map(|m| Cost::ActionMana { name: name.clone(), mana: m }),
                |p| { p.end_of_clause().then_some(())?; Some(Cost::ActionAlone(name.clone())) },
            )
        },
    )
}

/// A keyword's cost after its dash, which may be something another player
/// does: "Splice onto Arcane—An opponent gains 5 life".
pub(super) fn keyword_costs(p: &mut P) -> Option<Vec<Cost>> {
    alt!(p;
        costs,
        |p| {
            let who = player(p)?;
            p.verb("gain")?;
            let n = amount(p)?;
            p.word("life")?;
            Some(vec![Cost::PlayerGainsLife { who, n }])
        },
    )
}

/// A keyword action's name, one word or two ("collect evidence", "manifest
/// dread"), the longer tried first.
pub(super) fn keyword_action_phrase(p: &mut P) -> Option<String> {
    let w = p.peek_word()?;
    if let Some(Tok::Word(next)) = p.t.get(p.i + 1) {
        let two = format!("{w} {next}");
        if vocab().keyword_actions.contains(&two) {
            p.bump();
            p.bump();
            return Some(two);
        }
    }
    let one = vocab().keyword_actions.contains(w).then(|| w.to_string())?;
    p.bump();
    Some(one)
}

pub(super) fn costs(p: &mut P) -> Option<Vec<Cost>> {
    let mut v = cost_items(p)?;
    while let Some(c) = p.attempt(|p| {
        p.tok(&Tok::Comma)?;
        p.attempt(|p| p.word("and"));
        cost_items(p)
    }) {
        v.extend(c);
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

/// One cost, or "exile this Vehicle and four other artifact creatures you
/// control" (Mechtitan Core): the card and the others, one verb, two
/// costs.
fn cost_items(p: &mut P) -> Option<Vec<Cost>> {
    if let Some(v) = p.attempt(|p| {
        let make: fn(Objects) -> Cost = match p.peek_word()? {
            "exile" => Cost::Exile,
            "sacrifice" => Cost::Sacrifice,
            _ => return None,
        };
        p.bump();
        let me = objects(p)?;
        if me.quant != Quant::SelfRef {
            return None;
        }
        p.word("and")?;
        let others = objects(p)?;
        Some(vec![make(me), make(others)])
    }) {
        return Some(v);
    }
    cost(p).map(|c| vec![c])
}

/// A cost said with a gerund, after "by": "paying {3}{R} and exiling four
/// other cards from your graveyard", "paying life equal to its mana value",
/// "discarding a card".
fn gerund_cost(p: &mut P) -> Option<Cost> {
    alt!(p;
        |p| { p.words("paying life equal to")?; measure(p).map(Cost::PayLife) },
        |p| { p.word("paying")?; let n = amount(p)?; p.word("life")?; Some(Cost::PayLife(n)) },
        |p| {
            p.word("paying")?;
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Cost::Energy(n))
        },
        |p| { p.word("paying")?; mana(p).map(Cost::Mana) },
        |p| { p.word("exiling")?; objects(p).map(Cost::Exile) },
        |p| { p.word("sacrificing")?; objects(p).map(Cost::Sacrifice) },
        |p| { p.word("discarding")?; objects(p).map(Cost::Discard) },
        |p| { p.word("tapping")?; objects(p).map(Cost::TapUntapped) },
    )
}

/// "by paying {2}{W} rather than paying its mana cost", "by paying 3 life
/// in addition to paying their other costs": the costs and whether they
/// replace the mana cost (CR 118.9) or add to it (CR 118.8).
pub(super) fn paying(p: &mut P) -> Option<(Vec<Cost>, bool)> {
    p.word("by")?;
    let mut v = vec![gerund_cost(p)?];
    while let Some(c) = p.attempt(|p| {
        alt!(p;
            |p| { p.tok(&Tok::Comma)?; p.attempt(|p| p.word("and")); Some(()) },
            |p| p.word("and"),
        )?;
        gerund_cost(p)
    }) {
        v.push(c);
    }
    let rather = alt!(p;
        |p| p.one_of(&["rather than paying its mana cost", "rather than paying their mana costs"]).map(|_| true),
        |p| {
            p.words("in addition to")?;
            p.attempt(|p| p.word("paying"));
            p.one_of(&["its other costs", "their other costs"]).map(|_| false)
        },
    )?;
    Some((v, rather))
}

pub(super) fn counter_kind(p: &mut P) -> Option<CounterKind> {
    match p.peek()? {
        Tok::Pt(pt) if pt.signed => {
            let k = CounterKind::Pt(*pt);
            p.bump();
            Some(k)
        }
        Tok::Word(w) if !matches!(w.as_str(), "on" | "from" | "counter" | "counters") => {
            let k = CounterKind::Named(w.clone());
            p.bump();
            Some(k)
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
        // "Add six {G}".
        |p| { let n = amount(p)?; let m = mana(p)?; Some(ManaAdd::AmountOf { mana: m, n }) },
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
                // "any type that land produced": "that" is the land's, not
                // a relative pronoun as in "that … could produce".
                |p| { let o = objects(p)?; p.word("produced")?; Some(Palette::Produced(Box::new(o))) },
                |p| { p.word("among")?; objects(p).map(|o| Palette::ColorsOf(Box::new(o))) },
            ));
            Some((pick, within))
        },
    )
}
