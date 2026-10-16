//! Static abilities, replacement effects among them.

use super::*;

pub(super) fn static_sentence(p: &mut P) -> Option<Vec<Static>> {
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
    // "X can't be greater than the number of players in the game." alone.
    if let Some(s) = p.attempt(|p| {
        let x = super::x_limit(p)?;
        p.end_sentence()?;
        Some(vec![Static::X(x)])
    }) {
        return Some(s);
    }
    // "Enchanted land is a 3/3 black Ooze creature with haste. It's still a
    // land.": a "becomes" that holds while the static does.
    if let Some(s) = p.attempt(|p| {
        let what = objects(p)?;
        p.one_of(&["is", "are"])?;
        p.one_of(&["a", "an"])?;
        let pt = match p.peek()? {
            Tok::Pt(pt) if !pt.signed => *pt,
            _ => return None,
        };
        p.bump();
        let types = becomes_types(p);
        types
            .iter()
            .any(|a| *a == Adj::Type("creature".into()))
            .then_some(())?;
        let has = p
            .attempt(|p| {
                p.word("with")?;
                keyword_list(p)
            })
            .unwrap_or_default();
        let keeps = match p.attempt(keeps) {
            Some(k) => k,
            None => p
                .attempt(still)
                .unwrap_or_else(|| Keeps::implied_by(&types)),
        };
        p.end_sentence()?;
        Some(vec![Static::IsA {
            what,
            pt,
            types,
            has,
            keeps,
        }])
    }) {
        return Some(s);
    }
    // "You can't cast ~ during your first, second, or third turns of the
    // game": a cast restriction while it's one of those turns. Before the
    // rules that read "you can't cast ~" alone.
    if let Some(s) = p.attempt(|p| {
        p.words("you can't cast")?;
        let what = objects(p)?;
        p.words("during your")?;
        let mut v = vec![ordinal(p)?];
        while let Some(n) = p.attempt(|p| {
            p.one_of(&[", or", ",", "or"])?;
            ordinal(p)
        }) {
            v.push(n);
        }
        p.words("turns of the game")?;
        p.end_sentence()?;
        let rule = Static::PlayerRestrict {
            who: Player::You,
            rules: vec![PlayerRule::CantCast(what)],
        };
        Some(vec![Static::If {
            cond: Condition::YourNthTurn(v),
            then: vec![rule],
        }])
    }) {
        return Some(s);
    }
    // "You may exert this creature as it attacks. When you do, …" (CR
    // 701.43d), and Combat Celebrant's "If this creature hasn't been exerted
    // this turn, you may exert it as it attacks."
    if let Some(s) = p.attempt(|p| {
        let cond = p.attempt(|p| {
            p.word("if")?;
            let what = objects(p)?;
            p.words("hasn't been exerted this turn ,")?;
            let e = Event::Exerts {
                who: Player::You,
                what,
            };
            Some(Condition::Not(Box::new(Condition::DidThisTurn(Box::new(
                e,
            )))))
        });
        p.words("you may exert")?;
        let what = objects(p)?;
        p.word("as")?;
        p.one_of(&["it", "he", "she"])?;
        p.word("attacks")?;
        let then = p
            .attempt(|p| {
                p.end_sentence()?;
                if !p.at("when") {
                    return None;
                }
                effects(p)
            })
            .or_else(|| p.end_sentence().map(|_| Vec::new()))?;
        let s = Static::ExertAsAttacks { what, then };
        Some(vec![match cond {
            Some(cond) => Static::If {
                cond,
                then: vec![s],
            },
            None => s,
        }])
    }) {
        return Some(s);
    }
    // "Once each turn, you may cast …", "Once during each of your turns, you
    // may cast a creature spell from your graveyard."
    if let Some(s) = p.attempt(|p| {
        let yours = p.one_of(&["once during each of your turns ,", "once each turn ,"])?
            != "once each turn ,";
        let then = static_sentence(p)?;
        Some(vec![Static::Once { yours, then }])
    }) {
        return Some(s);
    }
    // "As this creature transforms into ~, you get an emblem with …".
    if let Some(s) = p.attempt(|p| {
        p.word("as")?;
        let what = objects(p)?;
        p.verb("transform")?;
        p.word("into")?;
        let into = objects(p)?;
        p.tok(&Tok::Comma)?;
        effects(p).map(|then| vec![Static::AsTransforms { what, into, then }])
    }) {
        return Some(s);
    }
    // "While an opponent is searching their library, they exile each card
    // they find. You may play those cards …": the sentences after it are
    // about what happens while it holds.
    if let Some(s) = p.attempt(|p| {
        p.word("while")?;
        let cond = condition(p)?;
        p.tok(&Tok::Comma)?;
        effects(p).map(|then| vec![Static::While { cond, then }])
    }) {
        return Some(s);
    }
    let s = alt!(p;
        // "This effect doesn't remove this Aura." (CR 702.16n)
        |p| {
            p.words("this effect doesn't remove")?;
            objects(p).map(|what| vec![Static::ProtectionDoesntRemove(what)])
        },
        |p| {
            let what = objects(p)?;
            let mut keywords = Vec::new();
            loop {
                let k = alt!(p;
                    |p| p.verb("saddle").map(|_| "saddle"),
                    |p| p.verb("crew").map(|_| "crew"),
                    |p| p.verb("station").map(|_| "station"),
                )?;
                keywords.push((k.to_string(), objects(p)?));
                if p.attempt(|p| p.word("and")).is_none() {
                    break;
                }
            }
            p.words("as though its power were")?;
            let by = amount(p)?;
            p.word("greater")?;
            Some(vec![Static::PowerGreaterFor { what, keywords, by }])
        },
        |p| {
            p.words("as long as")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            // "As long as you have one or fewer cards in hand, if you would
            // draw one or more cards, …": a replacement while it holds.
            let then = alt!(p; replacement, static_joined)?;
            Some(vec![Static::AsLongAs { cond, then }])
        },
        // "During your turn, as long as ~ has one or more loyalty counters on
        // him, he's a 3/4 Ninja creature and has hexproof."
        |p| {
            p.words("during your turn , as long as")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            let then = static_joined(p)?;
            Some(vec![Static::AsLongAs { cond: Condition::ItsYourTurn, then: vec![Static::AsLongAs { cond, then }] }])
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
            p.words("during your turn")?;
            Some(vec![Static::AsLongAs { cond: Condition::ItsYourTurn, then }])
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
    p.end_sentence()?;
    Some(s)
}

/// "can't attack or block, and its activated abilities can't be activated".
pub(super) fn static_joined(p: &mut P) -> Option<Vec<Static>> {
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
pub(super) fn would(p: &mut P, what: &Objects) -> Option<Event> {
    alt!(p;
        |p| p.verb("die").map(|_| Event::Dies(what.clone())),
        |p| { p.words("be put into")?; let to = zone(p)?; p.word("from")?; let from = zone(p)?; Some(Event::PutInto { what: what.clone(), to, from }) },
        // "would be put into a graveyard": from anywhere, as the event says
        // in a trigger.
        |p| { p.words("be put into")?; let to = zone(p)?; Some(Event::PutInto { what: what.clone(), to, from: Zone::Anywhere }) },
        |p| p.words("leave the battlefield").map(|_| Event::Leaves { what: what.clone(), from: Zone::Battlefield }),
        |p| { p.words("deal damage to")?; let to = recipients(p)?; Some(Event::DealsDamageTo { what: what.clone(), to }) },
        |p| { p.words("deal noncombat damage to")?; let to = recipients(p)?; Some(Event::DealsNoncombatDamageTo { what: what.clone(), to }) },
        |p| p.words("deal damage").map(|_| Event::DealsDamage(what.clone())),
        |p| p.words("be dealt damage").map(|_| Event::IsDealtDamage(what.clone())),
        |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); Some(Event::Enters { what: what.clone(), under: None, state: None }) },
    )
}

/// "one or more +1/+1 counters", "one or more counters".
pub(super) fn counters_phrase(p: &mut P) -> Option<(Quant, Option<CounterKind>)> {
    let n = quant(p)?;
    let kind = p.attempt(counter_kind);
    p.one_of(&["counters", "counter"])?;
    Some((n, kind))
}

pub(super) fn replacement(p: &mut P) -> Option<Vec<Static>> {
    p.word("if")?;
    let event = alt!(p;
        |p| {
            let by = alt!(p; actor, |p| p.word("you").map(|_| Actor::Player(Player::You)))?;
            p.words("would create")?;
            let what = objects(p)?;
            let under = p.attempt(|p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(w) });
            Some(Event::Creates { by, what, under })
        },
        |p| {
            let what = objects(p)?;
            p.words("would be created")?;
            let under = p.attempt(|p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(w) });
            Some(Event::Creates { by: Actor::Passive, what, under })
        },
        |p| {
            let by = p.attempt(|p| alt!(p; actor, |p| p.word("you").map(|_| Actor::Player(Player::You))));
            let (n, kind, on) = alt!(p;
                |p| { p.words("would put")?; let (n, kind) = counters_phrase(p)?; p.word("on")?; Some((n, kind, objects(p)?)) },
                |p| { let (n, kind) = counters_phrase(p)?; p.words("would be put on")?; Some((n, kind, objects(p)?)) },
            )?;
            Some(Event::CountersPut { by, n, kind, on })
        },
        // "If a land is tapped for two or more mana": no "would", since the
        // replacement is on the mana it produces.
        |p| {
            let what = objects(p)?;
            p.words("is tapped for")?;
            let n = number(p)?;
            p.words("or more mana")?;
            Some(Event::TappedForAtLeast { what, n })
        },
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("would")?;
            alt!(p;
                |p| p.words("draw one or more cards").map(|_| Event::DrawsOneOrMore(who.clone())),
                |p| {
                    p.words("draw a card except the first one")?;
                    p.one_of(&["you draw in each of your draw steps", "they draw in each of their draw steps"])?;
                    Some(Event::DrawsExceptFirstInDrawStep(who.clone()))
                },
                |p| p.words("draw a card").map(|_| Event::Draws(who.clone())),
                |p| p.words("gain life").map(|_| Event::GainsLife(who.clone())),
                |p| p.words("lose life").map(|_| Event::LosesLife(who.clone())),
                |p| p.words("begin an extra turn").map(|_| Event::BeginsExtraTurn(who.clone())),
            )
        },
        |p| { let what = objects(p)?; p.word("would")?; would(p, &what) },
    )?;
    // "If you would draw a card while your library has no cards in it".
    let event = match p.attempt(|p| {
        alt!(p;
            |p| { p.word("while")?; condition(p) },
            // "If an opponent would lose life during your turn".
            |p| p.words("during your turn").map(|_| Condition::ItsYourTurn),
        )
    }) {
        Some(cond) => Event::While {
            event: Box::new(event),
            cond: Box::new(cond),
        },
        None => event,
    };
    // "If a nontoken creature would enter and it wasn't cast".
    let event = match p.attempt(|p| {
        p.word("and")?;
        condition(p)
    }) {
        Some(cond) => Event::AndIf {
            event: Box::new(event),
            cond: Box::new(cond),
        },
        None => event,
    };
    p.tok(&Tok::Comma)?;
    // "instead create one of each" (Academy Manufactor): that many of each
    // predefined token the event names ([one-of-each]).
    if let Some(instead) = p.attempt(|p| {
        p.words("instead create one of each")?;
        one_of_each(&event, &Player::You)
    }) {
        return Some(vec![Static::Replacement {
            event: Box::new(event),
            instead,
        }]);
    }
    // "instead" may lead or trail; either way the clauses replace the event.
    p.attempt(|p| p.word("instead"));
    let mut instead = clauses(p)?;
    p.attempt(|p| p.word("instead"));
    // "… you may discard a land card instead. If you do, …. If you don't, …"
    while let Some(more) = p.attempt(|p| {
        p.tok(&Tok::Period)?;
        if !matches!(p.peek_word(), Some("if")) {
            return None;
        }
        let c = clause(p)?;
        matches!(c.first(), Some(Effect::IfYouDo(_) | Effect::IfYouDont(_))).then_some(c)
    }) {
        instead.extend(more);
    }
    Some(vec![Static::Replacement {
        event: Box::new(event),
        instead,
    }])
}

