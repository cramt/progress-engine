//! Rules questions with one right answer, each naming the rule it checks.
//! CR 613's own worked examples come first, then the cards ADR-0037's slice
//! is about.

use std::collections::BTreeSet;

use chancellor_of_the_spires::characteristics;
use chancellor_of_the_spires::model::*;

const YOU: PlayerId = PlayerId(0);

fn chars(types: &[CardType], mana_value: u32, p: Option<Pt>) -> Chars {
    Chars {
        types: types.iter().copied().collect(),
        supertypes: BTreeSet::new(),
        subtypes: BTreeSet::new(),
        colors: BTreeSet::new(),
        abilities: BTreeSet::new(),
        pt: p,
        mana_value,
        controller: YOU,
    }
}

fn creature(p: Pt, mana_value: u32, color: Color) -> Chars {
    let mut c = chars(&[CardType::Creature], mana_value, Some(p));
    c.colors.insert(color);
    c
}

fn enchantment(mana_value: u32) -> Chars {
    chars(&[CardType::Enchantment], mana_value, None)
}

fn aura() -> Chars {
    let mut c = enchantment(1);
    c.subtypes.insert(Subtype::Other("Aura".into()));
    c
}

fn of(ops: Vec<Op>, filter: Vec<Pred>) -> StaticAbility {
    StaticAbility {
        filter: Filter(filter),
        ops,
    }
}

fn creatures() -> Pred {
    Pred::Is(Prop::Type(CardType::Creature))
}

fn set<T: Ord>(items: impl IntoIterator<Item = T>) -> BTreeSet<T> {
    items.into_iter().collect()
}

fn pt_of(state: &State, o: ObjectId) -> Option<Pt> {
    characteristics(state)[&o].pt
}

// Cards, as far as the layers read them.

fn glorious_anthem(s: &mut State) -> ObjectId {
    s.enter(
        enchantment(3),
        vec![of(
            vec![Op::ModifyPt(pt(1, 1))],
            vec![creatures(), Pred::YouControl],
        )],
    )
}

fn humility(s: &mut State) -> ObjectId {
    s.enter(
        enchantment(4),
        vec![of(
            vec![Op::LoseAll, Op::SetBasePt(PtValue::Fixed(pt(1, 1)))],
            vec![creatures()],
        )],
    )
}

fn opalescence(s: &mut State) -> ObjectId {
    s.enter(
        enchantment(4),
        vec![of(
            vec![
                Op::AddTypes(set([CardType::Creature])),
                Op::SetBasePt(PtValue::ManaValue),
            ],
            vec![
                Pred::Is(Prop::Type(CardType::Enchantment)),
                Pred::IsNot(Prop::Subtype(Subtype::Other("Aura".into()))),
                Pred::Other,
            ],
        )],
    )
}

fn blood_moon(s: &mut State) -> ObjectId {
    s.enter(
        enchantment(3),
        vec![of(
            vec![Op::SetBasicLandType(BasicLandType::Mountain)],
            vec![
                Pred::Is(Prop::Type(CardType::Land)),
                Pred::IsNot(Prop::Supertype(Supertype::Basic)),
            ],
        )],
    )
}

fn urborg(s: &mut State) -> ObjectId {
    let mut c = chars(&[CardType::Land], 0, None);
    c.supertypes.insert(Supertype::Legendary);
    s.enter(
        c,
        vec![of(
            vec![Op::AddSubtypes(set([Subtype::BasicLand(
                BasicLandType::Swamp,
            )]))],
            vec![Pred::Is(Prop::Type(CardType::Land))],
        )],
    )
}

fn forest(s: &mut State) -> ObjectId {
    let mut c = chars(&[CardType::Land], 0, None);
    c.supertypes.insert(Supertype::Basic);
    c.subtypes.insert(Subtype::BasicLand(BasicLandType::Forest));
    s.enter(c, vec![])
}

