//! One effect: a verb, its subject and its objects.

use super::*;

pub(super) fn duration(p: &mut P) -> Option<Duration> {
    alt!(p;
        |p| p.words("until end of turn").map(|_| Duration::UntilEndOfTurn),
        |p| p.words("this turn").map(|_| Duration::ThisTurn),
        |p| p.words("until your next turn").map(|_| Duration::UntilYourNextTurn),
        |p| p.words("for as long as you control ~").map(|_| Duration::WhileSelfOnBattlefield),
        |p| p.words("until ~ leaves the battlefield").map(|_| Duration::WhileSelfOnBattlefield),
        |p| p.words("for the rest of the game").map(|_| Duration::RestOfGame),
        |p| { p.words("for as long as")?; condition(p).map(|c| Duration::ForAsLongAs(Box::new(c))) },
    )
}

/// What a "becomes" keeps of the types it had: "in addition to its other
/// types", "that's still a land".
pub(super) fn keeps(p: &mut P) -> Option<Keeps> {
    alt!(p;
        |p| p.one_of(&["in addition to its other types", "in addition to their other types"]).map(|_| Keeps::OtherTypes),
        |p| { p.one_of(&["that's still a", "that's still an", "that are still"])?; group(p).map(Keeps::Listed) },
    )
}

pub(super) fn cards(p: &mut P) -> Option<Amount> {
    let n = alt!(p;
        |p| { p.words("up to")?; amount(p).map(|n| Amount::UpTo(Box::new(n))) },
        amount_a,
    )?;
    // "draw two additional cards".
    let n = match p.attempt(|p| p.word("additional")) {
        Some(_) => Amount::Additional(Box::new(n)),
        None => n,
    };
    p.one_of(&["cards", "card"])?;
    // "draw that many cards plus one".
    Some(
        match p.attempt(|p| {
            p.word("plus")?;
            number(p)
        }) {
            Some(k) => Amount::Plus(Box::new(n), k),
            None => n,
        },
    )
}

/// "the top three cards of your library", or "the top card of" with no count.
pub(super) fn top_of_library(p: &mut P) -> Option<(Amount, Player)> {
    p.words("the top")?;
    let n = alt!(p;
        cards,
        |p| p.word("card").map(|_| Amount::N(1)),
    )?;
    p.word("of")?;
    let whose = whose(p)?;
    p.word("library")?;
    Some((n, whose))
}

/// The subject of a sentence whose verb takes a player.
pub(super) fn subject_player(p: &mut P) -> Player {
    p.attempt(player)
        .or_else(|| p.implied.clone())
        .unwrap_or(Player::You)
}

pub(super) fn token(p: &mut P) -> Option<(Amount, Token)> {
    // "a number of Treasure tokens equal to the result".
    let n = match p.attempt(|p| p.words("a number of")) {
        Some(_) => None,
        None => Some(amount_a(p)?),
    };
    // "an additional Food token": one more than the event made.
    let n = match n {
        Some(n) if p.attempt(|p| p.word("additional")).is_some() => {
            Some(Amount::Additional(Box::new(n)))
        }
        n => n,
    };
    let tapped = p.attempt(|p| p.word("tapped")).is_some();
    let pt = p.attempt(|p| match p.peek()? {
        Tok::Pt(pt) if !pt.signed => {
            let pt = *pt;
            p.bump();
            Some(pt)
        }
        _ => None,
    });
    let mut colors = Vec::new();
    while let Some(c) = p.attempt(|p| {
        p.attempt(|p| p.word("and"));
        let c = color_word(p.peek_word()?)?;
        p.bump();
        Some(c)
    }) {
        colors.push(c);
    }
    let mut types = Vec::new();
    let mut named = None;
    while let Some(a) = p.attempt(|p| {
        let w = p.peek_word()?;
        if w == "token" || w == "tokens" {
            return None;
        }
        let a = type_word(w).or_else(|| {
            vocab()
                .supertypes
                .contains(w)
                .then(|| Adj::Supertype(w.to_string()))
        })?;
        p.bump();
        Some(a)
    }) {
        match a {
            Adj::Type(t) | Adj::Supertype(t) => types.push(t),
            Adj::Subtype(s) => {
                if pt.is_none() && named.is_none() {
                    named = Some(s.clone());
                }
                types.push(s)
            }
            _ => {}
        }
    }
    if types.is_empty() {
        return None;
    }
    p.one_of(&["tokens", "token"])?;
    if pt.is_some() {
        named = None;
    }
    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    if p.attempt(|p| p.word("with")).is_some() {
        // A separator belongs to the list only with an item after it: "with
        // islandwalk, where X is …" ends the list at the comma.
        let mut first = true;
        loop {
            let item = p.attempt(|p| {
                if !first {
                    p.one_of(&[", and", "and", ","])?;
                }
                match p.attempt(quoted) {
                    Some(a) => Some(Err(a)),
                    None => keyword(p).map(Ok),
                }
            });
            match item {
                Some(Ok(k)) => keywords.push(k),
                Some(Err(a)) => abilities.push(a),
                None if first => return None,
                None => break,
            }
            first = false;
        }
    }
    let n = match n {
        Some(n) => n,
        None => {
            p.words("equal to")?;
            alt!(p;
                |p| p.words("the result").map(|_| Amount::DieResult),
                measure,
            )?
        }
    };
    // "that's tapped and attacking that player or a planeswalker they control".
    let (tapped, attacking) = match p.attempt(|p| {
        p.one_of(&["that's", "that are"])?;
        let tapped = p.attempt(|p| p.words("tapped and")).is_some();
        p.word("attacking")?;
        Some((tapped, recipient(p)?))
    }) {
        Some((t, r)) => (tapped || t, Some(r)),
        None => (tapped, None),
    };
    Some((
        n,
        Token {
            tapped,
            pt,
            colors,
            types,
            keywords,
            abilities,
            named,
            attacking,
        },
    ))
}

pub(super) fn pt_signed(p: &mut P) -> Option<Pt> {
    match p.peek()? {
        Tok::Pt(pt) if pt.signed => {
            let pt = *pt;
            p.bump();
            Some(pt)
        }
        _ => None,
    }
}

