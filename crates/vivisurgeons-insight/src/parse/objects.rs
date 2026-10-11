//! Players, and the objects a rule names: quantifier, filter, posts.

use super::*;

pub(super) fn player(p: &mut P) -> Option<Player> {
    alt!(p;
        |p| {
            let mut v = vec![alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?];
            p.word("and")?;
            v.push(alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?);
            p.word("each")?;
            Some(Player::EachOf(v))
        },
        |p| p.words("target player").map(|_| Player::Target),
        |p| p.words("target opponent").map(|_| Player::TargetOpponent),
        |p| p.words("each other player").map(|_| Player::EachOtherPlayer),
        |p| p.words("each player who can't").map(|_| Player::WhoCant(Box::new(Player::Each))),
        |p| p.words("each opponent who can't").map(|_| Player::WhoCant(Box::new(Player::EachOpponent))),
        |p| {
            p.words("each opponent attacking")?;
            let attacked = player_simple(p)?;
            Some(Player::Attacking { who: Box::new(Player::EachOpponent), attacked: Box::new(attacked) })
        },
        // "each player who searched a library this way".
        |p| {
            let who = alt!(p;
                |p| p.words("each player").map(|_| Player::Each),
                |p| p.words("each opponent").map(|_| Player::EachOpponent),
            )?;
            p.word("who")?;
            let did = did(p, Player::That)?;
            p.words("this way")?;
            Some(Player::DidThisWay { who: Box::new(who), did: Box::new(did) })
        },
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
        "modified" => State::Modified,
        "historic" => State::Historic,
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
    let next_is_type = || matches!(p.t.get(p.i + 1), Some(Tok::Word(n)) if type_word(n).is_some());
    // "Urza's Mine": a subtype the lexer split at its apostrophe.
    if p.t.get(p.i + 1) == Some(&Tok::Possessive) {
        let s = format!("{w}'s");
        if vocab().subtypes.contains(&s) {
            p.bump();
            p.bump();
            return Some(Adj::Subtype(s));
        }
    }
    let adj = if w == "commander" && next_is_type() {
        Adj::State(State::Commander)
    } else if w == "activated" || w == "triggered" {
        // Only before "ability": "activated" alone is no kind of object.
        if !matches!(p.t.get(p.i + 1), Some(Tok::Word(n)) if n == "ability" || n == "abilities") {
            return None;
        }
        Adj::Ability(if w == "activated" {
            AbilityKind::Activated
        } else {
            AbilityKind::Triggered
        })
    } else if let Some(c) = color_word(w) {
        Adj::Color(c)
    } else if let Some(s) = state_word(w) {
        Adj::State(s)
    // Scryfall's catalog has "Token" for token type lines; in rules text
    // "token" is the noun `kind` reads.
    } else if vocab().supertypes.contains(w) && w != "token" {
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
        |p| p.one_of(&["piles", "pile"]).map(|_| Noun::Pile),
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
    // "an artifact, enchantment, or token": a list of types whose last
    // alternative is a kind of object instead.
    let mut or_kind = None;
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
        let kind_last = || {
            let commas = !commas.is_empty();
            move |p: &mut P| {
                if commas {
                    p.tok(&Tok::Comma)?;
                }
                p.word("or")?;
                p.attempt(|p| p.one_of(&["a", "an"]));
                kind(p).filter(|k| !matches!(k, Noun::Counter(_)))
            }
        };
        let last = match last {
            None => match p.attempt(kind_last()) {
                Some(k) => {
                    or_kind = Some(k);
                    groups.extend(std::mem::take(&mut commas));
                    None
                }
                None => None,
            },
            l => l,
        };
        match last {
            Some(l) => {
                groups.extend(commas);
                groups.push(l);
            }
            None if or_kind.is_some() => {}
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
    let kind = match or_kind {
        Some(_) => None,
        None => {
            p.attempt(|p| kind(p).filter(|k| groups.is_empty() || !matches!(k, Noun::Counter(_))))
        }
    };
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
    let counter = matches!(kind, Some(Noun::Counter(_)));
    while let Some(more) = p.attempt(|p| posts_one(p, counter)) {
        posts.extend(more);
    }
    // "spell or nonland permanent": two kinds of object, so two filters. Both
    // must name their kind, or "creature or planeswalker" would split here
    // instead of reading as one filter's alternatives above.
    let bare_kind = groups.is_empty();
    let or = match (&kind, or_kind) {
        // Its posts follow the kind word but hold of both: "creature or
        // token you control".
        (_, Some(k)) => Some(Box::new(Filter {
            kind: Some(k),
            posts: posts.clone(),
            ..Filter::default()
        })),
        (Some(_), None) => p.attempt(|p| {
            // A list of kinds: "instant spell, sorcery spell, activated
            // ability, or triggered ability"; the comma needs a kind after it.
            let comma = p.attempt(|p| p.tok(&Tok::Comma)).is_some();
            let or = p.attempt(|p| p.word("or")).is_some();
            if !or && !comma {
                return None;
            }
            // "a token or a land": after a bare kind, the second may be types.
            if bare_kind {
                p.attempt(|p| p.one_of(&["a", "an"]));
            }
            let f = filter(p)?;
            // A comma with no "or" yet must be in a list that ends in one,
            // or "an enchantment spell, counter it" would read as a list.
            if comma && !or && f.or.is_none() {
                return None;
            }
            if matches!(f.kind, Some(Noun::Counter(_))) {
                return None;
            }
            (f.kind.is_some() || bare_kind && !f.alts.is_empty() && !comma).then_some(Box::new(f))
        }),
        (None, None) => None,
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
    let (op, value) = comparison(p)?;
    Some(Compare { stat, op, value })
}

/// What a number is compared with: "3 or less", "greater than its base power".
pub(super) fn comparison(p: &mut P) -> Option<(Op, Amount)> {
    alt!(p;
        |p| {
            let value = amount(p)?;
            let op = alt!(p;
                |p| p.words("or less").map(|_| Op::Le),
                |p| p.words("or greater").map(|_| Op::Ge),
                |p| p.words("or more").map(|_| Op::Ge),
            ).unwrap_or(Op::Eq);
            Some((op, value))
        },
        |p| {
            let op = alt!(p;
                |p| p.words("less than or equal to").map(|_| Op::Le),
                |p| p.words("greater than or equal to").map(|_| Op::Ge),
                |p| p.words("less than").map(|_| Op::Lt),
                |p| p.words("greater than").map(|_| Op::Gt),
                |p| p.words("equal to").map(|_| Op::Eq),
            )?;
            let outer = std::mem::replace(&mut p.in_value, true);
            let value = p.attempt(measure);
            p.in_value = outer;
            Some((op, value?))
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
            Some(Post::WithCounter(Some(k)))
        },
        // "with a counter on it", "with counters on them": of any kind.
        |p| {
            p.word("with")?;
            p.one_of(&["a counter", "one or more counters", "counters"])?;
            p.one_of(&["on it", "on them"])?;
            Some(Post::WithCounter(None))
        },
        |p| { p.word("with")?; let k = keyword_name(p)?; Some(Post::WithKeyword(k)) },
        |p| { p.word("without")?; let k = keyword_name(p)?; Some(Post::WithoutKeyword(k)) },
        |p| {
            if p.one_of(&["in", "from"])? == "from" && p.in_value { return None; }
            zone(p).map(Post::In)
        },
        |p| p.one_of(&["that's blocking", "that are blocking"]).map(|_| Post::Blocking),
        |p| p.words("other than ~").map(|_| Post::OtherThanSelf),
        |p| p.one_of(&["of the chosen type", "of that type"]).map(|_| Post::OfChosenType),
        |p| p.one_of(&["of the chosen color", "of that color"]).map(|_| Post::OfChosenColor),
        |p| p.words("attached to it").map(|_| Post::AttachedToIt),
        |p| {
            // "that shares at least one creature type with it" says the same.
            p.one_of(&["that shares a", "that share a", "that shares at least one", "that share at least one"])?;
            // "a card type": any one of them.
            let card_type = p.peek_word().filter(|w| *w == "card" || vocab().card_types.contains(*w))?.to_string();
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
        |p| {
            let w = p.one_of(&["from among them", "revealed this way", "from among those cards"])?;
            (w == "revealed this way" || !p.in_value).then_some(Post::FromAmongThem)
        },
        |p| { p.words("exiled with")?; objects(p).map(|o| Post::ExiledWith(Box::new(o))) },
        |p| p.words("you cast").map(|_| Post::Cast(Player::You)),
        |p| p.words("drawn this turn").map(|_| Post::DrawnThisTurn),
        |p| { p.word("named")?; pronoun(p).map(|o| Post::Named(Box::new(o))) },
        |p| { p.one_of(&["that target", "that targets"])?; objects(p).map(|o| Post::Targets(Box::new(o))) },
        |p| { p.word("that's")?; filter(p).map(Post::ThatIs) },
        |p| { let how = marked(p)?; p.words("this way")?; Some(Post::Marked(how)) },
        |p| p.one_of(&["who doesn't", "who don't"]).map(|_| Post::WhoDoesnt),
        |p| p.words("who can't").map(|_| Post::WhoCant),
        |p| { p.one_of(&["that aren't", "that isn't"])?; p.one_of(&["on", "in"])?; zone(p).map(Post::NotIn) },
        |p| { p.one_of(&["that aren't", "that isn't"])?; post(p).map(|q| Post::Not(Box::new(q))) },
        |p| p.one_of(&["that entered this turn", "that entered the battlefield this turn"]).map(|_| Post::EnteredThisTurn),
        |p| {
            if p.in_value { return None; }
            p.words("from among")?;
            objects(p).map(|o| Post::FromAmong(Box::new(o)))
        },
        |p| {
            p.words("with the greatest")?;
            let stat = stat(p)?;
            let or_tied = p.attempt(|p| {
                p.words("or tied for the greatest")?;
                // The tie is on the same number the greatest named.
                (super::stat(p)? == stat).then_some(())
            }).is_some();
            Some(Post::Greatest { stat, or_tied })
        },
        |p| {
            p.one_of(&["that had a counter put on them this way", "that had a counter put on it this way"])?;
            let on = antecedent();
            Some(Post::DidThisWay(Box::new(Event::CountersPut { by: None, n: Quant::One, kind: None, on })))
        },
        |p| { p.word("who")?; let e = did(p, Player::That)?; p.words("this way")?; Some(Post::DidThisWay(Box::new(e))) },
        |p| p.one_of(&["with the same name as one another", "with the same name"]).map(|_| Post::SameName),
        |p| p.one_of(&["that are one or more colors", "that's one or more colors"]).map(|_| Post::OneOrMoreColors),
        |p| p.words("with the chosen name").map(|_| Post::WithChosenName),
        |p| p.words("of each permanent type").map(|_| Post::OfEachPermanentType),
        // "each card they find": found by the search a condition named.
        |p| {
            let by = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            p.one_of(&["find", "finds"])?;
            Some(Post::MarkedBy { how: Marked::Found, by })
        },
        // "with lesser mana value", "with equal or lesser mana value": than
        // the object the ability is about.
        |p| {
            p.word("with")?;
            let op = alt!(p;
                |p| p.words("equal or lesser").map(|_| Op::Le),
                |p| p.word("lesser").map(|_| Op::Lt),
                |p| p.words("equal or greater").map(|_| Op::Ge),
                |p| p.word("greater").map(|_| Op::Gt),
            )?;
            let stat = stat(p)?;
            Some(Post::With(Compare { stat, op, value: stat_of(stat, antecedent()) }))
        },
        |p| p.words("with different names").map(|_| Post::DifferentNames),
        |p| { let pl = player_simple(p)?; p.one_of(&["owns", "own"])?; Some(Post::Owner(pl)) },
        |p| {
            let by = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            let how = marked(p)?;
            p.words("this way")?;
            Some(Post::MarkedBy { how, by })
        },
        // "creatures you control each with power greater than its base power":
        // "each" says the post holds of every one, as it would anyway.
        |p| { p.words("each with")?; compare(p).map(Post::With) },
        |p| { let pl = player_simple(p)?; p.one_of(&["casts", "cast"])?; Some(Post::Cast(pl)) },
    )
}

/// Posts that come as more than one: "that each have a different mana value
/// X or less" says the values differ and bounds each.
fn posts_one(p: &mut P, counter: bool) -> Option<Vec<Post>> {
    alt!(p;
        |p| {
            p.words("that each have a different")?;
            let stat = stat(p)?;
            let mut v = vec![Post::DifferentValues(stat)];
            if let Some((op, value)) = p.attempt(comparison) {
                v.push(Post::With(Compare { stat, op, value }));
            }
            Some(v)
        },
        // "target card that's an instant or sorcery from your graveyard": the
        // posts after the types are the card's, not the types'.
        |p| {
            p.word("that's")?;
            p.one_of(&["a", "an"])?;
            let mut f = filter(p)?;
            let posts = std::mem::take(&mut f.posts);
            let mut v = vec![Post::ThatIs(f)];
            v.extend(posts);
            Some(v)
        },
        // Only a counter is "on" something; elsewhere "on" starts a destination.
        |p| post(p).filter(|q| counter || !matches!(q, Post::On(_))).map(|q| vec![q]),
    )
}

/// What a player did, said after "who" with the player understood: "drew a
/// card", "searches a library".
pub(super) fn did(p: &mut P, who: Player) -> Option<Event> {
    alt!(p;
        |p| { p.one_of(&["drew", "draws", "draw"])?; p.words("a card")?; Some(Event::Draws(who.clone())) },
        |p| {
            p.one_of(&["searched", "searches", "search"])?;
            let zone = alt!(p;
                |p| p.words("a library").map(|_| Zone::ALibrary),
                |p| { let w = whose(p)?; p.word("library")?; Some(Zone::Library(w)) },
            )?;
            Some(Event::Searches { who: who.clone(), zone })
        },
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
        |p| { p.words("one or two target")?; Some(Quant::Target { n: Amount::Between(1, 2), up_to: false }) },
        |p| { p.words("up to")?; let n = amount(p)?; p.word("target")?; Some(Quant::Target { n, up_to: true }) },
        |p| { p.words("up to")?; amount(p).map(Quant::UpTo) },
        |p| p.one_of(&["another target", "a second target"]).map(|_| Quant::AnotherTarget),
        |p| { p.words("any number of target").map(|_| Quant::Target { n: Amount::X, up_to: true }) },
        |p| { let n = amount(p)?; p.word("target")?; Some(Quant::Target { n, up_to: false }) },
        |p| { let n = amount(p)?; p.words("or more")?; Some(Quant::AtLeast(n)) },
        |p| { let n = amount(p)?; p.words("or fewer").map(|_| Quant::AtMost(n)) },
        |p| p.word("target").map(|_| Quant::Target { n: Amount::N(1), up_to: false }),
        |p| p.words("the next").map(|_| Quant::Next),
        |p| p.word("additional").map(|_| Quant::Additional),
        // "each other creature": the "other" is the filter's own word.
        |p| p.word("each").map(|_| Quant::Each),
        |p| p.word("all").map(|_| Quant::All),
        |p| p.word("another").map(|_| Quant::Another),
        |p| p.words("one or more").map(|_| Quant::OneOrMore),
        |p| p.words("any number of").map(|_| Quant::AnyNumber),
        |p| p.one_of(&["a", "an"]).map(|_| Quant::One),
        // "a copy of any creature on the battlefield": one, of the chooser's
        // pick, as "a" would be.
        |p| p.word("any").map(|_| Quant::One),
        |p| amount(p).map(Quant::N),
    )
}

pub(super) fn pronoun(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| p.tok(&Tok::SelfRef).map(|_| self_ref()),
        |p| { p.word("this")?; let f = filter(p)?; Some(with_others(Quant::SelfRef, f)) },
        |p| p.one_of(&["each of them", "it", "them", "they"]).map(|_| antecedent()),
        // "each of those creatures": the earlier noun, each one.
        |p| { p.words("each of")?; let o = pronoun(p)?; (o.quant == Quant::Antecedent).then_some(o) },
        |p| p.words("the rest").map(|_| Objects { quant: Quant::Rest, filter: Filter::default() }),
        |p| p.one_of(&["the other", "the others"]).map(|_| antecedent()),
        // "twice that many of each of those kinds of counters": the counters
        // an event named, kind by kind.
        |p| p.words("each of those kinds of counters").map(|_| Objects {
            quant: Quant::Antecedent,
            filter: Filter { kind: Some(Noun::Counter(None)), ..Filter::default() },
        }),
        // "Put both cards onto the battlefield": the two just named.
        |p| { p.word("both")?; let f = filter(p)?; Some(Objects { quant: Quant::Antecedent, filter: f }) },
        |p| {
            p.words("up to")?;
            let n = amount(p)?;
            p.words("of them")?;
            Some(Objects { quant: Quant::UpTo(n), filter: Filter { posts: vec![Post::FromAmongThem], ..Filter::default() } })
        },
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
            let marked = p.attempt(marked);
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
            // "Shuffle your library, then exile the top card": of the library
            // just named, as "put it on top" is.
            let zone = p.attempt(|p| {
                p.word("of")?;
                let w = whose(p)?;
                p.word("library")?;
                Some(Zone::TopOfLibrary(w))
            }).unwrap_or(Zone::OwnersLibraryTop);
            Some(Objects {
                quant: Quant::N(n),
                filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(zone)], ..Filter::default() },
            })
        },
    )
}

/// What an earlier verb did to an object: "the exiled card".
pub(super) fn marked(p: &mut P) -> Option<Marked> {
    alt!(p;
        |p| p.word("exiled").map(|_| Marked::Exiled),
        |p| p.word("chosen").map(|_| Marked::Chosen),
        |p| p.word("revealed").map(|_| Marked::Revealed),
        |p| p.word("sacrificed").map(|_| Marked::Sacrificed),
        |p| p.word("copied").map(|_| Marked::Copied),
        |p| p.word("returned").map(|_| Marked::Returned),
        |p| p.word("destroyed").map(|_| Marked::Destroyed),
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

/// An object phrase depends on nothing but where it starts, and the grammar
/// tries the same one from many alternatives, so each start is read once.
pub(super) fn objects_no_player(p: &mut P) -> Option<Objects> {
    let start = p.i;
    // A comparison's value reads differently, so it neither uses nor fills
    // the memo.
    if p.in_value {
        return objects_read(p);
    }
    if let Some((o, end, furthest)) = p.objects_memo[start].clone() {
        p.furthest = p.furthest.max(furthest);
        if o.is_some() {
            p.i = end;
        }
        return o;
    }
    let before = std::mem::replace(&mut p.furthest, start);
    let o = objects_read(p);
    let furthest = p.furthest;
    p.furthest = before.max(furthest);
    p.objects_memo[start] = Some((o.clone(), p.i, furthest));
    o
}

fn objects_read(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| {
            let first = pronoun(p)?;
            if first.quant != Quant::SelfRef { return None; }
            p.words("or another")?;
            let filter = filter(p)?;
            Some(Objects { quant: Quant::SelfOrAnother, filter })
        },
        // "the artifact with the greatest mana value": one, picked out by
        // the post, not an earlier noun.
        |p| {
            p.word("the")?;
            let filter = filter(p)?;
            filter.posts.iter().any(|q| matches!(q, Post::Greatest { .. })).then_some(Objects { quant: Quant::One, filter })
        },
        pronoun,
        // "put a counter on each of up to two target creatures": each target.
        |p| {
            p.words("each of")?;
            let o = objects(p)?;
            matches!(o.quant, Quant::Target { .. }).then_some(o)
        },
        // "two or more of those creatures": some of an earlier noun.
        |p| {
            let quant = quant(p)?;
            if !matches!(quant, Quant::AtLeast(_) | Quant::AtMost(_) | Quant::N(_)) { return None; }
            p.word("of")?;
            let among = pronoun(p)?;
            (among.quant == Quant::Antecedent).then(|| Objects {
                quant,
                filter: Filter { posts: vec![Post::FromAmong(Box::new(among))], ..Filter::default() },
            })
        },
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
        // "a Shaman or another Wizard you control": the second alternative is
        // one other than this object, and the posts hold of both.
        |p| {
            p.one_of(&["a", "an"])?;
            let first = filter(p)?;
            if !first.posts.is_empty() || first.kind.is_some() || first.or.is_some() { return None; }
            p.words("or another")?;
            let mut second = filter(p)?;
            if second.kind.is_some() || second.or.is_some() { return None; }
            let mut alts = first.alts;
            for g in second.alts.iter_mut() {
                g.insert(0, Adj::State(State::Other));
            }
            alts.extend(second.alts);
            Some(Objects { quant: Quant::One, filter: Filter { alts, posts: second.posts, ..Filter::default() } })
        },
        |p| { let n = amount(p)?; p.words("target")?; p.word("targets").map(|_| Objects { quant: Quant::AnyTarget { n }, filter: Filter::default() }) },
        |p| {
            let quant = p.attempt(quant).unwrap_or(Quant::Plural);
            // "an exiled card": one in exile, where "the exiled card" is one
            // an earlier verb exiled.
            let exiled = p.attempt(|p| p.word("exiled")).is_some();
            let mut filter = filter(p)?;
            if exiled {
                filter.posts.insert(0, Post::In(Zone::Exile));
            }
            Some(Objects { quant, filter })
        },
    )
}

pub fn objects(p: &mut P) -> Option<Objects> {
    objects_no_player(p)
}