fn land_types(state: &State, o: ObjectId) -> BTreeSet<BasicLandType> {
    characteristics(state)[&o]
        .subtypes
        .iter()
        .filter_map(|s| match s {
            Subtype::BasicLand(t) => Some(*t),
            _ => None,
        })
        .collect()
}

fn has(state: &State, o: ObjectId, a: Ability) -> bool {
    characteristics(state)[&o].abilities.contains(&a)
}

// CR 613.4d's three examples: switching is layer 7d, after every 7c change.

#[test]
fn cr_613_4d_a_switch_applies_after_modifications_whenever_they_came() {
    let mut s = State::default();
    let c = s.enter(creature(pt(1, 3), 2, Color::Green), vec![]);
    s.resolve([c], vec![Op::ModifyPt(pt(0, 1))]);
    s.resolve([c], vec![Op::SwitchPt]);
    assert_eq!(pt_of(&s, c), Some(pt(4, 1)));
    s.resolve([c], vec![Op::ModifyPt(pt(5, 0))]);
    assert_eq!(pt_of(&s, c), Some(pt(4, 6)));
}

#[test]
fn cr_613_4d_when_the_modification_ends_the_switch_still_applies() {
    let mut s = State::default();
    let c = s.enter(creature(pt(1, 3), 2, Color::Green), vec![]);
    s.resolve([c], vec![Op::SwitchPt]);
    assert_eq!(pt_of(&s, c), Some(pt(3, 1)));
}

#[test]
fn cr_613_4d_two_switches_cancel() {
    let mut s = State::default();
    let c = s.enter(creature(pt(1, 3), 2, Color::Green), vec![]);
    s.resolve([c], vec![Op::ModifyPt(pt(0, 1))]);
    s.resolve([c], vec![Op::SwitchPt]);
    s.resolve([c], vec![Op::SwitchPt]);
    assert_eq!(pt_of(&s, c), Some(pt(1, 4)));
}

// CR 613.5's two examples: the layers are reapplied continuously.

#[test]
fn cr_613_5_honor_of_the_pure_applies_while_the_creature_is_white() {
    let mut s = State::default();
    s.enter(
        enchantment(2),
        vec![of(
            vec![Op::ModifyPt(pt(1, 1))],
            vec![
                creatures(),
                Pred::Is(Prop::Color(Color::White)),
                Pred::YouControl,
            ],
        )],
    );
    let c = s.enter(creature(pt(2, 2), 2, Color::Black), vec![]);
    assert_eq!(pt_of(&s, c), Some(pt(2, 2)));
    s.resolve([c], vec![Op::SetColors(set([Color::White]))]);
    assert_eq!(pt_of(&s, c), Some(pt(3, 3)));
    s.resolve([c], vec![Op::SetColors(set([Color::Red]))]);
    assert_eq!(pt_of(&s, c), Some(pt(2, 2)));
}

#[test]
fn cr_613_5_gray_ogre_sets_in_7b_under_everything_7c_adds() {
    let mut s = State::default();
    let ogre = s.enter(creature(pt(2, 2), 3, Color::Red), vec![]);
    s.put_counters(ogre, Counter::PlusOne, 1);
    assert_eq!(pt_of(&s, ogre), Some(pt(3, 3)));
    s.resolve([ogre], vec![Op::ModifyPt(pt(4, 4))]);
    assert_eq!(pt_of(&s, ogre), Some(pt(7, 7)));
    s.enter(
        enchantment(3),
        vec![of(
            vec![Op::ModifyPt(pt(0, 2))],
            vec![creatures(), Pred::YouControl],
        )],
    );
    assert_eq!(pt_of(&s, ogre), Some(pt(7, 9)));
    s.resolve([ogre], vec![Op::SetBasePt(PtValue::Fixed(pt(0, 1)))]);
    assert_eq!(pt_of(&s, ogre), Some(pt(5, 8)));
}

// CR 613.6: an effect's parts apply in their own layers, to one set.