pub(super) fn restriction(p: &mut P) -> Option<Restriction> {
    let r = restriction_bare(p)?;
    if let Some((who, cost, per)) = p.attempt(|p| {
        p.word("unless")?;
        let who = player(p)?;
        p.verb("pay")?;
        let cost = costs(p)?;
        let per = p.attempt(|p| {
            p.words("for each")?;
            alt!(p;
                // "for each of those creatures".
                |p| { p.word("of")?; pronoun(p) },
                objects,
            )
        });
        Some((who, cost, per))
    }) {
        return Some(Restriction::UnlessPays {
            rule: Box::new(r),
            who,
            cost,
            per,
        });
    }
    Some(
        match p.attempt(|p| {
            p.word("unless")?;
            condition(p)
        }) {
            Some(c) => Restriction::Unless(Box::new(r), c),
            None => r,
        },
    )
}

pub(super) fn restriction_bare(p: &mut P) -> Option<Restriction> {
    alt!(p;
        |p| { p.words("can block only")?; filter(p).map(Restriction::CanBlockOnly) },
        |p| p.words("can't be blocked by more than one creature").map(|_| Restriction::CantBeBlockedByMoreThanOne),
        |p| { p.words("can't be blocked by")?; filter(p).map(Restriction::CantBeBlockedBy) },
        |p| { p.words("can block an additional")?; p.attempt(|p| p.word("creature")); p.words("each combat").map(|_| Restriction::CanBlockAdditional(Amount::N(1))) },
        |p| p.words("can block any number of creatures").map(|_| Restriction::CanBlockAdditional(Amount::X)),
        |p| p.words("can't attack or block").map(|_| Restriction::CantAttackOrBlock),
        |p| { p.words("can't block")?; filter(p).map(Restriction::CantBlockAny) },
        |p| p.words("can't block").map(|_| Restriction::CantBlock),
        // "can't attack you or planeswalkers you control".
        |p| {
            p.words("can't attack")?;
            let first = recipient(p)?;
            let mut v = vec![first];
            while let Some(r) = p.attempt(|p| { p.one_of(&["or", "and/or"])?; recipient(p) }) {
                v.push(r);
            }
            (v.len() > 1).then_some(Restriction::CantAttackAnyOf(v))
        },
        |p| { p.words("can't attack")?; let pl = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?; Some(Restriction::CantAttackPlayer(pl)) },
        |p| {
            p.one_of(&["attacks a player other than", "attack a player other than"])?;
            let pl = alt!(p; player_simple, |p| p.word("you").map(|_| Player::You))?;
            p.words("if able")?;
            Some(Restriction::AttacksPlayerOtherThan(pl))
        },
        |p| p.words("can't attack").map(|_| Restriction::CantAttack),
        |p| { p.words("can't be blocked except by")?; filter(p).map(Restriction::CantBeBlockedExceptBy) },
        |p| p.words("can't be blocked").map(|_| Restriction::CantBeBlocked),
        |p| p.words("can't be countered").map(|_| Restriction::CantBeCountered),
        |p| p.words("can't be regenerated").map(|_| Restriction::CantBeRegenerated),
        |p| p.one_of(&["attacks each combat if able", "attack each combat if able"]).map(|_| Restriction::AttacksEachCombat),
        |p| p.one_of(&["attacks this turn if able", "attack this turn if able"]).map(|_| Restriction::AttacksThisTurn),
        |p| p.one_of(&["blocks each combat if able", "block each combat if able"]).map(|_| Restriction::BlocksEachCombat),
        |p| p.one_of(&["doesn't untap during your untap step", "don't untap during your untap step"]).map(|_| Restriction::DoesntUntap(Player::You)),
        |p| p.words("doesn't untap during its controller 's untap step").map(|_| Restriction::DoesntUntap(Player::ControllerOf(Box::new(antecedent())))),
        |p| p.one_of(&["doesn't untap during its controller 's next untap step", "don't untap during their controllers ' next untap steps", "don't untap during their controller 's next untap step"]).map(|_| Restriction::DoesntUntapNext),
    )
}

