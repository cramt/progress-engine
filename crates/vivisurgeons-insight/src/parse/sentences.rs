//! Effects joined into clauses and sentences, with what may lead or trail them.

use super::*;

/// What can trail an effect and qualify it: "unless that player pays {1}",
/// "for each creature you control", "where X is …", "if …", a delayed step.
pub(super) fn suffixed(p: &mut P, mut e: Effect) -> Effect {
    loop {
        if let Some((who, cost)) = p.attempt(|p| {
            p.word("unless")?;
            let who = player(p)?;
            let cost = unless_costs(p)?;
            // "unless you sacrifice three creatures, pay 3 life, and have …":
            // a list of what stops it that this rule reads only the first of.
            // Leaving the rest as instructions of their own would misread it.
            if p.attempt(|p| { p.tok(&Tok::Comma)?; p.one_of(&["pay", "sacrifice", "discard", "and"]) }).is_some() {
                return None;
            }
            Some((who, cost))
        }) {
            e = Effect::Unless {
                effect: Box::new(e),
                who,
                cost,
            };
        } else if let Some(cond) = p.attempt(|p| {
            // "unless it has the same name as another card exiled this way".
            p.word("unless")?;
            condition(p)
        }) {
            e = Effect::If {
                cond: Condition::Not(Box::new(cond)),
                then: vec![e],
            };
        } else if let Some(among) = p.attempt(|p| {
            p.words("for each color among")?;
            objects(p)
        }) {
            e = Effect::ForEachColor {
                among: Palette::ColorsOf(Box::new(among)),
                then: vec![e],
            };
        } else if let Some(each) = p.attempt(|p| {
            p.words("for each")?;
            objects(p)
        }) {
            e = Effect::ForEach {
                each,
                then: vec![e],
            };
        } else if let Some(x) = p.attempt(|p| {
            p.attempt(|p| p.tok(&Tok::Comma));
            p.words("where x is")?;
            measure(p)
        }) {
            e = Effect::Where { then: vec![e], x };
        } else if let Some(cond) = p.attempt(|p| {
            p.word("if")?;
            condition(p)
        }) {
            e = Effect::If {
                cond,
                then: vec![e],
            };
        } else if let Some(of) = p.attempt(|p| {
            // "exile it instead of putting it into its owner's graveyard".
            p.words("instead of putting")?;
            alt!(p;
                put_rest,
                // "exile it instead of putting it anywhere else": anywhere
                // but exile.
                |p| {
                    if !matches!(e, Effect::Exile(_)) { return None; }
                    let what = pronoun(p)?;
                    p.words("anywhere else")?;
                    Some(Effect::Move { what, from: None, to: Zone::AnywhereOtherThan(Box::new(Zone::Exile)), tapped: false, transformed: false, under: None, order: None })
                },
            )
        }) {
            e = Effect::InsteadOf {
                then: Box::new(e),
                of: Box::new(of),
            };
        } else if let Some(s) = p.attempt(|p| {
            p.words("at the beginning of")?;
            step(p)
        }) {
            e = Effect::Delayed {
                step: s,
                then: vec![e],
            };
        } else {
            return e;
        }
    }
}

/// What a player does to stop an "unless": "pays {1}", or a cost said as
/// its own verb, "sacrifices a nonland permanent or discards a card".
pub(super) fn unless_costs(p: &mut P) -> Option<Vec<Cost>> {
    if let Some(c) = p.attempt(|p| {
        p.verb("pay")?;
        costs(p)
    }) {
        return Some(c);
    }
    let one = |p: &mut P| {
        alt!(p;
            |p| { p.verb("sacrifice")?; objects(p).map(Cost::Sacrifice) },
            |p| { p.verb("discard")?; objects(p).map(Cost::Discard) },
        )
    };
    let mut v = vec![one(p)?];
    while let Some(c) = p.attempt(|p| {
        p.word("or")?;
        one(p)
    }) {
        v.push(c);
    }
    Some(vec![if v.len() == 1 {
        v.remove(0)
    } else {
        Cost::OneOf(v)
    }])
}

/// "Destroy target artifact, target creature, target enchantment, and
/// target land": one verb, a list of objects, so one effect per object.
fn object_list(p: &mut P) -> Option<Vec<Objects>> {
    let mut v = vec![objects(p)?];
    while let Some(o) = p.attempt(|p| {
        p.tok(&Tok::Comma)?;
        p.attempt(|p| p.word("and"));
        objects(p)
    }) {
        v.push(o);
    }
    (v.len() > 2).then_some(v)
}