#[test]
fn cr_613_6_a_color_and_a_pump_from_one_effect_apply_in_layers_5_and_7c() {
    let mut s = State::default();
    let c = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
    s.resolve(
        [c],
        vec![Op::ModifyPt(pt(1, 1)), Op::SetColors(set([Color::Blue]))],
    );
    let now = &characteristics(&s)[&c];
    assert_eq!(now.colors, set([Color::Blue]));
    assert_eq!(now.pt, Some(pt(3, 3)));
}

#[test]
fn cr_613_6_noncreature_artifacts_that_became_creatures_still_get_the_7b_part() {
    // The example's spell; March of the Machines is the static version,
    // whose filter no longer matches by 7b: its set began in layer 4.
    let mut s = State::default();
    let ring = s.enter(chars(&[CardType::Artifact], 1, None), vec![]);
    s.resolve(
        [ring],
        vec![
            Op::AddTypes(set([CardType::Creature])),
            Op::SetBasePt(PtValue::Fixed(pt(2, 2))),
        ],
    );
    assert_eq!(pt_of(&s, ring), Some(pt(2, 2)));

    let mut s = State::default();
    let ring = s.enter(chars(&[CardType::Artifact], 1, None), vec![]);
    s.enter(
        enchantment(4),
        vec![of(
            vec![
                Op::AddTypes(set([CardType::Creature])),
                Op::SetBasePt(PtValue::ManaValue),
            ],
            vec![
                Pred::Is(Prop::Type(CardType::Artifact)),
                Pred::IsNot(Prop::Type(CardType::Creature)),
            ],
        )],
    );
    let now = &characteristics(&s)[&ring];
    assert!(now.is(CardType::Creature));
    assert_eq!(now.pt, Some(pt(1, 1)));
}

// CR 613.9's two examples.

#[test]
fn cr_613_9_has_flying_and_loses_flying_apply_in_timestamp_order() {
    let flying = Ability::Keyword(Keyword::Flying);
    for gains_last in [false, true] {
        let mut s = State::default();
        let c = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
        let gain = s.enter(
            aura(),
            vec![of(vec![Op::Grant(flying)], vec![Pred::AttachedTo])],
        );
        let lose = s.enter(
            aura(),
            vec![of(vec![Op::Lose(flying)], vec![Pred::AttachedTo])],
        );
        let (first, last) = if gains_last {
            (lose, gain)
        } else {
            (gain, lose)
        };
        s.attach(first, c);
        s.attach(last, c);
        assert_eq!(has(&s, c, flying), gains_last);
    }
}

#[test]
fn cr_613_9_white_creatures_get_the_bonus_whatever_color_the_creature_was() {
    let mut s = State::default();
    s.enter(
        enchantment(2),
        vec![of(
            vec![Op::ModifyPt(pt(1, 1))],
            vec![creatures(), Pred::Is(Prop::Color(Color::White))],
        )],
    );
    let c = s.enter(creature(pt(2, 2), 2, Color::Black), vec![]);
    let paint = s.enter(
        aura(),
        vec![of(
            vec![Op::SetColors(set([Color::White]))],
            vec![Pred::AttachedTo],
        )],
    );
    s.attach(paint, c);
    assert_eq!(pt_of(&s, c), Some(pt(3, 3)));
}

// The slice's cards.

#[test]
fn counters_and_an_anthem_both_modify_in_7c() {
    // CR 613.4c. The counters don't annihilate here: that's a state-based
    // action (CR 704.5q), the state machine's.
    let mut s = State::default();
    let bears = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
    s.put_counters(bears, Counter::PlusOne, 2);
    s.put_counters(bears, Counter::MinusOne, 1);
    glorious_anthem(&mut s);
    assert_eq!(pt_of(&s, bears), Some(pt(4, 4)));
}