pub(super) fn static_body(p: &mut P) -> Option<Vec<Static>> {
    if let Some(s) = p.attempt(static_body_first) {
        return Some(s);
    }
    if let Some(s) = p.attempt(static_body_base) {
        return Some(s);
    }
    p.attempt(static_body_more)
}

fn static_body_base(p: &mut P) -> Option<Vec<Static>> {
    alt!(p;
        |p| p.words("you may cast this spell without paying its mana cost").map(|_| vec![Static::CastWithoutPaying]),
        |p| {
            p.words("if a triggered ability of")?;
            let of = objects(p)?;
            p.one_of(&["triggers , it triggers an additional time", "triggers , that ability triggers an additional time"])?;
            Some(vec![Static::AdditionalTrigger { of, cause: None }])
        },
        |p| {
            p.word("if")?;
            let what = objects(p)?;
            // "a creature entering", "a creature attacking"; "entering the
            // battlefield" is the same event (CR glossary, "enters").
            let cause = alt!(p;
                |p| { p.word("entering")?; p.attempt(|p| p.words("the battlefield")); Some(Event::Enters { what: what.clone(), under: None, state: None }) },
                |p| p.word("attacking").map(|_| Event::Attacks(what.clone())),
                // The object read the trailing "attacking" as its state.
                |p| {
                    if p.peek_word() != Some("causes") { return None; }
                    let mut x = what.clone();
                    let [g] = &mut x.filter.alts[..] else { return None };
                    (g.pop()? == Adj::State(State::Attacking)).then_some(Event::Attacks(x))
                },
            )?;
            p.words("causes a triggered ability of")?;
            let of = objects(p)?;
            p.words("to trigger , that ability triggers an additional time")?;
            Some(vec![Static::AdditionalTrigger { of, cause: Some(cause) }])
        },
        |p| {
            // "As long as enchanted permanent is a Vehicle, it's a creature in
            // addition to its other types": "it's" is the subject and verb.
            let what = alt!(p;
                |p| p.word("it's").map(|_| antecedent()),
                |p| { let w = objects(p)?; p.one_of(&["is", "are"])?; Some(w) },
            )?;
            let types = alt!(p;
                |p| p.one_of(&["the chosen type", "the chosen creature type"]).map(|_| TypeSet::Chosen),
                |p| p.words("every basic land type").map(|_| TypeSet::EveryBasicLandType),
                |p| p.words("every nonbasic land type").map(|_| TypeSet::EveryNonbasicLandType),
                |p| { p.attempt(|p| p.one_of(&["a", "an"])); group(p).map(TypeSet::Listed) },
            )?;
            p.one_of(&["in addition to its other types", "in addition to its other land types", "in addition to their other types", "in addition to their other land types", "in addition to its other creature types", "in addition to their other creature types"])?;
            // "… in addition to its other types and has "…"": a second
            // predicate of the same subject, which the predicate list reads.
            if p.peek_word() == Some("and") && matches!(p.t.get(p.i + 1), Some(Tok::Word(w)) if w == "has" || w == "have") {
                return None;
            }
            Some(vec![Static::AlsoIs { what, types }])
        },
        // "Creatures you control are every creature type": with no "in
        // addition", the new types replace the old (CR 205.1b).
        |p| {
            let what = objects(p)?;
            p.one_of(&["is", "are"])?;
            let types = alt!(p;
                |p| p.words("every creature type").map(|_| TypeSet::EveryCreatureType),
                |p| p.words("every basic land type").map(|_| TypeSet::EveryBasicLandType),
                |p| p.words("every nonbasic land type").map(|_| TypeSet::EveryNonbasicLandType),
                |p| {
                    p.attempt(|p| p.one_of(&["a", "an"]));
                    // Only what a card's type line and color say: "is
                    // attacking" is a condition, not a static.
                    let g = group(p).filter(|g| g.iter().all(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_) | Adj::Supertype(_) | Adj::Color(_))))?;
                    // "Nonbasic lands are Mountains": subtypes replace only
                    // their own set (CR 205.1a, 305.7).
                    Some(set_of(g))
                },
            )?;
            // "is a colorless land with "…"": the predicate list reads what
            // the new object has.
            // "is a Wizard in addition to its other types and has …": the
            // types are added, which the rule before or the predicate list reads.
            if matches!(p.peek_word(), Some("with" | "in")) { return None; }
            Some(vec![Static::Is { what, types }])
        },
        |p| {
            p.words("you may play")?;
            let n = amount_a(p)?;
            p.word("additional")?;
            p.one_of(&["lands", "land"])?;
            p.words("on each of your turns")?;
            Some(vec![Static::ExtraLand(n)])
        },
        |p| {
            let who = player(p)?;
            let mut rules = vec![player_rule(p)?];
            while let Some(r) = p.attempt(|p| { p.word("or")?; player_rule_bare(p) }) {
                rules.push(r);
            }
            Some(vec![Static::PlayerRestrict { who, rules }])
        },
        |p| {
            p.word("untap")?;
            let what = objects(p)?;
            p.word("during")?;
            let step = step(p)?;
            Some(vec![Static::UntapDuring { what, step }])
        },
        |p| p.words("you have no maximum hand size").map(|_| vec![Static::MaxHandSize]),
        |p| { p.words("you have")?; keyword_list(p).map(|k| vec![Static::YouHave(k)]) },
        |p| p.words("~ can be your commander").map(|_| vec![Static::CanBeCommander]),
        |p| {
            p.words("a deck can have")?;
            let n = alt!(p;
                |p| p.words("any number of").map(|_| None),
                |p| { p.words("up to")?; number(p).map(Some) },
            )?;
            p.words("cards named ~")?;
            Some(vec![Static::DeckCanHave(n)])
        },
        |p| p.words("you may look at the top card of your library any time").map(|_| vec![Static::LookAtTopAnyTime]),
        |p| {
            p.words("as an additional cost to cast this spell ,")?;
            let mut c = costs(p)?;
            let mut alts = Vec::new();
            while let Some(more) = p.attempt(|p| { p.word("or")?; costs(p) }) {
                alts.push(more);
            }
            if !alts.is_empty() {
                // One alternative is several costs only if written as one: keep
                // each alternative a single cost, or the "or" is ambiguous.
                if c.len() != 1 || alts.iter().any(|a| a.len() != 1) { return None; }
                c.extend(alts.into_iter().flatten());
                c = vec![Cost::OneOf(c)];
            }
            Some(vec![Static::AdditionalCost(c)])
        },
        |p| {
            p.words("you may")?;
            let c = costs(p)?;
            p.words("rather than pay this spell 's mana cost")?;
            // "… if there are thirteen or more creatures on the battlefield".
            Some(vec![match p.attempt(|p| { p.word("if")?; condition(p) }) {
                Some(cond) => Static::If { cond, then: vec![Static::AlternativeCost(c)] },
                None => Static::AlternativeCost(c),
            }])
        },
        |p| {
            let who = p.attempt(|p| alt!(p;
                |p| p.word("players").map(|_| Player::Each),
                |p| p.word("you").map(|_| Player::You),
            ));
            p.words("play with the top card of")?;
            let whose = whose(p)?;
            p.one_of(&["library", "libraries"])?;
            p.word("revealed")?;
            // The players who play so are the libraries' owners.
            if who.as_ref().is_some_and(|w| *w == Player::Each) != (whose == Player::That) { return None; }
            Some(vec![Static::PlayWithTopRevealed(who.unwrap_or(whose))])
        },
        |p| { p.word("skip")?; step(p).map(|s| vec![Static::SkipStep(s)]) },
        // "You control your opponents while they're searching their libraries."
        |p| {
            p.words("you control")?;
            let who = player(p)?;
            p.word("while")?;
            let cond = condition(p)?;
            // "while they're searching their libraries": each opponent while that
            // one searches, not every opponent while some "that" player does.
            if who == Player::EachOpponent && cond == (Condition::Searching { who: Player::That, zone: Zone::Library(Player::That) }) {
                let cond = Condition::Searching { who: Player::AnOpponent, zone: Zone::Library(Player::That) };
                return Some(vec![Static::AsLongAs { cond, then: vec![Static::ControlPlayer(Player::That)] }]);
            }
            Some(vec![Static::AsLongAs { cond, then: vec![Static::ControlPlayer(who)] }])
        },
        |p| { p.words("you control")?; objects(p).map(|o| vec![Static::Control(o)]) },
        |p| {
            p.words("you may cast")?;
            let mut what = objects(p)?;
            // "colorless spells and artifact spells": two kinds, one list.
            if let Some(more) = p.attempt(|p| { p.word("and")?; objects(p) }) {
                if what.quant != Quant::Plural || more.quant != Quant::Plural || what.filter.or.is_some() { return None; }
                what.filter.or = Some(Box::new(more.filter));
            }
            p.one_of(&["as though they had flash", "as though it had flash"])?;
            // "if you pay {2} more to cast it", "if you behold a Dragon as an
            // additional cost to cast it": a cost for the permission.
            if let Some(cost) = p.attempt(|p| {
                p.words("if you")?;
                alt!(p;
                    |p| { p.word("pay")?; let m = mana(p)?; p.words("more to cast it")?; Some(vec![Cost::Mana(m)]) },
                    |p| { let c = costs(p)?; p.words("as an additional cost to cast it")?; Some(c) },
                )
            }) {
                return Some(vec![Static::CastAsThoughFlashFor { what, cost }]);
            }
            let cast = Static::CastAsThoughFlash(what);
            // "You may cast this spell as though it had flash if you control
            // a Human": the permission only while the condition holds.
            Some(vec![match p.attempt(|p| { p.word("if")?; condition(p) }) {
                Some(cond) => Static::If { cond, then: vec![cast] },
                None => cast,
            }])
        },
        // "Cycling abilities you activate cost {2} less to activate".
        |p| {
            let kind = match descriptor(p)? { Adj::Ability(k) => k, _ => return None };
            p.word("abilities")?;
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.verb("activate")?;
            p.verb("cost")?;
            let by = mana(p)?;
            let less = p.one_of(&["less", "more"])? == "less";
            p.words("to activate")?;
            Some(vec![Static::ActivatedCostChange { kind, who, by, less }])
        },
        // "Activated abilities of artifacts you control cost {1} less to
        // activate".
        |p| {
            let kind = match descriptor(p)? { Adj::Ability(k) => k, _ => return None };
            p.words("abilities of")?;
            let of = objects(p)?;
            // "… of Equipment you control that target this creature": the
            // abilities target, not the Equipment, which the tree has no
            // place for.
            if of.filter.posts.iter().any(|q| matches!(q, Post::Targets(_))) { return None; }
            p.verb("cost")?;
            let by = mana(p)?;
            let less = p.one_of(&["less", "more"])? == "less";
            p.words("to activate")?;
            Some(vec![Static::AbilityCostChange { kind, of, by, less }])
        },
        // "Activated abilities of artifacts your opponents control can't be
        // activated", perhaps "unless they're mana abilities".
        |p| {
            p.words("activated abilities of")?;
            let what = objects(p)?;
            p.words("can't be activated")?;
            let rule = match p.attempt(|p| p.words("unless they're mana abilities")) {
                Some(_) => Restriction::NoNonManaActivatedAbilities,
                None => Restriction::NoActivatedAbilities,
            };
            Some(vec![Static::Restrict { what, rule }])
        },
        |p| {
            p.words("you may activate abilities of")?;
            let what = objects(p)?;
            p.words("as though")?;
            // "those creatures": the objects just named.
            if pronoun(p)?.quant != Quant::Antecedent { return None; }
            p.words("had haste")?;
            Some(vec![Static::ActivateAsThoughHaste(what)])
        },
        // "You may have this creature assign its combat damage as though it
        // weren't blocked", "For each non-Human creature you control, you
        // may have that creature assign …", "Enchanted creature's controller
        // may have it assign …".
        |p| {
            let what = alt!(p;
                |p| { p.words("you may have")?; let what = objects(p)?; p.word("assign")?; Some(what) },
                |p| {
                    p.words("for each")?;
                    let what = objects(p)?;
                    p.words(", you may have that creature assign")?;
                    Some(what)
                },
                |p| {
                    let what = objects(p)?;
                    p.words("'s controller may have it assign")?;
                    Some(what)
                },
            )?;
            p.words("its combat damage as though it weren't blocked")?;
            Some(vec![Static::AssignAsThoughUnblocked(what)])
        },
        // "Humans you control have each of the chosen abilities" (Greymond).
        |p| {
            let what = objects(p)?;
            p.one_of(&["have", "has"])?;
            p.words("each of the chosen abilities")?;
            Some(vec![Static::HasChosenAbilities(what)])
        },
        // "you may pay {0} rather than pay cycling costs", "… the equip cost of
        // the first equip ability you activate each turn" (Bruenor).
        |p| {
            p.words("you may pay")?;
            let cost = costs(p)?;
            p.words("rather than pay")?;
            let (keyword, first) = alt!(p;
                |p| { let k = p.peek_word().filter(|w| *w == "cycling" || *w == "equip")?.to_string(); p.bump(); p.word("costs")?; Some((k, None)) },
                |p| {
                    p.word("the")?;
                    let k = p.peek_word().filter(|w| *w == "cycling" || *w == "equip")?.to_string();
                    p.bump();
                    p.words("cost of the first")?;
                    if k == "cycling" { p.words("card you cycle")?; } else { p.words("equip ability you activate")?; }
                    let each = alt!(p;
                        |p| p.words("each turn").map(|_| EachTurn::Turn),
                        |p| p.words("during each of your turns").map(|_| EachTurn::YourTurn),
                    )?;
                    Some((k, Some(each)))
                },
            )?;
            Some(vec![Static::KeywordCostPaying { keyword, cost, first }])
        },
        // "For each {B} in a cost, you may pay 2 life rather than pay that
        // mana" (K'rrik).
        |p| {
            p.words("for each")?;
            let m = mana(p)?;
            (m.0.len() == 1).then_some(())?;
            p.words("in a cost")?;
            p.tok(&Tok::Comma)?;
            p.words("you may pay")?;
            let life = amount(p)?;
            p.words("life rather than pay that mana")?;
            Some(vec![Static::LifeForMana { mana: m, life }])
        },
        // "If you casting or copying an instant or sorcery spell causes a
        // triggered ability of a permanent you control to trigger, …".
        |p| {
            p.word("if")?;
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            type Verb = fn(Player, Objects) -> Event;
            let gerund = |p: &mut P| alt!(p;
                |p| p.word("casting").map(|_| casts as Verb),
                |p| p.word("copying").map(|_| (|who, what| Event::Copies { who, what }) as Verb),
            );
            let mut verbs = vec![gerund(p)?];
            while let Some(g) = p.attempt(|p| { p.word("or")?; gerund(p) }) {
                verbs.push(g);
            }
            let what = objects(p)?;
            p.words("causes a triggered ability of")?;
            let of = objects(p)?;
            p.words("to trigger , that ability triggers an additional time")?;
            let mut events: Vec<Event> = verbs.into_iter().map(|v| v(who.clone(), what.clone())).collect();
            let cause = if events.len() == 1 { events.remove(0) } else { Event::Or(events) };
            Some(vec![Static::AdditionalTrigger { of, cause: Some(cause) }])
        },
        |p| {
            let what = objects(p)?;
            p.one_of(&["isn't", "aren't"])?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let types = group(p)?;
            Some(vec![Static::IsNot { what, types }])
        },
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
        // "You may pay {0} rather than pay the mana cost for Zombie creature
        // spells you cast": the same alternative cost (CR 118.9), said
        // with the cost first.
        |p| {
            p.words("you may")?;
            let mut paid = vec![cost(p)?];
            while let Some(c) = p.attempt(|p| { p.word("and")?; cost(p) }) {
                paid.push(c);
            }
            p.words("rather than pay the mana cost for")?;
            let what = objects(p)?;
            Some(vec![Static::CastPaying { what, zone: None, cost: paid, rather: true }])
        },
        // "You may cast this card from your graveyard by paying {2}{W} rather
        // than paying its mana cost" (Raffine's Guidance).
        |p| {
            p.words("you may cast")?;
            let mut what = objects(p)?;
            let zone = match what.filter.posts.last() {
                Some(Post::In(z)) => { let z = z.clone(); what.filter.posts.pop(); Some(z) }
                _ => p.attempt(|p| { p.word("from")?; zone(p) }),
            };
            let (cost, rather) = paying(p)?;
            Some(vec![Static::CastPaying { what, zone, cost, rather }])
        },
        // "You may play lands and cast spells from the top of your library":
        // each verb and object, from the one zone named last.
        |p| {
            p.words("you may")?;
            let verb = |p: &mut P| alt!(p;
                |p| p.word("play").map(|_| CastVerb::Play),
                |p| p.word("cast").map(|_| CastVerb::Cast),
            );
            let first = verb(p)?;
            let mut items = vec![(first, objects(p)?)];
            while let Some(item) = p.attempt(|p| {
                p.word("and")?;
                let v = p.attempt(verb).unwrap_or(items.last().map(|(v, _)| *v)?);
                Some((v, objects(p)?))
            }) {
                items.push(item);
            }
            // The last object's own "from your graveyard" got there first.
            let last = &mut items.last_mut()?.1;
            let zone = match last.filter.posts.last() {
                Some(Post::In(z)) => { let z = z.clone(); last.filter.posts.pop(); z }
                _ => { p.word("from")?; zone(p)? }
            };
            Some(items.into_iter().map(|(verb, what)| Static::PlayFrom { verb, what, zone: zone.clone() }).collect())
        },
        |p| {
            let cost = costs(p)?;
            p.words("rather than pay its mana cost")?;
            Some(vec![Static::PaysRather { what: antecedent(), cost }])
        },
        |p| {
            let may = p.attempt(|p| p.words("you may have")).is_some();
            let what = objects(p)?;
            p.verb("enter")?;
            let tapped = p.attempt(|p| p.word("tapped")).is_some();
            p.words("as a copy of")?;
            let of = objects(p)?;
            let except = p.attempt(exceptions).unwrap_or_default();
            Some(vec![Static::EntersAsCopy { may, tapped, what, of, except }])
        },
        |p| {
            let what = objects(p)?;
            p.word("can")?;
            let keyword = keyword_name(p)?;
            let times = alt!(p;
                |p| p.word("twice").map(|_| 2),
                |p| { let n = number(p)?; p.word("times")?; Some(n) },
            )?;
            p.words("during each of")?;
            let during = alt!(p;
                |p| p.word("your").map(|_| Whose::Yours),
                |p| p.word("their").map(|_| Whose::ThatPlayers),
            )?;
            p.words("turns rather than once")?;
            Some(vec![Static::ActivationsPerTurn { what, keyword, times, during }])
        },
        |p| {
            p.words("the same is true for")?;
            let mut v = vec![objects(p)?];
            while let Some(o) = p.attempt(|p| { p.one_of(&[", and", "and", ","])?; objects(p) }) {
                v.push(o);
            }
            Some(vec![Static::SameFor(v)])
        },
        // "The escape cost is equal to the card's mana cost plus exile three
        // other cards from your graveyard."
        |p| {
            p.word("the")?;
            let keyword = keyword_name(p)?;
            p.words("cost is equal to")?;
            let of = objects(p)?;
            p.tok(&Tok::Possessive)?;
            p.words("mana cost")?;
            let mut cost = vec![Cost::ManaCostOf(of)];
            if p.attempt(|p| p.word("plus")).is_some() {
                cost.extend(costs(p)?);
            }
            Some(vec![Static::KeywordCost { keyword, cost }])
        },
        // "This creature and enchanted creature each get +X/+X, where X is
        // …": one static for each subject.
        |p| {
            let a = objects(p)?;
            p.word("and")?;
            let b = objects(p)?;
            p.word("each")?;
            let v = predicate_static(p, &a)?;
            let mut t = v.clone();
            for s in &mut t {
                match s {
                    Static::Where { inner, .. } => match &mut **inner {
                        Static::Modify { what, .. } => *what = b.clone(),
                        _ => return None,
                    },
                    Static::Modify { what, .. } => *what = b.clone(),
                    _ => return None,
                }
            }
            let mut v = v;
            v.extend(t);
            Some(v)
        },
        // "Equipped creature has ward {2}, is an Assassin in addition to its
        // other types, and can't be blocked": one subject, several predicates.
        |p| {
            let what = objects(p)?;
            let mut v = predicate_static(p, &what)?;
            while let Some(more) = p.attempt(|p| {
                p.one_of(&[", and", ",", "and"])?;
                // "…, and it loses all other abilities": the same subject again.
                p.attempt(|p| p.one_of(&["it", "they"]));
                predicate_static(p, &what)
            }) {
                v.extend(more);
            }
            Some(v)
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

/// Static templates no older rule reads any part of, tried only when those
/// fail.
fn static_body_more(p: &mut P) -> Option<Vec<Static>> {
    alt!(p;
        // "Permanents entering don't cause abilities of permanents your
        // opponents control to trigger", "Creatures entering don't cause
        // abilities to trigger".
        |p| {
            let what = objects(p)?;
            p.words("entering don't cause abilities")?;
            let of = p.attempt(|p| { p.word("of")?; objects(p) });
            p.words("to trigger")?;
            Some(vec![Static::EnteringDoesntTrigger { what, of }])
        },
        |p| {
            let who = player(p)?;
            p.words("can cast")?;
            let what = objects(p)?;
            p.words("only any time they could cast a sorcery")?;
            Some(vec![Static::CastOnlyAsSorcery { who, what }])
        },
        |p| {
            p.words("you may spend mana as though it were mana of any color")?;
            spend_uses(p).map(|u| vec![Static::SpendAsAnyColor(u)])
        },
        |p| {
            alt!(p;
                |p| p.words("you may spend mana as though it were mana of any type"),
                |p| p.words("mana of any type can be spent"),
            )?;
            spend_uses(p).map(|u| vec![Static::SpendAsAnyType(u)])
        },
        |p| {
            p.words("prevent all noncombat damage that would be dealt to")?;
            recipients(p).map(|r| vec![Static::PreventNoncombat(r)])
        },
        // On a permanent the prevention lasts as long as the static applies.
        |p| match prevent(p)? {
            Effect::Prevent { combat_only, n: None, to, by, dur: Duration::Permanent } => {
                Some(vec![Static::PreventDamage { combat_only, to, by }])
            }
            _ => None,
        },
        |p| {
            let what = objects(p)?;
            p.words("can't cause their controller to search their library")?;
            Some(vec![Static::CantCauseSearch(what)])
        },
        |p| {
            let who = whose(p)?;
            p.words("maximum hand size is")?;
            let n = number(p)?;
            Some(vec![Static::MaxHandSizeIs { who, n }])
        },
        |p| {
            p.words("as an additional cost to cast this spell , you may")?;
            costs(p).map(|c| vec![Static::OptionalAdditionalCost(c)])
        },
    )
}

/// Statics an older rule would read a shorter part of and commit to: "you
/// may cast this card from your graveyard" before "using its bestow
/// ability", "~'s power is equal to …" before "and its toughness …", a
/// copy before ". When you do, …".
fn static_body_first(p: &mut P) -> Option<Vec<Static>> {
    alt!(p;
        // "You may cast this card from your graveyard using its bestow ability."
        |p| {
            p.words("you may cast this card from")?;
            let zone = zone(p)?;
            p.words("using its")?;
            let keyword = keyword_name(p)?;
            p.word("ability")?;
            Some(vec![Static::CastFromUsing { zone, keyword }])
        },
        // "~'s power is equal to … and its toughness is equal to that number
        // plus 1."
        |p| {
            let what = objects(p)?;
            p.tok(&Tok::Possessive)?;
            p.words("power is equal to")?;
            let power = measure(p)?;
            p.words("and its toughness is equal to")?;
            let toughness = measure(p)?;
            Some(vec![
                Static::SetPower { what, equal_to: power },
                // "its toughness": the same object, by pronoun.
                Static::SetToughness { what: antecedent(), equal_to: toughness },
            ])
        },
        // "You may have ~ enter as a copy of …, except …. When you do, exile
        // that card.": the choice, and a reflexive trigger on it.
        |p| {
            // Only a choice can be followed by "When you do", so the
            // statics it could be are not read twice.
            if !p.at("you") { return None; }
            let mut s = static_body_base(p)?;
            if s.len() != 1 || !matches!(s[0], Static::EntersAsCopy { may: true, .. }) { return None; }
            p.words(". when you do ,")?;
            let then = clauses(p)?;
            Some(vec![Static::Reflexive { inner: Box::new(s.remove(0)), then }])
        },
    )
}

/// What a static says of its subject, read once for each predicate.
fn predicate_static(p: &mut P, what: &Objects) -> Option<Vec<Static>> {
    alt!(p;
        // "have all activated abilities of all creature cards exiled with ~".
        |p| {
            p.verb("have")?;
            p.word("all")?;
            let kind = match descriptor(p)? { Adj::Ability(k) => k, _ => return None };
            p.words("abilities of")?;
            let of = objects(p)?;
            Some(vec![Static::HasAbilitiesOf { what: what.clone(), kind, of }])
        },
        // "has vigilance if it's white, hexproof if it's blue, …, and
        // trample if it's green".
        |p| {
            p.verb("have")?;
            let one = |p: &mut P| {
                let k = keyword(p)?;
                p.word("if")?;
                Some((vec![k], condition_one(p)?))
            };
            let mut pairs = vec![one(p)?];
            while let Some(x) = p.attempt(|p| { p.one_of(&[", and", ","])?; one(p) }) { pairs.push(x); }
            (pairs.len() > 1).then(|| vec![Static::HasIf { what: what.clone(), pairs }])
        },
        |p| { p.verb("enter")?; p.word("prepared")?; Some(vec![Static::EntersPrepared(what.clone())]) },
        // "Equipped creature gets +1/+1 and is every creature type": a
        // predicate after another (CR 205.1b: no "in addition", so the
        // creature types are all of them).
        |p| {
            p.one_of(&["is", "are"])?;
            p.words("every creature type")?;
            Some(vec![Static::Is { what: what.clone(), types: TypeSet::EveryCreatureType }])
        },
        // "he's a 3/4 Ninja creature": a size and types, the size its base
        // power and toughness (CR 613.4b).
        |p| {
            // A pronoun's "'s", not a possessive: "it's", "he's". The word
            // before it says which; "this creature's" is a possessive.
            if !matches!(what.quant, Quant::Antecedent | Quant::SelfRef) || what.filter != Filter::default() { return None; }
            if matches!(p.t.get(p.i.wrapping_sub(1)), Some(Tok::Word(w)) if is_self_descriptor(w)) { return None; }
            p.tok(&Tok::Possessive)?;
            p.one_of(&["a", "an"])?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            let types = group(p)?;
            Some(vec![Static::Is { what: what.clone(), types: set_of(types) }, Static::BasePt { what: what.clone(), pt }])
        },
        // "gets +2/+0 for every seven cards in your graveyard".
        |p| {
            p.verb("get")?;
            let pt = pt_signed(p)?;
            p.words("for every")?;
            let n = number(p)?;
            let of = objects(p)?;
            Some(vec![Static::Modify { what: what.clone(), pt: Some(pt), has: vec![], per: Some(Amount::Every { n, of: Box::new(of) }) }])
        },
        |p| {
            p.verb("get")?;
            let pt = pt_signed(p)?;
            let per = p.attempt(|p| {
                p.words("for each")?;
                alt!(p;
                    |p| { p.words("color among")?; objects(p).map(|o| Amount::NumberOfColors(Box::new(Palette::ColorsOf(Box::new(o))))) },
                    |p| {
                        p.word("time")?;
                        let o = alt!(p;
                            |p| { let o = objects(p)?; p.word("has")?; Some(o) },
                            |p| p.word("it's").map(|_| antecedent()),
                        )?;
                        p.words("attacked this turn")?;
                        Some(Amount::TimesAttackedThisTurn(Box::new(o)))
                    },
                    |p| objects(p).map(|o| Amount::CountOf(Box::new(o))),
                )
            });
            let has_per = per.is_some();
            let has = p.attempt(|p| { p.word("and")?; p.verb("have")?; keyword_list(p) }).unwrap_or_default();
            let modify = Static::Modify { what: what.clone(), pt: Some(pt), has, per };
            // "This creature gets -X/-X, where X is your life total."
            if let Some(x) = p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                return Some(vec![Static::Where { inner: Box::new(modify), x }]);
            }
            let mut v = vec![modify];
            // "get +1/+1 for each creature you control and +1/+1 for each Aura
            // you control" (Eidolon of Countless Battles): a bonus for each.
            if has_per {
                while let Some((pt, of)) = p.attempt(|p| {
                    p.word("and")?;
                    let pt = pt_signed(p)?;
                    p.words("for each")?;
                    Some((pt, objects(p)?))
                }) {
                    v.push(Static::Modify { what: what.clone(), pt: Some(pt), has: vec![], per: Some(Amount::CountOf(Box::new(of))) });
                }
            }
            // "gets +2/+2 and is goaded".
            if p.attempt(|p| p.words("and is goaded")).is_some() {
                v.push(Static::Goaded(what.clone()));
            }
            // "gets +1/+1 and has trample and "Whenever …"", "gets +2/+2 and has "…"".
            if let Some(a) = p.attempt(|p| {
                p.word("and")?;
                p.attempt(|p| p.verb("have"));
                quoted(p)
            }) {
                v.push(Static::HasAbility { what: what.clone(), ability: Box::new(a) });
            }
            Some(v)
        },
        |p| {
            p.verb("have")?;
            let k = keyword_list(p)?;
            let modify = Static::Modify { what: what.clone(), pt: None, has: k, per: None };
            // "~ has annihilator X, where X is the number of +1/+1 counters on it."
            let mut v = vec![match p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                Some(x) => Static::Where { inner: Box::new(modify), x },
                None => modify,
            }];
            // "Enchanted creature has flying and "Sacrifice a creature: …""
            // (Fallen Ideal).
            if let Some(a) = p.attempt(|p| { p.one_of(&[", and", "and"])?; quoted(p) }) {
                v.push(Static::HasAbility { what: what.clone(), ability: Box::new(a) });
            }
            Some(v)
        },
        |p| { p.one_of(&["is", "are"])?; p.words("all colors")?; Some(vec![Static::IsAllColors(what.clone())]) },
        |p| { p.one_of(&["is", "are"])?; p.word("goaded")?; Some(vec![Static::Goaded(what.clone())]) },
        // "Enchanted creature has base power and toughness 9/9".
        |p| {
            p.verb("have")?;
            p.words("base power and toughness")?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            Some(vec![Static::BasePt { what: what.clone(), pt }])
        },
        // "has base power and base toughness each equal to its mana value"
        // (Opalescence).
        |p| {
            p.verb("have")?;
            p.one_of(&["base power and base toughness", "base power and toughness"])?;
            p.words("each equal to")?;
            let n = measure(p)?;
            Some(vec![Static::BasePtEqual { what: what.clone(), each_equal_to: n }])
        },
        // "This creature escapes with two +1/+1 counters on it".
        |p| {
            p.verb("escape")?;
            p.word("with")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.one_of(&["on it", "on him", "on her"])?;
            Some(vec![Static::EscapesWithCounters { what: what.clone(), n, kind }])
        },
        |p| {
            p.verb("enter")?;
            p.attempt(|p| p.words("the battlefield"));
            p.words("tapped unless")?;
            let unless = condition(p)?;
            Some(vec![Static::EntersTappedUnless { what: what.clone(), unless }])
        },
        |p| { p.verb("have")?; let a = quoted(p)?; Some(vec![Static::HasAbility { what: what.clone(), ability: Box::new(a) }]) },
        // "can't block or be blocked by non-Spirit creatures": two rules.
        |p| {
            p.words("can't block or be blocked by")?;
            let f = filter(p)?;
            Some(vec![
                Static::Restrict { what: what.clone(), rule: Restriction::CantBlockAny(f.clone()) },
                Static::Restrict { what: what.clone(), rule: Restriction::CantBeBlockedBy(f) },
            ])
        },
        |p| {
            let rule = restriction(p)?;
            let restrict = Static::Restrict { what: what.clone(), rule };
            // "… unless their controller pays {X} for each of those
            // creatures, where X is the number of enchantments you control".
            let restrict = match p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                Some(x) => Static::Where { inner: Box::new(restrict), x },
                None => restrict,
            };
            let mut v = vec![restrict];
            // "can't be blocked and has shroud".
            if let Some(has) = p.attempt(|p| { p.word("and")?; p.verb("have")?; keyword_list(p) }) {
                v.push(Static::Modify { what: what.clone(), pt: None, has, per: None });
            }
            Some(v)
        },
        |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); p.word("tapped")?; Some(vec![Static::EntersTapped(what.clone())]) },
        |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); p.word("untapped")?; Some(vec![Static::EntersUntapped(what.clone())]) },
        |p| { p.verb("lose")?; lost(p).map(|lost| vec![Static::Loses { what: what.clone(), lost }]) },
        |p| {
            p.verb("enter")?;
            p.attempt(|p| p.words("the battlefield"));
            p.word("with")?;
            // "with a number of +1/+1 counters on it equal to …".
            let counted = p.attempt(|p| p.words("a number of")).is_some();
            let n = if counted { Amount::X } else { amount_a(p)? };
            // "an additional +1/+1 counter": one more than it would have.
            let n = match p.attempt(|p| p.word("additional")) {
                Some(_) => Amount::Additional(Box::new(n)),
                None => n,
            };
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.one_of(&["on it", "on them", "on him", "on her"])?;
            // "with X +1/+1 counters on it, where X is …": the measure.
            if n == Amount::X {
                if let Some(m) = p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                    return Some(vec![Static::EntersWithCounters { what: what.clone(), n: m, kind }]);
                }
            }
            if counted {
                p.words("equal to")?;
                let m = measure(p)?;
                let n = match n { Amount::Additional(_) => Amount::Additional(Box::new(m)), _ => m };
                return Some(vec![Static::EntersWithCounters { what: what.clone(), n, kind }]);
            }
            // "a charge counter on it for each time it was kicked", "a
            // fellowship counter on it for each creature you control …".
            let each = p.attempt(|p| {
                p.words("for each")?;
                alt!(p;
                    |p| p.words("time it was kicked").map(|_| Amount::TimesKicked),
                    // Converge: "for each color of mana spent to cast it".
                    |p| { p.words("color of mana spent to cast")?; objects(p).map(|o| Amount::ColorsOfManaSpentToCast(Box::new(o))) },
                    |p| objects(p).map(|o| Amount::CountOf(Box::new(o))),
                )
            });
            let n = match each {
                Some(each) if n == Amount::N(1) => each,
                // "an additional loyalty counter on him for each …".
                Some(each) if n == Amount::Additional(Box::new(Amount::N(1))) => Amount::Additional(Box::new(each)),
                Some(_) => return None,
                None => n,
            };
            let enters = Static::EntersWithCounters { what: what.clone(), n, kind };
            // "enters with a +1/+1 counter on it if you attacked this turn":
            // the replacement applies only if the condition holds as it
            // enters.
            Some(vec![match p.attempt(|p| { p.word("if")?; condition(p) }) {
                Some(cond) => Static::If { cond, then: vec![enters] },
                None => enters,
            }])
        },
        // "costs {1} less to cast for each card type among cards in your
        // graveyard": a count of types, not of objects.
        |p| {
            p.verb("cost")?;
            let by = mana(p)?;
            let less = p.one_of(&["less", "more"])? == "less";
            p.words("to cast for each")?;
            let per = alt!(p;
                |p| { p.words("card type among")?; objects(p).map(|o| Amount::NumberOfCardTypes(Box::new(o))) },
                |p| { p.words("basic land type among")?; objects(p).map(|o| Amount::NumberOfBasicLandTypes(Box::new(o))) },
            )?;
            Some(vec![Static::CostChangePer { what: what.clone(), by, less, per }])
        },
        |p| {
            p.verb("cost")?;
            let by = mana(p)?;
            let less = p.one_of(&["less", "more"])? == "less";
            p.one_of(&["to cast", "to activate"])?;
            let per = p.attempt(|p| { p.words("for each")?; objects(p) });
            let change = Static::CostChange { what: what.clone(), by, less, per };
            let change = match p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                Some(x) => Static::Where { inner: Box::new(change), x },
                None => change,
            };
            // "This spell costs {3} less to cast if you control a creature with power 4 or greater."
            Some(vec![match p.attempt(|p| { p.word("if")?; condition(p) }) {
                Some(cond) => Static::If { cond, then: vec![change] },
                None => change,
            }])
        },
        |p| {
            p.verb("cost")?;
            p.words("an additional")?;
            let cost = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(vec![Cost::PayLife(n)]) },
                |p| mana(p).map(|m| vec![Cost::Mana(m)]),
            )?;
            p.words("to cast")?;
            Some(vec![Static::AdditionalCostFor { what: what.clone(), cost }])
        },
        |p| {
            p.one_of(&["is", "are"])?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let types = group(p)?;
            p.one_of(&["in addition to its other types", "in addition to their other types", "in addition to its other creature types", "in addition to their other creature types"])?;
            Some(vec![Static::AlsoIs { what: what.clone(), types: TypeSet::Listed(types) }])
        },
        // "is an Insect artifact creature with base power and toughness 0/1",
        // "is a colorless land with "{T}: Add {C}"": types that replace the
        // old, and what the new object has.
        |p| {
            p.one_of(&["is", "are"])?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let types = group(p).filter(|g| g.iter().all(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_) | Adj::Supertype(_) | Adj::Color(_))))?;
            let mut v = vec![Static::Is { what: what.clone(), types: set_of(types.clone()) }];
            if p.attempt(|p| p.word("with")).is_some() {
                v.push(alt!(p;
                    |p| {
                        p.words("base power and toughness")?;
                        let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
                        p.bump();
                        Some(Static::BasePt { what: what.clone(), pt })
                    },
                    // "is an artifact creature with power and toughness each
                    // equal to its mana value" (March of the Machines).
                    |p| {
                        p.words("power and toughness each equal to")?;
                        measure(p).map(|n| Static::BasePtEqual { what: what.clone(), each_equal_to: n })
                    },
                    |p| quoted(p).map(|a| Static::HasAbility { what: what.clone(), ability: Box::new(a) }),
                )?);
                // "… with base power and toughness 0/2 in addition to its
                // other types": the types are added, not replacing.
                if p.attempt(|p| p.one_of(&["in addition to its other types", "in addition to their other types", "in addition to its other creature types", "in addition to their other creature types"])).is_some() {
                    v[0] = Static::AlsoIs { what: what.clone(), types: TypeSet::Listed(types) };
                }
            }
            Some(v)
        },
    )
}