pub(super) fn effect(p: &mut P) -> Option<Effect> {
    alt!(p;
        // draw
        |p| { let who = subject_player(p); p.verb("draw")?; let n = cards(p)?; Some(Effect::Draw { who, n }) },
        |p| { let who = subject_player(p); p.verb("draw")?; p.words("cards equal to")?; let n = measure(p)?; Some(Effect::Draw { who, n }) },
        // discard
        |p| { let who = subject_player(p); p.verb("discard")?; p.one_of(&["their hand", "your hand"])?; Some(Effect::DiscardHand(who)) },
        |p| {
            let who = subject_player(p);
            p.verb("discard")?;
            let what = objects(p)?;
            let random = p.attempt(|p| p.words("at random")).is_some();
            Some(Effect::Discard { who, what, random })
        },
        // damage
        |p| {
            let source = p.attempt(objects);
            p.verb("deal")?;
            let (n, to) = alt!(p;
                |p| { let n = amount(p)?; p.word("damage")?; p.word("to")?; Some((n, recipients(p)?)) },
                |p| { p.words("damage equal to")?; let n = measure(p)?; p.word("to")?; Some((n, recipients(p)?)) },
                |p| { p.words("damage to")?; let r = recipients(p)?; p.words("equal to")?; Some((measure(p)?, r)) },
            )?;
            Some(Effect::Damage { source, n, to })
        },
        // destroy / exile
        |p| {
            p.word("destroy")?;
            let what = objects(p)?;
            let no_regen = p.attempt(|p| {
                p.tok(&Tok::Period)?;
                p.one_of(&["it", "they", "a creature destroyed this way", "creatures destroyed this way", "a permanent destroyed this way"])?;
                p.words("can't be regenerated")
            }).is_some();
            Some(Effect::Destroy { what, no_regen })
        },
        |p| {
            p.words("change the target of")?;
            let what = objects(p)?;
            p.word("to")?;
            let to = objects(p)?;
            Some(Effect::ChangeTargetTo { what, to })
        },
        |p| { p.words("change the target of")?; objects(p).map(Effect::ChangeTarget) },
        // "you untap all lands you control": an imperative said with its
        // subject, which is the "you" it implies anyway.
        |p| {
            p.word("you")?;
            if !p.peek_word().is_some_and(|w| matches!(w, "untap" | "tap" | "exile" | "destroy" | "return" | "put" | "attach")) { return None; }
            effect(p)
        },
        |p| { let who = subject_player(p); p.verb("lose")?; p.words("the game")?; Some(Effect::LoseGame(who)) },
        |p| {
            let what = objects(p)?;
            p.verb("lose")?;
            let keywords = keyword_list(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::LoseKeywords { what, keywords, dur })
        },
        |p| { p.word("exile")?; zone(p).map(Effect::ExileZone) },
        |p| {
            p.word("exile")?;
            let what = objects(p)?;
            p.word("until")?;
            let until = objects(p)?;
            p.words("leaves the battlefield")?;
            Some(Effect::ExileUntilLeaves { what, until })
        },
        |p| {
            p.word("exile")?;
            let what = objects(p)?;
            p.word("with")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counter on it", "counters on it", "counter on them", "counters on them"])?;
            Some(Effect::ExileWithCounters { what, n, kind })
        },
        |p| { p.word("exile")?; let what = objects(p)?; p.words("face down")?; Some(Effect::ExileFaceDown(what)) },
        |p| { p.word("exile")?; objects(p).map(Effect::Exile) },
        |p| {
            let who = subject_player(p);
            p.verb("pay")?;
            let cost = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(vec![Cost::PayLife(n)]) },
                // "pay {1} and 3 life".
                |p| { let m = mana(p)?; p.word("and")?; let n = amount(p)?; p.word("life")?; Some(vec![Cost::Mana(m), Cost::PayLife(n)]) },
                costs,
            )?;
            Some(Effect::Pay { who, cost })
        },
        |p| {
            p.word("prevent")?;
            let n = alt!(p;
                |p| p.word("all").map(|_| None),
                |p| { p.words("the next")?; amount(p).map(Some) },
            )?;
            let combat_only = p.attempt(|p| p.word("combat")).is_some();
            p.words("damage that would be dealt")?;
            // "dealt to and dealt by that creature": one object, both ways.
            if let Some(o) = p.attempt(|p| { p.words("to and dealt by")?; objects(p) }) {
                let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
                return Some(Effect::Prevent { combat_only, n, to: Some(vec![Recipient::Objects(o.clone())]), by: Some(o), dur });
            }
            let to = p.attempt(|p| { p.word("to")?; recipients(p) });
            let by = p.attempt(|p| { p.word("by")?; objects(p) });
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Prevent { combat_only, n, to, by, dur })
        },
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.words("a copy of")?;
            let of = objects(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let except = p.attempt(exceptions).unwrap_or_default();
            Some(Effect::BecomesCopy { what, of, dur, except })
        },
        // "you may have it become a copy of any creature on the battlefield".
        |p| {
            p.word("have")?;
            let what = objects(p)?;
            p.words("become a copy of")?;
            let of = objects(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let except = p.attempt(exceptions).unwrap_or_default();
            Some(Effect::BecomesCopy { what, of, dur, except })
        },
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let pt = p.attempt(|p| match p.peek()? { Tok::Pt(pt) if !pt.signed => { let pt = *pt; p.bump(); Some(pt) } _ => None });
            p.attempt(|p| p.words("base power and toughness"));
            let mut types = Vec::new();
            while let Some(a) = p.attempt(descriptor) {
                types.push(a);
            }
            if types.is_empty() && pt.is_none() { return None; }
            let gains = p.attempt(|p| { p.word("with")?; keyword_list(p) }).unwrap_or_default();
            let keeps = p.attempt(keeps).unwrap_or(Keeps::Nothing);
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Becomes { what, pt, types, gains, keeps, dur })
        },
        |p| { let who = subject_player(p); p.verb("win")?; p.words("the game")?; Some(Effect::WinGame(who)) },
        |p| {
            p.words("roll a")?;
            match p.peek_word()?.strip_prefix('d')?.parse() {
                Ok(n) => { p.bump(); Some(Effect::RollDie(n)) }
                Err(_) => None,
            }
        },
        |p| {
            let who = subject_player(p);
            p.one_of(&["don't lose", "doesn't lose"])?;
            let mana = alt!(p;
                |p| p.words("this mana").map(|_| KeptMana::This),
                |p| {
                    p.word("unspent")?;
                    let c = p.attempt(|p| { let c = color_word(p.peek_word()?)?; p.bump(); Some(c) });
                    p.word("mana")?;
                    Some(KeptMana::Unspent(c))
                },
            )?;
            p.words("as steps and phases end")?;
            Some(Effect::KeepsMana { who, mana })
        },
        |p| {
            p.words("double the")?;
            let stats = alt!(p;
                |p| p.words("power and toughness").map(|_| vec![Stat::Power, Stat::Toughness]),
                |p| stat(p).map(|s| vec![s]),
            )?;
            p.word("of")?;
            let of = objects(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Double { stats, of, dur })
        },
        |p| {
            p.word("move")?;
            let counters = objects(p)?;
            if !matches!(counters.filter.kind, Some(Noun::Counter(_))) { return None; }
            p.word("from")?;
            let from = objects(p)?;
            p.word("onto")?;
            let to = objects(p)?;
            Some(Effect::MoveCounters { counters, from, to })
        },
        // "put those counters on ~": the object's own "on" reads where.
        |p| {
            p.word("put")?;
            let mut those = objects(p)?;
            if those.quant != Quant::Antecedent || !matches!(those.filter.kind, Some(Noun::Counter(_))) { return None; }
            let on = match those.filter.posts.pop() { Some(Post::On(o)) => *o, _ => return None };
            Some(Effect::PutThoseCounters { those, on })
        },
        // "Each player exiles all creature cards from their graveyard, then
        // sacrifices all creatures they control": read by "A verb that takes
        // no player" below.
        |p| {
            let who = subject_player(p);
            p.verb("create")?;
            let n = amount_a(p)?;
            p.words("of each")?;
            Some(Effect::CreateOfEach { who, n })
        },
        // "It's an enchantment.": what it is as it enters, which "becomes"
        // would make it only afterwards (CR 611.2e).
        |p| {
            p.word("it's")?;
            p.one_of(&["a", "an"])?;
            let types = group(p)?;
            let keeps = p.attempt(keeps).unwrap_or(Keeps::Nothing);
            Some(Effect::IsNow { what: antecedent(), types, keeps })
        },
        // "it deals double that damage instead": a replacement's new amount.
        |p| {
            let source = objects(p)?;
            p.verb("deal")?;
            let n = alt!(p;
                |p| p.word("double").map(|_| Amount::Twice(Box::new(Amount::ThatDamage))),
                |p| p.word("triple").map(|_| Amount::Thrice(Box::new(Amount::ThatDamage))),
            )?;
            p.words("that damage")?;
            let to = p.attempt(|p| { p.word("to")?; recipients(p) });
            Some(Effect::ThatDamage { source, n, to })
        },
        |p| {
            p.words("double the number of")?;
            let kind = alt!(p;
                |p| { let k = counter_kind(p)?; p.word("counters")?; Some(Some(k)) },
                |p| p.word("counters").map(|_| None),
            )?;
            p.word("on")?;
            let on = objects(p)?;
            Some(Effect::DoubleCounters { kind, on })
        },
        |p| {
            p.word("distribute")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("among")?;
            let among = objects(p)?;
            Some(Effect::DistributeCounters { n, kind, among })
        },
        |p| { p.word("remove")?; let what = objects(p)?; p.words("from combat")?; Some(Effect::RemoveFromCombat(what)) },
        |p| {
            let who = subject_player(p);
            p.verb("separate")?;
            let what = objects(p)?;
            p.word("into")?;
            let n = number(p)?;
            p.word("piles")?;
            Some(Effect::SeparatePiles { who, what, n })
        },
        // "they get that many poison counters": counters on a player.
        |p| {
            let who = player(p)?;
            p.verb("get")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            Some(Effect::GetCounters { who, n, kind })
        },
        // "each player exiles cards from the top of their library until they
        // exile a nonland card".
        |p| {
            let who = subject_player(p);
            let verb = dig_verb(p)?;
            p.words("cards from the top of")?;
            let whose = whose(p)?;
            p.word("library")?;
            p.word("until")?;
            // "until they exile": the same player again, so nothing new to keep.
            let again = player(p)?;
            if again != who && !(again == Player::That && who != Player::You) { return None; }
            if dig_verb(p)? != verb { return None; }
            let until = objects(p)?;
            Some(Effect::UntilFound { who, verb, zone: Zone::TopOfLibrary(whose), until })
        },
        |p| {
            let who = subject_player(p);
            p.verb("shuffle")?;
            let w = whose(p)?;
            let mut zones = vec![zone_word(p, &w)?];
            while let Some(z) = p.attempt(|p| { p.one_of(&[", and", "and", ","])?; zone_word(p, &w) }) {
                zones.push(z);
            }
            if zones.len() < 2 { return None; }
            p.word("into")?;
            if whose(p)? != w { return None; }
            p.word("library")?;
            Some(Effect::ShuffleZones { who, zones })
        },
        |p| {
            let who = subject_player(p);
            p.one_of(&["have no maximum hand size", "has no maximum hand size"])?;
            let dur = duration(p)?;
            Some(Effect::NoMaxHandSize { who, dur })
        },
        // "it enters with a number of additional +1/+1 counters on it equal to …".
        |p| {
            let what = objects(p)?;
            // "This creature enters with …" is the card's own static.
            if what.quant == Quant::SelfRef { return None; }
            p.verb("enter")?;
            p.word("with")?;
            let counted = p.attempt(|p| p.words("a number of")).is_some();
            let n = if counted { None } else { Some(amount_a(p)?) };
            let additional = p.attempt(|p| p.word("additional")).is_some();
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.one_of(&["on it", "on them"])?;
            let n = match n {
                Some(n) => n,
                None => { p.words("equal to")?; measure(p)? }
            };
            let n = if additional { Amount::Additional(Box::new(n)) } else { n };
            Some(Effect::EntersWithCounters { what, n, kind })
        },
        |p| {
            let (after, phases) = alt!(p;
                |p| {
                    let after = this_phase(p)?;
                    p.tok(&Tok::Comma)?;
                    p.one_of(&["there is", "there's"])?;
                    Some((after, extra_phases(p)?))
                },
                |p| {
                    p.one_of(&["there is", "there's"])?;
                    let phases = extra_phases(p)?;
                    Some((this_phase(p)?, phases))
                },
            )?;
            Some(Effect::AdditionalPhases { after, phases })
        },
        |p| { let who = player(p)?; p.one_of(&["does the same", "do the same"])?; Some(Effect::DoesTheSame(who)) },
        |p| { let who = subject_player(p); p.verb("skip")?; p.words("that turn")?; Some(Effect::SkipTurn(who)) },
        // "If it would leave the battlefield, exile it instead of putting it
        // anywhere else": a replacement effect an effect creates.
        |p| {
            p.word("if")?;
            let what = objects(p)?;
            // "If ~ would be put into a graveyard …" on an instant is the
            // card's own static, which works from any zone.
            if what.quant == Quant::SelfRef { return None; }
            p.word("would")?;
            let event = would(p, &what)?;
            p.tok(&Tok::Comma)?;
            let instead = clauses(p)?;
            p.attempt(|p| p.word("instead"));
            Some(Effect::Replacement { event: Box::new(event), instead })
        },
        // "Those tokens plus an additional Food token are created instead".
        |p| {
            let those = pronoun(p)?;
            if those.quant != Quant::Antecedent { return None; }
            p.word("plus")?;
            let (n, token) = extra_token(p)?;
            p.words("are created")?;
            Some(Effect::CreateThosePlus { by: Actor::Passive, those, n, token })
        },
        |p| {
            let who = subject_player(p);
            p.verb("create")?;
            let those = pronoun(p)?;
            if those.quant != Quant::Antecedent { return None; }
            p.word("plus")?;
            let (n, token) = extra_token(p)?;
            Some(Effect::CreateThosePlus { by: Actor::Player(who), those, n, token })
        },
        // "create five of those tokens": more of the tokens an earlier
        // effect made.
        |p| {
            let who = subject_player(p);
            p.verb("create")?;
            let n = amount(p)?;
            p.word("of")?;
            let those = pronoun(p)?;
            if those.quant != Quant::Antecedent { return None; }
            Some(Effect::CreateThose { by: Actor::Player(who), n, those })
        },
        |p| {
            let what = objects(p)?;
            p.verb("gain")?;
            p.words("all creature types")?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::AlsoIs { what, types: TypeSet::EveryCreatureType, dur })
        },
        |p| {
            p.one_of(&["spend mana as though it were mana of any color", "spend mana as though it were mana of any type"])?;
            spend_uses(p).map(Effect::SpendAsAnyColor)
        },
        // "you may have that player lose 2 life": a verb you make another do.
        |p| {
            p.word("have")?;
            let who = player(p)?;
            let outer = p.implied.replace(who.clone());
            let e = p.attempt(effect);
            p.implied = outer;
            // The verb must be the named player's, not one with a subject of its own.
            e.filter(|e| subject_of(e) == Some(who))
        },
        |p| p.words("you become the monarch").map(|_| Effect::Designation("monarch".into())),
        |p| p.words("you take the initiative").map(|_| Effect::Designation("initiative".into())),
        |p| p.words("the ring tempts you").map(|_| Effect::Designation("ring".into())),
        |p| {
            p.word("choose")?;
            let c = alt!(p;
                |p| p.words("a color").map(|_| Choice::Color),
                |p| p.words("a creature type").map(|_| Choice::CreatureType),
                |p| p.words("a card type").map(|_| Choice::CardType),
                |p| p.words("a card name").map(|_| Choice::CardName),
                |p| p.words("a nonland card name").map(|_| Choice::NonlandCardName),
                |p| p.words("a number").map(|_| Choice::Number),
                // "choose artifact, creature, enchantment, instant, or
                // sorcery": a card type, from those listed.
                |p| {
                    let ty = |p: &mut P| { let w = p.peek_word().filter(|w| vocab().card_types.contains(*w))?.to_string(); p.bump(); Some(w) };
                    let mut v = vec![ty(p)?];
                    while let Some(t) = p.attempt(|p| { p.one_of(&[", or", ","])?; ty(p) }) { v.push(t); }
                    (v.len() > 1).then_some(Choice::CardTypeAmong(v))
                },
                |p| p.one_of(&["an opponent", "target opponent"]).map(|_| Choice::Opponent),
                |p| p.one_of(&["a player", "target player"]).map(|_| Choice::Player),
                |p| objects(p).map(Choice::Objects),
            )?;
            Some(Effect::Choose(c))
        },
        |p| {
            p.words("you get")?;
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Effect::GetEnergy(n))
        },
        |p| {
            p.words("you get an emblem with")?;
            let mut v = vec![quoted(p)?];
            while let Some(a) = p.attempt(|p| { p.word("and")?; quoted(p) }) { v.push(a); }
            Some(Effect::Emblem(v))
        },
        |p| { let what = objects(p)?; p.verb("enter")?; p.word("tapped")?; Some(Effect::EntersTapped(what)) },
        // "… into your hand and the rest on the bottom of your library": the
        // verb was said once for both.
        |p| {
            let what = objects(p)?;
            // A bare "on" here would start "on the battlefield" as a post.
            if p.peek_word() == Some("on") && !matches!(p.t.get(p.i + 1), Some(Tok::Word(w)) if w == "top" || w == "the") { return None; }
            let to = put_to(p)?;
            let order = p.attempt(order);
            Some(to.moving(what, None, order))
        },
        // moves
        |p| {
            p.word("return")?;
            let what = objects(p)?;
            let from = p.attempt(|p| { p.word("from")?; zone(p) });
            p.word("to")?;
            let to = destination(p)?;
            Some(to.moving(what, from, None))
        },
        // "Return to your hand target artifact card …": where first, then what.
        |p| {
            p.word("return")?;
            let from = p.attempt(|p| { p.word("from")?; zone(p) });
            p.word("to")?;
            let to = destination(p)?;
            let what = objects(p)?;
            Some(to.moving(what, from, None))
        },
        |p| { p.word("put")?; put_rest(p) },
        // counter
        |p| {
            p.word("counter")?;
            let what = objects(p)?;
            let unless = p.attempt(|p| { p.word("unless")?; let who = player(p)?; p.verb("pay")?; Some((who, costs(p)?)) });
            Some(Effect::Counter { what, unless })
        },
        // tokens
        |p| {
            p.word("create")?;
            p.words("a token that's a copy of")?;
            let of = objects(p)?;
            let except = p.attempt(exceptions).unwrap_or_default();
            Some(Effect::CreateCopy { n: Amount::N(1), of, except })
        },
        |p| {
            let who = subject_player(p);
            p.verb("create")?;
            let (n, made) = token(p)?;
            let first = Effect::Create { who: who.clone(), n, token: made };
            // "create a Food token or a Treasure token".
            let mut v = vec![first.clone()];
            while let Some((n, made)) = p.attempt(|p| { p.word("or")?; token(p) }) {
                v.push(Effect::Create { who: who.clone(), n, token: made });
            }
            Some(if v.len() == 1 { first } else { Effect::Or(v) })
        },
        // counters
        |p| {
            p.word("put")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("on")?;
            let on = objects(p)?;
            Some(Effect::PutCounters { n, kind, on })
        },
        |p| {
            p.word("remove")?;
            let n = amount_a(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.word("from")?;
            let from = objects(p)?;
            Some(Effect::RemoveCounters { n, kind, from })
        },
        // life
        |p| {
            let who = subject_player(p);
            p.verb("gain")?;
            let n = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(n) },
                |p| { p.words("life equal to")?; measure(p) },
            )?;
            Some(Effect::GainLife { who, n })
        },
        |p| {
            let who = subject_player(p);
            p.verb("lose")?;
            let n = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(n) },
                |p| { p.words("life equal to")?; measure(p) },
            )?;
            Some(Effect::LoseLife { who, n })
        },
        // mana
        |p| { p.word("add")?; mana_add(p).map(Effect::AddMana) },
        // library
        |p| {
            let who = subject_player(p);
            p.verb("search")?;
            let whose = whose(p)?;
            let zones = alt!(p;
                |p| p.words("library and/or graveyard").map(|_| vec![Zone::Library(whose.clone()), Zone::Graveyard(whose.clone())]),
                |p| p.words("graveyard , hand , and library").map(|_| vec![Zone::Graveyard(whose.clone()), Zone::Hand(whose.clone()), Zone::Library(whose.clone())]),
                |p| p.word("library").map(|_| vec![Zone::Library(whose.clone())]),
            )?;
            p.word("for")?;
            let what = objects(p)?;
            Some(Effect::Search { who, zones, what })
        },
        |p| {
            let who = subject_player(p);
            p.verb("shuffle")?;
            let what = objects(p)?;
            p.word("into")?;
            p.one_of(&["their library", "its owner 's library", "your library", "their owners ' libraries"])?;
            Some(Effect::ShuffleInto { who, what })
        },
        |p| { let who = subject_player(p); p.verb("shuffle")?; p.attempt(|p| p.one_of(&["your library", "their library"])); Some(Effect::Shuffle(who)) },
        |p| { p.word("scry")?; amount(p).map(Effect::Scry) },
        |p| { p.word("surveil")?; amount(p).map(Effect::Surveil) },
        |p| { let who = subject_player(p); p.verb("mill")?; let n = cards(p)?; Some(Effect::Mill { who, n }) },
        |p| { let who = subject_player(p); p.verb("mill")?; p.words("cards equal to")?; let n = measure(p)?; Some(Effect::Mill { who, n }) },
        |p| { let who = subject_player(p); p.verb("look at")?; let (n, whose) = top_of_library(p)?; Some(Effect::LookAtTop { who, whose, n }) },
        |p| { let who = subject_player(p); p.verb("look at")?; let whose = whose(p)?; p.word("hand")?; Some(Effect::LookAtHand { who, whose }) },
        |p| { let who = subject_player(p); p.verb("reveal")?; let (n, whose) = top_of_library(p)?; Some(Effect::RevealTop { who, whose, n }) },
        |p| { let who = subject_player(p); p.verb("reveal")?; let what = objects(p)?; Some(Effect::Reveal { who, what }) },
        // permanents
        |p| { p.word("tap")?; objects(p).map(Effect::Tap) },
        |p| { p.word("untap")?; objects(p).map(Effect::Untap) },
        |p| { let who = subject_player(p); p.verb("sacrifice")?; let what = objects(p)?; Some(Effect::Sacrifice { who, what }) },
        |p| { let a = objects(p)?; p.verb("fight")?; let b = objects(p)?; Some(Effect::Fight { a, b }) },
        |p| { p.words("gain control of")?; let what = objects(p)?; let dur = p.attempt(duration).unwrap_or(Duration::Permanent); Some(Effect::GainControl { what, dur }) },
        |p| { p.word("transform")?; objects(p).map(Effect::Transform) },
        |p| { p.word("regenerate")?; objects(p).map(Effect::Regenerate) },
        |p| { p.word("attach")?; let what = objects(p)?; p.word("to")?; let to = objects(p)?; Some(Effect::Attach { what, to }) },
        |p| {
            p.word("copy")?;
            let what = objects(p)?;
            let may_choose_new_targets = p.attempt(|p| {
                p.one_of(&["and may choose a new target for", "and may choose new targets for"])?;
                p.one_of(&["the copy", "that copy"])
            }).is_some();
            Some(Effect::Copy { what, may_choose_new_targets })
        },
        |p| { p.words("choose new targets for")?; objects(p).map(Effect::ChooseNewTargets) },
        |p| {
            p.word("play")?;
            let n = amount_a(p)?;
            p.word("additional")?;
            p.one_of(&["lands", "land"])?;
            let dur = duration(p)?;
            Some(Effect::ExtraLandPlays { n, dur })
        },
        |p| {
            p.word("cast")?;
            let what = objects(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            p.words("as though they had flash")?;
            Some(Effect::CastAsThoughFlash { what, dur })
        },
        |p| {
            let verb = alt!(p;
                |p| p.word("play").map(|_| CastVerb::Play),
                |p| p.word("cast").map(|_| CastVerb::Cast),
            )?;
            let what = objects(p)?;
            // "You may cast this card from your graveyard" is the card's own
            // rule about being cast, a static ability, never its effect.
            if what.quant == Quant::SelfRef { return None; }
            let free = |p: &mut P| p.one_of(&["without paying its mana cost", "without paying their mana costs"]).map(|_| Payment::Free);
            let payment = p.attempt(free);
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            // "play it this turn without paying its mana cost".
            let payment = payment.or_else(|| p.attempt(free)).unwrap_or(Payment::ManaCost);
            Some(Effect::Cast { verb, what, payment, dur })
        },
        |p| {
            // "they gain indestructible" is said of creatures as often as of
            // players, so a player gaining a keyword is named outright.
            if p.peek_word() == Some("they") { return None; }
            let who = player(p)?;
            p.verb("gain")?;
            let keywords = keyword_list(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::PlayerGains { who, keywords, dur })
        },
        |p| {
            let who = player(p)?;
            let rule = player_rule(p)?;
            let dur = duration(p)?;
            Some(Effect::PlayerRestrict { who, rule, dur })
        },
        |p| {
            let who = whose(p)?;
            p.words("life total can't change")?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::PlayerRestrict { who, rule: PlayerRule::LifeTotalCantChange, dur })
        },
        |p| { let what = objects(p)?; p.verb("phase out")?; Some(Effect::PhaseOut(what)) },
        // Replacement effects' own verbs: "it creates twice that many of those tokens".
        |p| {
            let by = actor(p)?;
            p.verb("create")?;
            let n = amount(p)?;
            p.word("of")?;
            let those = objects(p)?;
            Some(Effect::CreateThose { by, n, those })
        },
        |p| {
            let by = actor(p)?;
            p.verb("put")?;
            let n = amount(p)?;
            p.word("of")?;
            let mut those = objects(p)?;
            // "those counters on that permanent": the object's own "on" got
            // there first, and it is where they go.
            let on = match those.filter.posts.last() {
                Some(Post::On(o)) => { let o = (**o).clone(); those.filter.posts.pop(); o }
                _ => { p.word("on")?; objects(p)? }
            };
            Some(Effect::PutThose { by, n, those, on })
        },
        // "put twice that many of each of those kinds of counters on that
        // permanent or player instead", said as an imperative.
        |p| {
            let who = subject_player(p);
            p.verb("put")?;
            let n = amount(p)?;
            p.word("of")?;
            let those = pronoun(p)?;
            if those.quant != Quant::Antecedent || !matches!(those.filter.kind, Some(Noun::Counter(_))) { return None; }
            p.word("on")?;
            let on = objects(p)?;
            Some(Effect::PutThose { by: Actor::Player(who), n, those, on })
        },
        // "twice that many of those tokens are created instead".
        |p| {
            let n = amount(p)?;
            p.word("of")?;
            let those = objects(p)?;
            p.words("are created")?;
            Some(Effect::CreateThose { by: Actor::Passive, n, those })
        },
        // "that many plus one +1/+1 counters are put on it".
        |p| {
            let n = amount(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            p.one_of(&["are put on", "is put on"])?;
            let on = objects(p)?;
            Some(Effect::CountersArePut { n, kind, on })
        },
        // A verb that takes no player, with one doing it.
        |p| {
            let by = match p.attempt(player) {
                Some(by) => by,
                // "that player reveals …, then puts it into their hand".
                None if matches!(p.peek_word(), Some("puts" | "adds" | "exiles")) => p.implied.clone()?,
                None => return None,
            };
            if by == Player::You { return None; }
            let e = alt!(p;
                |p| { p.verb("put")?; put_rest(p) },
                |p| {
                    p.verb("put")?;
                    let n = amount_a(p)?;
                    let kind = counter_kind(p)?;
                    p.one_of(&["counters", "counter"])?;
                    p.word("on")?;
                    let on = objects(p)?;
                    Some(Effect::PutCounters { n, kind, on })
                },
                |p| { p.verb("choose")?; objects(p).map(|o| Effect::Choose(Choice::Objects(o))) },
                |p| { p.verb("add")?; mana_add(p).map(Effect::AddMana) },
                |p| { p.verb("exile")?; objects(p).map(Effect::Exile) },
                |p| {
                    p.verb("gain")?;
                    p.words("control of")?;
                    let what = objects(p)?;
                    let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
                    Some(Effect::GainControl { what, dur })
                },
            )?;
            Some(Effect::Performed { by, effect: Box::new(e) })
        },
        |p| p.words("take an extra turn after this one").map(|_| Effect::ExtraTurn),
        // "The next spell you cast this turn has improvise": said of a spell
        // not yet cast, so "has" and not "gains", and no duration.
        |p| {
            let what = objects(p)?;
            if what.quant != Quant::Next { return None; }
            p.verb("have")?;
            let gains = keyword_list(p)?;
            Some(Effect::Modify { what, pt: None, gains, dur: Duration::Permanent })
        },
        // modify
        |p| {
            let what = objects(p)?;
            // "permanents you control also gain indestructible": "also" adds
            // to what came before, as a later effect does anyway.
            p.attempt(|p| p.word("also"));
            let (pt, gains) = alt!(p;
                |p| {
                    p.verb("get")?;
                    let pt = pt_signed(p)?;
                    let gains = p.attempt(|p| { p.word("and")?; p.verb("gain")?; keyword_list(p) }).unwrap_or_default();
                    Some((Some(pt), gains))
                },
                // "gain trample and get +X/+X".
                |p| {
                    p.verb("gain")?;
                    let gains = keyword_list(p)?;
                    p.word("and")?;
                    p.verb("get")?;
                    Some((Some(pt_signed(p)?), gains))
                },
                |p| { p.verb("gain")?; Some((None, keyword_list(p)?)) },
            )?;
            // "That token gains haste.": a keyword with no duration stays. A
            // bonus with none needs "Until end of turn, …" before it, or
            // "+2/+0 and … +0/+2 until end of turn" would give the first forever.
            let dur = match p.attempt(duration) {
                Some(d) => d,
                None if pt.is_none() || p.during => Duration::Permanent,
                None => return None,
            };
            Some(Effect::Modify { what, pt, gains, dur })
        },
        |p| {
            let what = objects(p)?;
            p.verb("gain")?;
            let ability = quoted(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::GrantAbility { what, ability: Box::new(ability), dur })
        },
        |p| {
            let what = objects(p)?;
            let rule = restriction(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Restrict { what, rule, dur })
        },
        // keyword actions: "proliferate", "investigate", "~ explores", "amass Zombies 2"
        |p| {
            let by = p.attempt(objects);
            let w = p.peek_word()?;
            let base = w.strip_suffix("es").filter(|b| vocab().keyword_actions.contains(*b))
                .or_else(|| w.strip_suffix('s').filter(|b| vocab().keyword_actions.contains(*b)))
                .unwrap_or(w);
            if !vocab().keyword_actions.contains(base) { return None; }
            let base = base.to_string();
            p.bump();
            if let Some(n) = p.attempt(amount) {
                return Some(Effect::Action { by, name: base, n: Some(n) });
            }
            p.end_of_clause().then_some(Effect::Action { by, name: base, n: None })
        },
        |p| p.words("venture into the dungeon").map(|_| Effect::Action { by: None, name: "venture into the dungeon".into(), n: None }),
        |p| {
            p.word("amass")?;
            let of = match type_word(p.peek_word()?)? { Adj::Subtype(s) => s, _ => return None };
            p.bump();
            let n = amount(p)?;
            Some(Effect::Amass { of, n })
        },
        // "Its controller manifests the top card of their library", "goad
        // target creature": a keyword action done by a player to objects.
        |p| {
            let who = subject_player(p);
            let name = keyword_action_verb(p)?;
            let what = objects(p)?;
            Some(Effect::ActionOn { who, name, what })
        },
    )
}

