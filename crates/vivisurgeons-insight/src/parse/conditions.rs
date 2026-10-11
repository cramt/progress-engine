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
        |p| {
            let who = alt!(p;
                |p| p.word("they're").map(|_| Player::That),
                |p| { let w = player_simple(p)?; p.one_of(&["is", "are"])?; Some(w) },
            )?;
            p.word("searching")?;
            let w = whose(p)?;
            p.one_of(&["library", "libraries"])?;
            Some(Condition::Searching { who, zone: Zone::Library(w) })
        },
        |p| {
            p.word("it's")?;
            p.word("your")?;
            let mut v = vec![ordinal(p)?];
            while let Some(n) = p.attempt(|p| { p.one_of(&[", or", ",", "or"])?; ordinal(p) }) {
                v.push(n);
            }
            p.words("turn of the game")?;
            Some(Condition::YourNthTurn(v))
        },
        |p| { p.word("it's")?; step(p).map(Condition::InStep) },
        |p| { p.words("no mana was spent to cast")?; objects(p).map(Condition::NoManaSpentToCast) },
        |p| p.words("this spell was bargained").map(|_| Condition::WasBargained),
        |p| { p.words("you spend this mana to cast")?; objects(p).map(Condition::ManaSpentOn) },
        // "an opponent lost 2 or more life this turn", "an opponent was dealt
        // 7 or more damage this turn".
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            let n = alt!(p;
                |p| p.word("lost").map(|_| Amount::LifeLostThisTurn(who.clone())),
                |p| p.words("was dealt").map(|_| Amount::DamageDealtThisTurn(who.clone())),
            )?;
            let value = amount(p)?;
            let op = alt!(p;
                |p| p.words("or more").map(|_| Op::Ge),
                |p| p.words("or less").map(|_| Op::Le),
            )?;
            let unit = if matches!(n, Amount::LifeLostThisTurn(_)) { "life" } else { "damage" };
            p.word(unit)?;
            p.words("this turn")?;
            Some(Condition::AmountIs { n, op, value })
        },
        // "you didn't create a token this way": the event, not done.
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("didn't")?;
            let verb = player_verb(p, &who)?;
            let what = objects(p)?;
            p.words("this way")?;
            Some(Condition::Not(Box::new(Condition::ThisWay(Box::new(verb(what))))))
        },
        // "you've cast another spell named ~ this game".
        |p| {
            p.word("you've")?;
            p.word("cast")?;
            let what = objects(p)?;
            p.words("this game")?;
            Some(Condition::DidThisGame(Box::new(Event::Casts { who: Player::You, what })))
        },
        // "you control an Urza's Mine and an Urza's Power-Plant": one test per object.
        |p| {
            p.words("you control")?;
            let mut v = vec![objects(p)?];
            while let Some(o) = p.attempt(|p| { p.word("and")?; if !matches!(p.peek_word(), Some("a" | "an")) { return None; } objects(p) }) {
                v.push(o);
            }
            // "and an opponent controls …" starts a second condition instead.
            let players = v.iter().any(|o| matches!(o.filter.kind, Some(Noun::Player | Noun::Opponent)));
            let ends = matches!(p.peek(), Some(Tok::Comma | Tok::Period));
            (v.len() > 1 && !players && ends).then(|| Condition::And(v.into_iter().map(Condition::YouControl).collect()))
        },
        |p| p.words("you're the starting player").map(|_| Condition::YouAreStartingPlayer),
        |p| p.words("the gift was promised").map(|_| Condition::GiftPromised),
        |p| { p.words("you cast this spell during")?; step(p).map(Condition::CastDuring) },
        // "an opponent has cast a blue or black spell this turn", "you haven't
        // cast a spell this turn", "an opponent cast three or more spells this turn".
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            let not = match p.attempt(|p| p.one_of(&["has", "have", "haven't", "hasn't"])) {
                Some(w) => w.ends_with("n't"),
                None => false,
            };
            p.word("cast")?;
            let what = objects(p)?;
            p.words("this turn")?;
            let c = Condition::DidThisTurn(Box::new(Event::Casts { who, what }));
            Some(if not { Condition::Not(Box::new(c)) } else { c })
        },
        |p| { p.words("you control no")?; objects(p).map(Condition::YouDontControl) },
        |p| {
            let w = whose(p)?;
            p.words("library has no cards in it")?;
            let cards = Objects { quant: Quant::Plural, filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(Zone::Library(w))], ..Filter::default() } };
            Some(Condition::AmountIs { n: Amount::CountOf(Box::new(cards)), op: Op::Eq, value: Amount::N(0) })
        },
        |p| { let e = event(p)?; p.words("this way")?; Some(Condition::ThisWay(Box::new(e))) },
        |p| { p.words("any of that mana is spent on")?; objects(p).map(Condition::AnyManaSpentOn) },
        |p| {
            p.words("you attacked with")?;
            let what = objects(p)?;
            p.words("this turn")?;
            Some(Condition::AttackedWithThisTurn(what))
        },
        // "there are four or more card types among cards in your graveyard".
        |p| {
            p.words("there are")?;
            let value = number(p)?;
            p.words("or more card types among")?;
            let among = objects(p)?;
            Some(Condition::AmountIs { n: Amount::NumberOfCardTypes(Box::new(among)), op: Op::Ge, value: Amount::N(value) })
        },
        // "X is 10 or more", "its power is greater than each other creature's power".
        |p| {
            let n = measure(p)?;
            p.word("is")?;
            let (op, value) = comparison(p)?;
            Some(Condition::AmountIs { n, op, value })
        },
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
            let filter = filter(p)?;
            let n_of = Amount::CountOf(Box::new(Objects { quant: Quant::Plural, filter }));
            Some(Condition::AmountIs { n: n_of, op: Op::Ge, value: Amount::N(n) })
        },
        // One subject, read once, for every predicate that follows one.
        |p| {
            let what = objects(p)?;
            alt!(p;
                |p| { p.one_of(&["is", "are"])?; p.words("in your opening hand")?; Some(Condition::InOpeningHand(what.clone())) },
                |p| { p.one_of(&["remain exiled", "remains exiled"])?; Some(Condition::In { what: what.clone(), zone: Zone::Exile }) },
                |p| { p.one_of(&["is attached to", "are attached to"])?; let to = objects(p)?; Some(Condition::AttachedTo { what: what.clone(), to }) },
                |p| p.words("died this turn").map(|_| Condition::DidThisTurn(Box::new(Event::Dies(what.clone())))),
                |p| {
                    let not = p.one_of(&["was", "wasn't"])? == "wasn't";
                    p.words("cast this way")?;
                    let c = Condition::ThisWay(Box::new(Event::WasCast(what.clone())));
                    Some(if not { Condition::Not(Box::new(c)) } else { c })
                },
                // "it wasn't put onto the battlefield with this ability".
                |p| {
                    let past = matches!(p.one_of(&["wasn't", "isn't", "weren't", "aren't"])?, "wasn't" | "weren't");
                    let q = post(p)?;
                    let c = Condition::Not(Box::new(Condition::Is { what: what.clone(), filter: Filter { posts: vec![q], ..Filter::default() } }));
                    Some(if past { Condition::Past(Box::new(c)) } else { c })
                },
                // "two or more of those creatures are attacking you and/or
                // planeswalkers you control".
                |p| {
                    p.one_of(&["is attacking", "are attacking"])?;
                    let mut v = vec![recipient(p)?];
                    while let Some(r) = p.attempt(|p| { p.one_of(&["and/or", "or"])?; recipient(p) }) {
                        v.push(r);
                    }
                    Some(Condition::Is { what: what.clone(), filter: Filter { posts: vec![Post::AttackingAnyOf(v)], ..Filter::default() } })
                },
                |p| { p.one_of(&["is", "are"])?; p.one_of(&["on", "in"])?; let zone = zone(p)?; Some(Condition::In { what: what.clone(), zone }) },
                // "this enchantment has seven or more quest counters on it".
                |p| {
                    p.one_of(&["has", "have"])?;
                    let value = number(p)?;
                    p.words("or more")?;
                    let kind = counter_kind(p)?;
                    p.words("counters on it")?;
                    let counters = Objects { quant: Quant::Plural, filter: Filter { kind: Some(Noun::Counter(Some(kind))), posts: vec![Post::On(Box::new(what.clone()))], ..Filter::default() } };
                    Some(Condition::AmountIs { n: Amount::CountOf(Box::new(counters)), op: Op::Ge, value: Amount::N(value) })
                },
                // "if it had counters on it": as it was when it left.
                |p| {
                    let past = p.one_of(&["has", "have", "had"])? == "had";
                    p.words("counters on it")?;
                    let c = Condition::HasCounter { what: what.clone(), kind: None };
                    Some(if past { Condition::Past(Box::new(c)) } else { c })
                },
                |p| {
                    let not = p.one_of(&["has", "doesn't have"])? != "has";
                    p.words("the same name as")?;
                    let mut as_any_of = vec![objects(p)?];
                    while let Some(o) = p.attempt(|p| { p.word("or")?; objects(p) }) {
                        as_any_of.push(o);
                    }
                    let c = Condition::SameName { what: what.clone(), as_any_of };
                    Some(if not { Condition::Not(Box::new(c)) } else { c })
                },
                |p| { p.words("was cast from")?; let zone = zone(p)?; Some(Condition::CastFrom { what: what.clone(), zone }) },
                |p| {
                    p.one_of(&["has", "have"])?;
                    p.word("a")?;
                    let kind = counter_kind(p)?;
                    p.words("counter on it")?;
                    Some(Condition::HasCounter { what: what.clone(), kind: Some(kind) })
                },
                |p| { p.words("have total")?; let cmp = compare(p)?; Some(Condition::Total { what: what.clone(), cmp }) },
                // "if it was a creature": as it was when the event happened.
                |p| {
                    let past = p.one_of(&["is", "was", "are"])? == "was";
                    p.attempt(|p| p.one_of(&["a", "an"]));
                    let f = filter(p)?;
                    let c = Condition::Is { what: what.clone(), filter: f };
                    Some(if past { Condition::Past(Box::new(c)) } else { c })
                },
            )
        },
        |p| { p.word("it's")?; p.one_of(&["on", "in"])?; let zone = zone(p)?; Some(Condition::In { what: antecedent(), zone }) },
        |p| { p.word("it's")?; p.attempt(|p| p.one_of(&["a", "an"])); let f = filter(p)?; Some(Condition::Is { what: antecedent(), filter: f }) },
    )
}
