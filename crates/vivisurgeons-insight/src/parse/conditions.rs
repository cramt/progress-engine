//! Conditions: what "if", "unless" and "as long as" test.

use super::*;

pub(super) fn holding(p: &mut P) -> Option<Holding> {
    alt!(p;
        |p| p.word("life").map(|_| Holding::Life),
        |p| p.one_of(&["cards in hand", "cards in their hand", "cards in your hand"]).map(|_| Holding::CardsInHand),
        |p| p.one_of(&["cards in your graveyard", "cards in their graveyard"]).map(|_| Holding::CardsInGraveyard),
        |p| p.word("opponents").map(|_| Holding::Opponents),
    )
}

pub(super) fn condition(p: &mut P) -> Option<Condition> {
    let first = condition_one(p)?;
    // "this card is in your opening hand and you're not the starting player".
    let mut v = vec![first];
    while let Some(c) = p.attempt(|p| {
        p.word("and")?;
        condition_one(p)
    }) {
        v.push(c);
    }
    let c = if v.len() == 1 {
        v.remove(0)
    } else {
        Condition::And(v)
    };
    Some(match p.attempt(|p| p.words("as you cast this spell")) {
        Some(_) => Condition::AsYouCastIt(Box::new(c)),
        None => c,
    })
}

pub(super) fn condition_one(p: &mut P) -> Option<Condition> {
    alt!(p;
        |p| p.words("you're the starting player").map(|_| Condition::YouAreStartingPlayer),
        |p| p.words("you're not the starting player").map(|_| Condition::Not(Box::new(Condition::YouAreStartingPlayer))),
        |p| {
            p.word("you")?;
            p.word("created")?;
            let what = objects(p)?;
            p.words("this turn")?;
            Some(Condition::DidThisTurn(Box::new(Event::Creates { by: Actor::Player(Player::You), what, under: None })))
        },
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.one_of(&["have", "has"])?;
            let at_least = number(p)?;
            p.words("or more")?;
            let what = holding(p)?;
            Some(Condition::PlayerHas { who, at_least, what })
        },
        |p| {
            let who = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            p.one_of(&["controls", "control"])?;
            p.word("more")?;
            let what = filter(p)?;
            p.word("than")?;
            let than = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            Some(Condition::ControlsMore { who, what, than })
        },
        |p| { let who = player_simple(p)?; p.one_of(&["controls", "control"])?; let o = objects(p)?; Some(Condition::PlayerControls(who, o)) },
        |p| p.words("you cast it").map(|_| Condition::YouCastIt),
        |p| p.words("no spells were cast last turn").map(|_| Condition::NoSpellsCastLastTurn),
        |p| p.words("a player cast two or more spells last turn").map(|_| Condition::TwoSpellsCastByAPlayerLastTurn),
        |p| { p.words("you control")?; objects(p).map(Condition::YouControl) },
        |p| { p.words("you don't control")?; objects(p).map(Condition::YouDontControl) },
        |p| { p.words("an opponent controls")?; objects(p).map(|o| Condition::Opponent(Box::new(Condition::YouControl(o)))) },
        |p| p.words("it's your turn").map(|_| Condition::ItsYourTurn),
        |p| p.words("it's not your turn").map(|_| Condition::NotYourTurn),
        |p| { p.words("you have")?; let n = number(p)?; p.words("or more life")?; Some(Condition::LifeAtLeast(n)) },
        |p| { p.words("you have")?; let n = number(p)?; p.words("or less life")?; Some(Condition::LifeAtMost(n)) },
        |p| { p.one_of(&["this spell was kicked", "~ was kicked", "it was kicked"]).map(|_| Condition::WasKicked) },
        |p| p.words("you attacked this turn").map(|_| Condition::YouAttackedThisTurn),
        |p| {
            p.words("there are")?;
            let n = number(p)?;
            p.words("or more")?;
            let f = filter(p)?;
            Some(Condition::Graveyard(Amount::N(n), f))
        },
        // One subject, read once, for every predicate that follows one.
        |p| {
            let what = objects(p)?;
            alt!(p;
                |p| { p.one_of(&["is", "are"])?; p.words("in your opening hand")?; Some(Condition::InOpeningHand(what.clone())) },
                |p| { p.one_of(&["is", "are"])?; p.one_of(&["on", "in"])?; let zone = zone(p)?; Some(Condition::In { what: what.clone(), zone }) },
                |p| {
                    p.one_of(&["has", "have"])?;
                    p.word("a")?;
                    let kind = counter_kind(p)?;
                    p.words("counter on it")?;
                    Some(Condition::HasCounter { what: what.clone(), kind })
                },
                |p| { p.words("have total")?; let cmp = compare(p)?; Some(Condition::Total { what: what.clone(), cmp }) },
                |p| {
                    p.one_of(&["is", "was", "are"])?;
                    p.attempt(|p| p.one_of(&["a", "an"]));
                    let f = filter(p)?;
                    Some(Condition::Is { what: what.clone(), filter: f })
                },
            )
        },
        |p| { p.word("it's")?; p.one_of(&["on", "in"])?; let zone = zone(p)?; Some(Condition::In { what: antecedent(), zone }) },
        |p| { p.word("it's")?; p.attempt(|p| p.one_of(&["a", "an"])); let f = filter(p)?; Some(Condition::Is { what: antecedent(), filter: f }) },
    )
}
