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
    // "if a nonland permanent left the battlefield this turn or a spell was
    // warped this turn": either one.
    let mut alts = vec![c];
    while let Some(c) = p.attempt(|p| {
        // "Activate only if this land entered this turn or if you control a
        // basic land": "if" said again.
        p.one_of(&["or if", "or"])?;
        condition_one(p)
    }) {
        alts.push(c);
    }
    let c = if alts.len() == 1 {
        alts.remove(0)
    } else {
        Condition::Or(alts)
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
        // "it's the second creature spell you cast this turn".
        |p| {
            p.words("it's the")?;
            let n = ordinal(p)?;
            let mut spell = filter(p)?;
            let who = match spell.posts.pop()? {
                Post::WillCastThisTurn(w) => w,
                _ => return None,
            };
            Some(Condition::NthSpellThisTurn { what: antecedent(), n, spell, who })
        },
        // "five or more mana was spent to cast that spell", "at least four
        // mana was spent to cast it": how much, of any kind.
        |p| {
            let value = alt!(p;
                |p| { let n = amount(p)?; p.words("or more")?; Some(n) },
                |p| { p.words("at least")?; amount(p) },
            )?;
            p.words("mana was spent to cast")?;
            let what = objects(p)?;
            Some(Condition::AmountIs { n: Amount::ManaSpentToCast(Box::new(what)), op: Op::Ge, value })
        },
        |p| {
            let mana = mana(p)?;
            p.words("was spent to cast")?;
            let what = objects(p)?;
            Some(Condition::ManaSpent { mana, what })
        },
        |p| p.words("you have the city 's blessing").map(|_| Condition::CitysBlessing(Player::You)),
        |p| p.words("you have max speed").map(|_| Condition::MaxSpeed(Player::You)),
        // "this is the fourth time this ability has resolved this turn", and
        // after the first, "it's the second time".
        |p| {
            p.one_of(&["this is the", "it's the"])?;
            let n = ordinal(p)?;
            p.word("time")?;
            p.attempt(|p| p.words("this ability has resolved this turn"));
            Some(Condition::NthResolution(n))
        },
        |p| { p.word("the")?; let m = mana(p)?; p.words("cost was paid")?; Some(Condition::CostPaid(m)) },
        |p| p.words("this spell 's additional cost was paid").map(|_| Condition::AdditionalCostPaid),
        |p| p.words("you win the flip").map(|_| Condition::WonFlip(true)),
        |p| p.words("you lose the flip").map(|_| Condition::WonFlip(false)),
        |p| {
            p.words("it was kicked with its")?;
            let m = mana(p)?;
            p.word("kicker")?;
            Some(Condition::KickedWith(m))
        },
        |p| p.words("damage is prevented this way").map(|_| Condition::DamagePreventedThisWay),
        // "there is an instant card and a sorcery card in your graveyard":
        // each in the zone said once after the last.
        |p| {
            p.words("there is")?;
            let mut v = vec![objects(p)?];
            while let Some(o) = p.attempt(|p| { p.word("and")?; objects(p) }) { v.push(o); }
            if v.len() < 2 { return None; }
            let zone = match v.last()?.filter.posts.last()? { Post::In(z) => z.clone(), _ => return None };
            for o in v.iter_mut().rev().skip(1) {
                o.filter.posts.push(Post::In(zone.clone()));
            }
            Some(Condition::ThereAre(v))
        },
        // "there's a Lesson card in your graveyard": one object in a zone.
        |p| {
            p.one_of(&["there's", "there is"])?;
            let o = objects(p)?;
            if !matches!(o.filter.posts.last(), Some(Post::In(_))) { return None; }
            Some(Condition::ThereAre(vec![o]))
        },
        |p| p.words("you have an enduring story").map(|_| Condition::EnduringStory(Player::You)),
        // "you both own and control this land and a creature named Hanweir
        // Garrison".
        |p| {
            p.words("you both own and control")?;
            let first = objects(p)?;
            p.word("and")?;
            let second = objects(p)?;
            Some(Condition::OwnAndControl(vec![first, second]))
        },
        // "you sacrificed a creature this way", "you sacrificed a permanent
        // this turn".
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("sacrificed")?;
            let what = objects(p)?;
            let e = Box::new(Event::Sacrifices { who, what });
            alt!(p;
                |p| p.words("this way").map(|_| Condition::ThisWay(e.clone())),
                |p| p.words("this turn").map(|_| Condition::DidThisTurn(e.clone())),
            )
        },
        // "you exiled four or more cards this way".
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            p.word("exiled")?;
            let what = objects(p)?;
            p.words("this way")?;
            Some(Condition::ThisWay(Box::new(Event::Exiles { who, what })))
        },
        // "you had a land enter the battlefield under your control this turn".
        |p| {
            p.words("you had")?;
            let what = objects(p)?;
            p.words("enter the battlefield under your control this turn")?;
            Some(Condition::DidThisTurn(Box::new(Event::Enters { what, under: Some(Player::You), state: None })))
        },
        // "a graveyard has twenty or more cards in it": some one graveyard.
        |p| {
            let zone = alt!(p;
                |p| p.words("a graveyard").map(|_| Zone::AGraveyard),
                |p| p.words("a library").map(|_| Zone::ALibrary),
            )?;
            p.word("has")?;
            let n = number(p)?;
            let op = alt!(p;
                |p| p.words("or more").map(|_| Op::Ge),
                |p| p.words("or fewer").map(|_| Op::Le),
            )?;
            p.words("cards in it")?;
            Some(Condition::ZoneHasCards { zone, op, n })
        },
        |p| { p.words("you controlled")?; objects(p).map(|o| Condition::Past(Box::new(Condition::YouControl(o)))) },
        // "you gained life this turn", "an opponent searched their library
        // this turn".
        |p| {
            let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
            let e = alt!(p;
                |p| p.words("gained life").map(|_| Event::GainsLife(who.clone())),
                |p| p.words("lost life").map(|_| Event::LosesLife(who.clone())),
                |p| {
                    p.word("searched")?;
                    let w = whose(p)?;
                    p.word("library")?;
                    Some(Event::Searches { who: who.clone(), zone: Zone::Library(w) })
                },
            )?;
            p.words("this turn")?;
            Some(Condition::DidThisTurn(Box::new(e)))
        },
        |p| { p.words("you have")?; let n = number(p)?; p.words("or fewer cards in hand")?; Some(Condition::CardsInHandAtMost(n)) },        // "an opponent has 10 or less life": "you have" reads as its own.
        |p| {
            let who = player_simple(p)?;
            p.word("has")?;
            let at_most = number(p)?;
            p.one_of(&["or less", "or fewer"])?;
            let what = holding(p)?;
            Some(Condition::PlayerHasAtMost { who, at_most, what })
        },
        // "an opponent has more life than you".
        |p| {
            let who = player_simple(p)?;
            p.word("has")?;
            p.word("more")?;
            let what = holding(p)?;
            p.word("than")?;
            let than = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            Some(Condition::HasMore { who, what, than })
        },
        |p| { p.words("no mana was spent to cast")?; objects(p).map(Condition::NoManaSpentToCast) },
        |p| p.words("this spell was bargained").map(|_| Condition::WasBargained),
        // "if there are no charge counters on this Vehicle".
        |p| {
            p.words("there are no")?;
            let kind = p.attempt(counter_kind);
            p.words("counters on")?;
            let what = objects(p)?;
            Some(Condition::Not(Box::new(Condition::HasCounter { what, kind })))
        },
        // "If a creature card was exiled this way".
        |p| {
            let what = objects(p)?;
            if !matches!(what.quant, Quant::One | Quant::Another | Quant::AtLeast(_) | Quant::OneOrMore | Quant::N(_)) { return None; }
            p.one_of(&["is", "was", "are", "were"])?;
            let how = super::objects::marked(p)?;
            p.words("this way")?;
            Some(Condition::DoneThisWay { what, how })
        },
        // "an opponent lost 2 or more life this turn", "an opponent was dealt
        // 7 or more damage this turn".
        |p| {
            let who = super::amounts::life_subject(p)?;
            let n = alt!(p;
                |p| p.word("lost").map(|_| Amount::LifeLostThisTurn(who.clone())),
                // "if you've gained 3 or more life this turn".
                |p| p.word("gained").map(|_| Amount::LifeGainedThisTurn(who.clone())),
                |p| p.words("was dealt").map(|_| Amount::DamageDealtThisTurn(who.clone())),
            )?;
            let value = amount(p)?;
            let op = alt!(p;
                |p| p.words("or more").map(|_| Op::Ge),
                |p| p.words("or less").map(|_| Op::Le),
            )?;
            let unit = if matches!(n, Amount::DamageDealtThisTurn(_)) { "damage" } else { "life" };
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
            Some(Condition::DidThisGame(Box::new(casts(Player::You, what))))
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
            // "you've cast another spell this turn", "you didn't cast a
            // spell this turn".
            let (who, not) = alt!(p;
                |p| p.word("you've").map(|_| (Player::You, Some(false))),
                |p| {
                    let who = alt!(p; |p| p.word("you").map(|_| Player::You), player_simple)?;
                    let not = p.attempt(|p| p.one_of(&["has", "have", "haven't", "hasn't", "didn't"])).map(|w| w.ends_with("n't"));
                    Some((who, not))
                },
            )?;
            let not = not.unwrap_or(false);
            // "if you cycled two or more cards this turn".
            let cycled = p.one_of(&["cast", "cycled"])? == "cycled";
            let what = objects(p)?;
            p.words("this turn")?;
            let e = if cycled { Event::Cycles { who, what: Some(what) } } else { casts(who, what) };
            let c = Condition::DidThisTurn(Box::new(e));
            Some(if not { Condition::Not(Box::new(c)) } else { c })
        },
        // "you've drawn two or more cards this turn".
        |p| {
            p.words("you've drawn")?;
            let value = number(p)?;
            p.words("or more cards this turn")?;
            Some(Condition::AmountIs { n: Amount::CardsDrawnThisTurn(Player::You), op: Op::Ge, value: Amount::N(value) })
        },
        |p| { p.words("you control no")?; objects(p).map(Condition::YouDontControl) },
        |p| {
            let w = whose(p)?;
            p.words("library has no cards in it")?;
            let cards = Objects { quant: Quant::Plural, filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(Zone::Library(w))], ..Filter::default() } };
            Some(Condition::AmountIs { n: Amount::CountOf(Box::new(cards)), op: Op::Eq, value: Amount::N(0) })
        },
        |p| { let e = event(p)?; p.words("this way")?; Some(Condition::ThisWay(Box::new(e))) },
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
            let past = p.one_of(&["is", "was"])? == "was";
            let (op, value) = comparison(p)?;
            let c = Condition::AmountIs { n, op, value };
            // "if its mana value was 2 or less": as it was when the event
            // happened, as "if it was a creature" is.
            Some(if past { Condition::Past(Box::new(c)) } else { c })
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
        |p| { p.words("you cast it from")?; zone(p).map(|zone| Condition::YouCastItFrom { what: antecedent(), zone }) },
        |p| p.words("you cast it").map(|_| Condition::YouCastIt(antecedent())),
        |p| p.words("no spells were cast last turn").map(|_| Condition::NoSpellsCastLastTurn),
        |p| p.words("a player cast two or more spells last turn").map(|_| Condition::TwoSpellsCastByAPlayerLastTurn),
        |p| { p.words("you control")?; objects(p).map(Condition::YouControl) },
        |p| { p.words("you don't control")?; objects(p).map(Condition::YouDontControl) },
        |p| { p.words("an opponent controls")?; objects(p).map(|o| Condition::Opponent(Box::new(Condition::YouControl(o)))) },
        |p| p.words("it's your turn").map(|_| Condition::ItsYourTurn),
        |p| {
            p.words("it's the first combat phase of")?;
            let your_turn = p.one_of(&["the turn", "your turn"])? == "your turn";
            Some(Condition::FirstCombatPhase { your_turn })
        },
        |p| p.words("it's not your turn").map(|_| Condition::NotYourTurn),
        |p| p.words("it's an opponent 's turn").map(|_| Condition::OpponentsTurn),
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
                |p| { p.one_of(&["remain on the battlefield", "remains on the battlefield"])?; Some(Condition::In { what: what.clone(), zone: Zone::Battlefield }) },
                |p| { p.one_of(&["is attached to", "are attached to"])?; let to = objects(p)?; Some(Condition::AttachedTo { what: what.clone(), to }) },
                |p| p.words("died this turn").map(|_| Condition::DidThisTurn(Box::new(Event::Dies(what.clone())))),
                // "a creature died under your control this turn": one you
                // controlled as it died.
                |p| {
                    p.words("died under your control this turn")?;
                    let mut x = what.clone();
                    x.filter.posts.push(Post::Controller(Player::You));
                    Some(Condition::DidThisTurn(Box::new(Event::Dies(x))))
                },
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
                // "if this creature isn't prepared". Not an adjective
                // elsewhere: Codie's "prepared spell" is no permanent's state.
                |p| {
                    let not = p.one_of(&["is", "isn't"])? == "isn't";
                    p.word("prepared")?;
                    let f = Filter { alts: vec![vec![Adj::State(State::Prepared)]], ..Filter::default() };
                    let c = Condition::Is { what: what.clone(), filter: f };
                    Some(if not { Condition::Not(Box::new(c)) } else { c })
                },
                // "if this land isn't a creature": the present test, negated.
                |p| {
                    p.one_of(&["isn't", "aren't"])?;
                    p.attempt(|p| p.one_of(&["a", "an"]));
                    let f = filter(p)?;
                    Some(Condition::Not(Box::new(Condition::Is { what: what.clone(), filter: f })))
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
                // "if this enchantment has no charge counters on it".
                |p| {
                    p.one_of(&["has no", "have no"])?;
                    let kind = p.attempt(counter_kind);
                    p.words("counters on")?;
                    p.one_of(&["it", "them"])?;
                    Some(Condition::Not(Box::new(Condition::HasCounter { what: what.clone(), kind })))
                },
                // "if it had counters on it": as it was when it left.
                |p| {
                    let past = p.one_of(&["has", "have", "had"])? == "had";
                    p.words("counters on it")?;
                    let c = Condition::HasCounter { what: what.clone(), kind: None };
                    Some(if past { Condition::Past(Box::new(c)) } else { c })
                },
                // "if it had a -1/-1 counter on it", "if it had one or more
                // counters on it": as it was when it left.
                |p| {
                    p.word("had")?;
                    let kind = alt!(p;
                        |p| p.words("one or more counters on it").map(|_| None),
                        |p| p.words("a counter on it").map(|_| None),
                        |p| { p.word("a")?; let k = counter_kind(p)?; p.words("counter on it")?; Some(Some(k)) },
                    )?;
                    Some(Condition::Past(Box::new(Condition::HasCounter { what: what.clone(), kind })))
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
                |p| { p.words("wasn't cast from")?; let zone = zone(p)?; Some(Condition::Not(Box::new(Condition::CastFrom { what: what.clone(), zone }))) },
                |p| p.words("wasn't cast").map(|_| Condition::Not(Box::new(Condition::WasCast(what.clone())))),
                |p| { p.one_of(&["is", "are"])?; p.words("face down")?; Some(Condition::FaceDown(what.clone())) },
                // "a card is exiled with this land".
                |p| {
                    p.one_of(&["is", "are"])?;
                    p.words("exiled with")?;
                    // "if a card is exiled with it": the card itself (CR 607.2a).
                    let with = alt!(p; |p| p.word("it").map(|_| self_ref()), objects)?;
                    Some(Condition::Is { what: what.clone(), filter: Filter { posts: vec![Post::ExiledWith(Box::new(with))], ..Filter::default() } })
                },
                // "it entered from your library or was cast from your library",
                // "it entered under your control".
                |p| {
                    let entered = |p: &mut P| -> Option<Condition> {
                        p.word("entered")?;
                        alt!(p;
                            |p| { p.word("from")?; let z = zone(p)?; Some(Condition::Entered { what: what.clone(), from: Some(z), under: None }) },
                            |p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(Condition::Entered { what: what.clone(), from: None, under: Some(w) }) },
                        )
                    };
                    let first = entered(p)?;
                    match p.attempt(|p| { p.words("or was cast from")?; zone(p) }) {
                        Some(z) => Some(Condition::Or(vec![first, Condition::CastFrom { what: what.clone(), zone: z }])),
                        None => Some(first),
                    }
                },
                // "~ has one or more loyalty counters on him".
                |p| {
                    p.one_of(&["has", "have"])?;
                    p.words("one or more")?;
                    let kind = counter_kind(p)?;
                    p.word("counters")?;
                    p.word("on")?;
                    pronoun(p)?;
                    Some(Condition::HasCounter { what: what.clone(), kind: Some(kind) })
                },
                // "it has mana value 2 or less".
                |p| { p.one_of(&["has", "have"])?; let cmp = compare(p)?; Some(Condition::Has { what: what.clone(), cmp }) },
                |p| { p.one_of(&["targets", "target"])?; let target = objects(p)?; Some(Condition::Targets { what: what.clone(), target }) },
                // "this land entered this turn", "two or more creatures
                // entered the battlefield under your control this turn".
                |p| {
                    p.word("entered")?;
                    let full = p.attempt(|p| p.words("the battlefield")).is_some();
                    let under = p.attempt(|p| { p.word("under")?; let w = whose(p)?; p.word("control")?; Some(w) });
                    if under.is_some() && !full { return None; }
                    p.words("this turn")?;
                    Some(Condition::DidThisTurn(Box::new(Event::Enters { what: what.clone(), under, state: None })))
                },
                // "a nonland permanent left the battlefield this turn".
                |p| {
                    p.words("left the battlefield this turn")?;
                    Some(Condition::DidThisTurn(Box::new(Event::Leaves { what: what.clone(), from: Zone::Battlefield })))
                },
                // "a card left your graveyard this turn".
                |p| {
                    p.word("left")?;
                    let from = zone(p)?;
                    if from == Zone::Battlefield { return None; }
                    p.words("this turn")?;
                    Some(Condition::DidThisTurn(Box::new(Event::Leaves { what: what.clone(), from })))
                },
                // "if a Dragon was beheld": by this spell's cost (CR 701.4b).
                |p| p.words("was beheld").map(|_| Condition::Beheld(what.clone())),
                // "a spell was warped this turn": cast for its warp cost (CR
                // 702.185c).
                |p| {
                    p.words("was warped this turn")?;
                    Some(Condition::DidThisTurn(Box::new(Event::CastUsing { what: what.clone(), keyword: "warp".into() })))
                },
                // "this spell was cast using teamwork": its teamwork cost was
                // declared as it was cast (CR 702.194b).
                |p| {
                    p.words("was cast using")?;
                    let keyword = keyword_name(p)?;
                    Some(Condition::CastUsing { what: what.clone(), keyword })
                },
                // "a permanent left the battlefield under your control this turn".
                |p| {
                    p.words("left the battlefield under")?;
                    let who = whose(p)?;
                    p.words("control this turn")?;
                    Some(Condition::DidThisTurn(Box::new(Event::LeftUnderControlOf { what: what.clone(), who })))
                },
                |p| {
                    p.one_of(&["has", "have"])?;
                    p.word("a")?;
                    let kind = counter_kind(p)?;
                    p.words("counter on it")?;
                    Some(Condition::HasCounter { what: what.clone(), kind: Some(kind) })
                },
                |p| { p.words("have total")?; let cmp = compare(p)?; Some(Condition::Total { what: what.clone(), cmp }) },
                // "if enchanted creature is white and untapped": two
                // predicates of one object, both to hold. A filter reads
                // "white and untapped" as alternatives, as a list of kinds is.
                |p| {
                    p.word("is")?;
                    let plain = |g: &Vec<Adj>| !g.iter().any(super::objects::is_typed);
                    let a = group(p).filter(plain)?;
                    p.word("and")?;
                    let b = group(p).filter(plain)?;
                    let is = |g| Condition::Is { what: what.clone(), filter: Filter { alts: vec![g], ..Filter::default() } };
                    Some(Condition::And(vec![is(a), is(b)]))
                },
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