/// What "loses" takes: "flying", "all abilities", "all other card types and
/// abilities". "Other" is said once for the whole list.
pub(super) fn lost(p: &mut P) -> Option<Vec<Lost>> {
    if let Some(k) = p.attempt(keyword_list) {
        return Some(vec![Lost::Keywords(k)]);
    }
    p.word("all")?;
    let other = p.attempt(|p| p.word("other")).is_some();
    let one = |p: &mut P| {
        alt!(p;
            |p| p.one_of(&["abilities", "ability"]).map(|_| Characteristic::Abilities),
            |p| p.words("card types").map(|_| Characteristic::CardTypes),
            |p| p.words("creature types").map(|_| Characteristic::CreatureTypes),
        )
    };
    let mut v = vec![Lost::All { other, of: one(p)? }];
    while let Some(of) = p.attempt(|p| {
        p.one_of(&[", and", ",", "and"])?;
        one(p)
    }) {
        v.push(Lost::All { other, of });
    }
    Some(v)
}

pub(super) fn statics(p: &mut P) -> Option<Vec<Static>> {
    let mut v = Vec::new();
    while let Some(s) = p.attempt(static_sentence) {
        v.extend(s);
    }
    static_permission_rules(&mut v);
    (!v.is_empty()).then_some(v)
}