fn dig_verb(p: &mut P) -> Option<DigVerb> {
    alt!(p;
        |p| p.verb("exile").map(|_| DigVerb::Exile),
        |p| p.verb("reveal").map(|_| DigVerb::Reveal),
    )
}

/// "hand", "graveyard" after a possessive said once for both.
fn zone_word(p: &mut P, w: &Player) -> Option<Zone> {
    alt!(p;
        |p| p.word("hand").map(|_| Zone::Hand(w.clone())),
        |p| p.word("graveyard").map(|_| Zone::Graveyard(w.clone())),
    )
}

/// "after this main phase", "after this phase": `None` when any phase.
fn this_phase(p: &mut P) -> Option<Option<PhaseKind>> {
    p.words("after this")?;
    let kind = p.attempt(phase_kind);
    p.word("phase")?;
    Some(kind)
}

fn phase_kind(p: &mut P) -> Option<PhaseKind> {
    alt!(p;
        |p| p.word("combat").map(|_| PhaseKind::Combat),
        |p| p.word("main").map(|_| PhaseKind::Main),
    )
}

/// "an additional combat phase followed by an additional main phase".
fn extra_phases(p: &mut P) -> Option<Vec<PhaseKind>> {
    let one = |p: &mut P| {
        p.words("an additional")?;
        let k = phase_kind(p)?;
        p.word("phase")?;
        Some(k)
    };
    let mut v = vec![one(p)?];
    while let Some(k) = p.attempt(|p| {
        p.words("followed by")?;
        one(p)
    }) {
        v.push(k);
    }
    Some(v)
}