pub(super) fn clause(p: &mut P) -> Option<Vec<Effect>> {
    alt!(p;
        |p| {
            p.word("destroy")?;
            let v = object_list(p)?;
            Some(v.into_iter().map(|what| Effect::Destroy { what, no_regen: false }).collect())
        },
        // "Whenever you attack this turn, …": a delayed trigger for the turn.
        |p| {
            p.word("whenever")?;
            let event = event(p)?;
            p.words("this turn ,")?;
            let then = delayed_body(p)?;
            Some(vec![Effect::During { dur: Duration::ThisTurn, then: vec![Effect::DelayedWhenever { event, then }] }])
        },
        // "Until end of turn, whenever a player taps an Island for mana, …".
        |p| {
            p.word("whenever")?;
            let event = event(p)?;
            p.tok(&Tok::Comma)?;
            let then = delayed_body(p)?;
            Some(vec![Effect::DelayedWhenever { event, then }])
        },
        // "For each 1 damage prevented this way, create …": once per point.
        |p| {
            p.words("for each")?;
            (number(p)? == 1).then_some(())?;
            p.words("damage prevented this way ,")?;
            Some(vec![Effect::Repeat { times: Amount::DamagePrevented, then: clauses(p)? }])
        },
        // "creatures you control have base power and toughness X/X and gain
        // all creature types": one subject, two effects. A base size is what
        // "becomes X/X" sets (CR 613.4b), so it is a `Becomes`.
        |p| {
            let what = objects(p)?;
            p.verb("have")?;
            p.words("base power and toughness")?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            let also = p.attempt(|p| { p.word("and")?; p.verb("gain")?; p.words("all creature types") }).is_some();
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let mut v = vec![Effect::Becomes { what: what.clone(), pt: Some(pt), types: vec![], gains: vec![], keeps: Keeps::implied_by(&[]), dur: dur.clone() }];
            if also {
                v.push(Effect::AlsoIs { what, types: TypeSet::EveryCreatureType, dur });
            }
            Some(v)
        },
        // "This land becomes a 2/2 creature with all creature types until end
        // of turn. It's still a land.": the new types, and all creature types
        // on top of them.
        |p| {
            if !p.ahead("with all creature types") { return None; }
            let what = objects(p)?;
            p.verb("become")?;
            p.one_of(&["a", "an"])?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            let types = group(p)?;
            p.words("with all creature types")?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let keeps = still(p).unwrap_or(Keeps::Nothing);
            Some(vec![
                Effect::Becomes { what: what.clone(), pt: Some(pt), types, gains: vec![], keeps, dur: dur.clone() },
                Effect::AlsoIs { what, types: TypeSet::EveryCreatureType, dur },
            ])
        },
        // "gets +2/+2 and gains trample and "Whenever …"", "gains haste,
        // "When …," and "At …"": one "gains" for keywords and abilities, the
        // keywords first.
        |p| {
            if !p.ahead("\"") { return None; }
            let what = objects(p)?;
            p.attempt(|p| p.word("each"));
            let pt = p.attempt(|p| {
                p.verb("get")?;
                let pt = pt_signed(p)?;
                p.word("and")?;
                Some(pt)
            });
            p.verb("gain")?;
            let (gains, abilities) = keywords_then_quoted(p)?;
            if pt.is_none() && gains.is_empty() && abilities.len() < 2 { return None; }
            let dur = match p.attempt(duration) {
                Some(d) => d,
                None if pt.is_some() && !p.during => return None,
                None => Duration::Permanent,
            };
            let mut v = Vec::new();
            if pt.is_some() || !gains.is_empty() {
                v.push(Effect::Modify { what: what.clone(), pt, gains, dur: dur.clone() });
            }
            v.extend(abilities.into_iter().map(|a| Effect::GrantAbility { what: what.clone(), ability: Box::new(a), dur: dur.clone() }));
            Some(v)
        },
        // "gains lifelink until end of turn and can't be blocked this turn",
        // "gets +3/+3 until end of turn and must be blocked this turn if
        // able": one subject, a bonus and a rule, each with its duration.
        |p| {
            if !p.ahead("and can't") && !p.ahead("and must") { return None; }
            let e = effect(p)?;
            let what = match &e {
                Effect::Modify { what, .. } if what.quant != Quant::Next => what.clone(),
                _ => return None,
            };
            p.word("and")?;
            // "must be blocked this turn if able": the duration inside.
            if p.attempt(|p| p.words("must be blocked this turn if able")).is_some() {
                return Some(vec![e, Effect::Restrict { what, rule: Restriction::MustBeBlocked, dur: Duration::ThisTurn }]);
            }
            let rule = restriction(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(vec![e, Effect::Restrict { what, rule, dur }])
        },
        // "~ deals 1 damage to target player or planeswalker and 1 damage to
        // target creature …": one source, its verb said once.
        |p| {
            // The second "and N damage to" says this is the pair; most damage
            // clauses have one recipient list.
            let has_second = p.t[p.i..].windows(3).any(|w| {
                matches!(w, [Tok::Word(a), Tok::Num(_), Tok::Word(d)] if a == "and" && d == "damage")
            }) || p.ahead("and x damage");
            if !has_second { return None; }
            let source = objects(p)?;
            p.verb("deal")?;
            let one = |p: &mut P| {
                let n = amount(p)?;
                p.words("damage to")?;
                Some((n, recipients(p)?))
            };
            let (n1, to1) = one(p)?;
            p.word("and")?;
            let (n2, to2) = one(p)?;
            Some(vec![
                Effect::Damage { source: Some(source.clone()), n: n1, to: to1 },
                Effect::Damage { source: Some(source), n: n2, to: to2 },
            ])
        },
        // "creatures your opponents control attack each combat if able and
        // attack a player other than you if able": one subject, two rules.
        |p| {
            let what = objects(p)?;
            let mut rules = vec![restriction(p)?];
            while let Some(r) = p.attempt(|p| { p.word("and")?; restriction(p) }) {
                rules.push(r);
            }
            if rules.len() < 2 { return None; }
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(rules.into_iter().map(|rule| Effect::Restrict { what: what.clone(), rule, dur: dur.clone() }).collect())
        },
        // "If any of that mana is spent on a creature spell, it gains haste":
        // what the mana does to the spell it pays for, as it is spent (CR
        // 106.6).
        |p| {
            p.word("if")?;
            let of = alt!(p;
                |p| p.words("any of that mana is spent on").map(|_| SpentOf::Any),
                |p| p.words("you spend this mana to cast").map(|_| SpentOf::All),
            )?;
            let on = objects(p)?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::OnSpend { of, on, then: clauses(p)? }])
        },
        |p| {
            p.one_of(&["if you do", "if they do", "if that player does", "if the player does"])?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::IfYouDo(clauses(p)?)])
        },
        // "When you do, if …, …": the "if" right after the trigger condition
        // is an intervening "if" (CR 603.4), for a reflexive trigger too.
        |p| {
            p.words("when you do , if")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::WhenYouDoIf { cond, then: clauses(p)? }])
        },
        |p| {
            p.words("when you do")?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::WhenYouDo(clauses(p)?)])
        },
        // "When you next cast an instant or sorcery spell this turn, copy
        // that spell": once, the next time, this turn (CR 603.7b).
        |p| {
            p.words("when you next")?;
            let v = player_verb(p, &Player::You)?;
            let what = objects(p)?;
            let dur = duration(p)?;
            p.tok(&Tok::Comma)?;
            let then = delayed_body(p)?;
            Some(vec![Effect::DelayedNext { event: v(what), dur, then }])
        },
        // "When a creature card is exiled this way, …": reflexive (CR 603.12).
        |p| {
            p.word("when")?;
            let event = event(p)?;
            p.words("this way ,")?;
            Some(vec![Effect::WhenThisWay { event, then: clauses(p)? }])
        },
        // "When that creature dies this turn, …": a delayed trigger that
        // lasts the turn.
        |p| {
            p.word("when")?;
            let event = event(p)?;
            p.words("this turn ,")?;
            let then = delayed_body(p)?;
            Some(vec![Effect::During { dur: Duration::ThisTurn, then: vec![Effect::DelayedWhen { event, then }] }])
        },
        |p| {
            p.word("when")?;
            let event = event(p)?;
            p.tok(&Tok::Comma)?;
            let then = delayed_body(p)?;
            Some(vec![match event {
                // "when each opponent casts their first spell of the game":
                // a delayed trigger triggers once (CR 603.7b), and this one
                // triggers for each opponent (Chancellor of the Annex's
                // ruling), so it is one per opponent.
                Event::Casts { who: Player::EachOpponent, what } => Effect::ForEach {
                    each: Objects { quant: Quant::Plural, filter: Filter { kind: Some(Noun::Opponent), ..Filter::default() } },
                    then: vec![Effect::DelayedWhen { event: Event::Casts { who: Player::That, what }, then }],
                },
                event => Effect::DelayedWhen { event, then },
            }])
        },
        |p| {
            p.words("for each card type ,")?;
            Some(vec![Effect::ForEachCardType(clauses(p)?)])
        },
        |p| {
            p.one_of(&["if you don't", "if they don't", "if that player doesn't", "if the player doesn't", "if no one does"])?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::IfYouDont(clauses(p)?)])
        },
        // "Otherwise" is the other branch of the "if" before it: "Put it
        // onto the battlefield if it's a land card. Otherwise, put it into
        // your hand." No option was offered, so it is no "if you don't".
        |p| {
            p.word("otherwise")?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::Otherwise(clauses(p)?)])
        },
        // "If you control six or more lands, create … instead" replaces the
        // effect before it; "instead" may lead or trail.
        |p| {
            p.word("if")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            let lead = p.attempt(|p| p.word("instead")).is_some();
            let then = clauses(p)?;
            let trail = p.attempt(|p| p.word("instead")).is_some();
            Some(vec![match (lead, trail) {
                (false, false) => Effect::If { cond, then },
                (true, true) => return None,
                _ => Effect::Instead { cond, then },
            }])
        },
        |p| { p.words("at the beginning of")?; let step = step(p)?; p.tok(&Tok::Comma)?; let then = delayed_body(p)?; Some(vec![Effect::Delayed { step, then }]) },
        |p| {
            let dur = duration(p)?;
            p.tok(&Tok::Comma)?;
            let outer = std::mem::replace(&mut p.during, true);
            let then = p.attempt(clauses);
            p.during = outer;
            Some(vec![Effect::During { dur, then: then? }])
        },
        // "For each opponent, create …", "For each of those cards, pay 4 life
        // or …": the clauses once per object.
        |p| {
            p.words("for each")?;
            let each = alt!(p;
                |p| { p.word("of")?; pronoun(p) },
                objects,
            )?;
            p.tok(&Tok::Comma)?;
            let then = clauses(p)?;
            Some(vec![Effect::ForEach { each, then }])
        },
        |p| {
            p.words("for each color among")?;
            let among = objects(p)?;
            p.tok(&Tok::Comma)?;
            let then = clauses(p)?;
            Some(vec![Effect::ForEachColor { among: Palette::ColorsOf(Box::new(among)), then }])
        },
        // "You and permanents you control gain hexproof until end of turn":
        // one verb said of a player and of objects, so two effects.
        |p| {
            p.words("you and")?;
            let what = objects(p)?;
            p.word("gain")?;
            let keywords = keyword_list(p)?;
            let dur = duration(p)?;
            Some(vec![
                Effect::PlayerGains { who: Player::You, keywords: keywords.clone(), dur: dur.clone() },
                Effect::Modify { what, pt: None, gains: keywords, dur },
            ])
        },
        |p| { p.word("then")?; clause(p) },
        // "it loses "…" and gains "…"": one subject, two verbs.
        |p| {
            let what = objects(p)?;
            p.verb("lose")?;
            let lost = quoted(p)?;
            p.word("and")?;
            p.verb("gain")?;
            let gained = quoted(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(vec![
                Effect::LoseAbility { what: what.clone(), ability: Box::new(lost), dur: dur.clone() },
                Effect::GrantAbility { what, ability: Box::new(gained), dur },
            ])
        },
        |p| {
            let who = subject_player(p);
            p.word("may")?;
            // "Its controller may draw up to two cards": the verb after "may"
            // has the same subject, not an unstated "you".
            let outer = p.implied.replace(who.clone());
            let e = p.attempt(|p| {
                let e = effect(p)?;
                let e = suffixed(p, e);
                // "You may reveal an artifact card … or choose a face-up
                // artifact card …": one option of two, each said in full.
                let e = match p.attempt(|p| { p.word("or")?; let e = effect(p)?; Some(suffixed(p, e)) }) {
                    Some(other) => Effect::Or(vec![e, other]),
                    None => e,
                };
                let mut then = vec![e];
                // "That player may search their library …, put it onto the
                // battlefield, then shuffle": the steps after the first are
                // the same optional action, by the same player.
                while let Some(e) = p.attempt(|p| {
                    p.one_of(&[", then", ", and then", ", and", "and then", "and", ","])?;
                    if !p.peek_word().is_some_and(|w| IMPERATIVE.contains(&w)) { return None; }
                    let e = effect(p)?;
                    Some(suffixed(p, e))
                }) {
                    then.push(e);
                }
                Some(then)
            });
            p.implied = outer;
            Some(vec![Effect::May { who, then: e? }])
        },
        |p| {
            let e = effect(p)?;
            // "Add {B}{B}{B}{B}{B} instead if there are seven or more cards in your graveyard."
            if let Some(cond) = p.attempt(|p| { p.words("instead if")?; condition(p) }) {
                return Some(vec![Effect::Instead { cond, then: vec![e] }]);
            }
            let e = suffixed(p, e);
            // "Destroy that creature if it has mana value 4 or less instead if
            // a permanent left the battlefield …": the conditioned effect,
            // replacing the one before it.
            if let Some(cond) = p.attempt(|p| { p.words("instead if")?; condition(p) }) {
                return Some(vec![Effect::Instead { cond, then: vec![e] }]);
            }
            // "pay 4 life or put the card on top of your library": a choice of
            // two verbs, each said in full.
            let mut v = vec![e];
            while let Some(e) = p.attempt(|p| {
                p.word("or")?;
                if !p.peek_word().is_some_and(|w| w == "pay" || IMPERATIVE.contains(&w)) { return None; }
                let e = effect(p)?;
                Some(suffixed(p, e))
            }) {
                v.push(e);
            }
            Some(vec![if v.len() == 1 { v.remove(0) } else { Effect::Or(v) }])
        },
    )
}

