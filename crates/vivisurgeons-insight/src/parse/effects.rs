//! One effect: a verb, its subject and its objects.

use super::*;
use crate::lex::PtPart;

pub(super) fn duration(p: &mut P) -> Option<Duration> {
    alt!(p;
        |p| p.words("until end of turn").map(|_| Duration::UntilEndOfTurn),
        |p| p.words("this turn").map(|_| Duration::ThisTurn),
        |p| p.words("until your next turn").map(|_| Duration::UntilYourNextTurn),
        |p| p.words("until the end of your next turn").map(|_| Duration::UntilEndOfYourNextTurn),
        |p| p.words("for as long as you control ~").map(|_| Duration::WhileSelfOnBattlefield),
        |p| p.words("until ~ leaves the battlefield").map(|_| Duration::WhileSelfOnBattlefield),
        |p| p.words("for the rest of the game").map(|_| Duration::RestOfGame),
        |p| { p.words("for as long as")?; condition(p).map(|c| Duration::ForAsLongAs(Box::new(c))) },
        // "until this card is cast from exile".
        |p| {
            p.word("until")?;
            let what = objects(p)?;
            p.words("is cast from")?;
            let zone = zone(p)?;
            Some(Duration::UntilEvent(Box::new(Event::CastFrom { what, zone })))
        },
    )
}

/// The colors and types a "becomes" gives: "a 4/4 black and green Horror
/// creature" is two colors and one creature (CR 105.2).
pub(super) fn becomes_types(p: &mut P) -> Vec<Adj> {
    let mut types = Vec::new();
    while let Some(a) = p.attempt(|p| {
        if matches!(types.last(), Some(Adj::Color(_))) && p.attempt(|p| p.word("and")).is_some() {
            return descriptor(p).filter(|a| matches!(a, Adj::Color(_)));
        }
        descriptor(p)
    }) {
        types.push(a);
    }
    types
}

/// ". It's still a land.": a sentence of its own after a "becomes", saying
/// what "that's still a land" says inside it.
pub(super) fn still(p: &mut P) -> Option<Keeps> {
    // A closing quote ends the sentence before it: "with "…." It's still a
    // land."
    if p.i == 0 || p.t.get(p.i - 1) != Some(&Tok::Quote) {
        p.tok(&Tok::Period)?;
    }
    p.words("it's still")?;
    p.one_of(&["a", "an"])?;
    group(p).map(Keeps::Listed)
}

/// What a "becomes" keeps of the types it had: "in addition to its other
/// types", "that's still a land".
/// "pays {1} for each card in your hand", "pays {2} plus an additional {2}
/// for each card named ~ in each graveyard": one payment whose mana is
/// counted, not the effect done once for each object.
pub(super) fn per_costs(p: &mut P, mut c: Vec<Cost>) -> Vec<Cost> {
    let [Cost::Mana(m)] = c.as_slice() else {
        return c;
    };
    let m = m.clone();
    if let Some(per) = p.attempt(|p| {
        p.words("for each")?;
        objects(p)
    }) {
        return vec![Cost::ManaPer { mana: m, per }];
    }
    if let Some((mana, per)) = p.attempt(|p| {
        p.words("plus an additional")?;
        let m2 = mana(p)?;
        p.words("for each")?;
        Some((m2, objects(p)?))
    }) {
        c.push(Cost::ManaPer { mana, per });
    }
    c
}

pub(super) fn keeps(p: &mut P) -> Option<Keeps> {
    alt!(p;
        |p| p.one_of(&["in addition to its other colors and types", "in addition to their other colors and types"]).map(|_| Keeps::OtherColorsAndTypes),
        |p| p.one_of(&["in addition to its other types", "in addition to their other types"]).map(|_| Keeps::OtherTypes),
        |p| p.one_of(&["in addition to its other creature types", "in addition to their other creature types"]).map(|_| Keeps::OtherCreatureTypes),
        |p| { p.one_of(&["that's still a", "that's still an", "that are still"])?; group(p).map(Keeps::Listed) },
    )
}

pub(super) fn cards(p: &mut P) -> Option<Amount> {
    // "draw half X cards, rounded down".
    if let Some(n) = p.attempt(|p| {
        p.word("half")?;
        let of = amount(p)?;
        p.word("cards")?;
        let round_up = rounded(p)?;
        Some(Amount::Half {
            of: Box::new(of),
            round_up,
        })
    }) {
        return Some(n);
    }
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
    // "that many cards from the top of your library": the same cards, named
    // from the top down (`syn::top_cards`).
    if let Some(r) = p.attempt(|p| {
        let n = amount_a(p)?;
        p.one_of(&["cards", "card"])?;
        p.words("from the top of")?;
        let whose = whose(p)?;
        p.word("library")?;
        Some((n, whose))
    }) {
        return Some(r);
    }
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
    match p.attempt(player) {
        // "Any number of target players each mill two cards": "each"
        // restates that every one of them does.
        Some(Player::AnyNumberOfTarget) => {
            p.attempt(|p| p.word("each"));
            Player::AnyNumberOfTarget
        }
        Some(who) => who,
        None => p.implied.clone().unwrap_or(Player::You),
    }
}

pub(super) fn token(p: &mut P) -> Option<(Amount, Token)> {
    match token_named(p)? {
        (n, t, None) => Some((n, t)),
        _ => None,
    }
}

