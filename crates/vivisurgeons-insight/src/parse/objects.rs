//! Players, and the objects a rule names: quantifier, filter, posts.

use super::*;

pub(super) fn player(p: &mut P) -> Option<Player> {
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
        |p| { p.words("the owner of")?; objects_no_player(p).map(|o| Player::OwnerOf(Box::new(o))) },
        |p| { p.words("the controller of")?; objects_no_player(p).map(|o| Player::ControllerOf(Box::new(o))) },
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
pub(super) fn whose(p: &mut P) -> Option<Player> {
    alt!(p;
        |p| p.word("your").map(|_| Player::You),
        |p| p.word("their").map(|_| Player::That),
        |p| p.word("its").map(|_| Player::ControllerOf(Box::new(antecedent()))),
        |p| { p.words("its owner")?; p.tok(&Tok::Possessive)?; Some(Player::OwnerOf(Box::new(antecedent()))) },
        |p| { p.words("their owner")?; p.tok(&Tok::Possessive)?; Some(Player::OwnerOf(Box::new(antecedent()))) },
        |p| { let pl = player(p)?; p.tok(&Tok::Possessive)?; Some(pl) },
    )
}

/// "your" and "their" alone. `whose` also reads "target player's", which
/// starts with an object, so an object rule must not call it.
pub(super) fn whose_pronoun(p: &mut P) -> Option<Player> {
    alt!(p;
        |p| p.word("your").map(|_| Player::You),
        |p| p.word("their").map(|_| Player::That),
    )
}

// ---------------------------------------------------------------- objects

pub(super) fn color_word(w: &str) -> Option<Color> {
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

pub(super) fn state_word(w: &str) -> Option<State> {
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
pub(super) fn type_word(w: &str) -> Option<Adj> {
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

pub(super) fn descriptor(p: &mut P) -> Option<Adj> {
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

pub(super) fn kind(p: &mut P) -> Option<Noun> {
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
        |p| p.one_of(&["commanders", "commander"]).map(|_| Noun::Commander),
        |p| p.one_of(&["copies", "copy"]).map(|_| Noun::Copy),
        |p| p.one_of(&["counters", "counter"]).map(|_| Noun::Counter(None)),
        |p| {
            let k = counter_kind(p).filter(|k| !matches!(k.as_str(), "and" | "or" | "then" | "that" | "target" | "a" | "an" | "the"))?;
            p.one_of(&["counters", "counter"])?;
            Some(Noun::Counter(Some(k)))
        },
    )
}

pub(super) fn group(p: &mut P) -> Option<Vec<Adj>> {
    let mut v = Vec::new();
    while let Some(a) = p.attempt(descriptor) {
        v.push(a);
    }
    (!v.is_empty()).then_some(v)
}

pub(super) fn is_typed(a: &Adj) -> bool {
    matches!(a, Adj::Type(_) | Adj::Subtype(_))
}

pub(super) fn filter(p: &mut P) -> Option<Filter> {
    let mut groups = Vec::new();
    if let Some(g) = p.attempt(group) {
        groups.push(g);
        // A comma list of types ends in "or" or "and"; without one the comma
        // ends the object: "a Mountain, creatures you control have haste".
        let before_commas = p.i;
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
            // "an Island or a Mountain": an article after "or" still joins
            // one filter; after "and" it would start a second object.
            let conj = p.one_of(&["and/or", "or", "and"])?;
            if conj == "or" {
                p.attempt(|p| p.one_of(&["a", "an"]));
            }
            group(p)
        });
        match last {
            Some(l) => {
                groups.extend(commas);
                groups.push(l);
            }
            // "noncreature, nonland permanent": words before any type are an and.
            None if !groups[0].iter().any(is_typed) => {
                for c in commas {
                    groups[0].extend(c);
                }
            }
            None => p.i = before_commas,
        }
    }
    // A counter takes no type words: "enchantment and counter" is two clauses.
    let kind =
        p.attempt(|p| kind(p).filter(|k| groups.is_empty() || !matches!(k, Noun::Counter(_))));
    if groups.is_empty() && kind.is_none() {
        return None;
    }
    // "red instant or sorcery": the first group's words before its type
    // belong to every later group that is a bare type. A first group with no
    // type of its own ("noncreature or Dragon spell") is an alternative whole.
    if groups.len() > 1
        && groups[0].iter().any(is_typed)
        && groups[1..].iter().all(|g| g.iter().all(is_typed))
    {
        let lead: Vec<Adj> = groups[0]
            .iter()
            .take_while(|a| !is_typed(a))
            .cloned()
            .collect();
        for g in groups.iter_mut().skip(1) {
            g.splice(0..0, lead.iter().cloned());
        }
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
    // Only a counter is "on" something; elsewhere "on" starts a destination.
    let counter = matches!(kind, Some(Noun::Counter(_)));
    while let Some(post) = p.attempt(|p| post(p).filter(|q| counter || !matches!(q, Post::On(_)))) {
        posts.push(post);
    }
    // "spell or nonland permanent": two kinds of object, so two filters. Both
    // must name their kind, or "creature or planeswalker" would split here
    // instead of reading as one filter's alternatives above.
    let or = match kind {
        Some(_) => p.attempt(|p| {
            p.word("or")?;
            let f = filter(p)?;
            f.kind.as_ref()?;
            Some(Box::new(f))
        }),
        None => None,
    };
    Some(Filter {
        alts: groups,
        kind,
        posts,
        or,
    })
}

pub(super) fn compare(p: &mut P) -> Option<Compare> {
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

pub(super) fn post(p: &mut P) -> Option<Post> {
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
        |p| p.one_of(&["that's blocking", "that are blocking"]).map(|_| Post::Blocking),
        |p| p.words("other than ~").map(|_| Post::OtherThanSelf),
        |p| p.words("of the chosen type").map(|_| Post::OfChosenType),
        |p| p.words("of the chosen color").map(|_| Post::OfChosenType),
        |p| p.words("attached to it").map(|_| Post::AttachedToIt),
        |p| {
            p.one_of(&["that shares a", "that share a"])?;
            let card_type = p.peek_word().filter(|w| vocab().card_types.contains(*w))?.to_string();
            p.bump();
            p.word("type")?;
            let with = p.attempt(|p| { p.word("with")?; objects(p) }).map(Box::new);
            Some(Post::SharesType { card_type, with })
        },
        |p| p.words("with a basic land type").map(|_| Post::WithBasicLandType),
        |p| p.words("with a single target").map(|_| Post::WithSingleTarget),
        |p| p.words("of their choice").map(|_| Post::ChosenBy(Player::That)),
        |p| p.words("of your choice").map(|_| Post::ChosenBy(Player::You)),
        |p| p.one_of(&["you've cast this turn", "you cast this turn"]).map(|_| Post::CastThisTurn(Player::You)),
        |p| {
            p.words("with mana cost")?;
            let mut v = vec![mana(p)?];
            while let Some(m) = p.attempt(|p| { p.word("or")?; mana(p) }) { v.push(m); }
            Some(Post::ManaCost(v))
        },
        |p| { p.one_of(&["that's attacking", "that are attacking"])?; let pl = player_simple(p).or_else(|| p.word("you").map(|_| Player::You))?; Some(Post::AttackingPlayer(pl)) },
        |p| p.one_of(&["that's attacking", "that are attacking"]).map(|_| Post::Attacking),
        |p| p.words("on the battlefield").map(|_| Post::In(Zone::Battlefield)),
        |p| { p.word("on")?; let o = pronoun(p)?; Some(Post::On(Box::new(o))) },
        |p| { p.words("put onto the battlefield with")?; objects(p).map(|o| Post::PutOntoBattlefieldWith(Box::new(o))) },
        |p| p.one_of(&["from among them", "revealed this way", "from among those cards"]).map(|_| Post::FromAmongThem),
        |p| { p.words("exiled with")?; objects(p).map(|o| Post::ExiledWith(Box::new(o))) },
        |p| p.words("you cast").map(|_| Post::Cast(Player::You)),
        |p| { let pl = player_simple(p)?; p.one_of(&["casts", "cast"])?; Some(Post::Cast(pl)) },
    )
}

/// Players that can stand before "controls" without eating an object noun.
pub(super) fn player_simple(p: &mut P) -> Option<Player> {
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

pub(super) fn quant(p: &mut P) -> Option<Quant> {
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

pub(super) fn pronoun(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| p.tok(&Tok::SelfRef).map(|_| self_ref()),
        |p| { p.word("this")?; let f = filter(p)?; Some(with_others(Quant::SelfRef, f)) },
        |p| p.one_of(&["each of them", "it", "them", "they"]).map(|_| antecedent()),
        |p| p.words("the rest").map(|_| Objects { quant: Quant::Rest, filter: Filter::default() }),
        |p| p.one_of(&["the other", "the others"]).map(|_| antecedent()),
        |p| {
            let n = amount_a(p)?;
            let posts = if p.attempt(|p| p.words("of them")).is_some() { vec![Post::FromAmongThem] } else { vec![] };
            // A bare number names the cards just found: "put one onto the battlefield".
            if posts.is_empty() && !matches!(p.peek_word(), Some("onto" | "into" | "on")) { return None; }
            Some(Objects { quant: Quant::N(n), filter: Filter { posts, ..Filter::default() } })
        },
        |p| {
            p.one_of(&["that", "those", "the"])?;
            // "the exiled card", "the token", "those creatures".
            let marked = p.attempt(|p| alt!(p;
                |p| p.word("exiled").map(|_| Marked::Exiled),
                |p| p.word("chosen").map(|_| Marked::Chosen),
                |p| p.word("revealed").map(|_| Marked::Revealed),
                |p| p.word("sacrificed").map(|_| Marked::Sacrificed),
                |p| p.word("copied").map(|_| Marked::Copied),
                |p| p.word("returned").map(|_| Marked::Returned),
            ));
            let mut f = filter(p)?;
            if let Some(m) = marked {
                f.posts.insert(0, Post::Marked(m));
            }
            Some(Objects { quant: Quant::Antecedent, filter: f })
        },
        |p| { p.word("enchanted")?; let f = filter(p)?; Some(with_others(Quant::Enchanted, f)) },
        |p| { p.word("equipped")?; let f = filter(p)?; Some(with_others(Quant::Equipped, f)) },
        |p| {
            p.words("the top")?;
            let n = p.attempt(amount).unwrap_or(Amount::N(1));
            p.one_of(&["cards", "card"])?;
            p.word("of")?;
            let w = whose(p)?;
            p.word("library")?;
            Some(Objects {
                quant: Quant::N(n),
                filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(Zone::TopOfLibrary(w))], ..Filter::default() },
            })
        },
    )
}

/// "this creature and other Vampire creatures you control": one object, and
/// the others a filter names, which `filter` read as one list of groups.
pub(super) fn with_others(quant: Quant, mut f: Filter) -> Objects {
    let other = Adj::State(State::Other);
    if f.alts.len() < 2 || !f.alts[1..].iter().all(|g| g.first() == Some(&other)) {
        return Objects { quant, filter: f };
    }
    let first = f.alts.remove(0);
    for g in f.alts.iter_mut() {
        g.remove(0);
    }
    let one = Objects {
        quant,
        filter: Filter {
            alts: vec![first],
            ..Filter::default()
        },
    };
    Objects {
        quant: Quant::WithOthers(Box::new(one)),
        filter: f,
    }
}

pub(super) fn objects_no_player(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| {
            let first = pronoun(p)?;
            if first.quant != Quant::SelfRef { return None; }
            p.words("or another")?;
            let filter = filter(p)?;
            Some(Objects { quant: Quant::SelfOrAnother, filter })
        },
        pronoun,
        |p| {
            let of = whose_pronoun(p)?;
            let n = ["first", "second", "third"].iter().position(|w| p.word(w).is_some())? as u32 + 1;
            let filter = filter(p)?;
            p.words("each turn")?;
            Some(Objects { quant: Quant::NthEachTurn { n, of }, filter })
        },
        |p| {
            // "your commander": a possessive picks out the object, where a
            // possessive before a zone ("your graveyard") is read by `zone`.
            let who = whose_pronoun(p)?;
            let filter = filter(p)?;
            (filter.kind == Some(Noun::Commander)).then_some(Objects { quant: Quant::Possessive(who), filter })
        },
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