/// What a player can't do: "can't cast spells", "can't activate abilities of …".
pub(super) fn player_rule(p: &mut P) -> Option<PlayerRule> {
    p.word("can't")?;
    player_rule_bare(p)
}

pub(super) fn player_rule_bare(p: &mut P) -> Option<PlayerRule> {
    alt!(p;
        |p| {
            p.words("cast more than")?;
            let n = number(p)?;
            let what = filter(p)?;
            p.words("each turn")?;
            Some(PlayerRule::CantCastMoreThanEachTurn { n, what })
        },
        |p| p.words("gain life").map(|_| PlayerRule::CantGainLife),
        |p| p.words("get counters").map(|_| PlayerRule::CantGetCounters),
        // "can't attack you or planeswalkers you control".
        |p| {
            p.word("attack")?;
            super::objects::attacked_list(p).map(PlayerRule::CantAttack)
        },
        |p| p.words("search libraries").map(|_| PlayerRule::CantSearchLibraries),
        |p| { p.word("cast")?; objects(p).map(PlayerRule::CantCast) },
        |p| { p.words("activate abilities of")?; objects(p).map(PlayerRule::CantActivateAbilitiesOf) },
        |p| {
            p.words("draw more than")?;
            let n = number(p)?;
            p.one_of(&["cards", "card"])?;
            p.words("each turn")?;
            Some(PlayerRule::CantDrawMoreThanEachTurn(n))
        },
    )
}

