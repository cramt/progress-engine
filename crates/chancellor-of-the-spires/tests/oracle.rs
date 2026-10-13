//! Rules questions answered from the cards' Oracle text: read by Insight,
//! lowered into the engine's effects (ADR-0037), then through the layers.
//! The text is Scryfall's, as of 2026-10-13.

use chancellor_of_the_spires::characteristics;
use chancellor_of_the_spires::lower::{self, Card, Lowered};
use chancellor_of_the_spires::model::*;
use vivisurgeons_insight::ast;

const HUMILITY: Card = Card {
    name: "Humility",
    type_line: "Enchantment",
    oracle_text: "All creatures lose all abilities and have base power and toughness 1/1.",
    mana_value: 4,
    colors: &[Color::White],
    pt: None,
};

const TOPH: Card = Card {
    name: "Toph, the First Metalbender",
    type_line: "Legendary Creature — Human Warrior Ally",
    oracle_text: "Nontoken artifacts you control are lands in addition to their other types. \
(They don't gain the ability to {T} for mana.)\nAt the beginning of your end step, earthbend 2. \
(Target land you control becomes a 0/0 creature with haste that's still a land. Put two +1/+1 \
counters on it. When it dies or is exiled, return it to the battlefield tapped.)",
    mana_value: 4,
    colors: &[Color::Green, Color::Red, Color::White],
    pt: Some(pt(3, 3)),
};

const SOL_RING: Card = Card {
    name: "Sol Ring",
    type_line: "Artifact",
    oracle_text: "{T}: Add {C}{C}.",
    mana_value: 1,
    colors: &[],
    pt: None,
};

const TREASURE: Card = Card {
    name: "Treasure",
    type_line: "Token Artifact — Treasure",
    oracle_text: "{T}, Sacrifice this token: Add one mana of any color.",
    mana_value: 0,
    colors: &[],
    pt: None,
};

const FOREST: Card = Card {
    name: "Forest",
    type_line: "Basic Land — Forest",
    oracle_text: "({T}: Add {G}.)",
    mana_value: 0,
    colors: &[],
    pt: None,
};

fn read(card: &Card) -> Lowered {
    lower::card(card).unwrap_or_else(|r| panic!("{}: {r}", card.name))
}

fn enter(s: &mut State, card: &Card) -> ObjectId {
    let l = read(card);
    s.enter(l.chars, l.statics)
}

/// Toph's end step trigger, as it resolves on `land`: earthbend 2, read
/// from Toph's own text.
fn tophs_trigger_resolves(s: &mut State, land: ObjectId) {
    let toph = read(&TOPH);
    let [ast::Ability::Triggered { effect, .. }] = toph.unread.as_slice() else {
        panic!("Toph has one unread ability, the end step trigger");
    };
    let [action] = effect.as_slice() else {
        panic!("the trigger does one thing");
    };
    let r = lower::action(action).unwrap_or_else(|e| panic!("{e}"));
    s.resolve([land], r.ops);
    for (kind, n) in r.counters {
        s.put_counters(land, kind, n);
    }
}

#[test]
fn humility_and_toph_read_from_their_text() {
    let humility = read(&HUMILITY);
    assert_eq!(humility.statics.len(), 1);
    assert_eq!(
        humility.statics[0].ops,
        [Op::LoseAll, Op::SetBasePt(PtValue::Fixed(pt(1, 1)))]
    );

    let toph = read(&TOPH);
    assert!(toph.chars.is(CardType::Creature));
    assert!(toph.chars.supertypes.contains(&Supertype::Legendary));
    assert_eq!(toph.statics.len(), 1);
    assert_eq!(toph.statics[0].ops, [Op::AddTypes([CardType::Land].into())]);
    assert_eq!(
        toph.statics[0].filter,
        Filter(vec![
            Pred::Nontoken,
            Pred::Is(Prop::Type(CardType::Artifact)),
            Pred::YouControl
        ])
    );
    // The end step trigger is an ability Toph has, though the layers don't
    // run it, so Humility has something more to take. The static joins it
    // when Toph enters and the state gives it an id.
    assert_eq!(toph.unread.len(), 1);
    assert_eq!(toph.chars.abilities, [Ability::Unread(0)].into());
}