/// "an additional Food token": one more, on top of the tokens named.
fn extra_token(p: &mut P) -> Option<(Amount, Token)> {
    let (n, t) = token(p)?;
    matches!(&n, Amount::Additional(_)).then_some((n, t))
}

/// Keyword actions whose object is an object phrase and that no rule of
/// their own reads. "Exile", "destroy" and the rest have rules that read
/// what follows them; letting this one take them too would let a line those
/// rules refuse slip through as a bare action.
const ACTIONS_ON_OBJECTS: &[&str] = &[
    "goad", "manifest", "detain", "cloak", "suspect", "exert", "airbend", "harness",
];

/// One of those in either number: "manifest", "manifests".
fn keyword_action_verb(p: &mut P) -> Option<String> {
    let w = p.peek_word()?;
    let base = [w.strip_suffix("es"), w.strip_suffix('s'), Some(w)]
        .into_iter()
        .flatten()
        .find(|b| ACTIONS_ON_OBJECTS.contains(b))?
        .to_string();
    p.bump();
    Some(base)
}

/// ", except it's an artifact in addition to its other types", the values a
/// copy has that its original does not.
pub(super) fn exceptions(p: &mut P) -> Option<Vec<Exception>> {
    p.words(", except")?;
    let mut v = vec![exception(p)?];
    while let Some(e) = p.attempt(|p| {
        p.one_of(&[", and", ",", "and"])?;
        exception(p)
    }) {
        v.push(e);
    }
    Some(v)
}

