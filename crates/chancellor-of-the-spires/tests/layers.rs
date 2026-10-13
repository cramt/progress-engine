//! Rules questions with one right answer, each naming the rule it checks:
//! CR 613's own worked examples, whose cards and effects are hypothetical,
//! so they are built by hand. Real cards are read from their Oracle text in
//! tests/oracle.rs.

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
    // The example's spell. tests/oracle.rs has March of the Machines, the
    // static version, from its text.
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