/// A token, and the name "named Munitions" gives it.
pub(super) fn token_named(p: &mut P) -> Option<(Amount, Token, Option<String>)> {
    // "create half X Food tokens, rounded up": the rounding is said after
    // the tokens (CR 107.1a).
    let half = p.attempt(|p| p.word("half")).is_some();
    // "a number of Treasure tokens equal to the result".
    let n = match p.attempt(|p| p.words("a number of")) {
        Some(_) if !half => None,
        Some(_) => return None,
        None if half => Some(amount(p)?),
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
    // "a Monster Role token": a predefined Role (CR 111.10), named by the
    // word before its subtype.
    if let Some(w) = p.attempt(|p| {
        let w = p.peek_word()?.to_string();
        p.bump();
        p.word("role")?;
        matches!(p.peek_word(), Some("token" | "tokens")).then_some(w)
    }) {
        types.push("role".to_string());
        named = Some(format!("{w} role"));
    }
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
    let n = match n {
        Some(of) if half => Some(Amount::Half {
            of: Box::new(of),
            round_up: rounded(p)?,
        }),
        n => n,
    };
    if pt.is_some() {
        named = None;
    }
    // "a colorless artifact token named Munitions with …".
    let name = p.attempt(|p| {
        p.word("named")?;
        // "named Boulder that's tapped and attacking", "named Cadet, where
        // X is …": a relative clause or a definition after the name is no
        // part of it.
        card_name(p, &["with", "that's", "that", "where"])
    });
    // Overlord of the Hauntwoods: "a … land token named Everywhere that is
    // every basic land type": the five subtypes (CR 205.3i).
    if name.is_some()
        && p.attempt(|p| p.words("that is every basic land type"))
            .is_some()
    {
        types.extend(BASIC_LAND_TYPES.iter().map(|t| t.to_string()));
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
    // "a 1/2 blue Bird creature token with flying named Storm Crow": the
    // name after what the token has.
    let name = match name {
        None if !keywords.is_empty() || !abilities.is_empty() => p.attempt(|p| {
            p.word("named")?;
            card_name(p, &["that's", "that", "where", "at", "for", "until"])
        }),
        name => name,
    };
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
    // "that's tapped and attacking that player or a planeswalker they control";
    // with none named, its controller chooses (CR 508.4).
    let (tapped, attacking) = match p.attempt(|p| {
        p.one_of(&["that's", "that are"])?;
        let tapped = p.attempt(|p| p.words("tapped and")).is_some();
        p.word("attacking")?;
        let r = match p.attempt(recipient) {
            // "that's attacking that opponent" (Combat Calligrapher): the
            // player the trigger saw attacked, a player and not an object.
            Some(Recipient::Objects(o))
                if o.quant == Quant::Antecedent
                    && o.filter.kind == Some(Noun::Opponent)
                    && o.filter.alts.is_empty()
                    && o.filter.posts.is_empty() =>
            {
                Recipient::Player(Player::That)
            }
            Some(r) => r,
            None => Recipient::ControllerChooses,
        };
        Some((tapped, r))
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
        name,
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
        |p| p.words("can attack as though it didn't have defender").map(|_| Restriction::AttacksDespiteDefender),
        |p| { p.words("can block only")?; filter(p).map(Restriction::CanBlockOnly) },
        |p| p.words("can't be blocked by more than one creature").map(|_| Restriction::CantBeBlockedByMoreThanOne),
        |p| { p.words("can't be blocked by")?; filter(p).map(Restriction::CantBeBlockedBy) },
        |p| { p.words("can block an additional")?; p.attempt(|p| p.word("creature")); p.words("each combat").map(|_| Restriction::CanBlockAdditional(Amount::N(1))) },
        |p| p.words("can block any number of creatures").map(|_| Restriction::CanBlockAnyNumber),
        |p| p.words("can't attack or block").map(|_| Restriction::CantAttackOrBlock),
        // "can't block this creature", "can't block it": one attacker, the
        // card or what was named before.
        |p| {
            p.words("can't block")?;
            let o = objects(p)?;
            matches!(o.quant, Quant::SelfRef | Quant::Antecedent).then_some(Restriction::CantBlockObject(o))
        },
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
        |p| p.words("can't attack a player it has already attacked this turn").map(|_| Restriction::CantAttackPlayerAttackedThisTurn),
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
        |p| p.words("can't be sacrificed").map(|_| Restriction::CantBeSacrificed),
        |p| p.words("can't be regenerated").map(|_| Restriction::CantBeRegenerated),
        |p| p.words("can't enter the battlefield").map(|_| Restriction::CantEnterBattlefield),
        |p| p.words("can't be turned face up").map(|_| Restriction::CantBeTurnedFaceUp),
        |p| p.words("can't have counters put on it").map(|_| Restriction::CantHaveCountersPut),
        // "can't have hexproof or shroud", "can't have or gain flying".
        |p| {
            p.words("can't have")?;
            p.attempt(|p| p.words("or gain"));
            let mut v = vec![keyword(p)?];
            while let Some(k) = p.attempt(|p| { p.tok(&Tok::Comma)?; keyword(p) }) { v.push(k); }
            if let Some(k) = p.attempt(|p| { p.attempt(|p| p.tok(&Tok::Comma)); p.word("or")?; keyword(p) }) { v.push(k); }
            Some(Restriction::CantHave(v))
        },
        |p| p.words("must be blocked if able").map(|_| Restriction::MustBeBlocked),
        |p| p.one_of(&["attacks each combat if able", "attack each combat if able"]).map(|_| Restriction::AttacksEachCombat),
        |p| p.one_of(&["attacks this turn if able", "attack this turn if able"]).map(|_| Restriction::AttacksThisTurn),
        |p| p.one_of(&["blocks each combat if able", "block each combat if able"]).map(|_| Restriction::BlocksEachCombat),
        |p| p.one_of(&["doesn't untap during your untap step", "don't untap during your untap step"]).map(|_| Restriction::DoesntUntap(Player::You)),
        |p| p.words("doesn't untap during its controller 's untap step").map(|_| Restriction::DoesntUntap(Player::ControllerOf(Box::new(antecedent())))),
        |p| p.one_of(&["doesn't untap during its controller 's next untap step", "don't untap during their controllers ' next untap steps", "don't untap during their controller 's next untap step"]).map(|_| Restriction::DoesntUntapNext),
    )
}

pub(super) fn effect(p: &mut P) -> Option<Effect> {
    if let Some(e) = p.attempt(effect_first) {
        return Some(e);
    }
    if let Some(e) = p.attempt(effect_main) {
        return Some(e);
    }
    p.attempt(effect_more)
}

/// Templates an older rule would read a shorter part of and commit to:
/// bare "shuffle" before "shuffle this artifact and your graveyard into
/// …", "gain control of target opponent" before "during that player's next
/// turn", a token before "attached to it", "becomes an artifact creature"
/// before "with power and toughness each equal to …", a player exiling the
/// top cards before "until …".
fn effect_first(p: &mut P) -> Option<Effect> {
    alt!(p;
        // "exiles cards from the top of their library until that player has
        // exiled cards with total mana value 20 or greater".
        |p| {
            let who = subject_player(p);
            let verb = dig_verb(p)?;
            p.words("cards from the top of")?;
            let whose = whose(p)?;
            p.words("library until")?;
            let again = player(p)?;
            if again != who && !(again == Player::That && who != Player::You) { return None; }
            p.words("has exiled cards with total")?;
            let total = compare(p)?;
            Some(Effect::DigUntilTotal { who, verb, zone: Zone::TopOfLibrary(whose), total })
        },
        // "reveals cards from the top of their library until an artifact or
        // creature card is revealed": the passive of "until they reveal".
        |p| {
            let who = subject_player(p);
            let verb = dig_verb(p)?;
            p.words("cards from the top of")?;
            let whose = whose(p)?;
            p.words("library until")?;
            let until = objects(p)?;
            p.word("is")?;
            let said = match verb { DigVerb::Exile => "exiled", DigVerb::Reveal => "revealed" };
            p.word(said)?;
            Some(Effect::UntilFound { who, verb, zone: Zone::TopOfLibrary(whose), until })
        },
        // "Shuffle this artifact and your graveyard into their owner's
        // library": objects and whole zones, one shuffle.
        |p| {
            let who = subject_player(p);
            p.verb("shuffle")?;
            let mut what = Vec::new();
            let mut zones = Vec::new();
            loop {
                if let Some(z) = p.attempt(|p| { let w = whose(p)?; zone_word(p, &w) }) {
                    zones.push(z);
                } else {
                    what.push(objects(p)?);
                }
                if p.attempt(|p| p.word("and")).is_none() { break; }
            }
            if what.is_empty() || what.len() + zones.len() < 2 { return None; }
            p.word("into")?;
            p.one_of(&["their owner 's library", "their owners ' libraries", "your library"])?;
            Some(Effect::ShuffleAllInto { who, what, zones })
        },
        // "gain control of target opponent during that player's next turn".
        |p| {
            p.attempt(|p| p.word("you"));
            p.words("gain control of")?;
            let who = player_simple(p)?;
            p.words("during that player 's next turn")?;
            Some(Effect::ControlPlayerNextTurn(who))
        },
        // "becomes an artifact creature with power and toughness each equal
        // to its mana value".
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.one_of(&["a", "an"])?;
            let types = group(p)?;
            p.words("with power and toughness each equal to")?;
            let each_equal_to = measure(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let keeps = Keeps::implied_by(&types);
            Some(Effect::BecomesSized { what, types, keeps, each_equal_to, dur })
        },
        // "Create a Monster Role token attached to it."
        |p| {
            let who = subject_player(p);
            p.verb("create")?;
            let (n, token) = token(p)?;
            p.words("attached to")?;
            let to = objects(p)?;
            Some(Effect::CreateAttached { who, n, token, to })
        },
        // "double the value of X" (Unbound Flourishing): the spell's X.
        |p| {
            p.words("double the value of x")?;
            Some(Effect::DoubleX(antecedent()))
        },
        // "this creature becomes prepared", before "becomes" would read
        // "prepared" as a type it takes on.
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.word("prepared")?;
            Some(Effect::BecomesPrepared(what))
        },
        // "it becomes plotted" (CR 702.170c).
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.word("plotted")?;
            Some(Effect::BecomesPlotted(what))
        },
        // "target Mount you control becomes saddled until end of turn" (CR
        // 702.171b): it lasts the turn, which the text says.
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.words("saddled until end of turn")?;
            Some(Effect::BecomesSaddled(what))
        },
    )
}