fn exception(p: &mut P) -> Option<Exception> {
    alt!(p;
        |p| {
            p.one_of(&["it's a", "it's an"])?;
            let types = group(p)?;
            p.words("in addition to its other types")?;
            Some(Exception::AlsoIs(types))
        },
        |p| { p.one_of(&["it isn't", "it's not", "the token isn't", "the token is not"])?; group(p).map(Exception::IsNot) },
        |p| p.words("it has this ability").map(|_| Exception::HasThisAbility),
        |p| { p.one_of(&["it has", "the token has"])?; keyword_list(p).map(Exception::Has) },
        |p| {
            p.words("it enters with")?;
            let n = amount_a(p)?;
            let n = match p.attempt(|p| p.word("additional")) {
                Some(_) => Amount::Additional(Box::new(n)),
                None => n,
            };
            let kind = counter_kind(p)?;
            p.one_of(&["counter on it", "counters on it"])?;
            let cond = p.attempt(|p| { p.word("if")?; condition(p) }).map(Box::new);
            Some(Exception::EntersWithCounters { n, kind, cond })
        },
    )
}

/// "put" after its verb: what, from where, to where.
pub(super) fn put_rest(p: &mut P) -> Option<Effect> {
    let what = objects(p)?;
    let from = p.attempt(|p| {
        p.word("from")?;
        zone(p)
    });
    // "put them back": where they were looked at, the top.
    let to = alt!(p;
        |p| p.word("back").map(|_| Dest::to(Zone::OwnersLibraryTop)),
        put_to,
    )?;
    // "Put your commander into your hand from the command zone."
    let from = match from {
        Some(z) => Some(z),
        None => p.attempt(|p| {
            p.word("from")?;
            zone(p)
        }),
    };
    let order = p.attempt(order);
    Some(to.moving(what, from, order))
}