#[test]
fn giant_growth_survives_humility_whichever_came_first() {
    // CR 613.4b, 613.4c: Humility sets in 7b and the pump adds in 7c.
    for humility_first in [false, true] {
        let mut s = State::default();
        let bears = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
        if humility_first {
            humility(&mut s);
        }
        s.resolve([bears], vec![Op::ModifyPt(pt(3, 3))]);
        if !humility_first {
            humility(&mut s);
        }
        assert_eq!(
            pt_of(&s, bears),
            Some(pt(4, 4)),
            "humility first: {humility_first}"
        );
    }
}

#[test]
fn a_resolved_pump_fixes_its_set_and_an_anthem_reads_its_filter_again() {
    // CR 611.2c.
    let mut s = State::default();
    let first = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
    s.resolve([first], vec![Op::ModifyPt(pt(1, 1))]);
    let later = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
    assert_eq!(pt_of(&s, first), Some(pt(3, 3)));
    assert_eq!(pt_of(&s, later), Some(pt(2, 2)));

    glorious_anthem(&mut s);
    let latest = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
    assert_eq!(pt_of(&s, latest), Some(pt(3, 3)));
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
        let f = forest(&mut s);
        let (u, _) = if moon_first {
            let m = blood_moon(&mut s);
            (urborg(&mut s), m)
        } else {
            let u = urborg(&mut s);
            (u, blood_moon(&mut s))
        };
        assert_eq!(
            land_types(&s, u),
            set([BasicLandType::Mountain]),
            "moon first: {moon_first}"
        );
        assert_eq!(
            land_types(&s, f),
            set([BasicLandType::Forest]),
            "moon first: {moon_first}"
        );
    }
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
        let bears = s.enter(creature(pt(2, 2), 2, Color::Green), vec![]);
        let (h, o) = if humility_first {
            let h = humility(&mut s);
            (h, opalescence(&mut s))
        } else {
            let o = opalescence(&mut s);
            (humility(&mut s), o)
        };
        let now = characteristics(&s);
        let expect = if humility_first { pt(4, 4) } else { pt(1, 1) };
        assert_eq!(now[&h].pt, Some(expect), "humility first: {humility_first}");
        assert!(
            now[&h].abilities.is_empty(),
            "humility first: {humility_first}"
        );
        assert!(!now[&o].is(CardType::Creature));
        assert!(!now[&o].abilities.is_empty());
        assert_eq!(now[&bears].pt, Some(pt(1, 1)));
    }
}

// Toph, the First Metalbender: "Nontoken artifacts you control are lands in
// addition to their other types", and earthbend: "Target land you control
// becomes a 0/0 creature with haste that's still a land. Put N +1/+1
// counters on it."

fn toph(s: &mut State) -> ObjectId {
    let mut c = creature(pt(3, 3), 4, Color::Red);
    c.colors.extend([Color::Green, Color::White]);
    c.supertypes.insert(Supertype::Legendary);
    s.enter(
        c,
        vec![of(
            vec![Op::AddTypes(set([CardType::Land]))],
            vec![
                Pred::Is(Prop::Type(CardType::Artifact)),
                Pred::Nontoken,
                Pred::YouControl,
            ],
        )],
    )
}

fn earthbend(s: &mut State, land: ObjectId, n: u32) {
    s.resolve(
        [land],
        vec![
            Op::AddTypes(set([CardType::Creature])),
            Op::SetBasePt(PtValue::Fixed(pt(0, 0))),
            Op::Grant(Ability::Keyword(Keyword::Haste)),
        ],
    );
    s.put_counters(land, Counter::PlusOne, n);
}