#[test]
fn humility_takes_tophs_abilities_too_late_to_stop_artifacts_being_lands() {
    // Toph's effect is type-changing, layer 4. Humility removes Toph's
    // abilities in layer 6, after Toph's effect has applied (CR 613.1d,
    // 613.1f), so the order they came in doesn't matter.
    for humility_first in [false, true] {
        let mut s = State::default();
        let ring = enter(&mut s, &SOL_RING);
        let treasure = {
            let l = read(&TREASURE);
            s.create_token(l.chars, l.statics)
        };
        let theirs = {
            let mut l = read(&SOL_RING);
            l.chars.controller = PlayerId(1);
            s.enter(l.chars, l.statics)
        };
        let (toph, _) = if humility_first {
            let h = enter(&mut s, &HUMILITY);
            (enter(&mut s, &TOPH), h)
        } else {
            let t = enter(&mut s, &TOPH);
            (t, enter(&mut s, &HUMILITY))
        };
        let now = characteristics(&s);
        let msg = format!("humility first: {humility_first}");
        assert!(
            now[&ring].is(CardType::Land) && now[&ring].is(CardType::Artifact),
            "{msg}"
        );
        assert!(!now[&treasure].is(CardType::Land), "{msg}");
        assert!(!now[&theirs].is(CardType::Land), "{msg}");
        assert_eq!(now[&toph].pt, Some(pt(1, 1)), "{msg}");
        assert!(now[&toph].abilities.is_empty(), "{msg}");
    }
}

#[test]
fn a_land_earthbent_by_tophs_trigger_under_humility_follows_timestamps() {
    // The earthbent land is a creature from layer 4 on, so Humility reaches
    // it. Earthbend's grant of haste and Humility's removal are both layer
    // 6, and their base power and toughness both 7b, each in timestamp
    // order (CR 613.7); the counters add in 7c after either (CR 613.4c,
    // 701.66a). It stays a Forest (earthbend's ruling of 2025-10-02).
    let haste = Ability::Keyword(Keyword::Haste);
    for humility_first in [false, true] {
        let mut s = State::default();
        let forest = enter(&mut s, &FOREST);
        if humility_first {
            enter(&mut s, &HUMILITY);
            tophs_trigger_resolves(&mut s, forest);
        } else {
            tophs_trigger_resolves(&mut s, forest);
            enter(&mut s, &HUMILITY);
        }
        let now = &characteristics(&s)[&forest];
        let msg = format!("humility first: {humility_first}");
        assert!(
            now.is(CardType::Creature) && now.is(CardType::Land),
            "{msg}"
        );
        assert!(
            now.subtypes
                .contains(&Subtype::BasicLand(BasicLandType::Forest)),
            "{msg}"
        );
        let (p, hasty) = if humility_first {
            (pt(2, 2), true)
        } else {
            (pt(3, 3), false)
        };
        assert_eq!(now.pt, Some(p), "{msg}");
        assert_eq!(now.abilities.contains(&haste), hasty, "{msg}");
    }
}

#[test]
fn sol_ring_turned_into_a_land_by_toph_can_be_earthbent_and_humility_shrinks_it() {
    // Toph makes Sol Ring a land, which earthbend can then animate. Under
    // Humility (earlier), it is a 0/0 from 7b's later earthbend plus two
    // counters, and loses the mana ability its text gave it.
    let mut s = State::default();
    let ring = enter(&mut s, &SOL_RING);
    enter(&mut s, &HUMILITY);
    enter(&mut s, &TOPH);
    tophs_trigger_resolves(&mut s, ring);
    let now = &characteristics(&s)[&ring];
    assert!(now.is(CardType::Artifact) && now.is(CardType::Land) && now.is(CardType::Creature));
    assert_eq!(now.pt, Some(pt(2, 2)));
    assert_eq!(now.abilities, [Ability::Keyword(Keyword::Haste)].into());
}

#[test]
fn a_line_the_engine_cant_lower_is_refused_by_name() {
    let card = Card {
        name: "Crusade",
        type_line: "Enchantment",
        oracle_text: "White creatures get +1/+1.",
        mana_value: 2,
        colors: &[Color::White],
        pt: None,
    };
    let refusal = lower::card(&card)
        .err()
        .expect("a colour filter isn't lowered yet");
    assert!(refusal.to_string().contains("Color"), "{refusal}");
}