/// Who does what a replacement effect watches for.
pub(super) fn actor(p: &mut P) -> Option<Actor> {
    alt!(p;
        |p| p.words("an effect").map(|_| Actor::AnEffect),
        |p| p.word("it").map(|_| Actor::ThatEffect),
    )
}

/// One player or object an effect is done to.
pub(super) fn recipient(p: &mut P) -> Option<Recipient> {
    alt!(p;
        |p| {
            p.words("the player or planeswalker")?;
            let by = alt!(p;
                |p| p.word("it's").map(|_| antecedent()),
                |p| { let o = objects(p)?; p.word("is")?; Some(o) },
            )?;
            p.word("attacking")?;
            Some(Recipient::AttackedBy(by))
        },
        |p| {
            // "target player or planeswalker": one target of either kind.
            let pl = player_simple(p)?;
            let quant = match pl { Player::Target | Player::TargetOpponent => Quant::Target { n: Amount::N(1), up_to: false }, _ => return None };
            p.word("or")?;
            let f = filter(p)?;
            Some(Recipient::Or(vec![Recipient::Player(pl), Recipient::Objects(Objects { quant, filter: f })]))
        },
        // "that player or permanent": "that" said once for both.
        |p| {
            if player_simple(p)? != Player::That { return None; }
            p.word("or")?;
            let f = filter(p)?;
            Some(Recipient::Or(vec![Recipient::Player(Player::That), Recipient::Objects(Objects { quant: Quant::Antecedent, filter: f })]))
        },
        // "that player or a planeswalker they control".
        |p| {
            let pl = player_simple(p)?;
            p.word("or")?;
            let o = objects(p)?;
            if o.quant == Quant::Plural { return None; }
            Some(Recipient::Or(vec![Recipient::Player(pl), Recipient::Objects(o)]))
        },
        |p| player_simple(p).or_else(|| p.words("each player").map(|_| Player::Each)).or_else(|| p.words("each opponent").map(|_| Player::EachOpponent)).or_else(|| p.word("you").map(|_| Player::You)).map(Recipient::Player),
        |p| objects(p).map(Recipient::Objects),
    )
}

pub(super) fn recipients(p: &mut P) -> Option<Vec<Recipient>> {
    let one = recipient;
    let mut v = vec![one(p)?];
    while let Some(r) = p.attempt(|p| {
        p.attempt(|p| p.tok(&Tok::Comma));
        p.word("and")?;
        let r = one(p)?;
        // "any target and you gain 3 life": a recipient followed by a verb was
        // the subject of the next clause, not one more recipient.
        let ok = match p.peek_word() {
            None => true,
            Some(w) => matches!(
                w,
                "and" | "then" | "equal" | "where" | "for" | "this" | "instead" | "unless" | "if"
            ),
        };
        ok.then_some(r)
    }) {
        v.push(r);
    }
    Some(v)
}
