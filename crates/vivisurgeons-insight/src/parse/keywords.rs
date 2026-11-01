//! Keyword abilities, in a line or a list, and quoted abilities.

use super::*;

pub(super) fn keyword_name(p: &mut P) -> Option<String> {
    let candidates = vocab().keyword_abilities.get(p.peek_word()?)?;
    let k = candidates.iter().find(|k| p.words(k).is_some())?;
    Some(k.replace(" 's", "'s"))
}

pub(super) fn keyword(p: &mut P) -> Option<Keyword> {
    alt!(p;
        // "Start your engines!" is a keyword ability (CR 702.179), which
        // Scryfall files among the ability words; its "!" is part of it.
        |p| {
            p.words("start your engines")?;
            p.tok(&Tok::Other('!'))?;
            Some(Keyword { name: "start your engines!".into(), param: None })
        },
        // "Partner with Pir, Imaginative Rascal": the name to the line's end.
        |p| {
            p.words("partner with")?;
            let name = super::effects::card_name(p, &[])?;
            p.at_end().then(|| Keyword { name: "partner with".into(), param: Some(KeywordParam::PartnerWith(name)) })
        },
        |p| {
            p.words("protection from")?;
            if let Some(pal) = p.attempt(|p| { p.words("each color that's not in")?; commander_identity(p) }) {
                return Some(Keyword { name: "protection".into(), param: Some(KeywordParam::ColorsNotIn(pal)) });
            }
            if p.attempt(|p| p.words("the color of your choice")).is_some() {
                return Some(Keyword { name: "protection".into(), param: Some(KeywordParam::Color(Palette::ChoiceOf(Player::You))) });
            }
            // "protection from the chosen color", "… from that color".
            if let Some(pal) = p.attempt(|p| alt!(p;
                |p| p.words("the chosen color").map(|_| Palette::Chosen),
                |p| p.words("that color").map(|_| Palette::That),
            )) {
                return Some(Keyword { name: "protection".into(), param: Some(KeywordParam::Color(pal)) });
            }
            let mut cs = Vec::new();
            while let Some(c) = p.peek_word().and_then(color_word) {
                p.bump();
                cs.push(c);
                if p.attempt(|p| p.one_of(&["and from", ", from", "from"])).is_none() { break; }
            }
            if !cs.is_empty() {
                return Some(Keyword { name: "protection".into(), param: Some(KeywordParam::Colors(cs)) });
            }
            if p.attempt(|p| p.words("each color")).is_some() {
                let each = vec![Color::White, Color::Blue, Color::Black, Color::Red, Color::Green];
                return Some(Keyword { name: "protection".into(), param: Some(KeywordParam::Colors(each)) });
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
        // "hexproof from blue and from black".
        |p| {
            p.words("hexproof from")?;
            // "hexproof from that color", "… from the chosen color".
            if let Some(pal) = p.attempt(|p| alt!(p;
                |p| p.words("the chosen color").map(|_| Palette::Chosen),
                |p| p.words("that color").map(|_| Palette::That),
            )) {
                return Some(Keyword { name: "hexproof".into(), param: Some(KeywordParam::Color(pal)) });
            }
            let mut cs = vec![color_word(p.peek_word()?)?];
            p.bump();
            while let Some(c) = p.attempt(|p| { p.one_of(&["and from", ", from", ", and from"])?; let c = color_word(p.peek_word()?)?; p.bump(); Some(c) }) {
                cs.push(c);
            }
            Some(Keyword { name: "hexproof".into(), param: Some(KeywordParam::Colors(cs)) })
        },
        |p| {
            p.word("gift")?;
            let gift = alt!(p;
                |p| p.words("a card").map(|_| Gift::Card),
                |p| p.words("an extra turn").map(|_| Gift::ExtraTurn),
                |p| {
                    p.one_of(&["a", "an"])?;
                    let tapped = p.attempt(|p| p.word("tapped")).is_some();
                    let named = match type_word(p.peek_word()?)? { Adj::Subtype(s) => s, _ => return None };
                    p.bump();
                    Some(Gift::Token { tapped, named })
                },
            )?;
            Some(Keyword { name: "gift".into(), param: Some(KeywordParam::Gift(gift)) })
        },
        |p| {
            let name = keyword_name(p)?;
            if name == "splice" {
                p.word("onto")?;
                let onto = filter(p)?;
                let cost = alt!(p;
                    |p| { p.tok(&Tok::Dash)?; keyword_costs(p) },
                    |p| mana(p).map(|m| vec![Cost::Mana(m)]),
                )?;
                return Some(Keyword { name, param: Some(KeywordParam::Splice { onto, cost }) });
            }
            let param = alt!(p;
                // "Companion — Each creature card in your starting deck is …".
                |p| {
                    if name != "companion" { return None; }
                    p.tok(&Tok::Dash)?;
                    condition(p).map(|c| KeywordParam::Condition(Box::new(c)))
                },
                |p| { p.tok(&Tok::Dash)?; let c = keyword_costs(p)?; p.attempt(|p| p.tok(&Tok::Period)); Some(KeywordParam::Cost(c)) },
                // "Kicker {G} and/or {1}{U}".
                |p| { let a = mana(p)?; p.word("and/or")?; let b = mana(p)?; Some(KeywordParam::EitherOrBoth(vec![a, b])) },
                |p| mana(p).map(|m| KeywordParam::Cost(vec![Cost::Mana(m)])),
                // "Suspend 3—{1}{U}{U}".
                |p| { let n = number(p)?; p.tok(&Tok::Dash)?; let cost = costs(p)?; Some(KeywordParam::NCost { n: Amount::N(n), cost }) },
                |p| number(p).map(|n| KeywordParam::N(Amount::N(n))),
                |p| p.word("x").map(|_| KeywordParam::N(Amount::X)),
                // Only affinity is "for" something; "prowess for each counter"
                // is the token's count.
                |p| { if name != "affinity" { return None; } p.word("for")?; objects(p).map(|o| KeywordParam::Objects(Box::new(o))) },
                |p| { p.word("from")?; objects(p).map(|o| KeywordParam::Objects(Box::new(o))) },
                // "Craft with artifact {3}{W}{W}": the materials and the
                // cost (CR 702.167a).
                |p| {
                    if name != "craft" { return None; }
                    p.word("with")?;
                    let mut o = objects(p)?;
                    // "Craft with artifact": a bare noun is one of them, where
                    // "Craft with six artifacts" counts.
                    if o.quant == Quant::Plural && matches!(p.t.get(p.i.wrapping_sub(1)), Some(Tok::Word(w)) if !w.ends_with('s')) {
                        o.quant = Quant::One;
                    }
                    let m = mana(p)?;
                    Some(KeywordParam::CostFor { what: Box::new(o), cost: vec![Cost::Mana(m)] })
                },
                |p| {
                    // "Enchant creature you control", "Equip legendary creature {3}".
                    if !matches!(name.as_str(), "enchant" | "equip") { return None; }
                    let o = objects(p)?;
                    if let Some(m) = p.attempt(mana) {
                        return Some(KeywordParam::CostFor { what: Box::new(o), cost: vec![Cost::Mana(m)] });
                    }
                    Some(KeywordParam::Objects(Box::new(o)))
                },
            );
            Some(Keyword { name, param })
        },
    )
}

pub(super) fn keyword_line(p: &mut P) -> Option<Vec<Keyword>> {
    let mut v = vec![keyword(p)?];
    // "Firebending X, where X is this creature's power": the keyword's
    // number is the measure, as the line defines X for nothing else.
    if let [Keyword {
        param: Some(KeywordParam::N(n @ Amount::X)),
        ..
    }] = &mut v[..]
    {
        if let Some(m) = p.attempt(|p| {
            p.words(", where x is")?;
            measure(p)
        }) {
            *n = m;
            return Some(v);
        }
    }
    loop {
        // "protection from spells and from permanents that were cast this
        // turn" is two protection abilities (CR 702.16g).
        let and_from = matches!(v.last(), Some(Keyword { param: Some(KeywordParam::Objects(_)), name }) if name == "protection");
        let next = p.attempt(|p| {
            if and_from {
                if let Some(o) = p.attempt(|p| {
                    p.words("and from")?;
                    objects(p)
                }) {
                    return Some(Keyword {
                        name: "protection".into(),
                        param: Some(KeywordParam::Objects(Box::new(o))),
                    });
                }
            }
            p.one_of(&[",", ";"])
                .or_else(|| p.tok(&Tok::Comma).map(|_| ","))?;
            keyword(p)
        });
        match next {
            Some(k) => v.push(k),
            None => break,
        }
    }
    Some(v)
}

/// "flying", "flying and trample", "first strike, vigilance, and lifelink".
pub(super) fn keyword_list(p: &mut P) -> Option<Vec<Keyword>> {
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

/// "haste and "At the beginning …"", "flying, "When …," and "At …"": a
/// list of keywords, then of quoted abilities, with at least one ability.
/// Every such list in Oracle text names its keywords first.
pub(super) fn keywords_then_quoted(p: &mut P) -> Option<(Vec<Keyword>, Vec<Ability>)> {
    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    loop {
        let first = keywords.is_empty() && abilities.is_empty();
        let item = p.attempt(|p| {
            if !first {
                p.one_of(&[", and", "and", ","])?;
            }
            match p.attempt(quoted) {
                Some(a) => Some(Err(a)),
                None => keyword(p).map(Ok),
            }
        });
        match item {
            Some(Ok(k)) if abilities.is_empty() => keywords.push(k),
            Some(Ok(_)) => return None,
            Some(Err(a)) => abilities.push(a),
            None => break,
        }
    }
    (!abilities.is_empty()).then_some((keywords, abilities))
}

/// A quoted ability: `"{T}: Add {G}."`
pub(super) fn quoted(p: &mut P) -> Option<Ability> {
    p.tok(&Tok::Quote)?;
    // A quoted ability has subjects of its own.
    let outer = p.implied.take();
    let a = p.attempt(|p| ability(p, false));
    p.implied = outer;
    let a = a?;
    p.tok(&Tok::Quote)?;
    Some(a)
}