/// A delayed trigger's effect: its clauses, and the sentences after them.
/// "Whenever you attack this turn, create two tokens …. Sacrifice them at
/// the beginning of the next end step.": the tokens exist only once it has
/// triggered, so the sentences that follow are its (CR 603.7). Not a
/// sentence that is a trigger of its own (Phantasmal Mount's second "When
/// …"), and not one that names a new target: the spell's targets are chosen
/// as it is cast (CR 601.2c), so Time to Feed's "Target creature you control
/// fights that creature" happens as the spell resolves, not when the
/// creature dies.
fn delayed_body(p: &mut P) -> Option<Vec<Effect>> {
    let mut then = clauses(p)?;
    while let Some(more) = p.attempt(|p| {
        p.end_sentence()?;
        let own_trigger = p.at("when") || p.at("whenever") || p.at("at");
        if own_trigger || p.ahead("target") {
            return None;
        }
        clauses(p)
    }) {
        then.extend(more);
    }
    Some(then)
}

/// Verbs that start a clause with no subject of their own.
pub(super) const IMPERATIVE: &[&str] = &[
    "put",
    "return",
    "shuffle",
    "draw",
    "discard",
    "exile",
    "reveal",
    "sacrifice",
    "create",
    "attach",
    "tap",
    "untap",
    "mill",
    "cast",
    "play",
    "search",
    "scry",
    "surveil",
    "transform",
    "remove",
];

