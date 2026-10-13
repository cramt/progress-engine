//! Players, and the objects a rule names: quantifier, filter, posts.

use super::*;

/// A player phrase depends on nothing but where it starts, and almost every
/// effect rule asks for a subject first, so each start is read once, as
/// `objects_no_player` is.
pub(super) fn player(p: &mut P) -> Option<Player> {
    let start = p.i;
    // A comparison's value reads objects differently, so it bypasses both.
    if p.in_value {
        return player_read(p);
    }
    if let Some((pl, end, furthest)) = p.player_memo[start].clone() {
        p.furthest = p.furthest.max(furthest);
        if pl.is_some() {
            p.i = end;
        }
        return pl;
    }
    let before = std::mem::replace(&mut p.furthest, start);
    let pl = player_read(p);
    let furthest = p.furthest;
    p.furthest = before.max(furthest);
    p.player_memo[start] = Some((pl.clone(), p.i, furthest));
    pl
}

fn player_read(p: &mut P) -> Option<Player> {
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
        |p| p.words("up to one target player").map(|_| Player::UpToOneTarget),
        |p| p.words("any number of target players").map(|_| Player::AnyNumberOfTarget),
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
        |p| p.words("all opponents").map(|_| Player::EachOpponent),
        // "Players can't gain life this turn": a rule about each of them.
        |p| p.word("players").map(|_| Player::Each),
        |p| p.one_of(&["an opponent", "one of your opponents"]).map(|_| Player::AnOpponent),
        |p| p.words("a player").map(|_| Player::APlayer),
        |p| p.words("that player").map(|_| Player::That),
        |p| p.words("those players").map(|_| Player::That),
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
        // "its controller's graveyard": the player controlling it, who for a
        // stolen permanent or a spell is not its owner.
        |p| { p.words("its controller")?; p.tok(&Tok::Possessive)?; Some(Player::ControllerOf(Box::new(antecedent()))) },
        |p| { p.words("its owner")?; p.tok(&Tok::Possessive)?; Some(Player::OwnerOf(Box::new(antecedent()))) },
        // A bare "its graveyard", "its library": an object's hand, library
        // or graveyard is only ever its owner's (CR 400.3).
        |p| p.word("its").map(|_| Player::OwnerOf(Box::new(antecedent()))),
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
        "face-up" => State::FaceUp,
        "other" => State::Other,
        "modified" => State::Modified,
        "historic" => State::Historic,
        "enchanted" => State::Enchanted,
        "equipped" => State::Equipped,
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
    } else if matches!(
        w,
        "activated"
            | "triggered"
            | "loyalty"
            | "exhaust"
            | "cycling"
            | "mana"
            | "equip"
            | "ninjutsu"
    ) {
        // Only before "ability": "activated" alone is no kind of object, and
        // "loyalty" is a counter's kind before "counter".
        // "activated or triggered ability": each kind before the noun.
        let listed = w == "activated"
            && matches!(p.t.get(p.i + 1), Some(Tok::Word(c)) if c == "or" || c == "and")
            && matches!(p.t.get(p.i + 2), Some(Tok::Word(t)) if t == "triggered");
        if !listed
            && !matches!(p.t.get(p.i + 1), Some(Tok::Word(n)) if n == "ability" || n == "abilities")
        {
            return None;
        }
        Adj::Ability(match w {
            "activated" => AbilityKind::Activated,
            "triggered" => AbilityKind::Triggered,
            "exhaust" => AbilityKind::Exhaust,
            "cycling" => AbilityKind::Cycling,
            "mana" => AbilityKind::Mana,
            "equip" => AbilityKind::Equip,
            "ninjutsu" => AbilityKind::Ninjutsu,
            _ => AbilityKind::Loyalty,
        })
    } else if let Some(c) = color_word(w) {
        Adj::Color(c)
    } else if let Some(s) = state_word(w) {
        // "target enchanted permanent", "that's enchanted or equipped": a
        // state only after a quantifier or a verb of being; a bare
        // "enchanted creature" or "enchanted player" is the one this Aura
        // enchants (`Quant::Enchanted`, `Player::Enchanted`).
        if matches!(s, State::Enchanted | State::Equipped) {
            let before = p.i.checked_sub(1).and_then(|j| p.t.get(j));
            let after_quant = matches!(before, Some(Tok::Word(b)) if matches!(b.as_str(),
                "target" | "all" | "each" | "a" | "an" | "another" | "more" | "any"
                    | "that's" | "are" | "is" | "it's" | "was" | "were" | "or"));
            let player =
                matches!(p.t.get(p.i + 1), Some(Tok::Word(n)) if n == "player" || n == "players");
            if !after_quant || player {
                return None;
            }
        }
        // "creatures tapped this way": what an earlier verb did, a post.
        let this_way = matches!(p.t.get(p.i + 1), Some(Tok::Word(a)) if a == "this")
            && matches!(p.t.get(p.i + 2), Some(Tok::Word(b)) if b == "way");
        if this_way {
            return None;
        }
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
        |p| p.one_of(&["spells or abilities", "spell or ability", "spells and abilities"]).map(|_| Noun::SpellOrAbility),
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
            let k = counter_kind(p).filter(|k| !matches!(k, CounterKind::Named(w) if matches!(w.as_str(), "and" | "or" | "then" | "that" | "target" | "a" | "an" | "the")))?;
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
        // "enchanted creature or enchantment creature" lists a noun phrase
        // of its own, which takes nothing from the one before; "equipped
        // creature and Equipment" lists an Equipment, which is attached to
        // a creature, not equipped (CR 301.5) ([lead-words]).
        let takes = |g: &[Adj]| {
            let equipment = g == [Adj::Subtype("equipment".into())];
            let types = g.iter().filter(|a| matches!(a, Adj::Type(_))).count();
            types < 2 && !(equipment && lead.contains(&Adj::State(State::Equipped)))
        };
        for g in groups.iter_mut().skip(1) {
            if takes(g) {
                g.splice(0..0, lead.iter().cloned());
            }
        }
    }
    // "other activated and triggered abilities": the words before the first
    // kind of ability belong to each.
    let ability = |g: &[Adj]| matches!(g.last(), Some(Adj::Ability(_)));
    if groups.len() > 1
        && groups.iter().all(|g| ability(g))
        && groups[1..].iter().all(|g| g.len() == 1)
    {
        let lead: Vec<Adj> = groups[0][..groups[0].len() - 1].to_vec();
        for g in groups.iter_mut().skip(1) {
            g.splice(0..0, lead.iter().cloned());
        }
    }
    // "artifact, enchantment, or creature": each alternative names its own
    // kind, unlike "blue or black creature", which shares the last one's.
    let own_kinds = groups.len() > 1 && groups.iter().all(|g| g.iter().any(is_typed));
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
    // "Beast, Camel, Horse, Elephant, and Wolf creature": a card type after
    // a list of subtypes holds of each, as "white or black creature" does.
    // Words before the first subtype ("Other Ninja and Rogue creatures")
    // were spread over the list above, so a subtype list starts after them.
    let untyped = |g: &[Adj]| g.iter().take_while(|a| !is_typed(a)).count();
    let subtypes = |g: &[Adj]| g[untyped(g)..].iter().all(|a| matches!(a, Adj::Subtype(_)));
    if let Some((last, rest)) = groups.split_last_mut() {
        let lead = untyped(last)
            + last[untyped(last)..]
                .iter()
                .take_while(|a| matches!(a, Adj::Subtype(_)))
                .count();
        let trailing: Vec<Adj> = last[lead..].to_vec();
        if !rest.is_empty()
            && lead > untyped(last)
            && !trailing.is_empty()
            && trailing.iter().all(|a| matches!(a, Adj::Type(_)))
            && rest.iter().all(|g| g.len() > untyped(g) && subtypes(g))
        {
            for g in rest.iter_mut() {
                g.extend(trailing.iter().cloned());
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
    // "an instant card or a card with flash": an article said again starts
    // a noun phrase of its own, so what trails it is its alone.
    let mut article_again = false;
    let or = match (&kind, or_kind) {
        // Its posts follow the kind word but hold of both: "creature or
        // token you control".
        (_, Some(k)) => Some(Box::new(Filter {
            kind: Some(k),
            posts: posts.clone(),
            ..Filter::default()
        })),
        (Some(_), None) => p
            .attempt(|p| {
                // A list of kinds: "instant spell, sorcery spell, activated
                // ability, or triggered ability"; the comma needs a kind after it.
                let comma = p.attempt(|p| p.tok(&Tok::Comma)).is_some();
                let or = p.attempt(|p| p.word("or")).is_some();
                if !or && !comma {
                    return None;
                }
                // "a token or a land": after a bare kind, the second may be types;
                // "an instant card or a card with flash": after "or", a second
                // kind may take its own article.
                let again = (bare_kind || or) && p.attempt(|p| p.one_of(&["a", "an"])).is_some();
                let f = filter(p)?;
                // A comma with no "or" yet must be in a list that ends in one,
                // or "an enchantment spell, counter it" would read as a list.
                if comma && !or && f.or.is_none() {
                    return None;
                }
                if matches!(f.kind, Some(Noun::Counter(_))) {
                    return None;
                }
                (f.kind.is_some() || bare_kind && !f.alts.is_empty() && !comma)
                    .then_some((Box::new(f), again))
            })
            .map(|(f, again)| {
                article_again = again;
                f
            }),
        (None, None) => None,
    };
    let mut or = or;
    if own_kinds && or.is_none() && kind.is_none() {
        keyword_on_last_kind(&mut groups, &mut posts, &mut or);
    }
    if kind.is_some() {
        share_with_kinds(&groups, kind == Some(Noun::Card), &mut posts, &mut or);
        if !article_again && posts.is_empty() {
            share_trailing(kind.as_ref(), &mut posts, &or);
        }
    }
    Some(Filter {
        alts: groups,
        kind,
        posts,
        or,
    })
}

/// "a black or red permanent, spell, or card": colors said before a list
/// of kinds hold of each kind; "a creature card or a planeswalker card from
/// your graveyard": a zone said after a list of cards holds of each too.
/// Each kind's filter gets them, so no kind reads as any color or as
/// anywhere. A zone after a list of other kinds is the last one's: "target
/// spell, nonland permanent, or card in a graveyard".
fn share_with_kinds(
    groups: &[Vec<Adj>],
    card: bool,
    posts: &mut Vec<Post>,
    or: &mut Option<Box<Filter>>,
) {
    let Some(first_or) = or.as_deref_mut() else {
        return;
    };
    let colors = !groups.is_empty()
        && groups.iter().all(|g| {
            g.iter()
                .all(|a| matches!(a, Adj::Color(_) | Adj::NonColor(_)))
        });
    // The chain's last filter holds the posts said after the whole list.
    let mut last = &*first_or;
    let mut all_cards = card && last.kind == Some(Noun::Card);
    while let Some(next) = last.or.as_deref() {
        last = next;
        all_cards &= last.kind == Some(Noun::Card);
    }
    // Only to kinds said with nothing after them: "a card revealed this way
    // or a card in their graveyard" names a card in hand first.
    // "a Plains card or a creature card … from among the cards milled this
    // way": where the cards are picked from is a zone too.
    let zone = match last.posts.last() {
        Some(q @ (Post::In(_) | Post::FromAmong(_) | Post::FromAmongThem))
            if all_cards && posts.is_empty() =>
        {
            Some(q.clone())
        }
        _ => None,
    };
    if let Some(z) = &zone {
        posts.push(z.clone());
    }
    let mut next = Some(first_or);
    while let Some(f) = next {
        let is_last = f.or.is_none();
        if colors && f.alts.is_empty() {
            f.alts = groups.to_vec();
        }
        if let (Some(z), false) = (&zone, is_last) {
            if f.posts.is_empty() {
                f.posts.push(z.clone());
            }
        }
        next = f.or.as_deref_mut();
    }
}

/// "target artifact, enchantment, or creature with flying", "Walls and/or
/// creatures with flying": a keyword said after a list of kinds is the last
/// kind's. Atraxa's Fall lists "battle" before "creature with flying", which
/// would be pointless if flying held of the battle too, and a Wall with
/// flying is already a creature with flying. The last kind becomes its own
/// filter in the `or` chain; any other post still holds of every kind.
fn keyword_on_last_kind(
    groups: &mut Vec<Vec<Adj>>,
    posts: &mut Vec<Post>,
    or: &mut Option<Box<Filter>>,
) {
    let keyword = |q: &Post| matches!(q, Post::WithKeyword(_) | Post::WithoutKeyword(_));
    if !posts.iter().any(keyword) {
        return;
    }
    let last = groups.pop().expect("own_kinds has two groups");
    let shared: Vec<Post> = posts.iter().filter(|q| !keyword(q)).cloned().collect();
    *or = Some(Box::new(Filter {
        alts: vec![last],
        kind: None,
        posts: std::mem::replace(posts, shared),
        or: None,
    }));
}

/// "target spell or nonland permanent an opponent controls", "target spell
/// or permanent with mana value 1 or greater": with one article for the
/// list, what trails it holds of each kind it can. A spell has a controller
/// (CR 601.2a) as a permanent does (CR 110.2); a card elsewhere has none
/// (CR 108.4), and a zone is shared only among cards (`share_with_kinds`).
fn share_trailing(kind: Option<&Noun>, posts: &mut Vec<Post>, or: &Option<Box<Filter>>) {
    let Some(mut last) = or.as_deref() else {
        return;
    };
    while let Some(next) = last.or.as_deref() {
        last = next;
    }
    let holds = |q: &Post| match q {
        Post::In(_) | Post::NotIn(_) => false,
        Post::Controller(_) | Post::NotControlledBy(_) => {
            matches!(kind, Some(Noun::Spell | Noun::Permanent))
        }
        Post::With(_) | Post::ThatIs(_) => true,
        _ => false,
    };
    if !last.posts.is_empty() && last.posts.iter().all(holds) {
        posts.extend(last.posts.iter().cloned());
    }
}

pub(super) fn compare(p: &mut P) -> Option<Compare> {
    let stat = stat(p)?;
    let (op, value) = comparison(p)?;
    Some(Compare { stat, op, value })
}

/// What a number is compared with: "3 or less", "greater than its base power".
pub(super) fn comparison(p: &mut P) -> Option<(Op, Amount)> {
    alt!(p;
        // "4, 5, or 6", "2 or 3": one of the values.
        |p| {
            let mut v = vec![number(p).map(Amount::N)?];
            while let Some(n) = p.attempt(|p| { p.tok(&Tok::Comma)?; number(p).map(Amount::N) }) { v.push(n); }
            if v.len() > 1 { p.tok(&Tok::Comma)?; }
            p.word("or")?;
            v.push(number(p).map(Amount::N)?);
            // "3 or less", "3 or greater" are a bound, read below.
            Some((Op::Eq, Amount::AnyOf(v)))
        },
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
        |p| {
            p.word("from")?;
            p.one_of(&["a", "an"])?;
            let g = group(p)?;
            p.word("source")?;
            Some(Post::FromSource(g))
        },
        |p| p.words("you control").map(|_| Post::Controller(Player::You)),
        |p| p.words("you don't control").map(|_| Post::NotControlledBy(Player::You)),
        |p| p.words("you own").map(|_| Post::Owner(Player::You)),
        // Not "your opponents own": in Two-Headed Giant a teammate owns cards
        // too (CR 810.1).
        |p| p.words("you don't own").map(|_| Post::NotOwnedBy(Player::You)),
        |p| { let pl = player_simple(p)?; p.one_of(&["controls", "control"])?; Some(Post::Controller(pl)) },
        |p| { let pl = player_simple(p)?; p.one_of(&["doesn't control", "don't control"])?; Some(Post::NotControlledBy(pl)) },
        |p| { p.words("with total")?; compare(p).map(Post::Total) },
        |p| { p.word("with")?; compare(p).map(itself).map(Post::With) },
        |p| {
            p.word("with")?;
            p.one_of(&["a", "one or more"])?;
            let k = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.one_of(&["on it", "on them"])?;
            Some(Post::WithCounter(Some(k)))
        },
        // "each nonland permanent without a fate counter on it" (Oblivion
        // Stone).
        |p| {
            p.word("without")?;
            p.one_of(&["a", "an"])?;
            let k = counter_kind(p)?;
            p.word("counter")?;
            p.one_of(&["on it", "on them"])?;
            Some(Post::Not(Box::new(Post::WithCounter(Some(k)))))
        },
        // "with +1/+1 counters on them": the plural, with no article.
        |p| {
            p.word("with")?;
            let k = counter_kind(p)?;
            p.word("counters")?;
            p.words("on them")?;
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
        // "in graveyards and libraries": any zone of either kind.
        |p| {
            if p.one_of(&["in", "from"])? == "from" && p.in_value { return None; }
            let a = zone(p)?;
            p.one_of(&["and", "or"])?;
            let b = zone(p)?;
            let any = |z: &Zone| matches!(z, Zone::AGraveyard | Zone::ALibrary);
            (any(&a) && any(&b)).then(|| Post::In(Zone::AnyOf(vec![a, b])))
        },
        |p| {
            if p.one_of(&["in", "from"])? == "from" && p.in_value { return None; }
            zone(p).map(Post::In)
        },
        |p| p.one_of(&["that's blocking", "that are blocking"]).map(|_| Post::Blocking),
        |p| { p.words("other than")?; self_noun(p).map(|o| Post::OtherThanSelf(o.filter)) },
        |p| p.words("of the chosen type").map(|_| Post::OfChosenType),
        |p| p.words("of that type").map(|_| Post::OfThatType),
        |p| p.words("of the chosen color").map(|_| Post::OfChosenColor),
        |p| {
            p.word("with")?;
            let s = stat(p)?;
            p.word("of")?;
            p.one_of(&["the chosen quality", "that quality"])?;
            Some(Post::OfChosenParity(s))
        },
        |p| p.words("of that color").map(|_| Post::OfThatColor),
        |p| p.words("attached to it").map(|_| Post::AttachedToIt),
        |p| {
            p.one_of(&["that's attached to", "that are attached to"])?;
            objects(p).map(|o| Post::AttachedTo(Box::new(o)))
        },
        // "an ability that isn't a mana ability", "a creature that isn't a
        // Dragon": not of that kind.
        |p| {
            p.one_of(&["that isn't a", "that isn't an"])?;
            let f = filter(p)?;
            if f.posts.is_empty() && f.or.is_none() && !f.alts.is_empty() {
                Some(Post::Not(Box::new(Post::ThatIs(f))))
            } else {
                None
            }
        },
        // "creatures that aren't enchanted": not in that state.
        |p| {
            p.one_of(&["that aren't", "that isn't"])?;
            let s = p.peek_word().and_then(state_word).filter(|s| matches!(s, State::Enchanted | State::Equipped | State::Tapped | State::Untapped))?;
            p.bump();
            let f = Filter { alts: vec![vec![Adj::State(s)]], ..Filter::default() };
            Some(Post::Not(Box::new(Post::ThatIs(f))))
        },
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
        // "of an opponent's choice".
        |p| {
            p.word("of")?;
            let who = player_simple(p)?;
            p.tok(&Tok::Possessive)?;
            p.word("choice")?;
            Some(Post::ChosenBy(who))
        },
        |p| { p.words("with power or toughness")?; let (op, value) = comparison(p)?; Some(Post::PowerOrToughness { op, value }) },
        |p| p.words("that died this turn").map(|_| Post::DiedThisTurn),
        |p| { p.words("that died under")?; let who = whose_pronoun(p)?; p.words("control this turn")?; Some(Post::DiedUnderControlThisTurn(who)) },
        |p| { p.one_of(&["that was", "that were"])?; p.words("put there from the battlefield this turn").map(|_| Post::PutThereFromBattlefieldThisTurn) },
        // "a nonland card from it": the hand just revealed, said by pronoun.
        |p| {
            p.word("from")?;
            p.word("it")?;
            Some(Post::From(Box::new(antecedent())))
        },
        |p| p.words("you've cast this turn").map(|_| Post::CastThisTurn(Player::You)),
        |p| p.words("you cast this turn").map(|_| Post::WillCastThisTurn(Player::You)),
        // "with a cycling ability", "that has a cycling ability".
        |p| {
            alt!(p;
                |p| p.words("with a").map(|_| ()),
                |p| p.word("with").map(|_| ()),
                |p| p.one_of(&["that has a", "that have a"]).map(|_| ()),
            )?;
            let kind = match descriptor(p)? { Adj::Ability(k) => k, _ => return None };
            p.one_of(&["ability", "abilities"])?;
            Some(Post::WithAbilityOf(kind))
        },
        // "with {X} in its mana cost", "without {X} in their mana costs".
        |p| {
            let without = p.one_of(&["with", "without"])? == "without";
            p.tok(&Tok::Sym("X".into()))?;
            p.word("in")?;
            p.one_of(&["its", "their"])?;
            p.word("mana")?;
            p.one_of(&["costs", "cost"])?;
            Some(if without { Post::NoXInManaCost } else { Post::XInManaCost })
        },
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
            p.one_of(&["from among them", "from among those cards"])?;
            (!p.in_value).then_some(Post::FromAmongThem)
        },
        // "cards exiled with it": "it" is the object whose ability exiled
        // them, which on a card's own text is the card (CR 607.2a).
        |p| { p.words("exiled with it")?; Some(Post::ExiledWith(Box::new(self_ref()))) },
        |p| { p.words("exiled with")?; self_noun(p).map(|o| Post::ExiledWith(Box::new(o))) },
        |p| { p.words("exiled with")?; objects(p).map(|o| Post::ExiledWith(Box::new(o))) },
        |p| p.words("you cast").map(|_| Post::Cast(Player::You)),
        |p| p.words("drawn this turn").map(|_| Post::DrawnThisTurn),
        |p| { p.word("named")?; pronoun(p).map(|o| Post::Named(Box::new(o))) },
        // "a creature named Hanweir Garrison": another card, by its name.
        |p| { p.word("named")?; card_name(p, &[",", "with", "and"]).map(Post::NamedCard) },
        |p| { p.one_of(&["that target", "that targets"])?; objects(p).map(|o| Post::Targets(Box::new(o))) },
        // "a spell that's exactly two colors", "that isn't exactly two colors".
        |p| {
            let not = p.one_of(&["that's exactly", "that are exactly", "that isn't exactly", "that aren't exactly"])?.contains("n't");
            let n = number(p)?;
            p.one_of(&["colors", "color"])?;
            Some(if not { Post::Not(Box::new(Post::ExactlyColors(n))) } else { Post::ExactlyColors(n) })
        },
        |p| { p.word("that's")?; filter(p).map(Post::ThatIs) },
        |p| { let how = marked_this_way(p)?; p.words("this way")?; Some(Post::Marked(how)) },
        |p| p.one_of(&["who doesn't", "who don't"]).map(|_| Post::WhoDoesnt),
        |p| p.words("who can't").map(|_| Post::WhoCant),
        |p| { p.one_of(&["that aren't", "that isn't"])?; p.one_of(&["on", "in"])?; zone(p).map(Post::NotIn) },
        // "card not on the battlefield".
        |p| { p.word("not")?; p.one_of(&["on", "in"])?; zone(p).map(Post::NotIn) },
        |p| { p.one_of(&["that aren't", "that isn't"])?; post(p).map(|q| Post::Not(Box::new(q))) },
        |p| {
            p.words("that entered")?;
            p.attempt(|p| p.words("the battlefield"));
            p.words("under your control this turn")?;
            Some(Post::EnteredUnderThisTurn(Player::You))
        },
        |p| p.one_of(&["that entered this turn", "that entered the battlefield this turn"]).map(|_| Post::EnteredThisTurn),
        |p| {
            if p.in_value { return None; }
            p.words("from among")?;
            let mut o = objects(p)?;
            // "from among the nonland permanents they control": who controls
            // them says which they are, so "the" points back to nothing.
            if o.quant == Quant::Antecedent && o.filter.posts.iter().any(|q| matches!(q, Post::Controller(_))) {
                o.quant = Quant::All;
            }
            Some(Post::FromAmong(Box::new(o)))
        },
        // "with the greatest mana value among creatures and planeswalkers
        // they control": greatest within those.
        |p| {
            p.words("with the greatest")?;
            let stat = stat(p)?;
            p.word("among")?;
            let among = objects(p)?;
            Some(Post::GreatestAmong { stat, among: Box::new(among) })
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
        |p| { p.words("with the same name as")?; let o = pronoun(p)?; Some(Post::SameNameAs(Box::new(o))) },
        |p| p.one_of(&["with the same name as one another", "with the same name"]).map(|_| Post::SameName),
        |p| p.words("dealt damage this way").map(|_| Post::DealtDamageThisWay),
        |p| p.words("that were cast this turn").map(|_| Post::WasCastThisTurn),
        |p| p.words("who lost life this turn").map(|_| Post::LostLifeThisTurn),
        |p| { p.words("that could produce")?; mana(p).map(Post::CouldProduce) },
        // "that is snow or could produce {C}": either predicate.
        |p| {
            p.words("that is")?;
            let f = filter(p)?;
            p.words("or could produce")?;
            let m = mana(p)?;
            Some(Post::Either(vec![Post::ThatIs(f), Post::CouldProduce(m)]))
        },
        |p| {
            p.word("with")?;
            let n = number(p)?;
            p.words("or more card types among them")?;
            Some(Post::CardTypesAmongAtLeast(n))
        },
        // "cards exiled from their hand this way".
        |p| {
            let how = marked(p)?;
            p.word("from")?;
            let from = zone(p)?;
            p.words("this way")?;
            Some(Post::MarkedFrom { how, from })
        },
        |p| p.words("with that name").map(|_| Post::WithThatName),
        // "[with mana value 2 or less]": the words cleave removes.
        |p| {
            p.tok(&Tok::Other('['))?;
            let mut v = vec![post(p)?];
            while let Some(q) = p.attempt(post) { v.push(q); }
            p.tok(&Tok::Other(']'))?;
            Some(Post::Cleave(v))
        },
        |p| { p.words("other than")?; p.one_of(&["a", "an"])?; filter(p).map(Post::OtherThan) },
        // "you've cycled or discarded this turn".
        |p| {
            p.word("you've")?;
            let verb = |p: &mut P| alt!(p;
                |p| p.word("cycled").map(|_| PastVerb::Cycled),
                |p| p.word("discarded").map(|_| PastVerb::Discarded),
            );
            let mut verbs = vec![verb(p)?];
            while let Some(v) = p.attempt(|p| { p.word("or")?; verb(p) }) { verbs.push(v); }
            p.words("this turn")?;
            Some(Post::DoneThisTurn { who: Player::You, verbs })
        },
        |p| p.one_of(&["they've cast this turn", "that player has cast this turn"]).map(|_| Post::CastThisTurn(Player::That)),
        // "that player or that planeswalker's controller controls".
        |p| {
            let a = player_simple(p)?;
            p.word("or")?;
            let b = player(p)?;
            p.one_of(&["controls", "control"])?;
            Some(Post::Controller(Player::OneOf(vec![a, b])))
        },
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
        // "three or more creatures with different powers".
        |p| {
            p.words("with different")?;
            alt!(p;
                |p| p.word("powers").map(|_| Stat::Power),
                |p| p.word("toughnesses").map(|_| Stat::Toughness),
                |p| p.words("mana values").map(|_| Stat::ManaValue),
            ).map(Post::DifferentValues)
        },
        |p| { let pl = player_simple(p)?; p.one_of(&["owns", "own"])?; Some(Post::Owner(pl)) },
        |p| {
            let by = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            let how = marked(p)?;
            p.words("this way")?;
            Some(Post::MarkedBy { how, by })
        },
        // "creatures you control each with power greater than its base power":
        // "each" says the post holds of every one, as it would anyway.
        |p| { p.words("each with")?; compare(p).map(itself).map(Post::With) },
        |p| { let pl = player_simple(p)?; p.one_of(&["casts", "cast"])?; Some(Post::Cast(pl)) },
    )
}

/// "other than this creature", "exiled with this artifact": the card by its
/// own noun. That noun names one object already, so it takes no posts: what
/// trails it qualifies the objects the phrase is said of. "Creatures you
/// control other than this creature with total power 10 or greater" bounds
/// the tapped creatures' power, and Ichorid's "a black creature card other
/// than this card from your graveyard" is a card in your graveyard.
fn self_noun(p: &mut P) -> Option<Objects> {
    alt!(p;
        |p| p.card_name(),
        |p| {
            p.word("this")?;
            let alts: Vec<Vec<Adj>> = p.attempt(group).into_iter().collect();
            let kind = p.attempt(|p| kind(p).filter(|k| !matches!(k, Noun::Counter(_))));
            (!alts.is_empty() || kind.is_some()).then(|| Objects {
                quant: Quant::SelfRef,
                filter: Filter { alts, kind, ..Filter::default() },
            })
        },
    )
}

/// Rod of Absorption: "any number of spells from among cards exiled with
/// this artifact with total mana value X or less": a total bounds the
/// objects chosen, not the set they are chosen from, whose total nobody
/// picks (its ruling: "you cast as many spells with total mana value X or
/// less from among the exiled cards as you would like").
fn total_of_the_chosen(q: Post) -> Vec<Post> {
    match q {
        Post::FromAmong(mut o) if matches!(o.filter.posts.last(), Some(Post::Total(_))) => {
            let total = o.filter.posts.pop().expect("matched a last post");
            vec![Post::FromAmong(o), total]
        }
        // Kotose: "target card other than a basic land card from an
        // opponent's graveyard": where the card is is the target's, as
        // after "other than this card" ([after-self-noun]).
        Post::OtherThan(mut f) if matches!(f.posts.last(), Some(Post::In(_) | Post::From(_))) => {
            let zone = f.posts.pop().expect("matched a last post");
            vec![Post::OtherThan(f), zone]
        }
        q => vec![q],
    }
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
        // "a card with the same name as that spell in your graveyard": a
        // reference back names its object already, so the zone after it is
        // the card's ([same-name-zone]).
        |p| {
            p.words("with the same name as")?;
            let mut o = pronoun(p)?;
            if o.quant != Quant::Antecedent || !matches!(o.filter.posts.last(), Some(Post::In(_))) {
                return None;
            }
            let zone = o.filter.posts.pop()?;
            Some(vec![Post::SameNameAs(Box::new(o)), zone])
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
        // "cards that are black or red from all graveyards", "permanents
        // that are Spirits and/or enchantments": a filter's alternatives, the
        // posts after them the objects'. "That are tapped and attacking" is
        // both states at once, which alternatives cannot say, so it is left
        // to the token rule that reads it.
        |p| {
            p.words("that are")?;
            let mut f = filter(p)?;
            let states = f.alts.len() > 1 && f.alts.iter().all(|g| g.iter().all(|a| matches!(a, Adj::State(_))));
            if states { return None; }
            let posts = std::mem::take(&mut f.posts);
            let mut v = vec![Post::ThatIs(f)];
            v.extend(posts);
            Some(v)
        },
        // Only a counter is "on" something; elsewhere "on" starts a destination.
        |p| post(p).filter(|q| counter || !matches!(q, Post::On(_))).map(total_of_the_chosen),
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
        |p| p.one_of(&["an opponent", "one of your opponents"]).map(|_| Player::AnOpponent),
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
        |p| { p.words("one , two , or three target")?; Some(Quant::Target { n: Amount::Between(1, 3), up_to: false }) },
        |p| { p.words("up to")?; let n = amount(p)?; p.word("target")?; Some(Quant::Target { n, up_to: true }) },
        |p| { p.words("up to")?; amount(p).map(Quant::UpTo) },
        |p| p.words("another target").map(|_| Quant::AnotherTarget),
        |p| p.words("a second target").map(|_| Quant::SecondTarget),
        |p| p.words("any number of target").map(|_| Quant::AnyNumberOfTarget),
        |p| { let n = amount(p)?; p.word("target")?; Some(Quant::Target { n, up_to: false }) },
        |p| { let n = amount(p)?; p.words("or more")?; Some(Quant::AtLeast(n)) },
        |p| { p.words("at least")?; amount(p).map(Quant::AtLeast) },
        |p| { let n = amount(p)?; p.words("or fewer").map(|_| Quant::AtMost(n)) },
        |p| p.word("target").map(|_| Quant::Target { n: Amount::N(1), up_to: false }),
        |p| p.words("the next").map(|_| Quant::Next),
        |p| p.word("additional").map(|_| Quant::Additional),
        // "each other creature": the "other" is the filter's own word.
        |p| p.word("each").map(|_| Quant::Each),
        |p| p.words("all the").map(|_| Quant::All),
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
        |p| p.card_name(),
        // "this creature" names the card by its own type, so a list after
        // it is a second object, read by `self_and_more`.
        |p| {
            p.word("this")?;
            let f = filter(p)?;
            let o = with_others(Quant::SelfRef, f);
            (o.quant != Quant::SelfRef || o.filter.or.is_none() && o.filter.alts.len() < 2).then_some(o)
        },
        |p| p.one_of(&["each of them", "it", "them", "they"]).map(|_| antecedent()),
        // "put X counters on him", "he deals damage": Oracle text gives a
        // gendered pronoun only to a character card naming itself.
        |p| p.one_of(&["him", "her", "he", "she"]).map(|_| self_ref()),
        // "the rest of the exiled cards".
        |p| {
            p.words("the rest of")?;
            let among = pronoun(p)?;
            // "Put the rest of the cards on the bottom" (Winota), "the rest
            // of those cards": what the earlier choice left of the cards
            // looked at, which `Rest` says ([the-other]); a reference back
            // would be the card just chosen.
            let bare = Objects { quant: Quant::Antecedent, filter: Filter { kind: Some(Noun::Card), ..Filter::default() } };
            if among == bare {
                return Some(Objects { quant: Quant::Rest, filter: Filter::default() });
            }
            Some(Objects { quant: Quant::Rest, filter: Filter { posts: vec![Post::FromAmong(Box::new(among))], ..Filter::default() } })
        },
        // "the top half of their library, rounded up".
        |p| {
            p.words("the top half of")?;
            let w = whose(p)?;
            p.word("library")?;
            p.tok(&Tok::Comma)?;
            p.word("rounded")?;
            let round_up = p.one_of(&["up", "down"])? == "up";
            let library = Objects { quant: Quant::Plural, filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(Zone::Library(w.clone()))], ..Filter::default() } };
            let n = Amount::Half { of: Box::new(Amount::CountOf(Box::new(library))), round_up };
            Some(Objects { quant: Quant::N(n), filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(Zone::TopOfLibrary(w))], ..Filter::default() } })
        },
        // "each of those creatures": the earlier noun, each one.
        |p| { p.words("each of")?; let o = pronoun(p)?; (o.quant == Quant::Antecedent).then_some(o) },
        |p| p.words("the rest").map(|_| Objects { quant: Quant::Rest, filter: Filter::default() }),
        |p| p.one_of(&["the other", "the others"]).map(|_| Objects { quant: Quant::Rest, filter: Filter::default() }),
        // "twice that many of each of those kinds of counters": the counters
        // an event named, kind by kind.
        |p| p.words("each of those kinds of counters").map(|_| Objects {
            quant: Quant::Antecedent,
            filter: Filter { kind: Some(Noun::Counter(None)), ..Filter::default() },
        }),
        // "Put both cards onto the battlefield": the two just named.
        |p| { p.word("both")?; let f = filter(p)?; Some(Objects { quant: Quant::Both, filter: f }) },
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
        // Spinerock Tyrant: "you may copy it. If you do, those spells gain
        // wither": the spell copied and its copy (a copy of a spell is a
        // spell, CR 707.10), where "that spell" would be one of them.
        |p| {
            let copied = p.t[..p.i].iter().any(|t| matches!(t, Tok::Word(w) if w == "copy"));
            p.words("those spells")?;
            copied.then(|| Objects {
                quant: Quant::OriginalAndCopies,
                filter: Filter { kind: Some(Noun::Spell), ..Filter::default() },
            })
        },
        |p| {
            let start = p.i;
            p.one_of(&["that", "those", "the"])?;
            // "the exiled card", "the token", "those creatures".
            let marked = p.attempt(marked);
            let mut f = filter(p)?;
            match marked {
                // Isochron Scepter's "{2}, {T}: You may copy the exiled card":
                // nothing in this ability exiled it, so it is the card its
                // linked ability exiled with this object (CR 607.2a). An
                // instant's paragraphs are one spell, so there it is what
                // another of them exiled.
                Some(Marked::Exiled) if !p.spell && !exiled_earlier(&p.t[..start]) => {
                    f.posts.insert(0, Post::ExiledWith(Box::new(self_ref())));
                    return Some(Objects { quant: Quant::Linked, filter: f });
                }
                // Dance of Many's "When this enchantment leaves the
                // battlefield, exile the token": a token its linked ability
                // created (CR 607.1), where this one created none.
                None if f.kind == Some(Noun::Token)
                    && f.alts.is_empty()
                    && f.posts.is_empty()
                    && !p.spell
                    && !created_earlier(&p.t[..start]) =>
                {
                    f.posts.insert(0, Post::CreatedWith(Box::new(self_ref())));
                    return Some(Objects { quant: Quant::Linked, filter: f });
                }
                Some(m) => f.posts.insert(0, Post::Marked(m)),
                None => {}
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

/// Whether the words of an ability before a phrase exile something, so
/// that "the exiled card" can be what they exiled.
fn exiled_earlier(before: &[Tok]) -> bool {
    before
        .iter()
        .any(|t| matches!(t, Tok::Word(w) if w == "exile" || w == "exiles"))
}

/// Whether the words of an ability before a phrase create a token.
fn created_earlier(before: &[Tok]) -> bool {
    before
        .iter()
        .any(|t| matches!(t, Tok::Word(w) if w == "create" || w == "creates" || w == "created"))
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
        |p| p.word("milled").map(|_| Marked::Milled),
        |p| p.word("discarded").map(|_| Marked::Discarded),
    )
}

/// A mark said with "this way" after the noun: "creatures tapped this way".
/// "Tapped" alone before a noun is the state ("those tapped creatures"), so
/// it marks only here.
fn marked_this_way(p: &mut P) -> Option<Marked> {
    alt!(p;
        marked,
        |p| p.word("tapped").map(|_| Marked::Tapped),
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

/// "this creature or a commander you control", "this creature and land
/// creatures you control": the card by its type, then a second object.
fn self_and_more(p: &mut P) -> Option<Objects> {
    p.word("this")?;
    let me = Objects {
        quant: Quant::SelfRef,
        filter: Filter {
            alts: vec![group(p)?],
            ..Filter::default()
        },
    };
    let or = p.one_of(&["or a", "or an", "and"])?;
    // "this land and a creature named …" is a list of two objects, which
    // the caller reads; "this enchantment and counter that spell" is two
    // clauses.
    let filter = filter(p).filter(|f| !matches!(f.kind, Some(Noun::Counter(_))))?;
    // "this creature and other Vampire creatures" is `WithOthers`.
    if or == "and"
        && filter
            .alts
            .iter()
            .any(|g| g.first() == Some(&Adj::State(State::Other)))
    {
        return None;
    }
    Some(Objects {
        quant: if or == "and" {
            Quant::SelfAnd(Box::new(me))
        } else {
            Quant::SelfOrA(Box::new(me))
        },
        filter,
    })
}

fn objects_read(p: &mut P) -> Option<Objects> {
    alt!(p;
        self_and_more,
        // "~ and other Knights you control": the card by name, and the others.
        |p| {
            p.tok(&Tok::SelfRef)?;
            p.words("and other")?;
            let filter = filter(p)?;
            Some(Objects { quant: Quant::WithOthers(Box::new(self_ref())), filter })
        },
        |p| {
            let first = pronoun(p)?;
            if first.quant != Quant::SelfRef { return None; }
            p.words("or another")?;
            let filter = filter(p)?;
            Some(Objects { quant: Quant::SelfOrAnother(Box::new(first)), filter })
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
            matches!(o.quant, Quant::Target { .. } | Quant::AnyNumberOfTarget).then_some(o)
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
        // "the first creature spell you cast each turn": the same spell.
        |p| {
            p.word("the")?;
            let n = ["first", "second", "third"].iter().position(|w| p.word(w).is_some())? as u32 + 1;
            let mut filter = filter(p)?;
            if filter.kind != Some(Noun::Spell) { return None; }
            let Some(Post::Cast(of)) = filter.posts.first().cloned() else { return None };
            filter.posts.remove(0);
            p.words("each turn")?;
            Some(Objects { quant: Quant::NthEachTurn { n, of }, filter })
        },
        |p| {
            let of = whose_pronoun(p)?;
            let n = ordinal(p)?;
            let filter = filter(p)?;
            p.words("of the game")?;
            Some(Objects { quant: Quant::NthOfGame { n, of }, filter })
        },
        // "target nonland permanent and all other permanents with the same
        // name as that permanent": one verb for the one and the rest.
        |p| {
            if !p.ahead("and all other") { return None; }
            let quant = quant(p)?;
            if !matches!(quant, Quant::Target { .. }) { return None; }
            let first = filter(p)?;
            p.words("and all other")?;
            let rest = filter(p)?;
            let mut first = first;
            // "target creature card and all other cards with the same name
            // as that card from your graveyard": the zone holds of the
            // target card too ([same-name-zone], [zone-after-cards]).
            if first.kind == Some(Noun::Card) && !first.posts.iter().any(|q| matches!(q, Post::In(_))) {
                if let Some(z) = rest.posts.iter().find(|q| matches!(q, Post::In(_))) {
                    first.posts.push(z.clone());
                }
            }
            Some(Objects { quant: Quant::WithAllOthers(Box::new(Objects { quant, filter: first })), filter: rest })
        },
        |p| {
            // "your commander": a possessive picks out the object, where a
            // possessive before a zone ("your graveyard") is read by `zone`.
            let who = whose_pronoun(p)?;
            let filter = filter(p)?;
            (filter.kind == Some(Noun::Commander)).then_some(Objects { quant: Quant::Possessive(who), filter })
        },
        |p| p.words("any target").map(|_| Objects { quant: Quant::AnyTarget { n: Amount::N(1) }, filter: Filter::default() }),
        // "divided as you choose among one or two targets".
        |p| p.words("one or two targets").map(|_| Objects { quant: Quant::AnyTarget { n: Amount::Between(1, 2) }, filter: Filter::default() }),
        |p| p.words("one , two , or three targets").map(|_| Objects { quant: Quant::AnyTarget { n: Amount::Between(1, 3) }, filter: Filter::default() }),
        // "any other target": other than the object the sentence names
        // before it.
        |p| p.words("any other target").map(|_| Objects { quant: Quant::AnyOtherTarget, filter: Filter::default() }),
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
        // "up to one other target creature or spell": "other" said before
        // "target" holds of every kind the filter lists.
        |p| {
            let up_to = p.attempt(|p| p.words("up to")).is_some();
            let n = amount(p)?;
            p.words("other target")?;
            let filter = filter(p)?;
            Some(Objects { quant: Quant::OtherTarget { n, up_to }, filter })
        },
        |p| {
            let quant = p.attempt(quant).unwrap_or(Quant::Plural);
            // "an exiled card": one in exile, where "the exiled card" is one
            // an earlier verb exiled.
            let exiled = p.attempt(|p| p.word("exiled")).is_some();
            let mut filter = filter(p)?;
            if exiled {
                filter.posts.insert(0, Post::In(Zone::Exile));
            }
            // "a card with the same name": one object has no "one another"
            // to share a name with, so it is the same name as the object
            // named before (Bazaar of Wonders' spell, Winnow's target).
            if matches!(quant, Quant::One | Quant::Another) {
                for q in filter.posts.iter_mut() {
                    if *q == Post::SameName {
                        *q = Post::SameNameAs(Box::new(antecedent()));
                    }
                }
            }
            Some(Objects { quant, filter })
        },
    )
}

pub fn objects(p: &mut P) -> Option<Objects> {
    objects_no_player(p)
}

/// "with power greater than its base power", "each with toughness greater
/// than its power": the possessive in the comparison a filter tests an
/// object by is that object's own.
fn itself(mut c: Compare) -> Compare {
    if let Amount::PowerOf(o)
    | Amount::ToughnessOf(o)
    | Amount::ManaValueOf(o)
    | Amount::BasePowerOf(o) = &mut c.value
    {
        if **o == antecedent() {
            o.quant = Quant::Itself;
        }
    }
    c
}
