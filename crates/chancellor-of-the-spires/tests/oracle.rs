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
    let r = lower::resolution(action).unwrap_or_else(|e| panic!("{e}"));
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

const fn card(
    name: &'static str,
    type_line: &'static str,
    oracle_text: &'static str,
    mana_value: u32,
    colors: &'static [Color],
    pt: Option<Pt>,
) -> Card<'static> {
    Card {
        name,
        type_line,
        oracle_text,
        mana_value,
        colors,
        pt,
    }
}

const GRIZZLY_BEARS: Card = card(
    "Grizzly Bears",
    "Creature — Bear",
    "",
    2,
    &[Color::Green],
    Some(pt(2, 2)),
);
const GLORIOUS_ANTHEM: Card = card(
    "Glorious Anthem",
    "Enchantment",
    "Creatures you control get +1/+1.",
    3,
    &[Color::White],
    None,
);
const GIANT_GROWTH: Card = card(
    "Giant Growth",
    "Instant",
    "Target creature gets +3/+3 until end of turn.",
    1,
    &[Color::Green],
    None,
);
const INSPIRED_CHARGE: Card = card(
    "Inspired Charge",
    "Instant",
    "Creatures you control get +2/+1 until end of turn.",
    4,
    &[Color::White],
    None,
);
const BLOOD_MOON: Card = card(
    "Blood Moon",
    "Enchantment",
    "Nonbasic lands are Mountains.",
    3,
    &[Color::Red],
    None,
);
const URBORG: Card = card(
    "Urborg, Tomb of Yawgmoth",
    "Legendary Land",
    "Each land is a Swamp in addition to its other land types.",
    0,
    &[],
    None,
);
const OPALESCENCE: Card = card(
    "Opalescence",
    "Enchantment",
    "Each other non-Aura enchantment is a creature in addition to its other types and has base \
power and base toughness each equal to its mana value.",
    4,
    &[Color::White],
    None,
);
const MARCH_OF_THE_MACHINES: Card = card(
    "March of the Machines",
    "Enchantment",
    "Each noncreature artifact is an artifact creature with power and toughness each equal to its \
mana value. (Equipment that's a creature can't equip a creature.)",
    4,
    &[Color::Blue],
    None,
);
const CRUSADE: Card = card(
    "Crusade",
    "Enchantment",
    "White creatures get +1/+1.",
    2,
    &[Color::White],
    None,
);
const WILD_NACATL: Card = card(
    "Wild Nacatl",
    "Creature — Cat Warrior",
    "This creature gets +1/+1 as long as you control a Mountain.\nThis creature gets +1/+1 as long \
as you control a Plains.",
    1,
    &[Color::Green],
    Some(pt(1, 1)),
);

/// An instant resolves: each of its effects, read from its text, on the
/// objects the state machine would give it.
fn cast(s: &mut State, spell: &Card, on: &[ObjectId]) {
    let l = read(spell);
    let [ast::Ability::Spell(effects)] = l.unread.as_slice() else {
        panic!("{} is one spell ability", spell.name);
    };
    for e in effects {
        let r = lower::resolution(e).unwrap_or_else(|e| panic!("{e}"));
        s.resolve(on.iter().copied(), r.ops);
        for o in on {
            for (kind, n) in &r.counters {
                s.put_counters(*o, kind.clone(), *n);
            }
        }
    }
}

fn creatures_you_control(s: &State) -> Vec<ObjectId> {
    characteristics(s)
        .into_iter()
        .filter(|(_, c)| c.is(CardType::Creature) && c.controller == PlayerId(0))
        .map(|(id, _)| id)
        .collect()
}

fn land_types(s: &State, o: ObjectId) -> std::collections::BTreeSet<BasicLandType> {
    characteristics(s)[&o]
        .subtypes
        .iter()
        .filter_map(|t| match t {
            Subtype::BasicLand(t) => Some(*t),
            _ => None,
        })
        .collect()
}