/// "You may cast noncreature spells from the top of your library. If you
/// cast a spell this way, you may cast it as though it had flash": the
/// "if" is the rules for each spell cast through the permissions before
/// it (`Static::PermissionRules`), not a condition that holds for a while.
fn static_permission_rules(v: &mut Vec<Static>) {
    fn permits(s: &Static) -> bool {
        match s {
            Static::PlayFrom { .. } => true,
            // "You may cast artifact spells from your graveyard by paying 3
            // life …. If you cast a spell this way, …" (Noctis).
            Static::CastPaying { .. } => true,
            Static::If { then, .. } | Static::AsLongAs { then, .. } | Static::Once { then, .. } => {
                then.iter().any(permits)
            }
            _ => false,
        }
    }
    let rule = matches!(
        v.last(),
        Some(Static::If { cond: Condition::ThisWay(e), .. })
            if matches!(&**e, Event::Casts { who: Player::You, .. })
    );
    if !rule || !v[..v.len() - 1].iter().any(permits) {
        return;
    }
    let Some(Static::If {
        cond: Condition::ThisWay(e),
        then,
    }) = v.pop()
    else {
        unreachable!("matched above");
    };
    let Event::Casts { what, .. } = *e else {
        unreachable!("matched above");
    };
    let permissions = std::mem::take(v);
    v.push(Static::PermissionRules {
        permissions,
        spell: what,
        rules: then,
    });
}

