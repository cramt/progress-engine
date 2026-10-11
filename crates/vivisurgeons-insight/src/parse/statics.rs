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
        |p| p.words("leave the battlefield").map(|_| Event::Leaves { what: what.clone(), from: Zone::Battlefield }),
        |p| { p.words("deal damage to")?; let to = recipients(p)?; Some(Event::DealsDamageTo { what: what.clone(), to }) },
        |p| { p.words("deal noncombat damage to")?; let to = recipients(p)?; Some(Event::DealsNoncombatDamageTo { what: what.clone(), to }) },
        |p| p.words("deal damage").map(|_| Event::DealsDamage(what.clone())),
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
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("would")?;
            alt!(p;
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
    p.tok(&Tok::Comma)?;
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
        // "Creatures you control are every creature type": with no "in
        // addition", the new types replace the old (CR 205.1b).
        |p| {
            let what = objects(p)?;
            p.one_of(&["is", "are"])?;
            let types = alt!(p;
                |p| p.words("every creature type").map(|_| TypeSet::EveryCreatureType),
                |p| p.words("every basic land type").map(|_| TypeSet::EveryBasicLandType),
                |p| {
                    p.attempt(|p| p.one_of(&["a", "an"]));
                    // Only what a card's type line and color say: "is
                    // attacking" is a condition, not a static.
                    let g = group(p).filter(|g| g.iter().all(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_) | Adj::Supertype(_) | Adj::Color(_))))?;
                    Some(TypeSet::Listed(g))
                },
            )?;
            // "is a colorless land with "…"": the predicate list reads what
            // the new object has.
            if p.peek_word() == Some("with") { return None; }
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
            p.words("as though they had flash")?;
            Some(vec![Static::CastAsThoughFlash(what)])
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
        // "If you casting or copying an instant or sorcery spell causes a
        // triggered ability of a permanent you control to trigger, …".
        |p| {
            p.word("if")?;
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            type Verb = fn(Player, Objects) -> Event;
            let gerund = |p: &mut P| alt!(p;
                |p| p.word("casting").map(|_| (|who, what| Event::Casts { who, what }) as Verb),
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

/// What a static says of its subject, read once for each predicate.
fn predicate_static(p: &mut P, what: &Objects) -> Option<Vec<Static>> {
    alt!(p;
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
            let has = p.attempt(|p| { p.word("and")?; p.verb("have")?; keyword_list(p) }).unwrap_or_default();
            let modify = Static::Modify { what: what.clone(), pt: Some(pt), has, per };
            // "This creature gets -X/-X, where X is your life total."
            if let Some(x) = p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                return Some(vec![Static::Where { inner: Box::new(modify), x }]);
            }
            let mut v = vec![modify];
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
            Some(vec![match p.attempt(|p| { p.words(", where x is")?; measure(p) }) {
                Some(x) => Static::Where { inner: Box::new(modify), x },
                None => modify,
            }])
        },
        |p| { p.one_of(&["is", "are"])?; p.words("all colors")?; Some(vec![Static::IsAllColors(what.clone())]) },
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
            p.one_of(&["on it", "on them"])?;
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
                    |p| objects(p).map(|o| Amount::CountOf(Box::new(o))),
                )
            });
            let n = match each {
                Some(each) if n == Amount::N(1) => each,
                Some(_) => return None,
                None => n,
            };
            Some(vec![Static::EntersWithCounters { what: what.clone(), n, kind }])
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
            p.one_of(&["in addition to its other types", "in addition to their other types"])?;
            Some(vec![Static::AlsoIs { what: what.clone(), types: TypeSet::Listed(types) }])
        },
        // "is an Insect artifact creature with base power and toughness 0/1",
        // "is a colorless land with "{T}: Add {C}"": types that replace the
        // old, and what the new object has.
        |p| {
            p.one_of(&["is", "are"])?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let types = group(p).filter(|g| g.iter().all(|a| matches!(a, Adj::Type(_) | Adj::Subtype(_) | Adj::Supertype(_) | Adj::Color(_))))?;
            let mut v = vec![Static::Is { what: what.clone(), types: TypeSet::Listed(types) }];
            if p.attempt(|p| p.word("with")).is_some() {
                v.push(alt!(p;
                    |p| {
                        p.words("base power and toughness")?;
                        let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
                        p.bump();
                        Some(Static::BasePt { what: what.clone(), pt })
                    },
                    |p| quoted(p).map(|a| Static::HasAbility { what: what.clone(), ability: Box::new(a) }),
                )?);
            }
            Some(v)
        },
    )
}

/// What "loses" takes: "flying", "all abilities", "all other card types and
/// abilities". "Other" is said once for the whole list.
fn lost(p: &mut P) -> Option<Vec<Lost>> {
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
