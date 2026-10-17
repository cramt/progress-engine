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
        } else if let Some(post) = matches!(&e, Effect::Exile(o) | Effect::Destroy { what: o, .. } if matches!(o.quant, Quant::Each | Quant::All))
            .then(|| p.attempt(unless_each))
            .flatten()
        {
            // "exile each permanent unless it's colorless or it's only the
            // color its controller chose" (Selective Obliteration): the
            // "unless" says which permanents, each tested on its own, so it
            // is the objects' filter and not one condition on the spell.
            if let Effect::Exile(o) | Effect::Destroy { what: o, .. } = &mut e {
                o.filter.posts.push(post);
            }
        } else if let Some(mut cond) = p.attempt(|p| {
            // "unless it has the same name as another card exiled this way".
            p.word("unless")?;
            condition(p)
        }) {
            after_clause(&e, &mut cond);
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
        } else if let Some(times) = matches!(e, Effect::Copy { .. })
            .then(|| {
                // "Copy target triggered ability you control twice", "copy
                // that spell X times": the copy made that many times.
                p.attempt(|p| {
                    alt!(p;
                        |p| p.word("twice").map(|_| Amount::N(2)),
                        |p| { let n = amount(p)?; p.word("times")?; Some(n) },
                    )
                })
            })
            .flatten()
        {
            e = Effect::Repeat {
                times,
                then: vec![e],
            };
        } else if let Some(times) = p.attempt(|p| {
            // "copy it for each time you've cast your commander from the
            // command zone this game": the effect once per cast.
            p.words("for each")?;
            each_time(p)
        }) {
            // "copy it for each time you've cast your commander": "it" was
            // said before the commander was named.
            e.walk(&mut BeforeEach);
            e = Effect::Repeat {
                times,
                then: vec![e],
            };
        } else if let Some(each) = p.attempt(|p| {
            p.words("for each")?;
            objects(p)
        }) {
            // "it gets +1/+0 … for each other attacking creature": every
            // reference back in the clause was said before the loop's noun,
            // so none of them is the object a pass counts.
            e.walk(&mut BeforeEach);
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
        } else if let Some(mut cond) = p.attempt(|p| {
            p.word("if")?;
            condition(p)
        }) {
            after_clause(&e, &mut cond);
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
        } else if let Some(to) = p.attempt(|p| {
            // "put it on top of its owner's library instead of into that
            // player's graveyard": the same object, its verb said once.
            if !matches!(e, Effect::Move { .. }) {
                return None;
            }
            p.words("instead of into")?;
            zone(p)
        }) {
            let Effect::Move { what, .. } = &e else {
                unreachable!("checked above")
            };
            let of = Effect::Move { what: what.clone(), from: None, to, tapped: false, transformed: false, under: None, order: None };
            e = Effect::InsteadOf {
                then: Box::new(e),
                of: Box::new(of),
            };
        } else if let Some(s) = p.attempt(|p| {
            alt!(p;
                |p| { p.words("at the beginning of")?; step(p) },
                // "destroy that creature at end of combat": as the end of
                // combat step begins (CR 511.2).
                |p| p.words("at end of combat").map(|_| Step::EndOfCombat(Whose::The)),
            )
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
        let c = costs(p)?;
        Some(per_costs(p, c))
    }) {
        return Some(c);
    }
    let one = |p: &mut P| {
        alt!(p;
            |p| { p.verb("sacrifice")?; objects(p).map(Cost::Sacrifice) },
            |p| { p.verb("discard")?; objects(p).map(Cost::Discard) },
            // "sacrifice it unless you return an untapped Plains you control
            // to its owner's hand" (Karoo).
            |p| {
                p.verb("return")?;
                let x = objects(p)?;
                p.word("to")?;
                if owners_hand(p)? != Zone::OwnersHand { return None; }
                Some(Cost::ReturnToHand(x))
            },
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
    if v.len() > 2 {
        return Some(v);
    }
    // "Destroy target artifact and target enchantment", "untap it and all
    // Samurai you control": two, where the clause ends after the second.
    let second = p.attempt(|p| {
        p.word("and")?;
        let o = objects(p)?;
        matches!(p.peek(), Some(Tok::Period | Tok::Comma) | None).then_some(o)
    })?;
    (v.len() == 1).then(|| vec![v.remove(0), second])
}

pub(super) fn clause(p: &mut P) -> Option<Vec<Effect>> {
    let v = alt!(p;
        // "you gain life and draw cards equal to its power", "You draw cards
        // and lose life equal to the number of counters removed this way":
        // one amount said once for both.
        |p| {
            let who = subject_player(p);
            type Make = fn(Player, Amount) -> Effect;
            let part = |p: &mut P| -> Option<Make> {
                alt!(p;
                    |p| { p.verb("gain")?; p.word("life")?; Some((|who, n| Effect::GainLife { who, n }) as Make) },
                    |p| { p.verb("lose")?; p.word("life")?; Some((|who, n| Effect::LoseLife { who, n }) as Make) },
                    |p| { p.verb("draw")?; p.word("cards")?; Some((|who, n| Effect::Draw { who, n }) as Make) },
                )
            };
            let a = part(p)?;
            p.word("and")?;
            let b = part(p)?;
            p.words("equal to")?;
            let n = measure(p)?;
            Some(vec![a(who.clone(), n.clone()), b(who, n)])
        },
        |p| {
            p.word("destroy")?;
            let v = object_list(p)?;
            let no_regen = cant_be_regenerated(p, &v);
            Some(v.into_iter().map(|what| Effect::Destroy { what, no_regen }).collect())
        },
        |p| { p.word("exile")?; object_list(p).map(|v| v.into_iter().map(Effect::Exile).collect()) },
        // "create a Food token and a Treasure token": two tokens, one verb.
        |p| {
            p.word("create")?;
            let (n1, t1) = token(p)?;
            p.word("and")?;
            let (n2, t2) = token(p)?;
            if !matches!(p.peek(), Some(Tok::Period | Tok::Comma) | None) { return None; }
            let who = p.implied.clone().unwrap_or(Player::You);
            Some(vec![
                Effect::Create { who: who.clone(), n: n1, token: t1 },
                Effect::Create { who, n: n2, token: t2 },
            ])
        },
        |p| { p.word("tap")?; object_list(p).map(|v| v.into_iter().map(Effect::Tap).collect()) },
        |p| { p.word("untap")?; object_list(p).map(|v| v.into_iter().map(Effect::Untap).collect()) },
        // "Whenever you attack this turn, …": a delayed trigger for the turn.
        |p| {
            p.word("whenever")?;
            let event = event(p)?;
            // "… this combat," (Stensian Sanguinist) lasts the combat.
            let dur = if p.one_of(&["this turn", "this combat"])? == "this turn" { Duration::ThisTurn } else { Duration::ThisCombat };
            p.tok(&Tok::Comma)?;
            let event = intervening(p, event);
            let then = delayed_body(p)?;
            Some(vec![Effect::During { dur, then: vec![Effect::DelayedWhenever { event, then }] }])
        },
        // "Until end of turn, whenever a player taps an Island for mana, …".
        |p| {
            p.word("whenever")?;
            let event = event(p)?;
            p.tok(&Tok::Comma)?;
            let event = intervening(p, event);
            let then = delayed_body(p)?;
            Some(vec![Effect::DelayedWhenever { event, then }])
        },
        // "For each odd result, create …": once per die that came up odd.
        |p| {
            p.words("for each")?;
            let parity = alt!(p;
                |p| p.word("odd").map(|_| Parity::Odd),
                |p| p.word("even").map(|_| Parity::Even),
            )?;
            p.words("result ,")?;
            Some(vec![Effect::Repeat { times: Amount::RolledResults(parity), then: clauses(p)? }])
        },
        // "For each evidence vote, investigate." (CR 701.38): once per vote.
        |p| {
            p.words("for each")?;
            let w = super::conditions::vote_option(p, "vote")?;
            p.words("vote ,")?;
            Some(vec![Effect::Repeat { times: Amount::Votes(w), then: clauses(p)? }])
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
            // "has base power and toughness 10/10 and gains trample,
            // annihilator 2, and haste": the keywords of the same effect,
            // as "becomes a 4/4 … and gains flying" (Flayer of Loyalties).
            let gains = if also { vec![] } else {
                p.attempt(|p| { p.word("and")?; p.verb("gain")?; super::keyword_list(p) }).unwrap_or_default()
            };
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let mut v = vec![Effect::Becomes { what: what.clone(), pt: Some(pt), types: vec![], gains, keeps: Keeps::implied_by(&[]), dur: dur.clone() }];
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
            let keeps = still(p).unwrap_or_else(|| Keeps::implied_by(&types));
            Some(vec![
                Effect::Becomes { what: what.clone(), pt: Some(pt), types, gains: vec![], keeps, dur: dur.clone() },
                Effect::AlsoIs { what, types: TypeSet::EveryCreatureType, dur },
            ])
        },
        // "Until end of turn, creatures target player controls lose all
        // abilities and have base power and toughness 0/2" (Sudden Spoiling):
        // the abilities lost and the base size set, one subject, one
        // duration (layers 6 and 7b, CR 613.1f, 613.4b).
        |p| {
            let what = objects(p)?;
            p.verb("lose")?;
            p.words("all abilities and")?;
            p.verb("have")?;
            p.words("base power and toughness")?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(vec![
                Effect::Loses { what: what.clone(), lost: vec![Lost::All { other: false, of: Characteristic::Abilities }], dur: dur.clone() },
                Effect::Becomes { what, pt: Some(pt), types: vec![], gains: vec![], keeps: Keeps::implied_by(&[]), dur },
            ])
        },
        // "For as long as that land has a blight counter on it, it loses
        // all land types and abilities and has "{T}: Add {C}."" (Ultima):
        // what it loses and each quoted ability it has, one subject.
        |p| {
            let what = objects(p)?;
            p.verb("lose")?;
            if p.peek_word() != Some("all") { return None; }
            let lost = lost(p)?;
            p.word("and")?;
            p.verb("have")?;
            let mut granted = vec![quoted(p)?];
            while let Some(a) = p.attempt(|p| { p.word("and")?; quoted(p) }) {
                granted.push(a);
            }
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let mut v = vec![Effect::Loses { what: what.clone(), lost, dur: dur.clone() }];
            v.extend(granted.into_iter().map(|a| Effect::GrantAbility { what: what.clone(), ability: Box::new(a), dur: dur.clone() }));
            Some(v)
        },
        // "Until end of turn, this land becomes a 3/2 red Goblin creature
        // with "Whenever this creature attacks, …". It's still a land.": the
        // new types and keywords, and each quoted ability granted for as
        // long (Hanweir Battlements' kin, the Mutavault-style lands).
        |p| {
            if !p.ahead("\"") { return None; }
            let what = objects(p)?;
            // "they become 3/3 blue Bird creatures": plural, not read here.
            if p.peek_word() != Some("becomes") && what.quant == Quant::Antecedent { return None; }
            p.verb("become")?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let pt = p.attempt(|p| match p.peek()? { Tok::Pt(pt) if !pt.signed => { let pt = *pt; p.bump(); Some(pt) } _ => None });
            let types = becomes_types(p);
            if types.is_empty() && pt.is_none() { return None; }
            p.word("with")?;
            let (gains, abilities) = keywords_then_quoted(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let keeps = p.attempt(still).unwrap_or_else(|| Keeps::implied_by(&types));
            let mut v = vec![Effect::Becomes { what: what.clone(), pt, types, gains, keeps, dur: dur.clone() }];
            v.extend(abilities.into_iter().map(|a| Effect::GrantAbility { what: what.clone(), ability: Box::new(a), dur: dur.clone() }));
            Some(v)
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
                // "creatures your opponents control lose hexproof and shroud
                // and can't have hexproof or shroud" (Arcane Lighthouse).
                Effect::LoseKeywords { what, .. } => what.clone(),
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
                |p| p.one_of(&["that mana is spent on", "that mana is spent to cast"]).map(|_| SpentOf::All),
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
            let event = intervening(p, event);
            let then = delayed_body(p)?;
            Some(vec![Effect::During { dur: Duration::ThisTurn, then: vec![Effect::DelayedWhen { event, then }] }])
        },
        |p| {
            p.word("when")?;
            let event = event(p)?;
            p.tok(&Tok::Comma)?;
            let event = intervening(p, event);
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
        // "at end of combat, exile it" (Fortune, Loyal Steed): CR 511.2.
        |p| { p.words("at end of combat")?; p.tok(&Tok::Comma)?; let then = delayed_body(p)?; Some(vec![Effect::Delayed { step: Step::EndOfCombat(Whose::The), then }]) },
        |p| {
            let dur = duration(p)?;
            p.tok(&Tok::Comma)?;
            let outer = std::mem::replace(&mut p.during, true);
            let then = p.attempt(clauses);
            p.during = outer;
            let mut then = then?;
            // "Until end of turn, you may play that card" (Ire Shaman): the
            // permission lasts that long, as "you may play that card until
            // end of turn" says, rather than a lasting one given then.
            if let [Effect::May { then: inner, .. }] = &mut then[..] {
                if let [Effect::Cast { dur: d @ Duration::Permanent, .. }] = &mut inner[..] {
                    *d = dur;
                    return Some(then);
                }
            }
            // "Until end of turn, target creature … gains "…"" (Simic
            // Basilisk), "Until end of turn, target land becomes a 3/3":
            // one effect that lasts takes the duration as its own, the same
            // as when it trails ([lead-permission-duration]).
            if let [e] = &mut then[..] {
                if let Some(d @ Duration::Permanent) = e.dur_mut() {
                    *d = dur;
                    return Some(then);
                }
            }
            // "For as long as …, it loses all land types and abilities and
            // has "…"" (Ultima): what it loses and what it has, said as one
            // clause, each lasting that long.
            if let [Effect::Loses { what, dur: Duration::Permanent, .. }, rest @ ..] = &then[..] {
                let what = what.clone();
                if !rest.is_empty()
                    && rest.iter().all(|e| matches!(e, Effect::GrantAbility { what: w, dur: Duration::Permanent, .. } if *w == what))
                {
                    for e in &mut then {
                        if let Some(d) = e.dur_mut() {
                            *d = dur.clone();
                        }
                    }
                    return Some(then);
                }
            }
            Some(vec![Effect::During { dur, then }])
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
        |p| {
            // A paragraph opens with "Then" ("Coven — Then if …") only where
            // it continues a spell's instructions ([then-paragraph]);
            // anywhere else the word would be dropped with nothing to say it.
            let opens = p.i == 0 || p.t.get(p.i - 1) == Some(&Tok::Dash);
            if opens && !p.continues { return None; }
            p.word("then")?;
            clause(p)
        },
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
                while let Some((joiner, e)) = p.attempt(|p| {
                    let joiner = p.one_of(&[", then", ", and then", ", and", "and then", "and", ","])?;
                    if !p.peek_word().is_some_and(|w| IMPERATIVE.contains(&w)) { return None; }
                    let e = effect(p)?;
                    Some((joiner, suffixed(p, e)))
                }) {
                    if joiner == "and" {
                        if let Some(a) = then.last_mut() { play_and_cast_from(a, &e); }
                    }
                    then.push(e);
                }
                Some(then)
            });
            p.implied = outer;
            let then = e?;
            // "you may draw a card unless that player pays {1}": the payer
            // decides first, and the option is offered only if they don't
            // (Rhystic Study's ruling; CR 608.2d), so the "unless" holds the
            // "may", not the other way round.
            if let [Effect::Unless { effect, who: payer, cost }] = &then[..] {
                if *payer != who {
                    return Some(vec![Effect::Unless {
                        effect: Box::new(Effect::May { who, then: vec![(**effect).clone()] }),
                        who: payer.clone(),
                        cost: cost.clone(),
                    }]);
                }
            }
            // "Its controller may draw up to two cards at the beginning of
            // the next turn's upkeep": the spell makes the delayed trigger
            // whatever anyone chooses, and the choice is made as it resolves
            // (Arcane Denial's ruling; CR 603.7a, 608.2d).
            if let [Effect::Delayed { step, then: inner }] = &then[..] {
                return Some(vec![Effect::Delayed {
                    step: step.clone(),
                    then: vec![Effect::May { who, then: inner.clone() }],
                }]);
            }
            // "You may cast it without paying its mana cost if that spell's
            // mana value is 8 or less. If you don't, …": the option exists
            // only if the condition holds, so the "if" holds the "may", and
            // "If you don't" and "Otherwise" after it answer the choice and
            // the condition, not a choice that was always offered.
            if let [Effect::If { cond, then: inner }] = &then[..] {
                return Some(vec![Effect::If {
                    cond: cond.clone(),
                    then: vec![Effect::May { who, then: inner.clone() }],
                }]);
            }
            Some(vec![Effect::May { who, then }])
        },
        |p| {
            let e = effect(p)?;
            // "Add {B}{B}{B}{B}{B} instead if there are seven or more cards in your graveyard."
            if let Some(cond) = p.attempt(|p| { p.words("instead if")?; condition(p) }) {
                return Some(vec![Effect::Instead { cond, then: vec![e] }]);
            }
            let e = suffixed(p, e);
            // "Destroy target blocking creature at end of combat" (Silent
            // Assassin): the target is the ability's, chosen as it is put on
            // the stack (CR 601.2c, 602.2b), and the delayed trigger acts on
            // what was chosen ([target-before-delayed]).
            if let Some(v) = target_before_delayed(&e) {
                return Some(v);
            }
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
    );
    let mut v = v?;
    continue_target(&mut v);
    Some(v)
}

/// "Destroy target blocking creature at end of combat", "Up to two other
/// target planeswalkers you control phase out at the beginning of the next
/// end step", "Target player draws a card at the beginning of the next
/// turn's upkeep": a target said in a clause that a trailing step delays
/// is the ability's own, chosen as it is put on the stack (CR 601.2c,
/// 602.2b), so the tree chooses it first and the delayed trigger acts on
/// it by reference ([target-before-delayed]).
pub(super) fn target_before_delayed(e: &Effect) -> Option<Vec<Effect>> {
    let Effect::Delayed { step, then } = e else {
        return None;
    };
    let [inner] = &then[..] else { return None };
    let mut inner = inner.clone();
    let choice = if let Some(x) = crate::ast::delayed_target_mut(&mut inner) {
        let t = x.clone();
        x.quant = Quant::Antecedent;
        x.filter = Filter::default();
        Choice::Objects(t)
    } else if let Effect::Draw {
        who: who @ Player::Target,
        ..
    } = &mut inner
    {
        *who = Player::That;
        Choice::TargetPlayer
    } else {
        return None;
    };
    Some(vec![
        Effect::Choose(choice),
        Effect::Delayed {
            step: step.clone(),
            then: vec![inner],
        },
    ])
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

/// The object an effect's verb is said of, to change it.
fn object_subject_mut(e: &mut Effect) -> Option<&mut Objects> {
    match e {
        Effect::Modify { what, .. }
        | Effect::GrantAbility { what, .. }
        | Effect::LoseAbility { what, .. }
        | Effect::LoseKeywords { what, .. }
        | Effect::Loses { what, .. }
        | Effect::Restrict { what, .. }
        | Effect::Becomes { what, .. }
        | Effect::AlsoIs { what, .. } => Some(what),
        Effect::Damage { source, .. } => source.as_mut(),
        _ => None,
    }
}

/// A player subject continued by a later verb: a target, or the owner or
/// controller of a target ("The owner of target permanent shuffles it into
/// their library, then draws two cards"), is that player, chosen once.
fn continued_player(w: Player) -> Player {
    match &w {
        Player::Target => Player::That,
        Player::OwnerOf(o) | Player::ControllerOf(o) if matches!(o.quant, Quant::Target { .. }) => {
            Player::That
        }
        _ => w,
    }
}

/// "Target player draws two cards and loses 2 life", "target creature
/// loses all abilities and has base power and toughness 1/1": a target
/// said once and continued by a second verb is the one target, chosen
/// once, so every clause after the first refers back to it
/// ([continued-subject]): the player is `That`, the object `Antecedent`.
pub(super) fn continue_target(es: &mut [Effect]) {
    let Some((first, rest)) = es.split_first_mut() else {
        return;
    };
    if let Some(s) = subject_of(first).filter(|s| continued_player(s.clone()) == Player::That) {
        for e in rest.iter_mut() {
            if let Some(who) = subject_mut(e).filter(|w| **w == s) {
                *who = Player::That;
            }
        }
    }
    let Some(o) = object_subject_mut(first).cloned() else {
        return;
    };
    if !matches!(o.quant, Quant::Target { .. }) {
        return;
    }
    for e in rest.iter_mut() {
        if let Some(w) = object_subject_mut(e) {
            if *w == o {
                w.quant = Quant::Antecedent;
            }
        }
    }
}

/// `subject_of`, to change it.
pub(super) fn subject_mut(e: &mut Effect) -> Option<&mut Player> {
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
        | Effect::Performed { by: who, .. } => who,
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

/// "Simultaneously untap all tapped creatures and tap all untapped
/// creatures", "that player simultaneously sacrifices the artifact and
/// returns the artifact card to the battlefield": the clauses at once
/// ([simultaneously]).
fn simultaneously(p: &mut P) -> Option<Vec<Effect>> {
    let who = p.attempt(player);
    p.word("simultaneously")?;
    let implied = who.or_else(|| p.implied.clone());
    let then = clause_as_all(p, implied)?;
    (then.len() > 1).then(|| vec![Effect::Simultaneously(then)])
}

/// Joined clauses whose subject, if they have none, is `who`.
fn clause_as_all(p: &mut P, who: Option<Player>) -> Option<Vec<Effect>> {
    let outer = std::mem::replace(&mut p.implied, who);
    let c = p.attempt(clauses);
    p.implied = outer;
    c
}

/// A clause whose subject, if it has none, is `who`.
pub(super) fn clause_as(p: &mut P, who: Option<Player>) -> Option<Vec<Effect>> {
    let outer = std::mem::replace(&mut p.implied, who);
    let c = p.attempt(clause);
    p.implied = outer;
    c
}

/// "You may play those cards for as long as they remain exiled, and you may
/// spend mana as though it were mana of any color to cast them": the spend
/// permission is for casting what the play permission before it lets you
/// cast, so it lasts as long (Opposition Agent's ruling: the ability "is
/// trying to exile the card and give permission to play it"). The text says
/// the duration once, with the play permission.
fn spend_with_play(v: &mut [Effect]) {
    fn play_dur(e: &Effect) -> Option<&Duration> {
        match e {
            Effect::Cast { dur, .. } => Some(dur),
            Effect::May { then, .. } => match &then[..] {
                [e] => play_dur(e),
                _ => None,
            },
            _ => None,
        }
    }
    fn spend_dur(e: &mut Effect) -> Option<&mut Duration> {
        match e {
            Effect::SpendAsAnyColor { dur, .. } | Effect::SpendAsAnyType { dur, .. } => Some(dur),
            Effect::May { then, .. } => match &mut then[..] {
                [e] => spend_dur(e),
                _ => None,
            },
            _ => None,
        }
    }
    let mut play: Option<Duration> = None;
    for e in v.iter_mut() {
        if let Some(d) = play_dur(e) {
            play = Some(d.clone());
        } else if let (Some(d), Some(p)) = (spend_dur(e), &play) {
            *d = p.clone();
        }
    }
}

/// "Until end of turn, you may play lands and cast spells from your
/// graveyard", "You may play lands and cast spells from among the exiled
/// cards for as long as they remain exiled": the zone and the duration said
/// once after the last verb hold of the lands too, as the static's
/// `PlayFrom` already reads them. A land is played from that zone (`In`);
/// the spell keeps `CastFrom` ([spell-cast-from]).
fn play_and_cast_from(a: &mut Effect, b: &Effect) {
    let (
        Effect::Cast {
            what: first,
            dur: dur_a,
            ..
        },
        Effect::Cast {
            what: last,
            dur: dur_b,
            ..
        },
    ) = (a, b)
    else {
        return;
    };
    let zone_of = |q: &Post| match q {
        Post::In(z) | Post::CastFrom(z) => Some(Post::In(z.clone())),
        Post::FromAmong(_) => Some(q.clone()),
        _ => None,
    };
    if first.filter.posts.iter().any(|q| zone_of(q).is_some()) || first.quant != Quant::Plural {
        return;
    }
    let Some(zone) = last.filter.posts.iter().find_map(zone_of) else {
        return;
    };
    first.filter.posts.push(zone);
    if *dur_a == Duration::Permanent {
        *dur_a = dur_b.clone();
    }
}

pub(super) fn clauses(p: &mut P) -> Option<Vec<Effect>> {
    if let Some(v) = p.attempt(simultaneously) {
        return Some(v);
    }
    let mut v = clause(p)?;
    while let Some((joiner, more)) = p.attempt(|p| {
        let joiner = p.one_of(&[
            ", and then",
            ", then",
            ", and",
            "and then",
            "and",
            "then",
            ",",
        ])?;
        // "any number of target players each mill a card and lose 1 life":
        // after a plural subject the verb agrees in the plural, so a bare
        // verb continues it as the third person does after a singular one.
        let plural = matches!(
            v.last().and_then(subject_of),
            Some(
                Player::AnyNumberOfTarget
                    | Player::AnyNumberOfTargetOpponents
                    | Player::NTarget { .. }
                    | Player::EachOf(_)
            )
        );
        let agrees = |w: &str| {
            THIRD_PERSON.contains(&w)
                || plural
                    && THIRD_PERSON.iter().any(|t| {
                        t.strip_suffix('s')
                            .is_some_and(|b| b == w || b.strip_suffix('e') == Some(w))
                    })
        };
        let continues = p.peek_word().is_some_and(agrees);
        // A continued target is the one chosen ([continued-subject]).
        let who = if continues {
            v.last().and_then(subject_of).map(continued_player)
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
            *source = v.last().and_then(object_subject_of).map(|mut o| {
                if matches!(o.quant, Quant::Target { .. }) {
                    o.quant = Quant::Antecedent;
                }
                o
            });
        }
        Some((joiner, c))
    }) {
        let mut more = more;
        listed(&v, &mut more);
        // "exile it at end of combat, then return it to the battlefield
        // transformed" (Conqueror's Galleon): what follows "then" happens
        // when the delayed trigger does ([delayed-owns-later-sentences]).
        if matches!(joiner, ", then" | ", and then" | "and then" | "then") {
            if let Some(Effect::Delayed { then, .. }) = v.last_mut() {
                then.extend(more);
                continue;
            }
        }
        if joiner == "and" {
            if let (Some(a), Some(b)) = (v.last_mut(), more.first()) {
                play_and_cast_from(a, b);
            }
        }
        v.extend(more);
    }
    spend_with_play(&mut v);
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
    // "Choose three. You may choose the same mode more than once." (Eldrazi
    // Confluence), "Choose one or both. Each mode must target a different
    // player.": the modes follow the second sentence.
    let rule = p.attempt(|p| {
        p.tok(&Tok::Period)?;
        let rule = alt!(p;
            |p| p.words("you may choose the same mode more than once").map(|_| Choose::Repeatable(Box::new(choose.clone()))),
            |p| p.words("each mode must target a different player").map(|_| Choose::DifferentPlayers(Box::new(choose.clone()))),
        )?;
        p.tok(&Tok::Period)?;
        Some(rule)
    });
    if let Some(choose) = rule {
        let modes = modes(p)?;
        return (!modes.is_empty()).then_some(Effect::Modal { choose, modes });
    }
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
    let modes = modes(p)?;
    (!modes.is_empty()).then_some(Effect::Modal { choose, modes })
}

/// The bulleted modes of a modal spell or ability.
fn modes(p: &mut P) -> Option<Vec<Vec<Effect>>> {
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
    Some(modes)
}

/// "any time you could activate a mana ability", "… cast an instant".
fn any_time_window(p: &mut P) -> Option<AnyTimeWindow> {
    p.words("any time you could")?;
    alt!(p;
        |p| p.words("activate a mana ability").map(|_| AnyTimeWindow::ManaAbility),
        |p| p.words("cast an instant").map(|_| AnyTimeWindow::Instant),
    )
}

pub(super) fn sentence(p: &mut P) -> Option<Vec<Effect>> {
    alt!(p;
        |p| modal(p).map(|m| vec![m]),
        // "At the beginning of combat on your turn, you may choose two."
        // (Shadrix Silverquill): choosing none is allowed.
        |p| { p.words("you may")?; modal(p).map(|m| vec![Effect::May { who: Player::You, then: vec![m] }]) },
        // "Repeat the following process X times. …": the process is every
        // sentence after it.
        |p| {
            p.words("repeat the following process")?;
            let times = amount(p)?;
            p.words("times .")?;
            Some(vec![Effect::Repeat { times, then: effects(p)? }])
        },
        // "Starting with you, each player chooses …": the sentence's players in
        // turn order from you.
        |p| {
            p.words("starting with you ,")?;
            let then = clauses(p)?;
            p.end_sentence()?;
            Some(vec![Effect::StartingWith { first: Player::You, then }])
        },
        // "Until end of turn, any time you could activate a mana ability, you
        // may pay 1 life. If you do, add {C}.", or the window after the cost.
        |p| {
            let dur = duration(p)?;
            p.tok(&Tok::Comma)?;
            let lead = p.attempt(|p| { let w = any_time_window(p)?; p.tok(&Tok::Comma)?; Some(w) });
            p.words("you may")?;
            let cost = costs(p)?;
            let window = match lead {
                Some(w) => w,
                None => any_time_window(p)?,
            };
            p.end_sentence()?;
            p.words("if you do ,")?;
            let then = clauses(p)?;
            p.end_sentence()?;
            Some(vec![Effect::AnyTime { who: Player::You, window, cost, then, dur }])
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
            // "If you do, create three 1/1 … Scion creature tokens. They have
            // "…"": the plural, and a token made in a branch.
            if let Some(token) = last_token(&mut e) {
                if token.abilities.is_empty() && token.keywords.is_empty() {
                    if let Some(a) = p.attempt(|p| { p.one_of(&["it has", "they have"])?; let a = quoted(p)?; p.end_sentence()?; Some(a) }) {
                        token.abilities.push(a);
                    } else if let Some((k, a)) = p.attempt(|p| {
                        // "The token has enchant creature and "Whenever …""
                        // (Scriv, the Obligator): keywords too.
                        p.one_of(&["the token has", "the tokens have"])?;
                        let ka = super::keywords_then_quoted(p)?;
                        p.end_sentence()?;
                        Some(ka)
                    }) {
                        token.keywords = k;
                        token.abilities = a;
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
        // "Round down each time.": the halves before it took their
        // rounding from it (`P::rounds_later`).
        if !v.is_empty()
            && p.halved_before()
            && p.attempt(|p| {
                p.word("round")?;
                p.one_of(&["up", "down"])?;
                p.words("each time")?;
                p.end_sentence()
            })
            .is_some()
        {
            continue;
        }
        let s = match v.last().and_then(controllers_of_each) {
            Some(each) => p.attempt(|p| those_players(p, each)),
            None => None,
        };
        let Some(mut s) = s.or_else(|| p.attempt(sentence)) else {
            break;
        };
        // "You may cast the exiled card without paying its mana cost if it's
        // an instant spell …. If you don't, put that card into your hand"
        // (Discover the Impossible): the card goes to hand whether the "if"
        // failed or the option was declined, so this is the cast (or put)
        // the option offered not done ([didnt-put]), not an `IfYouDont`.
        if let ([Effect::IfYouDont(then)], Some(Effect::If { then: opt, .. })) =
            (&mut s[..], v.last())
        {
            if let [Effect::May { who, then: offered }] = &opt[..] {
                let not_done = match &offered[..] {
                    [Effect::Cast { what, .. }] => Some(Event::Casts {
                        who: who.clone(),
                        what: what.clone(),
                    }),
                    [Effect::Move {
                        what,
                        from: None,
                        to,
                        ..
                    }] => Some(Event::Puts {
                        who: who.clone(),
                        what: what.clone(),
                        to: to.clone(),
                    }),
                    _ => None,
                };
                if let Some(ev) = not_done {
                    let then = std::mem::take(then);
                    s = vec![Effect::If {
                        cond: Condition::Not(Box::new(Condition::ThisWay(Box::new(ev)))),
                        then,
                    }];
                }
            }
        }
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
        // "Look at the top X cards of your library, where X is your devotion
        // to blue. … If X is greater than or equal to …": X keeps the value
        // defined for it (CR 107.3i), so every sentence from the definition
        // to one that says X again is in its scope.
        // Tivash: "you may pay X life, where X is the amount of life you
        // gained this turn. If you do, create an X/X black Demon": the X of
        // a "may"'s only clause is the ability's, so the definition goes
        // around the "may" before the sentence after joins it.
        if says_x(&s) && !v.iter().any(|e| matches!(e, Effect::Where { .. })) {
            if let Some(k) = v.iter().rposition(|e| {
                matches!(e, Effect::May { then, .. } if matches!(&then[..], [Effect::Where { .. }]))
            }) {
                if let Effect::May { who, then } = v[k].clone() {
                    if let [Effect::Where { then: inner, x }] = &then[..] {
                        v[k] = Effect::Where {
                            then: vec![Effect::May { who, then: inner.clone() }],
                            x: x.clone(),
                        };
                    }
                }
            }
        }
        if let Some(k) = v.iter().rposition(|e| matches!(e, Effect::Where { .. })) {
            if says_x(&s) && !s.iter().any(|e| matches!(e, Effect::Where { .. })) {
                let tail: Vec<Effect> = v.drain(k + 1..).collect();
                if let Some(Effect::Where { then, .. }) = v.last_mut() {
                    then.extend(tail);
                    then.extend(s);
                }
                continue;
            }
        }
        listed(&v, &mut s);
        // "Tyvar gains indestructible until end of turn. Tap it.": the only
        // object the sentence before names is the card itself, so "it" is.
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
        ) || matches!(
            // "If you win the flip, … If you lose the flip, …" (Frenetic Sliver).
            (v.last(), s.first()),
            (
                Some(Effect::If {
                    cond: Condition::WonFlip(_),
                    ..
                }),
                Some(Effect::If {
                    cond: Condition::WonFlip(_),
                    ..
                })
            )
        );
        // "If you do, each opponent may sacrifice a permanent …. For each
        // opponent who doesn't, …": the players who declined were offered
        // the choice only in that branch, so the sentence about them is its.
        if let Some(Effect::IfYouDo(then) | Effect::If { then, .. } | Effect::Otherwise(then)) =
            v.last_mut()
        {
            let declined = |e: &Effect| matches!(e, Effect::ForEach { each, .. } if each.filter.posts.contains(&Post::WhoDoesnt));
            if matches!(then.last(), Some(Effect::May { .. })) && s.first().is_some_and(declined) {
                then.extend(s);
                continue;
            }
        }
        // "You may copy it. If you do, those spells gain wither. You may
        // choose new targets for the copy.": "if you do" is the branch the
        // may before it took, so the copy exists in it too.
        let n = v.len();
        let inherits = n >= 2
            && matches!(v[n - 1], Effect::IfYouDo(_))
            && branch_with_new_object(&mut v[n - 2]).is_some()
            && s.first().is_some_and(refers_back);
        if inherits {
            if let Some(Effect::IfYouDo(then)) = v.last_mut() {
                then.extend(s);
                continue;
            }
        }
        // "copy it if you control an artifact, then copy it if you control an
        // enchantment. You may choose new targets for the copies.": copies
        // made in two branches are both the sentence's, so it is neither's.
        let copies_before = v[..n.saturating_sub(1)].iter().any(makes_copy);
        let both = copies_before && v.last().is_some_and(makes_copy);
        let option = matches!(v.last(), Some(Effect::IfYouDo(_)));
        if let Some(branch) = v
            .last_mut()
            .and_then(branch_with_new_object)
            .filter(|_| !cases && !both)
        {
            // "Otherwise, …" is the other branch of the "if" before it, so
            // it goes where that "if" went (Thunderous Debut).
            let otherwise = matches!(branch.last(), Some(Effect::If { .. }))
                && matches!(s.first(), Some(Effect::Otherwise(_)));
            // "You may reveal a creature card … from among them. You may put
            // it onto the battlefield if it's your turn. If you don't put it
            // onto the battlefield, put it into your hand" (Whiskervale
            // Forerunner): the card exists only if it was revealed, so the
            // "if" is the reveal's; never the put's own branch ([didnt-put]).
            // Not "If you don't put a card into your hand this way" (Courier
            // of Comestibles), which holds when the option was declined.
            let didnt_put = matches!(
                s.first(),
                Some(Effect::If { cond: Condition::Not(c), .. })
                    if matches!(&**c, Condition::ThisWay(e) if matches!(&**e, Event::Puts { what, .. } if what.quant == Quant::Antecedent))
            ) && branch.iter().any(|e| {
                brings_in(e)
                    && !matches!(
                        e,
                        Effect::Move {
                            to: Zone::Battlefield,
                            ..
                        } | Effect::EntersWith { .. }
                            | Effect::EntersAttacking(_)
                            | Effect::EntersAttached { .. }
                    )
            });
            // "If X is 2, you may choose a player. They return a creature
            // they control to its owner's hand" (Multiple Choice): the
            // player exists only in the branch that chose one.
            let chose_player = branch
                .iter()
                .any(|e| matches!(e, Effect::Choose(Choice::Player | Choice::Opponent)))
                && matches!(
                    s.first(),
                    Some(Effect::Performed {
                        by: Player::That,
                        ..
                    })
                );
            if otherwise
                || didnt_put
                || chose_player
                || s.first().is_some_and(refers_back)
                || (option && s.first().is_some_and(casts_from_among_them))
            {
                branch.extend(s);
                continue;
            }
        }
        v.extend(s);
    }
    permission_rules(&mut v);
    no_regen_after(&mut v);
    new_targets_after(&mut v);
    enters_with_after(&mut v);
    source_of_ability(&mut v);
    // "… If you do, draw a card and repeat this process.": the process is
    // every instruction of the ability, said from the first.
    if v.iter().any(repeats) {
        v = vec![Effect::Process(v)];
    }
    (!v.is_empty()).then_some(v)
}

/// "Copy target instant or sorcery spell. You may choose new targets for
/// the copy.", "copy it for each time you've cast your commander from the
/// command zone this game. You may choose new targets for the copies.": the
/// choice is made as each copy is put onto the stack (CR 707.10c), so it
/// belongs to the copy effect, inside whatever loop or delayed trigger
/// holds it, and not to a "may" said after the copies exist
/// ([copy-new-targets]).
fn new_targets_after(v: &mut Vec<Effect>) {
    for e in v.iter_mut() {
        if let Effect::DelayedNext { then, .. }
        | Effect::DelayedWhen { then, .. }
        | Effect::DelayedWhenever { then, .. }
        | Effect::During { then, .. }
        | Effect::IfYouDont(then)
        | Effect::If { then, .. }
        | Effect::Where { then, .. }
        | Effect::ForEach { then, .. }
        | Effect::May { then, .. }
        | Effect::IfYouDo(then)
        | Effect::WhenYouDo(then) = e
        {
            new_targets_after(then);
        }
        if let Effect::Modal { modes, .. } = e {
            modes.iter_mut().for_each(new_targets_after);
        }
    }
    let mut i = 1;
    while i < v.len() {
        let about_copy = matches!(&v[i], Effect::May { who: Player::You, then }
            if matches!(then.as_slice(), [Effect::ChooseNewTargets(o)]
                if o.quant == Quant::Antecedent
                    && o.filter == Filter { kind: Some(Noun::Copy), ..Filter::default() }));
        // "copy target spell, then return it to its owner's hand. You may
        // choose new targets for the copy" (Narset's Reversal): past a move
        // of the original, the copy said before it.
        let at =
            if i >= 2 && v[i - 1].moves_the_original() && v[i - 2].trailing_copy() == Some(false) {
                i - 1
            } else {
                i
            };
        if about_copy && v[at - 1].trailing_copy() == Some(false) {
            // "copy it if you control an artifact, then copy it if you
            // control an enchantment. You may choose new targets for the
            // copies": each copy said before.
            for e in v[..at].iter_mut().rev() {
                match e.trailing_copy_mut() {
                    Some(flag) if !*flag => *flag = true,
                    _ => break,
                }
            }
            v.remove(i);
            continue;
        }
        i += 1;
    }
}

/// "Destroy target creature if it's white. A creature destroyed this way
/// can't be regenerated.", "Destroy up to X target nonblack creatures, where
/// X is …. They can't be regenerated.", "destroy target Knight and it can't
/// be regenerated": the destroy said inside an "if", a "where" or a branch
/// is the one the sentence stops regeneration of, as when it stands alone
/// ([no-regen-after]). A pronoun takes the destroy said last; "destroyed
/// this way" every destroy of the effect before.
fn no_regen_after(v: &mut Vec<Effect>) {
    fn branch(e: &mut Effect) -> Option<&mut Vec<Effect>> {
        match e {
            Effect::If { then, .. }
            | Effect::Instead { then, .. }
            | Effect::Where { then, .. }
            | Effect::May { then, .. }
            | Effect::IfYouDo(then)
            | Effect::IfYouDont(then)
            | Effect::Otherwise(then) => Some(then),
            _ => None,
        }
    }
    // The destroys `e` ends with (a pronoun) or holds anywhere (marked).
    fn set(e: &mut Effect, all: bool) -> bool {
        if let Effect::Destroy { no_regen, .. } = e {
            *no_regen = true;
            return true;
        }
        let Some(then) = branch(e) else { return false };
        if all {
            let mut any = false;
            for t in then.iter_mut() {
                any |= set(t, true);
            }
            any
        } else {
            then.last_mut().is_some_and(|t| set(t, false))
        }
    }
    for e in v.iter_mut() {
        if let Some(then) = branch(e) {
            no_regen_after(then);
        }
    }
    let mut i = 1;
    while i < v.len() {
        if let Effect::Restrict {
            what,
            rule: Restriction::CantBeRegenerated,
            dur: Duration::Permanent,
        } = &v[i]
        {
            let marked = what.filter.posts.contains(&Post::Marked(Marked::Destroyed));
            let pronoun = *what == antecedent();
            if (marked || pronoun) && set(&mut v[i - 1], marked) {
                v.remove(i);
                continue;
            }
        }
        i += 1;
    }
}

/// Whether effects say X anywhere outside a quoted ability.
fn says_x(es: &[Effect]) -> bool {
    struct X(bool);
    impl Visitor for X {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn amount(&mut self, a: &mut Amount) {
            self.0 |= *a == Amount::X;
        }
        // "create an X/X black Demon", "gets +X/+X": a size says X too.
        fn effect(&mut self, e: &mut Effect) {
            let pt = match e {
                Effect::Create { token, .. }
                | Effect::CreateNamed { token, .. }
                | Effect::CreateAttached { token, .. } => token.pt,
                Effect::Modify { pt, .. } | Effect::Becomes { pt, .. } => *pt,
                _ => None,
            };
            let x = |p: crate::lex::PtPart| {
                matches!(p, crate::lex::PtPart::X | crate::lex::PtPart::MinusX)
            };
            self.0 |= pt.is_some_and(|pt| x(pt.power) || x(pt.toughness));
        }
    }
    let mut x = X(false);
    for e in es {
        e.clone().walk(&mut x);
    }
    x.0
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

/// The effects of a `may`, `if you do`, `if you don't`, `if` or `otherwise`
/// that bring in an object that exists only if that branch was taken; where
/// it brings in none itself, a branch it ends with ("If they don't, you may copy that
/// spell": the copy exists only if you chose to).
fn branch_with_new_object(e: &mut Effect) -> Option<&mut Vec<Effect>> {
    let (then, option) = match e {
        Effect::May { then, .. } | Effect::IfYouDo(then) | Effect::IfYouDont(then) => (then, true),
        // "When you do, choose up to one target creature card exiled this
        // way. Create a token that's a copy of that card" (Espers to
        // Magicite): the reflexive trigger's target exists only in it.
        Effect::WhenYouDo(then) | Effect::WhenYouDoIf { then, .. } => (then, true),
        Effect::If { then, .. } | Effect::Otherwise(then) => (then, false),
        // "return it to the battlefield … at the beginning of the next end
        // step. That creature is a black Zombie …" (Grave Betrayal): the
        // returned permanent exists only once the delayed trigger resolves
        // ([delayed-owns-later-sentences]).
        // A coin flipped then (Fickle Efreet) is won or lost only then.
        // Only a permanent put onto the battlefield: Elemental Appeal's "Exile
        // it at the beginning of the next end step. If this spell was kicked,
        // that creature gets +7/+0" is said of the token now.
        Effect::Delayed { then, .. } => {
            fn enters(e: &Effect) -> bool {
                match e {
                    Effect::Move {
                        to: Zone::Battlefield,
                        ..
                    } => true,
                    Effect::EntersWith { effect, .. }
                    | Effect::EntersAttacking(effect)
                    | Effect::EntersAttached { effect, .. } => enters(effect),
                    _ => false,
                }
            }
            if then.iter().any(|e| matches!(e, Effect::FlipCoin)) {
                return Some(then);
            }
            if !then.iter().any(enters) {
                return None;
            }
            (then, false)
        }
        _ => return None,
    };
    if then.iter().any(brings_in) || (option && then.iter().any(acts_on_target)) {
        return Some(then);
    }
    then.last_mut().and_then(branch_with_new_object)
}

/// Whether an effect copies a spell or card, in any branch.
fn makes_copy(e: &Effect) -> bool {
    match e {
        Effect::Copy { .. } => true,
        Effect::If { then, .. }
        | Effect::May { then, .. }
        | Effect::IfYouDo(then)
        | Effect::IfYouDont(then)
        | Effect::Otherwise(then) => then.iter().any(makes_copy),
        _ => false,
    }
}

/// Whether an effect picks out a new object: one moved, exiled, made,
/// copied, found, chosen or revealed, not one an earlier clause named. A
/// card put onto the battlefield or exiled is a new object whatever named
/// it (CR 400.7), so "you may exile it. You may play that card from exile
/// this turn" plays a card that is there only if you exiled it.
fn brings_in(e: &Effect) -> bool {
    let new = |x: &Objects| !matches!(x.quant, Quant::Antecedent | Quant::SelfRef);
    match e {
        Effect::Move {
            what,
            to: Zone::Battlefield,
            ..
        } => what.quant != Quant::SelfRef,
        Effect::Exile(what) => what.quant != Quant::SelfRef,
        // "If you do, exile the top card of your library face down. You may
        // look at that card …" (Intet): a card exiled is a new object.
        Effect::ExileFaceDown(what) => what.quant != Quant::SelfRef,
        Effect::Move { what, .. }
        | Effect::Search { what, .. }
        | Effect::Reveal { what, .. }
        | Effect::Choose(Choice::Objects(what)) => new(what),
        // "If X is 2, you may choose a player. They return a creature they
        // control to its owner's hand." (Multiple Choice): the player is
        // chosen only in the branch.
        Effect::Choose(Choice::Player | Choice::Opponent) => true,
        Effect::Choose(Choice::Targets(v)) => v.iter().any(new),
        Effect::Create { .. }
        | Effect::CreateCopy { .. }
        | Effect::CreateNamed { .. }
        | Effect::CreateAttached { .. }
        | Effect::Copy { .. } => true,
        // "If you do, reveal cards from the top of your library until you
        // reveal a creature card. Put that card into your hand" (Foster).
        Effect::UntilFound { .. } => true,
        // "return target Pirate creature card … with a finality counter on it".
        Effect::EntersWith { effect, .. }
        | Effect::EntersAttacking(effect)
        | Effect::EntersAttached { effect, .. } => brings_in(effect),
        Effect::Or(v) => v.iter().all(brings_in),
        _ => false,
    }
}

/// Whether an effect acts on a target it names. "If you do, tap up to
/// eight target permanents. Those permanents don't untap …" (Lorthos): the
/// targets are acted on only if the option was taken, so what is said of
/// them after is the option's too. Not after a plain "if": "If you win the
/// flip, target Orc creature gets +2/+0 …. If you lose the flip, it gets
/// -0/-2" (Orcish Captain) are two cases side by side.
fn acts_on_target(e: &Effect) -> bool {
    e.sole_object()
        .is_some_and(|o| matches!(o.quant, Quant::Target { .. }))
}

/// Whether an effect is first said of a bare "it", "them" or "that
/// creature", or a definite "the token", "that card": an object an earlier
/// clause named. "If the token isn't a creature, …" says it in the "if".
fn refers_back(e: &Effect) -> bool {
    let it = |x: &Objects| matches!(x.quant, Quant::Antecedent | Quant::Listed);
    match e {
        // "If it's a land card, you may put it onto the battlefield. If you
        // don't put the card onto the battlefield, put it into your hand":
        // asked whatever the branch before did, so never inside it.
        Effect::If {
            cond: Condition::Not(c),
            ..
        } if matches!(**c, Condition::ThisWay(_)) => false,
        Effect::If { cond, .. } if tested(cond).is_some() => true,
        // "flip a coin at end of combat. If you lose the flip, …" (Fickle
        // Efreet): the flip the delayed trigger makes.
        Effect::If {
            cond: Condition::WonFlip(_),
            ..
        } => true,
        // "Return those cards to the battlefield … at the beginning of the
        // next end step. If a land enters this way, it enters tapped"
        // (Silver Surfer): what the return put onto the battlefield.
        Effect::If {
            cond: Condition::ThisWay(ev),
            ..
        } if matches!(**ev, Event::Enters { .. }) => true,
        Effect::Cast { what, .. } => it(what),
        // "You may look at that card for as long as it remains exiled" (Intet).
        Effect::LookAt { what, .. } => it(what),
        // "Put that card onto the battlefield tapped and attacking" (Fireflux
        // Squad), "return it … with a +1/+1 counter on it": said of the
        // moved card.
        Effect::EntersWith { effect, .. }
        | Effect::EntersAttacking(effect)
        | Effect::EntersAttached { effect, .. } => refers_back(effect),
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
        // "If you do, put it onto the battlefield under your control. It's a
        // Warlock in addition to its other types." (Lorcan).
        | Effect::IsNow { what, .. }
        | Effect::AlsoIs { what, .. }
        | Effect::GainControl { what, .. }
        | Effect::Restrict { what, .. }
        | Effect::EntersWithCounters { what, .. } => it(what),
        Effect::PutCounters { on, .. } => it(on),
        // "You may choose new targets for the copy".
        Effect::ChooseNewTargets(what) => it(what),
        // "Create a token that's a copy of that card".
        Effect::CreateCopy { of, .. } => it(of),
        // "For each of those cards, pay 4 life or …" (Sylvan Library).
        Effect::ForEach { each, .. } => it(each),
        // "When you do, exile the top two cards of your library. Until the
        // end of your next turn, you may play those cards" (Anep).
        Effect::Delayed { then, .. } | Effect::During { then, .. } | Effect::If { then, .. } => {
            then.first().is_some_and(refers_back)
        }
        _ => false,
    }
}

/// "If you do, exile the top card of your library. Until the end of your
/// next turn, you may play one of those cards" (Flameskull), "If you do,
/// exile the top four cards of your library. You may cast any number of
/// spells from among them" (Glorious Purpose): a cast picked from what the
/// option's branch brought in, so the option's too. Not after a plain "if"
/// or no branch: Locke's "If a land card was milled this way, create a
/// Treasure token. Until end of turn, you may cast a spell from among those
/// cards" casts whatever was milled.
/// "Put the rest …" stays outside ([branch-owns-new-object]).
fn casts_from_among_them(e: &Effect) -> bool {
    fn back(x: &Objects) -> bool {
        matches!(x.quant, Quant::Antecedent | Quant::Listed)
            || x.filter.posts.iter().any(|p| match p {
                Post::FromAmongThem => true,
                Post::FromAmong(o) => back(o),
                _ => false,
            })
    }
    match e {
        Effect::Cast { what, .. } => back(what),
        Effect::May { then, .. } | Effect::During { then, .. } => {
            then.first().is_some_and(casts_from_among_them)
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

/// "Until the end of your next turn, you may play those cards. If you cast
/// a spell this way, mana of any type can be spent to cast it": the "if"
/// after a lasting permission to cast is part of it (`PermissionRules`).
/// A cast made as the ability resolves ("You may cast that card without
/// paying its mana cost. If you cast a creature spell this way, …") has
/// happened by the "if", which stays one.
fn permission_rules(v: &mut Vec<Effect>) {
    fn lasting(e: &Effect, during: bool) -> bool {
        match e {
            Effect::Cast { dur, .. } => during || *dur != Duration::Permanent,
            Effect::May { then, .. } => then.iter().any(|e| lasting(e, during)),
            Effect::During { then, .. } => then.iter().any(|e| lasting(e, true)),
            _ => false,
        }
    }
    let mut i = 1;
    while i < v.len() {
        let rule = matches!(
            &v[i],
            Effect::If { cond: Condition::ThisWay(e), .. }
                if matches!(&**e, Event::Casts { who: Player::You, .. })
        );
        if rule && lasting(&v[i - 1], false) {
            let Effect::If {
                cond: Condition::ThisWay(e),
                then,
            } = v.remove(i)
            else {
                unreachable!("matched above");
            };
            let Event::Casts { what, .. } = *e else {
                unreachable!("matched above");
            };
            let permission = Box::new(v[i - 1].clone());
            v[i - 1] = Effect::PermissionRules {
                permission,
                spell: what,
                rules: then,
            };
        } else {
            i += 1;
        }
    }
}

/// Aberrant Return: "Put one, two, or three target creature cards from
/// graveyards onto the battlefield under your control. Each of them enters
/// with an additional -1/-1 counter on it": the counters are a replacement
/// on the move's entering (CR 614.1c), as "return it to the battlefield
/// with a -1/-1 counter on it" is (`EntersWith`), not counters put on after
/// it entered. Only where the sentence after says it of the moved objects
/// and nothing else; "If a Hero enters this way, it enters with …" adds the
/// counters to some of them only, and stays as said.
fn enters_with_after(v: &mut Vec<Effect>) {
    let mut i = 1;
    while i < v.len() {
        let moved = matches!(
            &v[i - 1],
            Effect::Move {
                to: Zone::Battlefield,
                ..
            }
        );
        let counters = matches!(
            &v[i],
            Effect::EntersWithCounters { what, .. } if *what == antecedent()
        );
        if moved && counters {
            let Effect::EntersWithCounters { n, kind, .. } = v.remove(i) else {
                unreachable!("matched above");
            };
            let effect = Box::new(v[i - 1].clone());
            v[i - 1] = Effect::EntersWith { effect, n, kind };
        } else {
            i += 1;
        }
    }
}

/// Ouphe Vandals: "Counter target activated ability from an artifact source
/// and destroy that artifact if it's on the battlefield". "That artifact"
/// is the ability's source (CR 113.7), named by the ability's "from an
/// artifact source", and "it" is that artifact: the ruling has the ability
/// target the ability on the stack, not the artifact. Both read as bare
/// references back, whose nearest object is the ability.
fn source_of_ability(v: &mut [Effect]) {
    let source = |e: &Effect| -> Option<Vec<Adj>> {
        let (Effect::Counter { what, .. } | Effect::Copy { what, .. }) = e else {
            return None;
        };
        what.filter.posts.iter().find_map(|q| match q {
            Post::FromSource(g) => Some(g.clone()),
            _ => None,
        })
    };
    let Some(k) = v.iter().position(|e| source(e).is_some()) else {
        return;
    };
    let types = source(&v[k]).expect("found above");
    let ability = Objects {
        quant: Quant::Antecedent,
        filter: Filter {
            kind: Some(Noun::Ability),
            ..Filter::default()
        },
    };
    let of = |filter: Filter| Objects {
        quant: Quant::SourceOf(Box::new(ability.clone())),
        filter,
    };
    for e in v[k + 1..].iter_mut() {
        let Effect::If { cond, then } = e else {
            continue;
        };
        let [Effect::Destroy { what, .. }] = &mut then[..] else {
            continue;
        };
        let named = what.quant == Quant::Antecedent
            && what.filter
                == Filter {
                    alts: vec![types.clone()],
                    ..Filter::default()
                };
        if !named {
            continue;
        }
        *what = of(what.filter.clone());
        if let Condition::In { what: it, .. } = cond {
            if *it == antecedent() {
                *it = of(Filter::default());
            }
        }
    }
}

/// The token the last effect creates, in the branch it ends with.
fn last_token(e: &mut [Effect]) -> Option<&mut Token> {
    match e.last_mut()? {
        Effect::Create { token, .. } | Effect::CreateAttached { token, .. } => Some(token),
        Effect::May { then, .. } | Effect::IfYouDo(then) | Effect::If { then, .. } => {
            last_token(then)
        }
        _ => None,
    }
}

/// "Exile up to one target artifact you control and up to one target
/// creature you control. Then return them", Rohgahh's "tap ~ and all
/// creatures named Kobolds of Kher Keep, then an opponent gains control of
/// them": after a verb said once for two or more objects, the references
/// back that open the next clause are all of them ([listed-objects]).
fn listed(before: &[Effect], after: &mut [Effect]) {
    struct Them {
        done: bool,
    }
    impl Visitor for Them {
        fn enter(&mut self, o: &mut Objects) -> bool {
            if self.done {
                return false;
            }
            match o.quant {
                Quant::Antecedent => o.quant = Quant::Listed,
                // "one of those cards".
                _ => {
                    self.done = true;
                    for p in &mut o.filter.posts {
                        if let Post::FromAmong(x) = p {
                            if x.quant == Quant::Antecedent {
                                x.quant = Quant::Listed;
                            }
                        }
                    }
                }
            }
            false
        }
    }
    let verb = |e: &Effect| match e {
        Effect::Destroy { .. } => Some(0),
        Effect::Exile(_) => Some(1),
        Effect::Tap(_) => Some(2),
        Effect::Untap(_) => Some(3),
        _ => None,
    };
    // "When this creature dies, exile it. If you do, exile the top card of
    // your library. Until the end of your next turn, you may play one of
    // those cards" (Flameskull): the card the option exiled and the one its
    // branch exiled.
    let branched = match before {
        [.., a, Effect::IfYouDo(then)] => {
            verb(a).is_some() && then.len() == 1 && verb(&then[0]) == verb(a)
        }
        _ => false,
    };
    let Some(last) = before.last().and_then(verb) else {
        if branched {
            if let Some(first) = after.first_mut() {
                first.walk(&mut Them { done: false });
            }
        }
        return;
    };
    if before
        .iter()
        .rev()
        .take_while(|e| verb(e) == Some(last))
        .count()
        < 2
    {
        return;
    }
    let Some(first) = after.first_mut() else {
        return;
    };
    first.walk(&mut Them { done: false });
}

/// "When the creature an opponent controls dies this turn, if you control
/// your commander, you become the monarch": the "if" right after a delayed
/// trigger's condition is an intervening "if" (CR 603.4), as a printed
/// trigger's is.
fn intervening(p: &mut P, event: Event) -> Event {
    match p.attempt(|p| {
        p.word("if")?;
        let cond = condition(p)?;
        p.tok(&Tok::Comma)?;
        Some(cond)
    }) {
        Some(cond) => Event::InterveningIf {
            event: Box::new(event),
            cond: Box::new(cond),
        },
        None => event,
    }
}

/// "unless it's colorless or it's only the color its controller chose":
/// what each object must not be, as a post of the objects.
fn unless_each(p: &mut P) -> Option<Post> {
    p.word("unless")?;
    let mut v = vec![];
    loop {
        p.word("it's")?;
        let q = alt!(p;
            |p| {
                p.words("only the color its controller chose")?;
                Some(Post::OnlyColorChosenBy(Player::ControllerOf(Box::new(Objects {
                    quant: Quant::Itself,
                    filter: Filter::default(),
                }))))
            },
            |p| {
                let a = p.peek_word().and_then(color_word)?;
                p.bump();
                Some(Post::ThatIs(Filter { alts: vec![vec![Adj::Color(a)]], ..Filter::default() }))
            },
        )?;
        v.push(q);
        if p.attempt(|p| p.word("or")).is_none() {
            break;
        }
    }
    // A lone "unless it's colorless" is still read as a condition.
    if v.len() < 2 || !matches!(p.peek(), Some(Tok::Period) | None) {
        return None;
    }
    Some(Post::Not(Box::new(Post::Either(v))))
}