/// Verbs in the third person that, with no subject before them, continue
/// the last clause's subject: "Each player discards their hand, then draws …".
pub(super) const THIRD_PERSON: &[&str] = &[
    "draws",
    "discards",
    "gains",
    "loses",
    "mills",
    "searches",
    "shuffles",
    "reveals",
    "sacrifices",
    "creates",
    "puts",
    "pays",
    "adds",
    "exiles",
];

/// The player an effect's verb is said of, where it has one.
pub(super) fn subject_of(e: &Effect) -> Option<Player> {
    Some(match e {
        Effect::Draw { who, .. }
        | Effect::Discard { who, .. }
        | Effect::DiscardHand(who)
        | Effect::GainLife { who, .. }
        | Effect::LoseLife { who, .. }
        | Effect::Search { who, .. }
        | Effect::Shuffle(who)
        | Effect::ShuffleInto { who, .. }
        | Effect::ShuffleZones { who, .. }
        | Effect::Mill { who, .. }
        | Effect::Reveal { who, .. }
        | Effect::RevealTop { who, .. }
        | Effect::LookAtTop { who, .. }
        | Effect::Sacrifice { who, .. }
        | Effect::Create { who, .. }
        | Effect::Pay { who, .. }
        | Effect::May { who, .. }
        | Effect::RevealHand(who)
        | Effect::UntilFound { who, .. }
        | Effect::DigUntilTotal { who, .. }
        | Effect::Performed { by: who, .. } => who.clone(),
        _ => return None,
    })
}

