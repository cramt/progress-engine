//! Trigger conditions: steps and events.

use super::*;

pub(super) fn whose_step(p: &mut P) -> Option<Whose> {
    alt!(p;
        |p| p.words("your next").map(|_| Whose::YourNext),
        // "your first upkeep": of the game. "Your first main phase" is the
        // precombat one of each turn (CR 505.1a), read by `step`.
        |p| { p.words("your first")?; (p.peek_word() != Some("main")).then_some(Whose::YourFirst) },
        |p| p.word("your").map(|_| Whose::Yours),
        |p| p.words("each opponent 's").map(|_| Whose::EachOpponent),
        |p| p.words("each other player 's").map(|_| Whose::EachOther),
        |p| p.words("each player 's").map(|_| Whose::Each),
        |p| p.word("each").map(|_| Whose::Each),
        |p| p.words("that player 's").map(|_| Whose::ThatPlayers),
        |p| p.word("that").map(|_| Whose::That),
        |p| p.words("the next turn 's").map(|_| Whose::NextTurns),
        |p| p.words("the next").map(|_| Whose::Next),
        |p| p.word("the").map(|_| Whose::The),
    )
}

pub(super) fn step(p: &mut P) -> Option<Step> {
    // "each of your main phases": both of them (CR 505.1).
    if p.attempt(|p| p.words("each of your main phases")).is_some() {
        return Some(Step::Main(Whose::Yours));
    }
    let w = p.attempt(whose_step);
    let named = w.is_some();
    let w = w.unwrap_or(Whose::The);
    let s = alt!(p;
        |p| p.words("untap step").map(|_| Step::Untap(w.clone())),
        |p| p.word("upkeep").map(|_| Step::Upkeep(w.clone())),
        |p| p.words("draw step").map(|_| Step::DrawStep(w.clone())),
        |p| p.one_of(&["precombat main phase", "first main phase"]).map(|_| Step::PrecombatMain(w.clone())),
        |p| p.words("postcombat main phase").map(|_| Step::PostcombatMain(w.clone())),
        |p| p.words("second main phase").map(|_| Step::SecondMain(w.clone())),
        |p| p.words("main phase").map(|_| Step::Main(w.clone())),
        |p| p.words("end step").map(|_| Step::EndStep(w.clone())),
        |p| p.words("end of combat").map(|_| Step::EndOfCombat(w.clone())),
        |p| p.word("combat").map(|_| Step::Combat(w.clone())),
    )?;
    // "At the beginning of combat on your turn": the turn says whose.
    if p.attempt(|p| p.words("on your turn")).is_some() {
        return (!named || w == Whose::The).then(|| step_of(s, Whose::Yours));
    }
    if p.attempt(|p| p.words("on each opponent 's turn")).is_some() {
        return (!named && matches!(s, Step::Combat(_))).then(|| step_of(s, Whose::EachOpponent));
    }
    named.then_some(s)
}

pub(super) fn step_of(s: Step, w: Whose) -> Step {
    match s {
        Step::Upkeep(_) => Step::Upkeep(w),
        Step::DrawStep(_) => Step::DrawStep(w),
        Step::PrecombatMain(_) => Step::PrecombatMain(w),
        Step::Combat(_) => Step::Combat(w),
        Step::EndStep(_) => Step::EndStep(w),
        Step::EndOfCombat(_) => Step::EndOfCombat(w),
        Step::SecondMain(_) => Step::SecondMain(w),
        Step::PostcombatMain(_) => Step::PostcombatMain(w),
        Step::Main(_) => Step::Main(w),
        Step::Untap(_) => Step::Untap(w),
    }
}