#[test]
fn counters_and_an_anthem_both_modify_in_7c() {
    // CR 613.4c. The counters don't annihilate here: that's a state-based
    // action (CR 704.5q), the state machine's.
    let mut s = State::default();
    let bears = enter(&mut s, &GRIZZLY_BEARS);
    s.put_counters(bears, Counter::PlusOne, 2);
    s.put_counters(bears, Counter::MinusOne, 1);
    enter(&mut s, &GLORIOUS_ANTHEM);
    assert_eq!(characteristics(&s)[&bears].pt, Some(pt(4, 4)));
}

#[test]
fn giant_growth_survives_humility_whichever_came_first() {
    // CR 613.4b, 613.4c: Humility sets in 7b and the pump adds in 7c.
    for humility_first in [false, true] {
        let mut s = State::default();
        let bears = enter(&mut s, &GRIZZLY_BEARS);
        if humility_first {
            enter(&mut s, &HUMILITY);
        }
        cast(&mut s, &GIANT_GROWTH, &[bears]);
        if !humility_first {
            enter(&mut s, &HUMILITY);
        }
        let msg = format!("humility first: {humility_first}");
        assert_eq!(characteristics(&s)[&bears].pt, Some(pt(4, 4)), "{msg}");
    }
}

#[test]
fn inspired_charge_fixes_its_set_and_glorious_anthem_reads_its_filter_again() {
    // CR 611.2c: the creatures Inspired Charge affects are those you
    // controlled as it resolved; Glorious Anthem's are whatever matches now.
    let mut s = State::default();
    let first = enter(&mut s, &GRIZZLY_BEARS);
    let now = creatures_you_control(&s);
    cast(&mut s, &INSPIRED_CHARGE, &now);
    let later = enter(&mut s, &GRIZZLY_BEARS);
    assert_eq!(characteristics(&s)[&first].pt, Some(pt(4, 3)));
    assert_eq!(characteristics(&s)[&later].pt, Some(pt(2, 2)));

    enter(&mut s, &GLORIOUS_ANTHEM);
    let latest = enter(&mut s, &GRIZZLY_BEARS);
    assert_eq!(characteristics(&s)[&latest].pt, Some(pt(3, 3)));
}

#[test]
fn blood_moon_turns_off_urborg_whichever_came_first() {
    // CR 613.8a: Blood Moon removes Urborg's ability (CR 305.7), which
    // changes the existence of Urborg's effect, so Urborg's effect depends
    // on Blood Moon's and waits for it (CR 613.8b) despite timestamps, and
    // then no longer exists. Urborg's ruling of 2021-03-19: set to a basic
    // land type by Magus of the Moon, "it won't turn lands into Swamps, no
    // matter in what order those effects started to apply."
    for moon_first in [false, true] {
        let mut s = State::default();
        let forest = enter(&mut s, &FOREST);
        let urborg = if moon_first {
            enter(&mut s, &BLOOD_MOON);
            enter(&mut s, &URBORG)
        } else {
            let u = enter(&mut s, &URBORG);
            enter(&mut s, &BLOOD_MOON);
            u
        };
        let msg = format!("moon first: {moon_first}");
        assert_eq!(
            land_types(&s, urborg),
            [BasicLandType::Mountain].into(),
            "{msg}"
        );
        assert_eq!(
            land_types(&s, forest),
            [BasicLandType::Forest].into(),
            "{msg}"
        );
    }
}

#[test]
fn urborg_alone_makes_every_land_a_swamp() {
    let mut s = State::default();
    let forest = enter(&mut s, &FOREST);
    let urborg = enter(&mut s, &URBORG);
    let swamp_forest = [BasicLandType::Swamp, BasicLandType::Forest].into();
    assert_eq!(land_types(&s, forest), swamp_forest);
    assert_eq!(land_types(&s, urborg), [BasicLandType::Swamp].into());
}