/// The object an effect's verb is said of, where it has one.
fn object_subject_of(e: &Effect) -> Option<Objects> {
    match e {
        Effect::Modify { what, .. } | Effect::GrantAbility { what, .. } => Some(what.clone()),
        Effect::Damage { source, .. } => source.clone(),
        _ => None,
    }
}

/// A clause whose subject, if it has none, is `who`.
pub(super) fn clause_as(p: &mut P, who: Option<Player>) -> Option<Vec<Effect>> {
    let outer = std::mem::replace(&mut p.implied, who);
    let c = p.attempt(clause);
    p.implied = outer;
    c
}

pub(super) fn clauses(p: &mut P) -> Option<Vec<Effect>> {
    let mut v = clause(p)?;
    while let Some(more) = p.attempt(|p| {
        p.one_of(&[
            ", and then",
            ", then",
            ", and",
            "and then",
            "and",
            "then",
            ",",
        ])?;
        let continues = p.peek_word().is_some_and(|w| THIRD_PERSON.contains(&w));
        let who = if continues {
            v.last().and_then(subject_of)
        } else {
            p.implied.clone()
        };
        let deals = p.peek_word() == Some("deals");
        let mut c = clause_as(p, who)?;
        // "This creature gets +2/+0 until end of turn and deals 1 damage to
        // you": the damage is the earlier subject's.
        if let (
            true,
            Some(Effect::Damage {
                source: source @ None,
                ..
            }),
        ) = (deals, c.first_mut())
        {
            *source = v.last().and_then(object_subject_of);
        }
        Some(c)
    }) {
        v.extend(more);
    }
    // "it deals X damage to each opponent and you gain X life, where X is
    // …": the trailing definition binds X in every clause of the sentence.
    if v.len() > 1 && matches!(v.last(), Some(Effect::Where { .. })) {
        if let Some(Effect::Where { then, x }) = v.pop() {
            v.extend(then);
            return Some(vec![Effect::Where { then: v, x }]);
        }
    }
    Some(v)
}