/// What "is a …" sets. Subtypes with no card type replace only the
/// subtypes of their set, and a color said with them the color (CR 205.1a,
/// 305.7, 105.3): Blood Moon's lands are still lands, Lignify's Treefolk
/// and Eye of Nidhogg's black Dragon still creatures ([is-subtypes]).
fn set_of(g: Vec<Adj>) -> TypeSet {
    let subtype = g.iter().any(|a| matches!(a, Adj::Subtype(_)));
    if subtype && !g.iter().any(|a| matches!(a, Adj::Type(_))) {
        TypeSet::Subtypes(g)
    } else {
        TypeSet::Listed(g)
    }
}

/// "If this card is in your opening hand, you may begin the game with it on
/// the battlefield", Gemstone Caverns' "… and you're not the starting player
/// … with a luck counter on it. If you do, exile a card from your hand", and
/// "You may reveal this card from your opening hand. If you do, …": an
/// opening-hand action (CR 103.6, [opening-hand]).
pub(super) fn opening_hand(p: &mut P) -> Option<Ability> {
    if p.attempt(|p| p.words("you may reveal this card from your opening hand . if you do ,"))
        .is_some()
    {
        return Some(Ability::OpeningHandAction(OpeningHandAction::Reveal(
            effects(p)?,
        )));
    }
    p.word("if")?;
    pronoun(p)?;
    p.words("is in your opening hand")?;
    let not_starting_player = p
        .attempt(|p| p.words("and you're not the starting player"))
        .is_some();
    p.words(", you may begin the game with")?;
    // "it", "~": the card itself, which the tree need not name.
    pronoun(p)?;
    p.words("on the battlefield")?;
    let counters = p.attempt(|p| {
        p.word("with")?;
        let n = amount_a(p)?;
        let kind = counter_kind(p)?;
        p.one_of(&["counters", "counter"])?;
        p.one_of(&["on it", "on them"])?;
        Some((n, kind))
    });
    p.tok(&Tok::Period)?;
    let if_you_do = p
        .attempt(|p| {
            p.words("if you do ,")?;
            effects(p)
        })
        .unwrap_or_default();
    Some(Ability::OpeningHandAction(
        OpeningHandAction::BeginOnBattlefield {
            not_starting_player,
            counters,
            if_you_do,
        },
    ))
}