#[test]
fn humility_and_opalescence_by_timestamp() {
    // Opalescence's layer 4 part makes Humility a creature, so Humility's
    // layer 6 part removes Humility's own ability. Both effects began
    // applying before then and keep applying (CR 613.6). In 7b they apply
    // in timestamp order: the later one sets Humility's power and
    // toughness. Opalescence affects only other enchantments, so it is no
    // creature and keeps its ability. Other creatures are 1/1 either way.
    // Humility's and Opalescence's ruling of 2009-10-01 walks the same
    // layers: Opalescence first leaves Humility 1/1, Humility first 4/4.
    for humility_first in [false, true] {
        let mut s = State::default();
        let bears = enter(&mut s, &GRIZZLY_BEARS);
        let (h, o) = if humility_first {
            let h = enter(&mut s, &HUMILITY);
            (h, enter(&mut s, &OPALESCENCE))
        } else {
            let o = enter(&mut s, &OPALESCENCE);
            (enter(&mut s, &HUMILITY), o)
        };
        let now = characteristics(&s);
        let msg = format!("humility first: {humility_first}");
        let expect = if humility_first { pt(4, 4) } else { pt(1, 1) };
        assert_eq!(now[&h].pt, Some(expect), "{msg}");
        assert!(now[&h].abilities.is_empty(), "{msg}");
        assert!(!now[&o].is(CardType::Creature), "{msg}");
        assert!(!now[&o].abilities.is_empty(), "{msg}");
        assert_eq!(now[&bears].pt, Some(pt(1, 1)), "{msg}");
    }
}

#[test]
fn march_of_the_machines_sizes_the_artifacts_it_made_creatures() {
    // CR 613.6, the static version of its third example: by 7b Sol Ring is
    // no longer a noncreature artifact, but March's effect began applying
    // to it in layer 4 and keeps doing so. "An artifact creature" keeps its
    // other card types (CR 205.1b): Toph's land stays a land (March's
    // ruling of 2004-12-01, and an artifact land is 0/0).
    let mut s = State::default();
    let ring = enter(&mut s, &SOL_RING);
    enter(&mut s, &TOPH);
    enter(&mut s, &MARCH_OF_THE_MACHINES);
    let now = &characteristics(&s)[&ring];
    assert!(now.is(CardType::Artifact) && now.is(CardType::Creature) && now.is(CardType::Land));
    assert_eq!(now.pt, Some(pt(1, 1)));
}

#[test]
fn crusade_pumps_white_creatures_whoever_controls_them() {
    let mut s = State::default();
    let mut theirs = read(&GRIZZLY_BEARS).chars;
    theirs.colors = [Color::White].into();
    theirs.controller = PlayerId(1);
    let theirs = s.enter(theirs, vec![]);
    let green = enter(&mut s, &GRIZZLY_BEARS);
    enter(&mut s, &CRUSADE);
    let now = characteristics(&s);
    assert_eq!(now[&theirs].pt, Some(pt(3, 3)));
    assert_eq!(now[&green].pt, Some(pt(2, 2)));
}

#[test]
fn a_line_the_engine_cant_lower_is_refused_by_name() {
    let refusal = lower::card(&WILD_NACATL)
        .err()
        .expect("\"as long as\" isn't lowered yet");
    assert!(refusal.to_string().contains("AsLongAs"), "{refusal}");
}

#[test]
fn a_karoo_carries_every_ability_unread_and_humility_cant_reach_it() {
    // "Enters tapped" is a replacement effect (CR 614.1d) and changes no
    // characteristic, so lowering carries it with the trigger and the mana
    // ability. Humility takes only creatures' abilities: the land, not a
    // creature, keeps all three.
    const SELESNYA_SANCTUARY: Card = card(
        "Selesnya Sanctuary",
        "Land",
        "This land enters tapped.\nWhen this land enters, return a land you control to its owner's \
hand.\n{T}: Add {G}{W}.",
        0,
        &[],
        None,
    );
    let karoo = read(&SELESNYA_SANCTUARY);
    assert!(karoo.statics.is_empty());
    assert_eq!(karoo.unread.len(), 3);
    let mut s = State::default();
    let k = s.enter(karoo.chars, karoo.statics);
    enter(&mut s, &HUMILITY);
    assert_eq!(characteristics(&s)[&k].abilities.len(), 3);
}