pub(super) fn modal(p: &mut P) -> Option<Effect> {
    p.word("choose")?;
    let choose = alt!(p;
        |p| p.words("one or both").map(|_| Choose::OneOrBoth),
        |p| p.words("one or more").map(|_| Choose::OneOrMore),
        |p| p.words("any number").map(|_| Choose::AnyNumber),
        |p| { p.words("up to")?; number(p).map(Choose::UpTo) },
        |p| p.word("one").map(|_| Choose::One),
        |p| number(p).map(Choose::N),
    )?;
    let choose = match p.attempt(|p| {
        p.one_of(&[
            "that hasn't been chosen this turn",
            "that haven't been chosen this turn",
        ])
    }) {
        Some(_) => Choose::NotChosenThisTurn(Box::new(choose)),
        None => choose,
    };
    // "Choose one. If you control a commander as you cast this spell, you
    // may choose both instead." The modes follow the second sentence.
    let choose = match p.attempt(|p| {
        p.tok(&Tok::Period)?;
        p.word("if")?;
        let cond = condition(p)?;
        p.tok(&Tok::Comma)?;
        let may = p.attempt(|p| p.words("you may")).is_some();
        p.word("choose")?;
        let to = alt!(p;
            |p| p.word("both").map(|_| Choose::N(2)),
            |p| number(p).map(Choose::N),
        )?;
        p.words("instead .")?;
        Some((cond, to, may))
    }) {
        Some((cond, to, may)) => Choose::Upgrade {
            base: Box::new(choose),
            cond: Box::new(cond),
            to: Box::new(to),
            may,
        },
        None => {
            p.tok(&Tok::Dash)?;
            choose
        }
    };
    let mut modes = Vec::new();
    while p.attempt(|p| p.tok(&Tok::Bullet)).is_some() {
        // "• Sell Contraband — Create a Treasure token.": a flavour label.
        let label = p.attempt(|p| {
            let mut label = Vec::new();
            while let Some(w) = p.peek_word() {
                label.push(w.to_string());
                p.bump();
            }
            p.tok(&Tok::Dash)?;
            (!label.is_empty() && label.len() <= 6).then(|| label.join(" "))
        });
        let mut mode = Vec::new();
        while !matches!(p.peek(), None | Some(Tok::Bullet)) {
            mode.extend(sentence(p)?);
        }
        modes.push(match label {
            Some(label) => vec![Effect::Labelled { label, then: mode }],
            None => mode,
        });
    }
    (!modes.is_empty()).then_some(Effect::Modal { choose, modes })
}

