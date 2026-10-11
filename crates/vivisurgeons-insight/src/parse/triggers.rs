//! Trigger conditions: steps and events.

use super::*;

pub(super) fn whose_step(p: &mut P) -> Option<Whose> {
    alt!(p;
        |p| p.words("your next").map(|_| Whose::YourNext),
        |p| p.word("your").map(|_| Whose::Yours),
        |p| p.words("each opponent 's").map(|_| Whose::EachOpponent),
        |p| p.words("each other player 's").map(|_| Whose::EachOther),
        |p| p.words("each player 's").map(|_| Whose::Each),
        |p| p.word("each").map(|_| Whose::Each),
        |p| p.words("that player 's").map(|_| Whose::ThatPlayers),
        |p| p.word("that").map(|_| Whose::That),
        |p| p.words("the next turn 's").map(|_| Whose::The),
        |p| p.words("the next").map(|_| Whose::The),
        |p| p.word("the").map(|_| Whose::The),
    )
}

pub(super) fn step(p: &mut P) -> Option<Step> {
    let w = p.attempt(whose_step);
    let named = w.is_some();
    let w = w.unwrap_or(Whose::The);
    let s = alt!(p;
        |p| p.words("untap step").map(|_| Step::Untap(w.clone())),
        |p| p.word("upkeep").map(|_| Step::Upkeep(w.clone())),
        |p| p.words("draw step").map(|_| Step::DrawStep(w.clone())),
        |p| p.one_of(&["precombat main phase", "first main phase"]).map(|_| Step::PrecombatMain(w.clone())),
        |p| p.one_of(&["postcombat main phase", "second main phase"]).map(|_| Step::SecondMain(w.clone())),
        |p| p.words("main phase").map(|_| Step::Main(w.clone())),
        |p| p.words("end step").map(|_| Step::EndStep(w.clone())),
        |p| p.words("end of combat").map(|_| Step::EndOfCombat(w.clone())),
        |p| p.word("combat").map(|_| Step::Combat(w.clone())),
    )?;
    // "At the beginning of combat on your turn": the turn says whose.
    if p.attempt(|p| p.words("on your turn")).is_some() {
        return (!named || w == Whose::The).then(|| step_of(s, Whose::Yours));
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
        Step::Main(_) => Step::Main(w),
        Step::Untap(_) => Step::Untap(w),
    }
}

/// A player's verb in a trigger that takes an object: "you cast", "you copy".
pub(super) fn player_verb(p: &mut P, who: &Player) -> Option<Box<dyn Fn(Objects) -> Event>> {
    let who = who.clone();
    alt!(p;
        |p| { p.verb("cast")?; let w = who.clone(); Some(Box::new(move |what| Event::Casts { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("copy")?; let w = who.clone(); Some(Box::new(move |what| Event::Copies { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("create")?; let w = who.clone(); Some(Box::new(move |what| Event::Creates { by: Actor::Player(w.clone()), what, under: None }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("sacrifice")?; let w = who.clone(); Some(Box::new(move |what| Event::Sacrifices { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
        |p| { p.verb("discard")?; let w = who.clone(); Some(Box::new(move |what| Event::Discards { who: w.clone(), what }) as Box<dyn Fn(Objects) -> Event>) },
    )
}

/// One of several things a player does in a trigger: "attacks you with two
/// or more creatures", "draws their second card each turn".
fn player_predicate(p: &mut P, who: &Player) -> Option<Event> {
    alt!(p;
        |p| {
            p.verb("attack")?;
            let at = p.attempt(|p| alt!(p; player_simple, |p| p.word("you").map(|_| Player::You)));
            p.word("with")?;
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
        |p| { let v = player_verb(p, who)?; objects(p).map(v) },
    )
}

pub(super) fn single_event(p: &mut P) -> Option<Event> {
    alt!(p;
        |p| p.words("you attack").map(|_| Event::YouAttack),
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
            if v.len() == 1 && !matches!(v[0], Event::AttacksWith { .. } | Event::PlaysLand(_) | Event::TapsFor { .. } | Event::CountersPut { .. } | Event::Puts { .. } | Event::Exiles { .. }) {
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
            p.words("card each turn")?;
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
        |p| { let who = player(p)?; p.verb("cast")?; let what = objects(p)?; Some(Event::Casts { who, what }) },
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
        |p| { p.verb("leave")?; let from = zone(p)?; Some(Event::Leaves { what: what.clone(), from }) },
        |p| p.one_of(&["is countered", "are countered"]).map(|_| Event::Countered(what.clone())),
        |p| p.verb("attack or block").map(|_| Event::AttacksOrBlocks(what.clone())),
        |p| p.verb("attack").map(|_| Event::Attacks(what.clone())),
        |p| p.verb("block").map(|_| Event::Blocks(what.clone())),
        |p| p.one_of(&["becomes blocked", "become blocked"]).map(|_| Event::BecomesBlocked(what.clone())),
        |p| { p.verb("deal")?; p.words("combat damage to")?; p.one_of(&["a player or planeswalker", "a player", "an opponent", "one or more players"])?; Some(Event::DealsCombatDamageToPlayer(what.clone())) },
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
            // "When this creature enters and whenever an opponent draws …":
            // two triggers for one effect, which is either event.
            Some(Trigger::When(Box::new(match p.attempt(|p| { p.word("and")?; p.one_of(&["whenever", "when"])?; event(p) }) {
                Some(more) => Event::Or(vec![e, more]),
                None => e,
            })))
        },
    )
}