fn effect_main(p: &mut P) -> Option<Effect> {
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
                // "~ deals 4 damage instead if …": to whatever the damage it
                // replaces was dealt to.
                |p| { let n = amount(p)?; p.word("damage")?; (p.peek_word() == Some("instead")).then(|| (n, vec![Recipient::Same])) },
                |p| { p.words("damage equal to")?; let n = measure(p)?; p.word("to")?; Some((n, recipients(p)?)) },
                |p| { p.words("damage to")?; let r = recipients(p)?; p.words("equal to")?; Some((measure(p)?, r)) },
            )?;
            Some(Effect::Damage { source, n, to })
        },
        // destroy / exile
        |p| {
            p.word("destroy")?;
            let what = objects(p)?;
            let no_regen = cant_be_regenerated(p, std::slice::from_ref(&what));
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
            if !p.peek_word().is_some_and(|w| matches!(w, "untap" | "tap" | "exile" | "destroy" | "return" | "put" | "attach" | "choose")) { return None; }
            effect(p)
        },
        // "you may have this land deal 3 damage to any target": after "may",
        // an object's verb is said with "have".
        |p| {
            p.word("have")?;
            let e = effect(p)?;
            // "you may have it become a 0/0 Elemental creature until end of
            // turn" (Crawling Barrens).
            // "you may have target creature get -1/-1 until end of turn".
            matches!(e, Effect::Damage { source: Some(_), .. } | Effect::Fight { .. } | Effect::Becomes { .. } | Effect::Modify { .. } | Effect::LoseKeywords { .. } | Effect::Performed { .. }).then_some(e)
        },
        |p| { p.words("damage can't be prevented")?; duration(p).map(Effect::DamageCantBePrevented) },
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
        // "Exile cards from the top of your library until you exile a
        // nonland card" is the dig below, not "exile cards".
        |p| {
            p.word("exile")?;
            let dig = p.at("cards") && matches!(p.t.get(p.i + 1), Some(Tok::Word(w)) if w == "from") && p.ahead("library until");
            if dig { return None; }
            objects(p).map(Effect::Exile)
        },
        |p| {
            let who = subject_player(p);
            p.verb("pay")?;
            let cost = alt!(p;
                |p| { let n = amount(p)?; p.word("life")?; Some(vec![Cost::PayLife(n)]) },
                |p| p.words("any amount of life").map(|_| vec![Cost::PayLife(Amount::AnyAmount)]),
                // "you may pay life equal to its power".
                |p| { p.words("life equal to")?; measure(p).map(|n| vec![Cost::PayLife(n)]) },
                |p| { p.words("any amount of")?; sym(p, "E")?; Some(vec![Cost::PayEnergy(Amount::AnyAmount)]) },
                // "pay {1} and 3 life".
                |p| { let m = mana(p)?; p.word("and")?; let n = amount(p)?; p.word("life")?; Some(vec![Cost::Mana(m), Cost::PayLife(n)]) },
                costs,
            )?;
            Some(Effect::Pay { who, cost })
        },
        prevent,
        |p| {
            let what = objects(p)?;
            p.verb("become")?;
            p.words("a copy of")?;
            let of = objects(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            let except = p.attempt(exceptions).unwrap_or_default();
            Some(Effect::BecomesCopy { what, of, dur, except })
        },
        // "this creature's base power and toughness become 4/2 until end of
        // turn", "you may have this creature's base power become equal to
        // that creature's power" (CR 613.4b).
        |p| {
            let have = p.attempt(|p| p.word("have")).is_some();
            let what = alt!(p;
                |p| p.word("its").map(|_| antecedent()),
                |p| { let o = objects(p)?; p.tok(&Tok::Possessive)?; Some(o) },
            )?;
            p.word("base")?;
            let both = alt!(p;
                |p| p.words("power and toughness").map(|_| true),
                |p| p.word("power").map(|_| false),
            )?;
            match (have, both) {
                (true, _) => p.word("become")?,
                (false, true) => p.word("become")?,
                (false, false) => p.word("becomes")?,
            }
            let (power, toughness) = alt!(p;
                |p| match p.peek()? {
                    Tok::Pt(Pt { power: PtPart::N(a), toughness: PtPart::N(b), signed: false }) if both => {
                        let (a, b) = (*a, *b);
                        p.bump();
                        Some((Some(Amount::N(u32::try_from(a).ok()?)), Some(Amount::N(u32::try_from(b).ok()?))))
                    }
                    _ => None,
                },
                |p| {
                    if !both { return None; }
                    p.words("equal to")?;
                    let of = alt!(p;
                        |p| { let o = objects(p)?; p.tok(&Tok::Possessive)?; p.words("power and toughness")?; Some(o) },
                        |p| { p.words("the power and toughness of")?; objects(p) },
                    )?;
                    Some((Some(Amount::PowerOf(Box::new(of.clone()))), Some(Amount::ToughnessOf(Box::new(of)))))
                },
                |p| {
                    if both { return None; }
                    p.words("equal to")?;
                    Some((Some(measure(p)?), None))
                },
            )?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::BaseBecomes { what, power, toughness, dur })
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
            let types = becomes_types(p);
            if types.is_empty() && pt.is_none() { return None; }
            // "becomes a Kithkin Scout with base power and toughness 2/3":
            // the size said after the types (CR 613.4b, as "a 2/3 Kithkin
            // Scout").
            let pt = match pt {
                Some(pt) => Some(pt),
                None => p.attempt(|p| {
                    p.words("with base power and toughness")?;
                    match p.peek()? { Tok::Pt(pt) if !pt.signed => { let pt = *pt; p.bump(); Some(pt) } _ => None }
                }),
            };
            // "… with flying", "… and gains flying": what the same effect
            // adds, in layer 6 either way (CR 613.1f, 613.6).
            let gains = p.attempt(|p| {
                alt!(p; |p| p.word("with"), |p| { p.word("and")?; p.verb("gain") })?;
                keyword_list(p)
            }).unwrap_or_default();
            let keeps = p.attempt(keeps);
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            // ". It's still a land." after the duration.
            let keeps = match keeps {
                None => p.attempt(still).unwrap_or_else(|| Keeps::implied_by(&types)),
                Some(k) => k,
            };
            Some(Effect::Becomes { what, pt, types, gains, keeps, dur })
        },
        |p| { let who = subject_player(p); p.verb("win")?; p.words("the game")?; Some(Effect::WinGame(who)) },
        // "your life total becomes 15", "target player's life total becomes 10".
        |p| {
            let who = whose(p)?;
            p.words("life total becomes")?;
            let n = alt!(p;
                |p| { p.words("equal to")?; measure(p) },
                measure,
                amount,
            )?;
            Some(Effect::LifeTotalBecomes { who, n })
        },
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
        // "Double target creature's power until end of turn".
        |p| {
            p.word("double")?;
            let of = objects(p)?;
            p.tok(&Tok::Possessive)?;
            let stats = alt!(p;
                |p| p.words("power and toughness").map(|_| vec![Stat::Power, Stat::Toughness]),
                |p| stat(p).map(|s| vec![s]),
            )?;
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
        // "It's an enchantment.": what it is as it enters, which "becomes"
        // would make it only afterwards (CR 611.2e).
        // "That creature is a black Zombie in addition to its other colors
        // and types": the same of a named earlier object.
        |p| {
            let what = alt!(p;
                |p| p.word("it's").map(|_| antecedent()),
                |p| {
                    let w = objects(p)?;
                    if w.quant != Quant::Antecedent { return None; }
                    p.word("is")?;
                    Some(w)
                },
            )?;
            // "That creature is black and is a Nightmare …" (Chainer): the
            // color said first, its own predicate.
            let colored = p
                .attempt(|p| {
                    let g = group(p)?;
                    if g.is_empty() || !g.iter().all(|a| matches!(a, Adj::Color(_))) { return None; }
                    p.words("and is")?;
                    Some(g)
                })
                .unwrap_or_default();
            p.one_of(&["a", "an"])?;
            // "Each of them is a 1/1 Spirit with flying in addition to its
            // other types" (Storm of Souls): the size and keywords it has.
            let pt = p.attempt(|p| match p.peek()? { Tok::Pt(pt) if !pt.signed => { let pt = *pt; p.bump(); Some(pt) } _ => None });
            let mut types = group(p)?;
            if !colored.is_empty() && types.iter().any(|a| matches!(a, Adj::Color(_))) { return None; }
            let gains = match pt {
                Some(_) => p.attempt(|p| { p.word("with")?; keyword_list(p) }).unwrap_or_default(),
                None => vec![],
            };
            let keeps = p.attempt(keeps).unwrap_or_else(|| Keeps::implied_by(&types));
            types.splice(0..0, colored);
            Some(Effect::IsNow { what, types, keeps, pt, gains })
        },
        // "it deals that much damage plus 2 instead".
        |p| {
            let source = objects(p)?;
            p.verb("deal")?;
            p.words("that much damage plus")?;
            let k = number(p)?;
            let to = p.attempt(|p| { p.word("to")?; recipients(p) });
            Some(Effect::ThatDamage { source, n: Amount::Plus(Box::new(Amount::ThatDamage), k), to })
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
            // "If that creature would die this turn, exile it instead": the
            // replacement lasts the turn.
            let this_turn = p.attempt(|p| p.words("this turn")).is_some();
            p.tok(&Tok::Comma)?;
            let instead = clauses(p)?;
            p.attempt(|p| p.word("instead"));
            let e = Effect::Replacement { event: Box::new(event), instead };
            Some(if this_turn { Effect::During { dur: Duration::ThisTurn, then: vec![e] } } else { e })
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
            p.words("spend mana as though it were mana of any color")?;
            spend_uses(p).map(|uses| Effect::SpendAsAnyColor { uses, dur: Duration::Permanent })
        },
        |p| {
            p.words("spend mana as though it were mana of any type")?;
            spend_uses(p).map(|uses| Effect::SpendAsAnyType { uses, dur: Duration::Permanent })
        },
        // "and mana of any type can be spent to cast that spell": the same
        // permission (CR 609.4b), said in the passive.
        |p| {
            p.words("mana of any type can be spent")?;
            spend_uses(p).map(|uses| Effect::SpendAsAnyType { uses, dur: Duration::Permanent })
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
                |p| p.words("odd or even").map(|_| Choice::OddOrEven),
                |p| p.words("a creature type").map(|_| Choice::CreatureType),
                |p| p.words("a card type").map(|_| Choice::CardType),
                |p| p.words("a permanent type").map(|_| Choice::PermanentType),
                |p| p.words("a card name").map(|_| Choice::CardName),
                |p| p.words("a nonland card name").map(|_| Choice::NonlandCardName),
                |p| p.words("a land card name").map(|_| Choice::LandCardName),
                |p| p.words("a number").map(|_| Choice::Number),
                // "choose artifact, creature, enchantment, instant, or
                // sorcery": a card type, from those listed.
                |p| {
                    let ty = |p: &mut P| { let w = p.peek_word().filter(|w| vocab().card_types.contains(*w))?.to_string(); p.bump(); Some(w) };
                    let mut v = vec![ty(p)?];
                    while let Some(t) = p.attempt(|p| { p.one_of(&[", or", ","])?; ty(p) }) { v.push(t); }
                    (v.len() > 1).then_some(Choice::CardTypeAmong(v))
                },
                // "choose target opponent" targets; "choose an opponent" does not.
                |p| p.words("an opponent").map(|_| Choice::Opponent),
                |p| p.words("target opponent").map(|_| Choice::TargetOpponent),
                |p| p.words("a player").map(|_| Choice::Player),
                |p| p.words("target player").map(|_| Choice::TargetPlayer),
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
        // "Each opponent loses half their life, rounded up", "Target
        // opponent mills half their library, rounded down": half of a
        // count, rounded as said (CR 107.1a).
        |p| {
            let who = subject_player(p);
            p.verb("lose")?;
            p.word("half")?;
            let w = whose(p)?;
            p.word("life")?;
            let round_up = rounded(p)?;
            Some(Effect::LoseLife { who, n: Amount::Half { of: Box::new(Amount::LifeTotal(w)), round_up } })
        },
        // "each player loses a third of their life, rounded up".
        |p| {
            let who = subject_player(p);
            p.verb("lose")?;
            p.words("a third of")?;
            let w = whose(p)?;
            p.word("life")?;
            let round_up = rounded(p)?;
            Some(Effect::LoseLife { who, n: Amount::Third { of: Box::new(Amount::LifeTotal(w)), round_up } })
        },
        |p| {
            let who = subject_player(p);
            p.verb("mill")?;
            p.word("half")?;
            let w = whose(p)?;
            p.word("library")?;
            let round_up = rounded(p)?;
            let cards = Objects { quant: Quant::Plural, filter: Filter { kind: Some(Noun::Card), posts: vec![Post::In(Zone::Library(w))], ..Filter::default() } };
            Some(Effect::Mill { who, n: Amount::Half { of: Box::new(Amount::CountOf(Box::new(cards))), round_up } })
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
            Some(entering_with(p, to.moving(what, from, None)))
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
            let unless = p.attempt(|p| { p.word("unless")?; let who = player(p)?; p.verb("pay")?; let c = costs(p)?; Some((who, per_costs(p, c))) });
            Some(Effect::Counter { what, unless })
        },
        // "turn a face-down creature you control face up".
        |p| {
            p.word("turn")?;
            let what = objects(p)?;
            p.words("face up")?;
            Some(Effect::TurnFaceUp(what))
        },
        // tokens
        |p| {
            p.word("create")?;
            // "two tapped tokens that are copies of", "a tapped and
            // attacking token that's a copy of": attacking whom its
            // controller chooses (CR 508.4).
            let n = amount_a(p)?;
            let tapped = p.attempt(|p| p.word("tapped")).is_some();
            let attacking = p
                .attempt(|p| if tapped { p.words("and attacking") } else { p.word("attacking") })
                .map(|_| Recipient::ControllerChooses);
            if n == Amount::N(1) {
                p.words("token that's a copy of")?;
            } else {
                p.words("tokens that are copies of")?;
            }
            let of = objects(p)?;
            let except = p.attempt(exceptions).unwrap_or_default();
            Some(Effect::CreateCopy { n, tapped, attacking, of, except })
        },
        |p| {
            let who = subject_player(p);
            p.verb("create")?;
            let (n, token, name) = token_named(p)?;
            Some(Effect::CreateNamed { who, n, token, name: name? })
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
        // "put a number of +1/+1 counters on ~ equal to that creature's
        // power", the amount before or after the object.
        |p| {
            p.words("put a number of")?;
            let kind = counter_kind(p)?;
            p.word("counters")?;
            let (n, on) = alt!(p;
                |p| { p.word("on")?; let on = objects(p)?; p.words("equal to")?; Some((measure(p)?, on)) },
                |p| { p.words("equal to")?; let n = measure(p)?; p.word("on")?; Some((n, objects(p)?)) },
            )?;
            Some(Effect::PutCounters { n, kind, on })
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
                // "gain half X life, rounded down".
                |p| {
                    p.word("half")?;
                    let of = amount(p)?;
                    p.word("life")?;
                    Some(Amount::Half { of: Box::new(of), round_up: rounded(p)? })
                },
                // "you gain that much life plus 1 instead": the number added
                // after the noun.
                |p| { p.words("that much life plus")?; let k = number(p)?; Some(Amount::Plus(Box::new(Amount::ThatMany), k)) },
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
        // "You add {B}{B} and draw a card": "you" said before the instruction.
        |p| { p.attempt(|p| p.word("you")); p.word("add")?; mana_add(p).map(Effect::AddMana) },
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
            let mut and = vec![];
            while let Some(more) = p.attempt(|p| { p.word("and")?; objects(p) }) {
                and.push(more);
            }
            Some(Effect::Search { who, zones, what, and })
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
        |p| { let who = subject_player(p); p.verb("exchange")?; p.words("life totals with")?; let with = player(p)?; Some(Effect::ExchangeLifeTotals { who, with }) },
        // "Exchange your life total with this creature's toughness" (Tree of
        // Redemption).
        |p| {
            p.word("exchange")?;
            let who = whose(p)?;
            p.words("life total with")?;
            let what = objects(p)?;
            p.tok(&Tok::Possessive)?;
            let stat = stat(p)?;
            if stat == Stat::ManaValue { return None; }
            Some(Effect::ExchangeLifeWithStat { who, what, stat })
        },
        |p| {
            let who = subject_player(p);
            p.verb("look at")?;
            let what = objects(p)?;
            // "the creature cards in target opponent's hand" is every one
            // there, not a reference back, which `objects` would make it.
            let zoned = what.filter.posts.iter().any(|q| matches!(q, Post::In(_)));
            if what.quant == Quant::Antecedent && zoned {
                return None;
            }
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::LookAt { who, what, dur })
        },
        |p| { let who = subject_player(p); p.verb("reveal")?; let (n, whose) = top_of_library(p)?; Some(Effect::RevealTop { who, whose, n }) },
        |p| { let who = subject_player(p); p.verb("reveal")?; let what = objects(p)?; Some(Effect::Reveal { who, what }) },
        // permanents
        |p| { p.word("tap")?; objects(p).map(Effect::Tap) },
        |p| { p.word("untap")?; objects(p).map(Effect::Untap) },
        |p| { let who = subject_player(p); p.verb("sacrifice")?; let what = objects(p)?; Some(Effect::Sacrifice { who, what }) },
        |p| { let a = objects(p)?; p.verb("fight")?; p.words("each other")?; Some(Effect::FightEachOther(a)) },
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
        // "you may cast it by paying {1} rather than paying its mana cost".
        |p| {
            p.word("cast")?;
            let what = objects(p)?;
            if what.quant == Quant::SelfRef { return None; }
            let (cost, rather) = paying(p)?;
            Some(Effect::CastPaying { what, cost, rather })
        },
        |p| {
            let verb = alt!(p;
                |p| p.word("play").map(|_| CastVerb::Play),
                |p| p.word("cast").map(|_| CastVerb::Cast),
            )?;
            let what = objects(p)?;
            let free = |p: &mut P| p.one_of(&["without paying its mana cost", "without paying their mana costs"]).map(|_| Payment::Free);
            let payment = p.attempt(free);
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            // "You may cast this card from your graveyard" is the card's own
            // rule about being cast, a static ability, never its effect; "You
            // may cast this card for as long as it remains exiled" after an
            // ability exiled it is that ability's permission.
            if what.quant == Quant::SelfRef && !matches!(dur, Duration::ForAsLongAs(_)) { return None; }
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
        // "that many plus one +1/+1 counters are put on it", "that many
        // -1/-1 counters minus one are put on it".
        |p| {
            let n = amount(p)?;
            let kind = counter_kind(p)?;
            p.one_of(&["counters", "counter"])?;
            let n = match p.attempt(|p| { p.word("minus")?; number(p) }) {
                Some(k) => Amount::Minus(Box::new(n), Box::new(Amount::N(k))),
                None => n,
            };
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
                // "chooses an artifact, a creature, an enchantment, and a
                // planeswalker from among the nonland permanents they control".
                |p| {
                    p.verb("choose")?;
                    let one = |p: &mut P| { p.one_of(&["a", "an"])?; filter(p) };
                    let mut picks = vec![one(p)?];
                    while let Some(f) = p.attempt(|p| { p.one_of(&[", and", ","])?; one(p) }) { picks.push(f); }
                    // The last pick's own post read the "from among" said
                    // once for all of them.
                    let among = match picks.last_mut()?.posts.pop()? {
                        Post::FromAmong(o) => *o,
                        _ => return None,
                    };
                    if picks.len() < 2 || picks.iter().any(|f| !f.posts.is_empty()) { return None; }
                    Some(Effect::Choose(Choice::OneOfEach { picks, among }))
                },
                |p| { p.verb("choose")?; objects(p).map(|o| Effect::Choose(Choice::Objects(o))) },
                |p| { p.verb("add")?; mana_add(p).map(Effect::AddMana) },
                |p| { p.verb("scry")?; amount(p).map(Effect::Scry) },
                |p| { p.verb("surveil")?; amount(p).map(Effect::Surveil) },
                // "Its controller investigates": a keyword action the
                // player performs (CR 701), with no object of its own.
                |p| {
                    let w = p.peek_word()?;
                    let name = w.strip_suffix("es").filter(|b| vocab().keyword_actions.contains(*b))
                        .or_else(|| w.strip_suffix('s').filter(|b| vocab().keyword_actions.contains(*b)))?
                        .to_string();
                    p.bump();
                    let n = p.attempt(amount);
                    p.end_of_clause().then_some(Effect::Action { by: None, name, n })
                },
                // "Each player exiles the top card of their library face down".
                |p| { p.verb("exile")?; let what = objects(p)?; p.words("face down")?; Some(Effect::ExileFaceDown(what)) },
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
        // "Take an extra turn after this one", "Target player takes two
        // extra turns after this one" (Time Stretch).
        |p| {
            let who = subject_player(p);
            p.verb("take")?;
            let n = amount_a(p)?;
            p.word("extra")?;
            p.one_of(&["turns", "turn"])?;
            p.words("after this one")?;
            Some(Effect::ExtraTurn { who, n, after: TurnRef::ThisOne })
        },
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
            // "One or two target creatures each get +2/+2": "each" restates
            // what a bonus to them gives each.
            p.attempt(|p| p.word("each"));
            let (pt, gains) = alt!(p;
                |p| {
                    p.verb("get")?;
                    // "get an additional -2/-2": on top of the earlier bonus,
                    // as a later one is anyway.
                    p.attempt(|p| p.words("an additional"));
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
        // "can attack this turn as though it didn't have defender": the
        // duration inside.
        |p| {
            let what = objects(p)?;
            p.words("can attack this turn as though it didn't have defender")?;
            Some(Effect::Restrict { what, rule: Restriction::AttacksDespiteDefender, dur: Duration::ThisTurn })
        },
        |p| {
            let what = objects(p)?;
            let rule = restriction(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Restrict { what, rule, dur })
        },
        // keyword actions: "proliferate", "investigate", "~ explores", "amass Zombies 2"
        // "manifest dread", "empower Jace 5": an action of two words.
        |p| {
            let w = p.peek_word()?;
            let next = match p.t.get(p.i + 1) { Some(Tok::Word(n)) => n, _ => return None };
            let name = format!("{w} {next}");
            if !vocab().keyword_actions.contains(&name) { return None; }
            p.bump();
            p.bump();
            if let Some(n) = p.attempt(amount) {
                return Some(Effect::Action { by: None, name, n: Some(n) });
            }
            p.end_of_clause().then_some(Effect::Action { by: None, name, n: None })
        },
        // "Proliferate X times": the action, that many times over.
        |p| {
            let name = p.peek_word()?.to_string();
            if !vocab().keyword_actions.contains(&name) { return None; }
            p.bump();
            let times = alt!(p;
                |p| p.word("twice").map(|_| Amount::N(2)),
                |p| { let t = amount(p)?; p.word("times")?; Some(t) },
            )?;
            Some(Effect::Repeat { times, then: vec![Effect::Action { by: None, name, n: None }] })
        },
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

/// Templates no older rule reads any part of, tried only when those fail.
fn effect_more(p: &mut P) -> Option<Effect> {
    alt!(p;
        // "Target player reveals their hand": their own.
        |p| {
            let who = subject_player(p);
            p.verb("reveal")?;
            p.one_of(&["their hand", "your hand"])?;
            Some(Effect::RevealHand(who))
        },
        // "up to one other target creature loses all abilities".
        |p| {
            let what = objects(p)?;
            p.verb("lose")?;
            if p.peek_word() != Some("all") || p.t.get(p.i + 1) == Some(&Tok::Word("counters".into())) { return None; }
            let lost = lost(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Loses { what, lost, dur })
        },
        // "gains your choice of double strike or lifelink until end of turn".
        |p| {
            let what = objects(p)?;
            p.verb("gain")?;
            p.words("your choice of")?;
            let mut keywords = vec![keyword(p)?];
            while let Some(k) = p.attempt(|p| { p.one_of(&[", or", "or", ","])?; keyword(p) }) { keywords.push(k); }
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::GainsChoiceOf { what, keywords, dur })
        },
        // "you gain 1 life and get {E}": "you" said once for both.
        |p| {
            if subject_player(p) != Player::You { return None; }
            p.verb("get")?;
            let mut n = 0;
            while sym(p, "E").is_some() { n += 1; }
            (n > 0).then_some(Effect::GetEnergy(n))
        },
        |p| { p.words("you get")?; let n = amount(p)?; sym(p, "E")?; Some(Effect::GetEnergyAmount(n)) },
        |p| { p.words("repeat this process")?; (p.peek_word() != Some("until")).then_some(Effect::RepeatProcess) },
        |p| {
            p.words("after that turn ,")?;
            let who = player(p)?;
            p.verb("take")?;
            p.words("an extra turn")?;
            Some(Effect::ExtraTurn { who, n: Amount::N(1), after: TurnRef::That })
        },
        // "The flashback cost is equal to its mana cost."
        |p| {
            p.word("the")?;
            let keyword = keyword_name(p)?;
            p.words("cost is equal to its mana cost")?;
            Some(Effect::KeywordCostIs { keyword, cost: vec![Cost::ManaCostOf(antecedent())] })
        },
        // "Each opponent gets an emblem with …"; "you get an emblem" is
        // `Emblem`'s.
        |p| {
            let who = player(p)?;
            if who == Player::You { return None; }
            p.verb("get")?;
            p.words("an emblem with")?;
            let mut v = vec![quoted(p)?];
            while let Some(a) = p.attempt(|p| { p.word("and")?; quoted(p) }) { v.push(a); }
            Some(Effect::EmblemFor { who, abilities: v })
        },
        |p| p.words("flip a coin").map(|_| Effect::FlipCoin),
        |p| p.words("end the turn").map(|_| Effect::EndTheTurn),
        // "instant and sorcery spells you cast cost {1} less to cast", for a
        // while. "This spell costs …" is the card's own static, which works
        // while it is cast.
        |p| {
            let what = objects(p)?;
            if what.quant == Quant::SelfRef { return None; }
            p.verb("cost")?;
            let by = mana(p)?;
            let less = p.one_of(&["less", "more"])? == "less";
            p.words("to cast")?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::CostChange { what, by, less, dur })
        },
        // "deals 2 damage divided as you choose among one or two targets".
        |p| {
            let source = p.attempt(objects);
            p.verb("deal")?;
            let n = amount(p)?;
            p.words("damage divided as you choose among")?;
            let among = objects(p)?;
            Some(Effect::DamageDivided { source, n, among })
        },
        // "Put a flying counter, a first strike counter, and a lifelink
        // counter on that creature": several kinds, one object.
        |p| {
            p.word("put")?;
            let one = |p: &mut P| {
                let n = amount_a(p)?;
                let kind = alt!(p; |p| keyword_name(p).map(CounterKind::Named), counter_kind)?;
                p.one_of(&["counters", "counter"])?;
                Some((n, kind))
            };
            let mut counters = vec![one(p)?];
            while let Some(c) = p.attempt(|p| { p.one_of(&[", and", "and", ","])?; one(p) }) { counters.push(c); }
            if counters.len() < 2 { return None; }
            p.word("on")?;
            let on = objects(p)?;
            Some(Effect::PutCountersEach { counters, on })
        },
        |p| { p.words("remove all counters from")?; objects(p).map(Effect::RemoveAllCounters) },
        // "then remove all fate counters from all permanents" (Oblivion Stone).
        |p| {
            p.words("remove all")?;
            let kind = counter_kind(p)?;
            p.words("counters from")?;
            let from = objects(p)?;
            Some(Effect::RemoveAllCountersOf { kind, from })
        },
        // "remove up to three counters from target creature", "remove a
        // counter from her": of any kinds.
        |p| {
            p.word("remove")?;
            let n = alt!(p;
                |p| p.words("any number of").map(|_| Amount::AnyAmount),
                |p| { p.words("up to")?; amount(p).map(|n| Amount::UpTo(Box::new(n))) },
                amount_a,
            )?;
            p.one_of(&["counters", "counter"])?;
            p.word("from")?;
            let from = objects(p)?;
            Some(Effect::RemoveAnyCounters { n, from })
        },
        // "put its counters on target creature you control": every counter
        // the object had, kind by kind.
        |p| {
            p.word("put")?;
            p.word("its")?;
            let kind = p.attempt(|p| {
                let k = counter_kind(p)?;
                (p.peek_word() == Some("counters")).then_some(k)
            });
            p.word("counters")?;
            p.word("on")?;
            let on = objects(p)?;
            let filter = Filter { kind: Some(Noun::Counter(kind)), posts: vec![Post::On(Box::new(antecedent()))], ..Filter::default() };
            Some(Effect::PutThoseCounters { those: Objects { quant: Quant::All, filter }, on })
        },
        |p| { let who = player(p)?; p.verb("lose")?; p.words("all counters")?; Some(Effect::LosesAllCounters(who)) },
        // "The next time a source of your choice would deal damage to you
        // this turn, prevent that damage."
        |p| {
            p.words("the next time")?;
            let what = objects(p)?;
            p.word("would")?;
            let event = would(p, &what)?;
            let dur = duration(p)?;
            p.tok(&Tok::Comma)?;
            let then = clauses(p)?;
            Some(Effect::NextTime { event: Box::new(event), dur, then })
        },
        |p| p.words("prevent that damage").map(|_| Effect::PreventThatDamage),
        // "meld them into Hanweir, the Writhing Township": the name, to the
        // sentence's end.
        |p| {
            p.word("meld")?;
            let what = objects(p)?;
            p.word("into")?;
            let into = card_name(p, &[])?;
            Some(Effect::Meld { what, into })
        },
        // "it produces {C} instead of any other type and amount".
        |p| {
            let what = pronoun(p)?;
            p.verb("produce")?;
            let mana = mana(p)?;
            p.words("instead of any other type and amount")?;
            Some(Effect::ProducesInstead { what, mana })
        },
        // "Prevent all damage that black and/or red sources would deal to
        // creatures you control this turn": the prevention said actively.
        |p| {
            p.words("prevent all damage that")?;
            let by = objects(p)?;
            p.words("would deal to")?;
            let to = recipients(p)?;
            let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
            Some(Effect::Prevent { combat_only: false, n: None, to: Some(to), by: Some(by), dur })
        },
    )
}

/// A card's name, read to the first of `stops` or the sentence's end: the
/// lexer folds case, so the name's own words cannot say where it ends.
pub(super) fn card_name(p: &mut P, stops: &[&str]) -> Option<String> {
    let mut words: Vec<String> = Vec::new();
    loop {
        match p.peek() {
            Some(Tok::Word(w)) if !stops.contains(&w.as_str()) => {
                words.push(w.clone());
                p.bump();
            }
            // "named Cadet, where X is …": a comma before a stop word ends
            // the name as the word would.
            Some(Tok::Comma)
                if !stops.contains(&",")
                    && !words.is_empty()
                    && !matches!(p.t.get(p.i + 1), Some(Tok::Word(w)) if stops.contains(&w.as_str())) =>
            {
                words.push(",".into());
                p.bump();
            }
            _ => break,
        }
    }
    (!words.is_empty() && words.last().map(String::as_str) != Some(","))
        .then(|| words.join(" ").replace(" ,", ","))
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
    "goad", "manifest", "detain", "cloak", "suspect", "exert", "airbend", "harness", "behold",
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

/// "it's", "the copy is", "they're": the copy or copies an exception is
/// said of.
const COPY_IS: &[&str] = &[
    "it's",
    "it is",
    "the token is",
    "that token is",
    "the copy is",
    "they're",
    "they are",
    "the tokens are",
    "those tokens are",
    "the copies are",
];
/// "it isn't", "the copy isn't", "they're not".
const COPY_ISNT: &[&str] = &[
    "it isn't",
    "it's not",
    "it is not",
    "the token isn't",
    "the token is not",
    "that token isn't",
    "the copy isn't",
    "the copy is not",
    "they're not",
    "they aren't",
    "the tokens aren't",
    "those tokens aren't",
    "those creatures aren't",
    "the copies aren't",
];

fn exception(p: &mut P) -> Option<Exception> {
    alt!(p;
        |p| {
            p.one_of(COPY_IS)?;
            p.one_of(&["a", "an"])?;
            let types = group(p)?;
            p.one_of(&["in addition to its other types", "in addition to their other types"])?;
            Some(Exception::AlsoIs(types))
        },
        |p| { p.one_of(COPY_ISNT)?; group(p).map(Exception::IsNot) },
        // "except it's a 4/4 black Zombie", "except it's legendary", "except
        // the token is 1/1" (CR 707.9b).
        |p| {
            p.one_of(COPY_IS)?;
            p.attempt(|p| p.one_of(&["a", "an"]));
            let pt = p.attempt(|p| match p.peek()? {
                Tok::Pt(pt) if !pt.signed => { let pt = *pt; p.bump(); Some(pt) }
                _ => None,
            });
            let adjs = p.attempt(group).unwrap_or_default();
            if pt.is_none() && adjs.is_empty() { return None; }
            if p.peek_word() == Some("in") { return None; }
            // "except it's an artifact and it loses all other card types":
            // setting a card type already replaces the others (CR 205.1a).
            if adjs.iter().any(|a| matches!(a, Adj::Type(_))) {
                p.attempt(|p| p.one_of(&[
                    "and it loses all other card types",
                    "and loses all other card types",
                    "and they lose all other card types",
                    "and lose all other card types",
                ]));
            }
            Some(Exception::Is { pt, adjs })
        },
        |p| p.words("it has this ability").map(|_| Exception::HasThisAbility),
        |p| p.one_of(&["his name is ~", "her name is ~", "its name is ~"]).map(|_| Exception::NameIsSelf),
        // "he's a 4/4 Spider Human Hero in addition to his other types".
        // "it's a 2/2 black Zombie in addition to its other colors and types"
        // (Ratadrabik): a color added too.
        |p| {
            alt!(p;
                |p| { p.one_of(&["he", "she", "it"])?; p.tok(&Tok::Possessive) },
                |p| p.word("it's"),
            )?;
            p.one_of(&["a", "an"])?;
            let pt = match p.peek()? { Tok::Pt(pt) if !pt.signed => *pt, _ => return None };
            p.bump();
            let types = group(p)?;
            let colors = types.iter().any(|a| matches!(a, Adj::Color(_)));
            p.words("in addition to")?;
            p.one_of(&["his", "her", "its"])?;
            if colors { p.words("other colors and types")?; } else { p.words("other types")?; }
            Some(Exception::AlsoIsSized { pt, types })
        },
        |p| {
            p.one_of(&["it has", "the token has"])?;
            let (keywords, abilities) = alt!(p;
                keywords_then_quoted,
                |p| keyword_list(p).map(|k| (k, vec![])),
            )?;
            Some(Exception::Has { keywords, abilities })
        },
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
        // "put one of those cards back on top of your library".
        |p| { p.word("back")?; put_to(p).filter(|d| matches!(d.zone_ref(), Zone::TopOfLibrary(_))) },
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
    Some(entering_with(p, to.moving(what, from, order)))
}

/// "… onto the battlefield with a stun counter on it": a move to the
/// battlefield whose permanent enters with counters.
fn entering_with(p: &mut P, e: Effect) -> Effect {
    if !matches!(
        e,
        Effect::Move {
            to: Zone::Battlefield,
            ..
        }
    ) {
        return e;
    }
    // "to the battlefield tapped and attacking" (CR 508.4).
    let tapped = matches!(e, Effect::Move { tapped: true, .. });
    if p.attempt(|p| {
        if tapped {
            p.word("and")?;
        }
        p.word("attacking")
    })
    .is_some()
    {
        return Effect::EntersAttacking(Box::new(e));
    }
    // "to the battlefield attached to that creature" (CR 303.4f).
    if let Some(to) = p.attempt(|p| {
        p.words("attached to")?;
        objects(p)
    }) {
        return Effect::EntersAttached {
            effect: Box::new(e),
            to,
        };
    }
    match p.attempt(|p| {
        p.word("with")?;
        let n = amount_a(p)?;
        let n = match p.attempt(|p| p.word("additional")) {
            Some(_) => Amount::Additional(Box::new(n)),
            None => n,
        };
        let kind = counter_kind(p)?;
        p.one_of(&["counters", "counter"])?;
        p.one_of(&["on it", "on them"])?;
        Some((n, kind))
    }) {
        Some((n, kind)) => Effect::EntersWith {
            effect: Box::new(e),
            n,
            kind,
        },
        None => e,
    }
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
            // "a player or battle": the article said once for both.
            let quant = match pl {
                Player::Target | Player::TargetOpponent => Quant::Target { n: Amount::N(1), up_to: false },
                Player::APlayer => Quant::One,
                _ => return None,
            };
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
        // "that artifact's controller", "that source's controller".
        |p| {
            let o = objects_no_player(p)?;
            p.tok(&Tok::Possessive)?;
            alt!(p;
                |p| p.word("controller").map(|_| Player::ControllerOf(Box::new(o.clone()))),
                |p| p.word("owner").map(|_| Player::OwnerOf(Box::new(o.clone()))),
            ).map(Recipient::Player)
        },
        // "this enchantment deals 1 damage to them": damage to a bare "them"
        // is always a player in Oracle text, the singular "they"; objects
        // are "each of them".
        |p| p.word("them").map(|_| Recipient::Player(Player::That)),
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

/// "Prevent all combat damage that would be dealt to this creature", "…
/// the next 3 damage that would be dealt to any target this turn".
pub(super) fn prevent(p: &mut P) -> Option<Effect> {
    p.word("prevent")?;
    let n = alt!(p;
        |p| p.word("all").map(|_| None),
        |p| { p.words("the next")?; amount(p).map(Some) },
    )?;
    let combat_only = p.attempt(|p| p.word("combat")).is_some();
    p.words("damage that would be dealt")?;
    // "dealt to and dealt by that creature": one object, both ways.
    if let Some(o) = p.attempt(|p| {
        p.words("to and dealt by")?;
        objects(p)
    }) {
        let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
        return Some(Effect::Prevent {
            combat_only,
            n,
            to: Some(vec![Recipient::Objects(o.clone())]),
            by: Some(o),
            dur,
        });
    }
    let to = p.attempt(|p| {
        p.word("to")?;
        recipients(p)
    });
    let by = p.attempt(|p| {
        p.word("by")?;
        objects(p)
    });
    let dur = p.attempt(duration).unwrap_or(Duration::Permanent);
    Some(Effect::Prevent {
        combat_only,
        n,
        to,
        by,
        dur,
    })
}

/// "Destroy that creature. The creature can't be regenerated.", "Destroy
/// target nonblack creature and target land. They can't be regenerated.":
/// the sentence after a destroy that says what it destroyed can't be
/// regenerated, which is the destroy's own: a regeneration shield replaces
/// that destruction (CR 701.19a), and the sentence stops it (CR 701.19c). It names the
/// destroyed objects by pronoun, by a definite noun of their type, or as
/// what was "destroyed this way" ([no-regen-after]).
pub(super) fn cant_be_regenerated(p: &mut P, what: &[Objects]) -> bool {
    let types: Vec<String> = what
        .iter()
        .flat_map(|o| {
            o.filter.alts.iter().filter_map(|g| match g.last() {
                Some(Adj::Type(t)) => Some(t.clone()),
                _ => None,
            })
        })
        .chain(["permanent".to_string()])
        .collect();
    let noun = |p: &mut P| {
        let w = p.peek_word()?.to_string();
        let sg = w.strip_suffix('s').unwrap_or(&w).to_string();
        types.contains(&sg).then(|| p.bump())
    };
    p.attempt(|p| {
        p.tok(&Tok::Period)?;
        alt!(p;
            |p| p.one_of(&["it", "they"]).map(|_| ()),
            |p| { p.one_of(&["that", "the", "those"])?; noun(p) },
            |p| { p.attempt(|p| p.one_of(&["a", "an"])); noun(p)?; p.words("destroyed this way") },
        )?;
        p.words("can't be regenerated")
    })
    .is_some()
}