/// A player's verb in a trigger that takes an object: "you cast", "you copy".
pub(super) fn player_verb(p: &mut P, who: &Player) -> Option<Box<dyn Fn(Objects) -> Event>> {
    let who = who.clone();
    alt!(p;
        |p| { p.verb("cast")?; let w = who.clone(); Some(Box::new(move |what| casts(w.clone(), what)) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("copy")?; let w = who.clone(); Some(Box::new(move |what| Event::Copies { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("create")?; let w = who.clone(); Some(Box::new(move |what| Event::Creates { by: Actor::Player(w.clone()), what, under: None }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("sacrifice")?; let w = who.clone(); Some(Box::new(move |what| Event::Sacrifices { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("discard")?; let w = who.clone(); Some(Box::new(move |what| Event::Discards { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
        // "you cycle or discard another card".
        |p| { p.verb("cycle")?; let w = who.clone(); Some(Box::new(move |what| Event::Cycles { who: w.clone(), what: Some(what) }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("activate")?; let w = who.clone(); Some(Box::new(move |what| Event::Activates { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
    )
}

/// One of several things a player does in a trigger: "attacks you with two
/// or more creatures", "draws their second card each turn".
fn player_predicate(p: &mut P, who: &Player) -> Option<Event> {
    alt!(p;
        |p| {
            p.verb("attack")?;
            let at = p.attempt(|p| alt!(p; player_simple, |p| p.word("you").map(|_| Player::You)));
            if p.attempt(|p| p.word("with")).is_none() {
                // "attacks you and/or one or more planeswalkers you control" is
                // read by the rule for attacked objects.
                let ends = matches!(p.peek(), Some(Tok::Comma));
                return (*who != Player::You && ends).then(|| Event::PlayerAttacks { who: who.clone(), at });
            }
            let what = objects(p)?;
            Some(Event::AttacksWith { who: who.clone(), what, at })
        },
        |p| {
            p.verb("draw")?;
            whose_pronoun(p)?;
            let n = ordinal(p)?;
            p.words("card each turn")?;
            Some(Event::DrawsNth { who: who.clone(), n })
        },
        |p| { p.verb("play")?; p.words("a land")?; Some(Event::PlaysLand(who.clone())) },
        |p| {
            p.verb("tap")?;
            let what = objects(p)?;
            p.word("for")?;
            let mana = mana(p)?;
            Some(Event::TapsFor { who: who.clone(), what, mana })
        },
        |p| {
            p.verb("put")?;
            let (n, kind) = counters_phrase(p)?;
            p.word("on")?;
            let on = objects(p)?;
            Some(Event::CountersPut { by: Some(Actor::Player(who.clone())), n, kind, on })
        },
        |p| {
            p.verb("put")?;
            let what = objects(p)?;
            let to = put_to(p)?;
            Some(Event::Puts { who: who.clone(), what, to: to.zone() })
        },
        |p| { p.verb("exile")?; let what = objects(p)?; Some(Event::Exiles { who: who.clone(), what }) },
        // "Whenever you scry or surveil": a keyword action with no object.
        |p| {
            let action = ["scry", "surveil"].into_iter().find(|a| p.verb(a).is_some())?;
            Some(Event::Performs { who: who.clone(), action: action.into() })
        },
        |p| { let v = player_verb(p, who)?; objects(p).map(v) },
    )
}

pub(super) fn single_event(p: &mut P) -> Option<Event> {
    alt!(p;
        // "Whenever you attack with two or more creatures" is the player's
        // attack with objects, read below.
        |p| { p.words("you attack")?; (p.peek_word() != Some("with")).then_some(Event::YouAttack) },
        // "an opponent attacks you with two or more creatures, draws their
        // second card each turn, or casts their second spell each turn": one
        // player, a list of verbs.
        |p| {
            let who = player(p)?;
            let mut v = vec![player_predicate(p, &who)?];
            // Each item with where it started and whether "or" led it: a
            // list ends in "or", so a trailing bare comma ends the trigger.
            let mut items = Vec::new();
            while let Some(item) = p.attempt(|p| {
                let start = p.i;
                let sep = p.one_of(&[", or", ",", "or"])?;
                Some((start, sep != ",", player_predicate(p, &who)?))
            }) {
                items.push(item);
            }
            while let Some(&(start, false, _)) = items.last() {
                p.i = start;
                items.pop();
            }
            v.extend(items.into_iter().map(|(_, _, e)| e));
            // A lone verb the rules below read is theirs to read.
            if v.len() == 1 && !matches!(v[0], Event::AttacksWith { .. } | Event::PlayerAttacks { .. } | Event::PlaysLand(_) | Event::TapsFor { .. } | Event::CountersPut { .. } | Event::Puts { .. } | Event::Exiles { .. } | Event::Performs { .. }) {
                return None;
            }
            Some(if v.len() == 1 { v.remove(0) } else { Event::Or(v) })
        },
        |p| p.words("enchanted player is attacked").map(|_| Event::IsAttacked(Player::Enchanted)),
        |p| {
            p.word("the")?;
            let n = ordinal(p)?;
            let kind = counter_kind(p)?;
            p.words("counter is put on")?;
            let on = objects(p)?;
            Some(Event::NthCounterPutOn { n, kind, on })
        },
        |p| {
            let (n, kind) = counters_phrase(p)?;
            p.one_of(&["are put on", "is put on"])?;
            let on = objects(p)?;
            Some(Event::CountersPut { by: None, n, kind, on })
        },
        |p| {
            let of = objects_no_player(p)?;
            p.tok(&Tok::Possessive)?;
            p.words("ability causes")?;
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.words("to add one or more mana of")?;
            let pal = palette_named(p)?;
            Some(Event::AddsMana { who, by_ability_of: of, of: pal })
        },
        |p| { p.words("that mana is spent to cast")?; objects(p).map(Event::ManaSpentToCast) },
        |p| {
            let who = player(p)?;
            p.verb("draw")?;
            whose_pronoun(p)?;
            let n = ["first", "second", "third"].iter().position(|w| p.word(w).is_some())? as u32 + 1;
            // "your third card in a turn": the same count as "each turn".
            p.one_of(&["card each turn", "card in a turn"])?;
            Some(Event::DrawsNth { who, n })
        },
        // "you cast or copy an instant or sorcery spell": two verbs, one object.
        |p| {
            let who = player(p)?;
            let first = player_verb(p, &who)?;
            let mut verbs = vec![first];
            while let Some(v) = p.attempt(|p| { p.word("or")?; player_verb(p, &who) }) {
                verbs.push(v);
            }
            let what = objects(p)?;
            Some(match verbs.len() {
                1 => verbs[0](what),
                _ => Event::Or(verbs.iter().map(|v| v(what.clone())).collect()),
            })
        },
        |p| { let who = player(p)?; p.verb("cast")?; let what = objects(p)?; Some(casts(who, what)) },
        |p| { let who = player(p)?; p.verb("gain")?; p.word("life")?; Some(Event::GainsLife(who)) },
        |p| { let who = player(p)?; p.verb("lose")?; p.word("life")?; Some(Event::LosesLife(who)) },
        |p| {
            let who = player(p)?;
            p.verb("draw")?;
            p.word("a")?;
            p.words("card except the first one")?;
            p.one_of(&["they draw in each of their draw steps", "you draw in each of your draw steps"])?;
            Some(Event::DrawsExceptFirstInDrawStep(who))
        },
        |p| { let who = player(p)?; p.verb("draw")?; p.words("a card")?; Some(Event::Draws(who)) },
        |p| {
            let who = player(p)?;
            p.verb("search")?;
            let w = whose(p)?;
            p.word("library")?;
            Some(Event::Searches { who, zone: Zone::Library(w) })
        },
        |p| { let who = player(p)?; p.verb("discard")?; let what = objects(p)?; Some(Event::Discards { who, what }) },
        |p| { let who = player(p)?; p.verb("cycle")?; let what = p.attempt(objects); Some(Event::Cycles { who, what }) },
        |p| { let who = player(p)?; p.verb("sacrifice")?; let what = objects(p)?; Some(Event::Sacrifices { who, what }) },
        |p| { let who = player(p)?; p.verb("tap")?; let what = objects(p)?; p.words("for mana")?; Some(Event::TapsForMana { who, what }) },
        |p| {
            // "enters or attacks": one subject, several predicates.
            let what = objects(p)?;
            let first = predicate(p, &what)?;
            let mut v = vec![first];
            while let Some(e) = p.attempt(|p| { p.word("or")?; predicate(p, &what) }) {
                v.push(e);
            }
            Some(if v.len() == 1 { v.remove(0) } else { Event::Or(v) })
        },
    )
}

pub(super) fn predicate(p: &mut P, what: &Objects) -> Option<Event> {
    alt!(p;
        // "a creature attacks you or a planeswalker you control": before a
        // bare "attacks" takes the verb.
        |p| {
            p.verb("attack")?;
            let mut to = vec![recipient(p)?];
            while let Some(r) = p.attempt(|p| { p.one_of(&["or", "and/or"])?; recipient(p) }) { to.push(r); }
            Some(Event::AttacksAnyOf { what: what.clone(), to })
        },
        |p| {
            let e = alt!(p;
                |p| p.verb("attack").map(|_| Event::Attacks(what.clone())),
                |p| p.verb("block").map(|_| Event::Blocks(what.clone())),
            )?;
            p.words("for the first time each turn")?;
            Some(Event::FirstTimeEachTurn(Box::new(e)))
        },
        |p| {
            p.verb("enter")?;
            p.attempt(|p| p.words("the battlefield"));
            let under = p.attempt(|p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(w) });
            let state = p.attempt(|p| alt!(p;
                |p| p.word("untapped").map(|_| State::Untapped),
                |p| p.word("tapped").map(|_| State::Tapped),
            ));
            Some(Event::Enters { what: what.clone(), under, state })
        },
        |p| p.verb("die").map(|_| Event::Dies(what.clone())),
        |p| { p.verb("attack")?; p.word("alone")?; Some(Event::AttacksAlone(what.clone())) },
        |p| {
            p.verb("attack")?;
            let at = p.attempt(recipient);
            p.words("and isn't blocked")?;
            Some(Event::AttacksUnblocked { what: what.clone(), at })
        },
        |p| { p.verb("transform")?; p.word("into")?; let into = objects(p)?; Some(Event::TransformsInto { what: what.clone(), into }) },
        |p| { p.verb("leave")?; let from = zone(p)?; Some(Event::Leaves { what: what.clone(), from }) },
        |p| p.one_of(&["is countered", "are countered"]).map(|_| Event::Countered(what.clone())),
        |p| p.verb("attack or block").map(|_| Event::AttacksOrBlocks(what.clone())),
        |p| p.verb("attack").map(|_| Event::Attacks(what.clone())),
        |p| p.verb("block").map(|_| Event::Blocks(what.clone())),
        |p| p.one_of(&["becomes blocked", "become blocked"]).map(|_| Event::BecomesBlocked(what.clone())),
        |p| {
            p.verb("deal")?;
            p.words("combat damage to")?;
            let to = match p.one_of(&["a player or planeswalker", "a player", "an opponent", "one or more players"])? {
                // "a player or battle" is the recipient rule's.
                "a player" if p.attempt(|p| p.words("or battle")).is_some() => return None,
                "a player" => return Some(Event::DealsCombatDamageToPlayer(what.clone())),
                "an opponent" => Recipient::Player(Player::AnOpponent),
                "one or more players" => Recipient::Objects(Objects { quant: Quant::OneOrMore, filter: Filter { kind: Some(Noun::Player), ..Filter::default() } }),
                _ => Recipient::Or(vec![
                    Recipient::Player(Player::APlayer),
                    Recipient::Objects(Objects { quant: Quant::One, filter: Filter { alts: vec![vec![Adj::Type("planeswalker".into())]], ..Filter::default() } }),
                ]),
            };
            Some(Event::DealsCombatDamageTo { what: what.clone(), to })
        },
        // "deals combat damage to a creature", "… to a player or battle".
        |p| { p.verb("deal")?; p.words("combat damage to")?; let to = recipient(p)?; Some(Event::DealsCombatDamageTo { what: what.clone(), to }) },
        // "Whenever equipped creature deals combat damage, …": to anything.
        |p| { p.verb("deal")?; p.words("combat damage")?; (p.peek() == Some(&Tok::Comma)).then(|| Event::DealsCombatDamage(what.clone())) },
        |p| { p.one_of(&["is exiled", "are exiled"])?; Some(Event::Exiled(what.clone())) },
        |p| { p.one_of(&["is returned to hand", "are returned to hand"])?; Some(Event::ReturnedToHand(what.clone())) },
        |p| { p.verb("deal")?; p.words("damage to")?; let to = recipients(p)?; Some(Event::DealsDamageTo { what: what.clone(), to }) },
        |p| { p.verb("deal")?; p.word("damage")?; Some(Event::DealsDamage(what.clone())) },
        |p| { p.one_of(&["becomes level", "become level"])?; let n = number(p)?; Some(Event::BecomesLevel { what: what.clone(), n }) },
        |p| p.one_of(&["is dealt damage", "are dealt damage"]).map(|_| Event::IsDealtDamage(what.clone())),
        |p| p.one_of(&["becomes tapped", "become tapped"]).map(|_| Event::BecomesTapped(what.clone())),
        |p| p.one_of(&["is tapped for mana", "are tapped for mana"]).map(|_| Event::TappedForMana(what.clone())),
        |p| { p.one_of(&["is put into", "are put into"])?; let to = zone(p)?; p.word("from")?; let from = zone(p)?; Some(Event::PutInto { what: what.clone(), to, from }) },
        // "is put into exile" with no "from": from any zone, as "from anywhere" says.
        |p| { p.one_of(&["is put into", "are put into"])?; let to = zone(p)?; Some(Event::PutInto { what: what.clone(), to, from: Zone::Anywhere }) },
        |p| { p.one_of(&["becomes the target of", "become the target of"])?; let of = p.attempt(objects); Some(Event::BecomesTarget { what: what.clone(), of }) },
    )
}

pub(super) fn event(p: &mut P) -> Option<Event> {
    let first = single_event(p)?;
    let mut rest = Vec::new();
    // "Whenever another creature dies, or a creature card is put into …".
    while let Some(e) = p.attempt(|p| {
        p.attempt(|p| p.tok(&Tok::Comma));
        p.word("or")?;
        single_event(p)
    }) {
        rest.push(e);
    }
    if rest.is_empty() {
        Some(first)
    } else {
        let mut v = vec![first];
        v.extend(rest);
        Some(Event::Or(v))
    }
}

pub(super) fn trigger(p: &mut P) -> Option<Trigger> {
    alt!(p;
        |p| { p.words("at the beginning of")?; step(p).map(Trigger::At) },
        |p| {
            p.one_of(&["whenever", "when"])?;
            let e = event(p)?;
            // "Whenever you gain life for the first time each turn".
            let e = match p.attempt(|p| p.words("for the first time each turn")) {
                Some(_) => Event::FirstTimeEachTurn(Box::new(e)),
                None => e,
            };
            // "Whenever you cast an instant or sorcery spell during your turn",
            // "… while this creature has a -1/-1 counter on it": the event,
            // only while the condition holds.
            let e = match p.attempt(|p| alt!(p;
                |p| p.words("during your turn").map(|_| Condition::ItsYourTurn),
                |p| p.words("during an opponent 's turn").map(|_| Condition::OpponentsTurn),
                // "Whenever this creature attacks while saddled": the
                // attacker is saddled (CR 702.171b).
                |p| {
                    p.words("while saddled")?;
                    let Event::Attacks(x) = &e else { return None };
                    let f = Filter { alts: vec![vec![Adj::State(State::Saddled)]], ..Filter::default() };
                    Some(Condition::Is { what: x.clone(), filter: f })
                },
                |p| { p.word("while")?; condition(p) },
            )) {
                Some(cond) => Event::While { event: Box::new(e), cond: Box::new(cond) },
                None => e,
            };
            // "When this creature enters and whenever an opponent draws …":
            // two trigger conditions for one effect (CR 603.1b), which is
            // one trigger on either event.
            // "When this artifact enters and at the beginning of your upkeep".
            if let Some(s) = p.attempt(|p| { p.words("and at the beginning of")?; step(p) }) {
                return Some(Trigger::WhenAndAt(Box::new(e), s));
            }
            Some(Trigger::When(Box::new(match p.attempt(|p| { p.word("and")?; p.one_of(&["whenever", "when"])?; event(p) }) {
                Some(more) => Event::Or(vec![e, more]),
                None => e,
            })))
        },
    )
}
