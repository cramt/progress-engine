//! Keyword abilities, in a line or a list, and quoted abilities.

use super::*;

pub(super) fn keyword_name(p: &mut P) -> Option<String> {
    let candidates = vocab().keyword_abilities.get(p.peek_word()?)?;
    candidates.iter().find(|k| p.words(k).is_some()).cloned()
}

pub(super) fn keyword(p: &mut P) -> Option<Keyword> {
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
        |p| {
            let name = keyword_name(p)?;
            let param = alt!(p;
                |p| { p.tok(&Tok::Dash)?; let c = costs(p)?; p.attempt(|p| p.tok(&Tok::Period)); Some(KeywordParam::Cost(c)) },
                |p| mana(p).map(|m| KeywordParam::Cost(vec![Cost::Mana(m)])),
                |p| number(p).map(|n| KeywordParam::N(Amount::N(n))),
                |p| p.word("x").map(|_| KeywordParam::N(Amount::X)),
                // Only affinity is "for" something; "prowess for each counter"
                // is the token's count.
                |p| { if name != "affinity" { return None; } p.word("for")?; objects(p).map(|o| KeywordParam::Objects(Box::new(o))) },
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

pub(super) fn keyword_line(p: &mut P) -> Option<Vec<Keyword>> {
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