pub(super) fn sentence(p: &mut P) -> Option<Vec<Effect>> {
    alt!(p;
        |p| modal(p).map(|m| vec![m]),
        // "Repeat the following process X times. …": the process is every
        // sentence after it.
        |p| {
            p.words("repeat the following process")?;
            let times = amount(p)?;
            p.words("times .")?;
            Some(vec![Effect::Repeat { times, then: effects(p)? }])
        },
        |p| {
            p.words("repeat this process until")?;
            let until = event(p)?;
            // "whichever comes first" is what "until A or B" means anyway.
            p.attempt(|p| p.words(", whichever comes first"));
            p.end_sentence()?;
            Some(vec![Effect::RepeatUntil(Box::new(until))])
        },
        |p| {
            let mut e = clauses(p)?;
            p.end_sentence()?;
            // "Create a 1/1 colorless Eldrazi Scion creature token. It has
            // "Sacrifice this token: Add {C}."": what the token has as it is
            // created, said as a sentence of its own.
            if let Some(Effect::Create { token, .. }) = e.last_mut() {
                if token.abilities.is_empty() && token.keywords.is_empty() {
                    if let Some(a) = p.attempt(|p| { p.words("it has")?; let a = quoted(p)?; p.end_sentence()?; Some(a) }) {
                        token.abilities.push(a);
                    }
                }
            }
            Some(e)
        },
    )
}

pub fn effects(p: &mut P) -> Option<Vec<Effect>> {
    let mut v: Vec<Effect> = Vec::new();
    loop {
        let s = match v.last().and_then(controllers_of_each) {
            Some(each) => p.attempt(|p| those_players(p, each)),
            None => None,
        };
        let Some(s) = s.or_else(|| p.attempt(sentence)) else {
            break;
        };
        // "When you do, put two +1/+1 counters … on target attacking
        // creature. It becomes an Angel …": "it" is what the reflexive
        // trigger targeted, so the sentence is that trigger's too.
        if let Some(
            Effect::WhenYouDo(then)
            | Effect::WhenYouDoIf { then, .. }
            | Effect::WhenThisWay { then, .. },
        ) = v.last_mut()
        {
            if s.first().is_some_and(continues_target) {
                then.extend(s);
                continue;
            }
        }
        // "Tyvar gains indestructible until end of turn. Tap it.": the only
        // object the sentence before names is the card itself, so "it" is.
        let mut s = s;
        if let (Some(prev), Some(first)) = (v.last(), s.first_mut()) {
            if let (Some(me), Some(it)) = (prev.sole_object(), first.sole_object_mut()) {
                if me.quant == Quant::SelfRef && *it == antecedent() {
                    *it = me.clone();
                }
            }
        }
        // "You may put a creature card from your hand onto the battlefield.
        // That creature gains haste.": the creature is there only if the
        // option was taken, so the sentence about it is the option's too.
        // "If it's a creature card, create …. If it's a land card, put that
        // card onto the battlefield.": cases of one object, side by side.
        let cases = matches!(
            (v.last(), s.first()),
            (Some(Effect::If { cond: a, .. }), Some(Effect::If { cond: b, .. }))
                if tested(a).is_some() && tested(a) == tested(b)
        );
        if let Some(branch) = v
            .last_mut()
            .and_then(branch_with_new_object)
            .filter(|_| !cases)
        {
            // "Otherwise, …" is the other branch of the "if" before it, so
            // it goes where that "if" went (Thunderous Debut).
            let otherwise = matches!(branch.last(), Some(Effect::If { .. }))
                && matches!(s.first(), Some(Effect::Otherwise(_)));
            if otherwise || s.first().is_some_and(refers_back) {
                branch.extend(s);
                continue;
            }
        }
        v.extend(s);
    }
    // "… If you do, draw a card and repeat this process.": the process is
    // every instruction of the ability, said from the first.
    if v.iter().any(repeats) {
        v = vec![Effect::Process(v)];
    }
    (!v.is_empty()).then_some(v)
}

/// Whether an effect says "repeat this process", in any branch.
fn repeats(e: &Effect) -> bool {
    match e {
        Effect::RepeatProcess | Effect::RepeatUntil(_) => true,
        Effect::If { then, .. }
        | Effect::Instead { then, .. }
        | Effect::May { then, .. }
        | Effect::IfYouDo(then)
        | Effect::IfYouDont(then)
        | Effect::Otherwise(then) => then.iter().any(repeats),
        _ => false,
    }
}

