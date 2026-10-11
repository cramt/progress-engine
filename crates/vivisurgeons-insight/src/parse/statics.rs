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
    let s = alt!(p;
        |p| {
            p.words("as long as")?;
            let cond = condition(p)?;
            p.tok(&Tok::Comma)?;
            let then = static_joined(p)?;
            Some(vec![Static::AsLongAs { cond, then }])
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
        |p| { p.words("deal damage")?; p.attempt(|p| { p.word("to")?; recipients(p) }); Some(Event::DealsDamage(what.clone())) },
        |p| p.words("be dealt damage").map(|_| Event::IsDealtDamage(what.clone())),
        |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); Some(Event::Enters { what: what.clone(), under: None, state: None }) },
    )
}

/// "one or more +1/+1 counters", "one or more counters".
pub(super) fn counters_phrase(p: &mut P) -> Option<(Quant, Option<String>)> {
    let n = quant(p)?;
    let kind = p.attempt(counter_kind);
    p.one_of(&["counters", "counter"])?;
    Some((n, kind))
}

pub(super) fn replacement(p: &mut P) -> Option<Vec<Static>> {
    p.word("if")?;
    let event = alt!(p;
        |p| {
            let by = actor(p)?;
            p.words("would create")?;
            let what = objects(p)?;
            let under = p.attempt(|p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(w) });
            Some(Event::Creates { by, what, under })
        },
        |p| {
            let by = p.attempt(actor);
            let (n, kind, on) = alt!(p;
                |p| { p.words("would put")?; let (n, kind) = counters_phrase(p)?; p.word("on")?; Some((n, kind, objects(p)?)) },
                |p| { let (n, kind) = counters_phrase(p)?; p.words("would be put on")?; Some((n, kind, objects(p)?)) },
            )?;
            Some(Event::CountersPut { by, n, kind, on })
        },
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("would")?;
            alt!(p;
                |p| p.words("draw a card").map(|_| Event::Draws(who.clone())),
                |p| p.words("gain life").map(|_| Event::GainsLife(who.clone())),
                |p| p.words("lose life").map(|_| Event::LosesLife(who.clone())),
            )
        },
        |p| { let what = objects(p)?; p.word("would")?; would(p, &what) },
    )?;
    p.tok(&Tok::Comma)?;
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
    alt!(p;
        |p| p.words("you may cast this spell without paying its mana cost").map(|_| vec![Static::CastWithoutPaying]),
        |p| {
            p.words("if a triggered ability of")?;
            let of = objects(p)?;
            p.words("triggers , it triggers an additional time")?;
            Some(vec![Static::AdditionalTrigger { of, cause: None }])
        },
        |p| {
            p.word("if")?;
            let what = objects(p)?;
            p.word("entering")?;
            p.words("causes a triggered ability of")?;
            let of = objects(p)?;
            p.words("to trigger , that ability triggers an additional time")?;
            Some(vec![Static::AdditionalTrigger { of, cause: Some(Event::Enters { what, under: None, state: None }) }])
        },
        |p| {
            p.words("you may begin the game with")?;
            let what = pronoun(p)?;
            p.words("on the battlefield")?;
            let counters = p.attempt(|p| {
                p.word("with")?;
                let n = amount_a(p)?;
                let kind = counter_kind(p)?;
                p.one_of(&["counters", "counter"])?;
                p.one_of(&["on it", "on them"])?;
                Some((n, kind))
            });
            let then = p.attempt(|p| {
                p.tok(&Tok::Period)?;
                let c = clause(p)?;
                matches!(c.first(), Some(Effect::IfYouDo(_))).then_some(c)
            }).unwrap_or_default();
            Some(vec![Static::BeginOnBattlefield { what, counters, then }])
        },
        |p| {
            let what = objects(p)?;
            p.one_of(&["is", "are"])?;
            let types = alt!(p;
                |p| p.words("the chosen type").map(|_| TypeSet::Chosen),
                |p| p.words("every basic land type").map(|_| TypeSet::EveryBasicLandType),
                |p| { p.attempt(|p| p.one_of(&["a", "an"])); group(p).map(TypeSet::Listed) },
            )?;
            p.one_of(&["in addition to its other types", "in addition to its other land types", "in addition to their other types", "in addition to their other land types"])?;
            Some(vec![Static::AlsoIs { what, types }])
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
            Some(vec![Static::AlternativeCost(c)])
        },
        |p| { p.words("you control")?; objects(p).map(|o| vec![Static::Control(o)]) },
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
        |p| {
            p.words("you may")?;
            let what = alt!(p;
                |p| { p.word("play")?; objects(p) },
                |p| { p.word("cast")?; objects(p) },
            )?;
            let mut what = what;
            // The object's own "from your graveyard" got there first.
            let zone = match what.filter.posts.last() {
                Some(Post::In(z)) => { let z = z.clone(); what.filter.posts.pop(); z }
                _ => { p.word("from")?; zone(p)? }
            };
            Some(vec![Static::PlayFrom { what, zone }])
        },
        |p| {
            let what = objects(p)?;
            let r = alt!(p;
                |p| {
                    p.verb("get")?;
                    let pt = pt_signed(p)?;
                    let per = p.attempt(|p| { p.words("for each")?; objects(p) });
                    let has = p.attempt(|p| { p.word("and")?; p.verb("have")?; keyword_list(p) }).unwrap_or_default();
                    Some(vec![Static::Modify { what: what.clone(), pt: Some(pt), has, per }])
                },
                |p| { p.verb("have")?; let k = keyword_list(p)?; Some(vec![Static::Modify { what: what.clone(), pt: None, has: k, per: None }]) },
                |p| {
                    p.verb("enter")?;
                    p.attempt(|p| p.words("the battlefield"));
                    p.words("tapped unless")?;
                    let unless = condition(p)?;
                    Some(vec![Static::EntersTappedUnless { what: what.clone(), unless }])
                },
                |p| { p.verb("have")?; let a = quoted(p)?; Some(vec![Static::HasAbility { what: what.clone(), ability: Box::new(a) }]) },
                |p| {
                    let rule = restriction(p)?;
                    let mut v = vec![Static::Restrict { what: what.clone(), rule }];
                    // "can't be blocked and has shroud".
                    if let Some(has) = p.attempt(|p| { p.word("and")?; p.verb("have")?; keyword_list(p) }) {
                        v.push(Static::Modify { what: what.clone(), pt: None, has, per: None });
                    }
                    Some(v)
                },
                |p| { p.verb("enter")?; p.attempt(|p| p.words("the battlefield")); p.word("tapped")?; Some(vec![Static::EntersTapped(what.clone())]) },
                |p| {
                    p.verb("enter")?;
                    p.attempt(|p| p.words("the battlefield"));
                    p.word("with")?;
                    let n = amount_a(p)?;
                    let kind = counter_kind(p)?;
                    p.one_of(&["counters", "counter"])?;
                    p.one_of(&["on it", "on them"])?;
                    // "a charge counter on it for each time it was kicked".
                    let n = match p.attempt(|p| p.words("for each time it was kicked")) {
                        Some(_) if n == Amount::N(1) => Amount::TimesKicked,
                        Some(_) => return None,
                        None => n,
                    };
                    Some(vec![Static::EntersWithCounters { what: what.clone(), n, kind }])
                },
                |p| {
                    p.verb("cost")?;
                    let by = mana(p)?;
                    let less = p.one_of(&["less", "more"])? == "less";
                    p.one_of(&["to cast", "to activate"])?;
                    let per = p.attempt(|p| { p.words("for each")?; objects(p) });
                    let change = Static::CostChange { what: what.clone(), by, less, per };
                    Some(vec![match p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                        Some(x) => Static::Where { inner: Box::new(change), x },
                        None => change,
                    }])
                },
            )?;
            Some(r)
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

pub(super) fn statics(p: &mut P) -> Option<Vec<Static>> {
    let mut v = Vec::new();
    while let Some(s) = p.attempt(static_sentence) {
        v.extend(s);
    }
    (!v.is_empty()).then_some(v)
}

/// What a player can't do: "can't cast spells", "can't activate abilities of …".
pub(super) fn player_rule(p: &mut P) -> Option<PlayerRule> {
    p.word("can't")?;
    player_rule_bare(p)
}

pub(super) fn player_rule_bare(p: &mut P) -> Option<PlayerRule> {
    alt!(p;
        |p| { p.word("cast")?; objects(p).map(PlayerRule::CantCast) },
        |p| { p.words("activate abilities of")?; objects(p).map(PlayerRule::CantActivateAbilitiesOf) },
    )
}
