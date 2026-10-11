//! Effects joined into clauses and sentences, with what may lead or trail them.

use super::*;

/// What can trail an effect and qualify it: "unless that player pays {1}",
/// "for each creature you control", "where X is …", "if …", a delayed step.
pub(super) fn suffixed(p: &mut P, mut e: Effect) -> Effect {
    loop {
        if let Some((who, cost)) = p.attempt(|p| {
            p.word("unless")?;
            let who = player(p)?;
            Some((who, unless_costs(p)?))
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
        // "Until end of turn, whenever a player taps an Island for mana, …".
        |p| {
            p.word("whenever")?;
            let event = event(p)?;
            p.tok(&Tok::Comma)?;
            let mut then = clauses(p)?;
            // "…, copy it. You may choose new targets for the copy.": the
            // copy's own option, so the trigger's too.
            if let Some(more) = p.attempt(|p| {
                p.tok(&Tok::Period)?;
                let c = clause(p)?;
                matches!(&c[..], [Effect::May { then, .. }] if matches!(&then[..], [Effect::ChooseNewTargets(o)] if o.filter.kind == Some(Noun::Copy))).then_some(c)
            }) {
                then.extend(more);
            }
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
        // "becomes X/X" sets.
        |p| {
            let what = objects(p)?;
            p.verb("have")?;
            p.words("base power and toughness")?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            let also = p.attempt(|p| { p.word("and")?; p.verb("gain")?; p.words("all creature types") }).is_some();
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let mut v = vec![Effect::Becomes { what: what.clone(), pt: Some(pt), types: vec![], gains: vec![], keeps: Keeps::Nothing, dur: dur.clone() }];
            if also {
                v.push(Effect::AlsoIs { what, types: TypeSet::EveryCreatureType, dur });
            }
            Some(v)
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
        |p| {
            p.one_of(&["if you do", "if they do", "if that player does", "if the player does"])?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::IfYouDo(clauses(p)?)])
        },
        |p| {
            p.words("when you do")?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::WhenYouDo(clauses(p)?)])
        },
        |p| {
            p.word("when")?;
            let event = event(p)?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::DelayedWhen { event, then: clauses(p)? }])
        },
        |p| {
            p.one_of(&["if you don't", "if they don't", "if that player doesn't", "if the player doesn't", "if no one does", "otherwise"])?;
            p.tok(&Tok::Comma)?;
            Some(vec![Effect::IfYouDont(clauses(p)?)])
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
        |p| { p.words("at the beginning of")?; let step = step(p)?; p.tok(&Tok::Comma)?; let then = clauses(p)?; Some(vec![Effect::Delayed { step, then }]) },
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
                let mut then = vec![suffixed(p, e)];
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
        | Effect::Mill { who, .. }
        | Effect::Reveal { who, .. }
        | Effect::RevealTop { who, .. }
        | Effect::LookAtTop { who, .. }
        | Effect::Sacrifice { who, .. }
        | Effect::Create { who, .. }
        | Effect::Pay { who, .. }
        | Effect::May { who, .. }
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
        p.words(", you may choose")?;
        let to = alt!(p;
            |p| p.word("both").map(|_| Choose::N(2)),
            |p| number(p).map(Choose::N),
        )?;
        p.words("instead .")?;
        Some((cond, to))
    }) {
        Some((cond, to)) => Choose::Upgrade {
            base: Box::new(choose),
            cond: Box::new(cond),
            to: Box::new(to),
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
        |p| { let e = clauses(p)?; p.end_sentence()?; Some(e) },
    )
}

pub fn effects(p: &mut P) -> Option<Vec<Effect>> {
    let mut v = Vec::new();
    while let Some(s) = p.attempt(sentence) {
        v.extend(s);
    }
    (!v.is_empty()).then_some(v)
}