/// The objects of a "For each …, its controller …" loop, whose controllers
/// a later "those players" names.
fn controllers_of_each(e: &Effect) -> Option<Objects> {
    let Effect::ForEach { each, then } = e else {
        return None;
    };
    let its = Objects {
        quant: Quant::Antecedent,
        filter: Filter::default(),
    };
    (then.first().and_then(subject_of) == Some(Player::ControllerOf(Box::new(its))))
        .then(|| each.clone())
}

/// "Those players put the exiled cards onto the battlefield, then shuffle"
/// after the loop: every controller the loop named, at once (the cards
/// enter at the same time, and a player who exiled none still shuffles:
/// Indomitable Creativity's rulings).
fn those_players(p: &mut P, each: Objects) -> Option<Vec<Effect>> {
    p.words("those players")?;
    p.word("put")?;
    let first = put_rest(p)?;
    p.words(", then shuffle")?;
    p.end_sentence()?;
    let who = Player::Those(Box::new(Player::ControllerOf(Box::new(each))));
    Some(vec![
        Effect::Performed {
            by: who.clone(),
            effect: Box::new(first),
        },
        Effect::Shuffle(who),
    ])
}

/// The effects of a `may`, `if you do`, `if` or `otherwise` that bring in
/// an object that exists only if that branch was taken.
fn branch_with_new_object(e: &mut Effect) -> Option<&mut Vec<Effect>> {
    let then = match e {
        Effect::May { then, .. }
        | Effect::IfYouDo(then)
        | Effect::If { then, .. }
        | Effect::Otherwise(then) => then,
        _ => return None,
    };
    then.iter().any(brings_in).then_some(then)
}

/// Whether an effect picks out a new object: one moved, exiled, made,
/// found, chosen or revealed, not one an earlier clause named. A card put
/// onto the battlefield is a new object whatever named it (CR 400.7).
fn brings_in(e: &Effect) -> bool {
    let new = |x: &Objects| !matches!(x.quant, Quant::Antecedent | Quant::SelfRef);
    match e {
        Effect::Move {
            what,
            to: Zone::Battlefield,
            ..
        } => what.quant != Quant::SelfRef,
        Effect::Move { what, .. }
        | Effect::Exile(what)
        | Effect::Search { what, .. }
        | Effect::Reveal { what, .. }
        | Effect::Choose(Choice::Objects(what)) => new(what),
        Effect::Create { .. }
        | Effect::CreateCopy { .. }
        | Effect::CreateNamed { .. }
        | Effect::CreateAttached { .. } => true,
        Effect::Or(v) => v.iter().all(brings_in),
        _ => false,
    }
}

/// Whether an effect is first said of a bare "it", "them" or "that
/// creature", or a definite "the token", "that card": an object an earlier
/// clause named. "If the token isn't a creature, …" says it in the "if".
fn refers_back(e: &Effect) -> bool {
    let it = |x: &Objects| x.quant == Quant::Antecedent;
    match e {
        Effect::If { cond, .. } if tested(cond).is_some() => true,
        Effect::Cast { what, .. } => it(what),
        Effect::May { then, .. } => then.first().is_some_and(refers_back),
        Effect::Modify { what, .. }
        | Effect::GrantAbility { what, .. }
        | Effect::Becomes { what, .. }
        | Effect::Move { what, .. }
        | Effect::Sacrifice { what, .. }
        | Effect::Destroy { what, .. }
        | Effect::Exile(what)
        | Effect::Tap(what)
        | Effect::Untap(what)
        | Effect::EntersWithCounters { what, .. } => it(what),
        Effect::PutCounters { on, .. } => it(on),
        Effect::Delayed { then, .. } | Effect::If { then, .. } => {
            then.first().is_some_and(refers_back)
        }
        _ => false,
    }
}

/// The earlier object an "if it's …" or "if it isn't …" tests.
fn tested(c: &Condition) -> Option<&Objects> {
    match c {
        Condition::Is { what, .. } if what.quant == Quant::Antecedent => Some(what),
        Condition::Not(c) => tested(c),
        _ => None,
    }
}

/// Whether an effect is said of a bare "it", so continues the last one's
/// target.
fn continues_target(e: &Effect) -> bool {
    let it = |x: &Objects| x.quant == Quant::Antecedent && x.filter == Filter::default();
    match e {
        Effect::Becomes { what, .. }
        | Effect::Modify { what, .. }
        | Effect::GrantAbility { what, .. } => it(what),
        _ => false,
    }
}