#[test]
fn humility_takes_tophs_ability_too_late_to_stop_artifacts_being_lands() {
    // Toph's effect is type-changing, layer 4. Humility removes Toph's
    // ability in layer 6, after Toph's effect has applied (CR 613.1d,
    // 613.1f), so the order they came in doesn't matter.
    for humility_first in [false, true] {
        let mut s = State::default();
        let ring = s.enter(chars(&[CardType::Artifact], 1, None), vec![]);
        let treasure = s.create_token(chars(&[CardType::Artifact], 0, None), vec![]);
        let mut theirs = chars(&[CardType::Artifact], 1, None);
        theirs.controller = PlayerId(1);
        let theirs = s.enter(theirs, vec![]);
        let (t, _) = if humility_first {
            let h = humility(&mut s);
            (toph(&mut s), h)
        } else {
            let t = toph(&mut s);
            (t, humility(&mut s))
        };
        let now = characteristics(&s);
        let msg = format!("humility first: {humility_first}");
        assert!(now[&ring].is(CardType::Land), "{msg}");
        assert!(now[&ring].is(CardType::Artifact), "{msg}");
        assert!(!now[&treasure].is(CardType::Land), "{msg}");
        assert!(!now[&theirs].is(CardType::Land), "{msg}");
        assert_eq!(now[&t].pt, Some(pt(1, 1)), "{msg}");
        assert!(now[&t].abilities.is_empty(), "{msg}");
    }
}

#[test]
fn an_earthbent_land_under_humility_depends_on_which_came_first() {
    // The earthbent land is a creature from layer 4 on, so Humility reaches
    // it. Both effects grant or remove abilities in layer 6 and set base
    // power and toughness in 7b, each in timestamp order (CR 613.7); the
    // counters add in 7c after either (CR 613.4c). It stays a Forest
    // (earthbend's ruling of 2025-10-02: the land keeps its subtypes).
    let haste = Ability::Keyword(Keyword::Haste);
    for humility_first in [false, true] {
        let mut s = State::default();
        let f = forest(&mut s);
        if humility_first {
            humility(&mut s);
            earthbend(&mut s, f, 2);
        } else {
            earthbend(&mut s, f, 2);
            humility(&mut s);
        }
        let now = &characteristics(&s)[&f];
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
        if humility_first {
            assert_eq!(now.pt, Some(pt(2, 2)), "{msg}");
            assert!(now.abilities.contains(&haste), "{msg}");
        } else {
            assert_eq!(now.pt, Some(pt(3, 3)), "{msg}");
            assert!(!now.abilities.contains(&haste), "{msg}");
        }
    }
}

/// The research's game-scale board (docs/research/rules-engine.md,
/// *Incremental evaluation*): 300 objects and 30 effects, two in layer 4,
/// 14 targeted setters and 14 anthems. Run with
/// `cargo test --release -p chancellor-of-the-spires --test layers -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing_on_the_research_board() {
    let mut s = State::default();
    let mut ids = Vec::new();
    for i in 0..300 {
        let c = if i % 2 == 0 {
            creature(pt(2, 2), 2, Color::Green)
        } else {
            chars(&[CardType::Artifact], 1, None)
        };
        ids.push(s.enter(c, vec![]));
    }
    s.enter(
        enchantment(4),
        vec![of(
            vec![
                Op::AddTypes(set([CardType::Creature])),
                Op::SetBasePt(PtValue::ManaValue),
            ],
            vec![
                Pred::Is(Prop::Type(CardType::Artifact)),
                Pred::IsNot(Prop::Type(CardType::Creature)),
            ],
        )],
    );
    s.enter(
        enchantment(3),
        vec![of(
            vec![Op::AddSubtypes(set([Subtype::Creature("Zombie".into())]))],
            vec![creatures()],
        )],
    );
    for k in 0..14 {
        s.resolve([ids[k * 7]], vec![Op::SetBasePt(PtValue::Fixed(pt(0, 1)))]);
        glorious_anthem(&mut s);
    }
    let runs = 50;
    let t = std::time::Instant::now();
    for _ in 0..runs {
        std::hint::black_box(characteristics(&s));
    }
    let each = t.elapsed() / runs;
    let now = characteristics(&s);
    let zombies = now
        .values()
        .filter(|c| c.subtypes.contains(&Subtype::Creature("Zombie".into())))
        .count();
    eprintln!("{} objects, one run {each:?}, {zombies} zombies", now.len());
}
