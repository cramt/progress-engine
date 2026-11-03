//! Spells' and triggered abilities' effects, run through the game: targets
//! chosen on casting and checked again on resolution, a hand revealed and
//! a card chosen from it, damage, scry, draw, an enters trigger; and
//! combat, with what attacking triggers.

use std::path::Path;
use std::sync::OnceLock;

use chancellor_of_the_spires::deck::{self, CardText, DeckCards};
use chancellor_of_the_spires::game::*;
use chancellor_of_the_spires::model::{pt, PlayerId};
use chip_scryfall::index::IndexFile;

const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

fn index() -> &'static IndexFile {
    static INDEX: OnceLock<IndexFile> = OnceLock::new();
    INDEX.get_or_init(|| {
        IndexFile::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../decks/index.jsonl"))
            .expect("decks/index.jsonl")
    })
}

fn card(name: &str) -> CardText {
    deck::card(name, index()).unwrap_or_else(|e| panic!("{e}"))
}

/// A 60-card library of these cards, in these numbers.
fn deck(cards: &[(&str, usize)]) -> DeckCards {
    DeckCards {
        hidden: 0,
        commander: Vec::new(),
        library: cards
            .iter()
            .flat_map(|(n, k)| (0..*k).map(|_| card(n)))
            .collect(),
    }
}

/// A constructed game, from the first seed that deals me at least as many
/// of each card as `wants` says.
fn deal(me: &[(&str, usize)], them: &[(&str, usize)], wants: &[(&str, usize)]) -> Game {
    deal_both(me, them, wants, &[])
}

/// As `deal`, and them at least as many as `they_want` says.
fn deal_both(
    me: &[(&str, usize)],
    them: &[(&str, usize)],
    wants: &[(&str, usize)],
    they_want: &[(&str, usize)],
) -> Game {
    for seed in 0..500 {
        let game = Game::start(Setup {
            format: Format::Constructed,
            decks: vec![deck(me), deck(them)],
            mulligans: false,
            seed,
        })
        .unwrap();
        let has = |p: PlayerId, wants: &[(&str, usize)]| {
            wants
                .iter()
                .all(|(w, k)| named(&game, game.player(p).hand(), w) >= *k)
        };
        if has(ME, wants) && has(THEM, they_want) {
            let mut game = game;
            pass_until(&mut game, ME, Step::PrecombatMain);
            return game;
        }
    }
    panic!("no seed deals {wants:?}")
}

fn in_hand(game: &Game, p: PlayerId, name: &str) -> Option<CardId> {
    game.player(p)
        .hand()
        .iter()
        .copied()
        .find(|c| game.card(*c).is_some_and(|t| t.name == name))
}

fn named(game: &Game, cards: &imbl::Vector<CardId>, name: &str) -> usize {
    cards
        .iter()
        .filter(|c| game.card(**c).is_some_and(|t| t.name == name))
        .count()
}

fn step(game: &mut Game, a: Answer) -> Decision {
    game.step(a).unwrap_or_else(|e| panic!("{e}"))
}

/// Pass until `who` has priority in their own `step`, discarding the
/// first cards in hand to hand size.
fn pass_until(game: &mut Game, who: PlayerId, s: Step) {
    for _ in 0..1000 {
        match game.decision() {
            Decision::Priority { player, .. }
                if player == who && game.active() == who && game.step_now() == s =>
            {
                return;
            }
            Decision::Discard { count, hand, .. } => {
                step(game, Answer::Discard(hand[..count].to_vec()));
            }
            Decision::Attackers { .. } => {
                step(game, Answer::Attack(vec![]));
            }
            Decision::Blockers { .. } => {
                step(game, Answer::Block(vec![]));
            }
            _ => {
                step(game, Answer::Act(Action::Pass));
            }
        }
    }
    panic!("never reached {s:?}")
}

/// Pass to my next turn's precombat main phase.
fn my_next_main(game: &mut Game) {
    pass_until(game, THEM, Step::Upkeep);
    pass_until(game, ME, Step::PrecombatMain);
}

/// Play a land of this name from my hand.
fn play(game: &mut Game, name: &str) {
    play_as(game, ME, name)
}

fn play_as(game: &mut Game, p: PlayerId, name: &str) {
    let c = in_hand(game, p, name).expect("in hand");
    step(game, Answer::Act(Action::PlayLand(c)));
}

/// Cast this card from my hand, choose these targets if it has any, and
/// pay with the first payment that taps what pays it.
fn cast(game: &mut Game, name: &str, targets: Option<Vec<Target>>) {
    cast_as(game, ME, name, targets)
}

fn cast_as(game: &mut Game, p: PlayerId, name: &str, targets: Option<Vec<Target>>) {
    let c = in_hand(game, p, name).expect("in hand");
    let mut d = step(game, Answer::Act(Action::Cast(c)));
    if let Some(t) = targets {
        assert!(matches!(d, Decision::Targets { .. }), "{d:?}");
        d = step(game, Answer::Targets(t));
    }
    loop {
        let Decision::Pay { actions, .. } = d else {
            return;
        };
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(game, Answer::Pay(pay));
    }
}

/// Both players pass once: the top of the stack resolves (CR 608.1).
fn resolve(game: &mut Game) -> Decision {
    step(game, Answer::Act(Action::Pass));
    step(game, Answer::Act(Action::Pass))
}

fn creature(game: &Game, name: &str) -> Option<chancellor_of_the_spires::model::ObjectId> {
    game.permanents()
        .iter()
        .find(|(_, p)| game.card(p.card).is_some_and(|t| t.name == name))
        .map(|(id, _)| *id)
}

/// CR 601.2c, 701.20a, 701.9a, 119.3, 608.2n: Thoughtseize targets a
/// player as it is cast; resolving, their hand is revealed, I choose a
/// nonland card of it, they discard it, and I lose 2 life. A land is
/// never a legal choice. The spell then goes to my graveyard.
#[test]
fn thoughtseize_discards_the_nonland_card_chosen_from_the_revealed_hand() {
    let mut game = deal(
        &[("Swamp", 30), ("Thoughtseize", 30)],
        &[("Forest", 50), ("Grizzly Bears", 10)],
        &[("Swamp", 1), ("Thoughtseize", 1)],
    );
    play(&mut game, "Swamp");
    cast(&mut game, "Thoughtseize", Some(vec![Target::Player(THEM)]));
    let Decision::ChooseFromHand {
        player,
        whose,
        legal,
        hand,
        unsure,
    } = resolve(&mut game)
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((player, whose, unsure), (ME, THEM, 0));
    assert_eq!(hand.len(), 7);
    assert!(legal
        .iter()
        .all(|c| game.card(*c).unwrap().name == "Grizzly Bears"));
    // A land is not a nonland card.
    let forest = *hand
        .iter()
        .find(|c| game.card(**c).unwrap().name == "Forest")
        .expect("a Forest in a hand of 50 Forests");
    assert!(game.step(Answer::Pick(Some(forest))).is_err());
    match legal.first() {
        Some(bears) => {
            step(&mut game, Answer::Pick(Some(*bears)));
            assert_eq!(
                named(&game, game.player(THEM).graveyard(), "Grizzly Bears"),
                1
            );
            assert_eq!(game.player(THEM).hand().len(), 6);
        }
        // No nonland card: nothing is chosen (CR 701.20a).
        None => {
            step(&mut game, Answer::Pick(None));
        }
    }
    assert_eq!(game.player(ME).life(), 18);
    assert_eq!(named(&game, game.player(ME).graveyard(), "Thoughtseize"), 1);
    assert!(game.stack().is_empty());
}

/// CR 120.3a, 120.3e, 704.5g, 115.4: Lightning Bolt deals 3 damage to any
/// target, a player losing that much life, a creature having it marked and
/// dying of it as state-based actions are checked.
#[test]
fn lightning_bolt_hits_a_player_or_kills_a_creature() {
    let mut game = deal(
        &[
            ("Forest", 15),
            ("Mountain", 15),
            ("Birds of Paradise", 10),
            ("Lightning Bolt", 20),
        ],
        &[("Forest", 60)],
        &[
            ("Forest", 1),
            ("Mountain", 1),
            ("Birds of Paradise", 1),
            ("Lightning Bolt", 1),
        ],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Birds of Paradise", None);
    resolve(&mut game);
    let birds = creature(&game, "Birds of Paradise").expect("Birds entered");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    // The legal targets are both players and the creature (CR 115.4).
    let c = in_hand(&game, ME, "Lightning Bolt").unwrap();
    let Decision::Targets { slots, .. } = game.clone().step(Answer::Act(Action::Cast(c))).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        slots,
        vec![vec![
            Target::Player(ME),
            Target::Player(THEM),
            Target::Object(birds)
        ]]
    );
    let mut at_them = game.clone();
    cast(
        &mut at_them,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    resolve(&mut at_them);
    assert_eq!(at_them.player(THEM).life(), 17);
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Object(birds)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Birds of Paradise").is_none());
    assert_eq!(
        named(&game, game.player(ME).graveyard(), "Birds of Paradise"),
        1
    );
}

/// CR 608.2b: a spell whose only target is illegal as it resolves doesn't
/// resolve, and does nothing. Two Bolts at one Birds: the second cast
/// resolves first and kills it, and the first finds its target gone (a
/// permanent that leaves is a new object, CR 400.7) and leaves my life
/// as it was.
#[test]
fn a_spell_whose_target_is_gone_does_nothing() {
    let mut game = deal(
        &[
            ("Forest", 10),
            ("Mountain", 20),
            ("Birds of Paradise", 5),
            ("Lightning Bolt", 25),
        ],
        &[("Forest", 60)],
        &[
            ("Forest", 1),
            ("Birds of Paradise", 1),
            ("Mountain", 2),
            ("Lightning Bolt", 2),
        ],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Birds of Paradise", None);
    resolve(&mut game);
    let birds = creature(&game, "Birds of Paradise").unwrap();
    // Two turns of land drops for two Mountains.
    for _ in 0..2 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    let life = game.player(THEM).life();
    for _ in 0..2 {
        cast(
            &mut game,
            "Lightning Bolt",
            Some(vec![Target::Object(birds)]),
        );
    }
    assert_eq!(game.stack().len(), 2);
    resolve(&mut game);
    assert!(creature(&game, "Birds of Paradise").is_none());
    resolve(&mut game);
    assert!(game.stack().is_empty());
    assert_eq!(
        named(&game, game.player(ME).graveyard(), "Lightning Bolt"),
        2
    );
    assert_eq!(game.player(THEM).life(), life);
}

/// CR 701.22a, 121.2: Preordain looks at the top two, puts one on the
/// bottom, and draws the other.
#[test]
fn preordain_scries_two_then_draws() {
    let mut game = deal(
        &[("Island", 30), ("Preordain", 30)],
        &[("Forest", 60)],
        &[("Island", 1), ("Preordain", 1)],
    );
    play(&mut game, "Island");
    cast(&mut game, "Preordain", None);
    let Decision::Scry { player, cards } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((player, cards.len()), (ME, 2));
    let library = game.player(ME).library().clone();
    assert_eq!(cards, library.iter().take(2).copied().collect::<Vec<_>>());
    // Every card looked at goes somewhere, once.
    assert!(game
        .step(Answer::Scry {
            top: vec![cards[0]],
            bottom: vec![],
        })
        .is_err());
    let hand = game.player(ME).hand().len();
    step(
        &mut game,
        Answer::Scry {
            top: vec![cards[1]],
            bottom: vec![cards[0]],
        },
    );
    assert!(game.player(ME).hand().contains(&cards[1]));
    assert_eq!(game.player(ME).hand().len(), hand + 1);
    assert_eq!(game.player(ME).library().back(), Some(&cards[0]));
}

/// CR 603.2, 603.3, 603.6a, 122.1: Champion of the Parish triggers when
/// another Human I control enters, not when it enters itself; the
/// ability goes on the stack the next time a player would receive
/// priority, and resolving puts a +1/+1 counter on it.
#[test]
fn champion_of_the_parish_grows_when_another_human_enters() {
    let mut game = deal(
        &[
            ("Plains", 30),
            ("Champion of the Parish", 15),
            ("Elite Vanguard", 15),
        ],
        &[("Forest", 60)],
        &[
            ("Plains", 2),
            ("Champion of the Parish", 1),
            ("Elite Vanguard", 1),
        ],
    );
    play(&mut game, "Plains");
    cast(&mut game, "Champion of the Parish", None);
    resolve(&mut game);
    let champion = creature(&game, "Champion of the Parish").unwrap();
    assert!(game.stack().is_empty(), "it doesn't see itself enter");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Elite Vanguard", None);
    resolve(&mut game);
    assert!(matches!(
        game.stack().back(),
        Some(StackObject::Trigger { object, controller: ME, .. }) if *object == champion
    ));
    resolve(&mut game);
    assert_eq!(game.characteristics()[&champion].pt, Some(pt(2, 2)));
}

/// CR 603.6a, 701.22a: Temple of Deceit's "When this land enters, scry
/// 1" triggers as it is played and asks me to scry.
#[test]
fn a_temple_scries_as_it_enters() {
    let mut game = deal(
        &[("Temple of Deceit", 60)],
        &[("Forest", 60)],
        &[("Temple of Deceit", 1)],
    );
    play(&mut game, "Temple of Deceit");
    assert!(matches!(
        game.stack().back(),
        Some(StackObject::Trigger { controller: ME, .. })
    ));
    let Decision::Scry { cards, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(cards.len(), 1);
    step(
        &mut game,
        Answer::Scry {
            top: vec![],
            bottom: cards.clone(),
        },
    );
    assert_eq!(game.player(ME).library().back(), Some(&cards[0]));
}

/// Pass until the active player declares attackers (CR 508.1).
fn to_attackers(
    game: &mut Game,
) -> (
    Vec<chancellor_of_the_spires::model::ObjectId>,
    Vec<PlayerId>,
) {
    for _ in 0..100 {
        match game.decision() {
            Decision::Attackers { can, defenders, .. } => return (can, defenders),
            _ => {
                step(game, Answer::Act(Action::Pass));
            }
        }
    }
    panic!("no declaration of attackers")
}

/// Pass, without blocking, until combat is over.
fn through_combat(game: &mut Game) {
    while game.step_now() <= Step::EndOfCombat {
        match game.decision() {
            Decision::Blockers { .. } => step(game, Answer::Block(vec![])),
            _ => step(game, Answer::Act(Action::Pass)),
        };
    }
}

fn pt_of(
    game: &Game,
    id: chancellor_of_the_spires::model::ObjectId,
) -> chancellor_of_the_spires::model::Pt {
    game.characteristics()[&id].pt.unwrap()
}

/// CR 302.6, 508.1a, 508.1f, 510.1b, 120.3a: a creature that came under
/// my control this turn can't attack; next turn it attacks, taps, and
/// unblocked deals its power to the player it attacks.
#[test]
fn an_unblocked_creature_deals_its_power_to_the_player() {
    let mut game = deal(
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Forest", 60)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    // Summoning sick: no creature can attack, so nothing is asked.
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 20);
    my_next_main(&mut game);
    let (can, defenders) = to_attackers(&mut game);
    assert_eq!((can, defenders), (vec![bears], vec![THEM]));
    step(&mut game, Answer::Attack(vec![(bears, THEM)]));
    assert!(game.permanents()[&bears].tapped);
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
}

/// CR 509.1a, 702.9b, 510.1c–d, 704.5g: Grizzly Bears can't block Storm
/// Crow, which flies; blocking Grizzly Bears they trade, each dealt
/// lethal damage.
#[test]
fn blocks_obey_flying_and_blocked_creatures_trade() {
    let mut game = deal_both(
        &[
            ("Forest", 20),
            ("Island", 20),
            ("Grizzly Bears", 10),
            ("Storm Crow", 10),
        ],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[
            ("Forest", 2),
            ("Island", 1),
            ("Grizzly Bears", 1),
            ("Storm Crow", 1),
        ],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Forest");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Island");
    cast(&mut game, "Storm Crow", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let theirs = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    let crow = creature(&game, "Storm Crow").unwrap();
    let (can, _) = to_attackers(&mut game);
    assert_eq!(can, vec![crow], "my Bears are summoning sick");
    step(&mut game, Answer::Attack(vec![(crow, THEM)]));
    let blockers = loop {
        match game.decision() {
            Decision::Blockers { player, can, .. } => {
                assert_eq!(player, THEM);
                break can;
            }
            _ => step(&mut game, Answer::Act(Action::Pass)),
        };
    };
    assert_eq!(blockers, vec![theirs]);
    assert!(game.step(Answer::Block(vec![(theirs, crow)])).is_err());
    step(&mut game, Answer::Block(vec![]));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 19);
    my_next_main(&mut game);
    let mine = game
        .permanents()
        .iter()
        .find(|(id, p)| **id != theirs && game.card(p.card).unwrap().name == "Grizzly Bears")
        .map(|(id, _)| *id)
        .unwrap();
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(mine, THEM), (crow, THEM)]));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    step(&mut game, Answer::Block(vec![(theirs, mine)]));
    through_combat(&mut game);
    assert!(!game.permanents().contains_key(&mine));
    assert!(!game.permanents().contains_key(&theirs));
    assert_eq!(game.player(THEM).life(), 18);
}

/// CR 510.4, 702.7b: Youthful Knight's first strike deals its damage in a
/// step of its own, so the Grizzly Bears blocking it die before they deal
/// theirs.
#[test]
fn first_strike_kills_the_blocker_first() {
    let mut game = deal_both(
        &[("Plains", 40), ("Youthful Knight", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Plains", 2), ("Youthful Knight", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Youthful Knight", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    let knight = creature(&game, "Youthful Knight").unwrap();
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(knight, THEM)]));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    step(&mut game, Answer::Block(vec![(bears, knight)]));
    let mut steps = Vec::new();
    while game.step_now() <= Step::EndOfCombat {
        steps.push(game.step_now());
        step(&mut game, Answer::Act(Action::Pass));
    }
    assert!(steps.contains(&Step::FirstStrikeDamage));
    assert!(!game.permanents().contains_key(&bears));
    assert!(game.permanents().contains_key(&knight));
    assert_eq!(game.permanents()[&knight].damage, 0);
}

/// CR 702.83a, 506.5, 603.3, 514.2: Noble Hierarch's exalted triggers when
/// a creature I control attacks alone, which gets +1/+1 until end of turn,
/// and is a 2/2 again after cleanup.
#[test]
fn exalted_pumps_a_creature_attacking_alone_until_end_of_turn() {
    let mut game = deal(
        &[
            ("Forest", 30),
            ("Noble Hierarch", 15),
            ("Grizzly Bears", 15),
        ],
        &[("Forest", 60)],
        &[("Forest", 2), ("Noble Hierarch", 1), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Noble Hierarch", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(bears, THEM)]));
    assert!(matches!(
        game.stack().back(),
        Some(StackObject::Trigger { triggering: Some(t), .. }) if *t == bears
    ));
    resolve(&mut game);
    assert_eq!(pt_of(&game, bears), pt(3, 3));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 17);
    pass_until(&mut game, THEM, Step::Upkeep);
    assert_eq!(pt_of(&game, bears), pt(2, 2));
}

/// CR 702.108a, 601.2i: Monastery Swiftspear's prowess triggers as I cast
/// Lightning Bolt, a noncreature spell, and resolves first, above it.
#[test]
fn prowess_triggers_on_a_noncreature_spell() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Monastery Swiftspear", 15),
            ("Lightning Bolt", 15),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 2),
            ("Monastery Swiftspear", 1),
            ("Lightning Bolt", 1),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Monastery Swiftspear", None);
    resolve(&mut game);
    let spear = creature(&game, "Monastery Swiftspear").unwrap();
    assert!(
        game.stack().is_empty(),
        "a creature spell isn't noncreature"
    );
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    assert_eq!(game.stack().len(), 2);
    resolve(&mut game);
    assert_eq!(pt_of(&game, spear), pt(2, 3));
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 17);
}

/// CR 508.3a, 113.7a: Scorch Spitter's "Whenever this creature attacks,
/// it deals 1 damage to the player or planeswalker it's attacking".
#[test]
fn an_attack_trigger_damages_the_player_attacked() {
    let mut game = deal(
        &[("Mountain", 40), ("Scorch Spitter", 20)],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Scorch Spitter", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Scorch Spitter", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let spitter = creature(&game, "Scorch Spitter").unwrap();
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(spitter, THEM)]));
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 19);
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
}

/// CR 603.2, 120.3e, 113.7a: Jackal Pup's "Whenever this creature is dealt
/// damage, it deals that much damage to you": Lightning Bolt kills it,
/// and its trigger, triggered as the damage was dealt, still deals its
/// controller 3, the amount of the event.
#[test]
fn jackal_pup_deals_its_controller_the_damage_it_is_dealt() {
    let mut game = deal(
        &[("Mountain", 30), ("Jackal Pup", 15), ("Lightning Bolt", 15)],
        &[("Forest", 60)],
        &[("Mountain", 2), ("Jackal Pup", 1), ("Lightning Bolt", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Jackal Pup", None);
    resolve(&mut game);
    let pup = creature(&game, "Jackal Pup").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Lightning Bolt", Some(vec![Target::Object(pup)]));
    resolve(&mut game);
    assert!(!game.permanents().contains_key(&pup));
    assert!(matches!(
        game.stack().back(),
        Some(StackObject::Trigger {
            amount: Some(3),
            ..
        })
    ));
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 17);
}

/// CR 608.2h, 605.3b: Cloudpost's "Add {C} for each Locus on the
/// battlefield" counts every Locus, "on the battlefield" naming the zone a
/// permanent filter reads already.
#[test]
fn cloudpost_taps_for_each_locus_on_the_battlefield() {
    let mut game = deal(
        &[("Cloudpost", 30), ("Forest", 30)],
        &[("Forest", 60)],
        &[("Cloudpost", 2)],
    );
    play(&mut game, "Cloudpost");
    my_next_main(&mut game);
    play(&mut game, "Cloudpost");
    my_next_main(&mut game);
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("priority")
    };
    let adds: Vec<ManaPool> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Mana(m) => Some(m.adds),
            _ => None,
        })
        .collect();
    let two = ManaPool::of([Mana::Colorless, Mana::Colorless]);
    assert_eq!(adds, vec![two, two], "each Cloudpost adds {{C}}{{C}}");
}

/// Goblin Lackey attacks unblocked and deals damage to a player, so its
/// trigger resolves: until the combat damage step's priority, passing.
fn lackey_connects(game: &mut Game) -> CardId {
    play(game, "Mountain");
    cast(game, "Goblin Lackey", None);
    resolve(game);
    my_next_main(game);
    let lackey = creature(game, "Goblin Lackey").unwrap();
    to_attackers(game);
    step(game, Answer::Attack(vec![(lackey, THEM)]));
    for _ in 0..20 {
        if let Decision::May { card, .. } = game.decision() {
            return card;
        }
        match game.decision() {
            Decision::Blockers { .. } => step(game, Answer::Block(vec![])),
            _ => step(game, Answer::Act(Action::Pass)),
        };
    }
    panic!("Goblin Lackey's trigger never asked")
}

/// CR 120.3, 510.2, 603.2, 603.5, 608.2d: Goblin Lackey's "Whenever this
/// creature deals damage to a player, you may put a Goblin permanent card
/// from your hand onto the battlefield" triggers on its combat damage; as
/// it resolves I choose to, and choose a Goblin, never a land.
#[test]
fn goblin_lackey_may_put_a_goblin_from_hand_onto_the_battlefield() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Goblin Lackey", 15),
            ("Mogg Fanatic", 15),
        ],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Goblin Lackey", 1), ("Mogg Fanatic", 1)],
    );
    let lackey = lackey_connects(&mut game);
    assert_eq!(game.card(lackey).unwrap().name, "Goblin Lackey");
    assert_eq!(game.player(THEM).life(), 19);
    let fanatic = in_hand(&game, ME, "Mogg Fanatic").unwrap();
    let Decision::PutFromHand { legal, unsure, .. } = step(&mut game, Answer::May(true)) else {
        panic!("a card to choose")
    };
    assert_eq!(unsure, 0);
    assert!(legal.contains(&fanatic));
    let mountain = in_hand(&game, ME, "Mountain");
    if let Some(m) = mountain {
        assert!(!legal.contains(&m), "a land isn't a Goblin");
        assert!(game.clone().step(Answer::Pick(Some(m))).is_err());
    }
    assert!(game.clone().step(Answer::Pick(None)).is_err());
    step(&mut game, Answer::Pick(Some(fanatic)));
    assert!(creature(&game, "Mogg Fanatic").is_some());
    assert!(in_hand(&game, ME, "Mogg Fanatic") != Some(fanatic));
}

/// CR 603.5: declining the "may", nothing is put onto the battlefield.
#[test]
fn goblin_lackey_may_be_declined() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Goblin Lackey", 15),
            ("Mogg Fanatic", 15),
        ],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Goblin Lackey", 1), ("Mogg Fanatic", 1)],
    );
    lackey_connects(&mut game);
    step(&mut game, Answer::May(false));
    assert!(creature(&game, "Mogg Fanatic").is_none());
    assert!(matches!(game.decision(), Decision::Priority { .. }));
}

/// Pass until a "may" is asked, and say whose card's.
fn to_may(game: &mut Game) -> CardId {
    for _ in 0..200 {
        match game.decision() {
            Decision::May { card, .. } => return card,
            Decision::Discard { count, hand, .. } => {
                step(game, Answer::Discard(hand[..count].to_vec()));
            }
            Decision::Attackers { .. } => {
                step(game, Answer::Attack(vec![]));
            }
            _ => {
                step(game, Answer::Act(Action::Pass));
            }
        }
    }
    panic!("no may")
}

/// CR 503.1a, 603.5, 608.2h: Aether Vial's "At the beginning of your
/// upkeep, you may put a charge counter on this artifact" triggers in my
/// upkeep and not theirs; with one counter, its "{T}: You may put a
/// creature card with mana value equal to the number of charge counters
/// on this artifact from your hand onto the battlefield" offers Goblin
/// Lackey, mana value 1, and not Grizzly Bears, 2.
#[test]
fn aether_vial_counts_up_in_my_upkeep_and_puts_a_creature_of_that_mana_value() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Aether Vial", 10),
            ("Goblin Lackey", 10),
            ("Grizzly Bears", 10),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 1),
            ("Aether Vial", 1),
            ("Goblin Lackey", 1),
            ("Grizzly Bears", 1),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Aether Vial", None);
    resolve(&mut game);
    let vial_card = to_may(&mut game);
    assert_eq!(game.card(vial_card).unwrap().name, "Aether Vial");
    assert_eq!((game.active(), game.step_now()), (ME, Step::Upkeep));
    step(&mut game, Answer::May(true));
    let vial = creature(&game, "Aether Vial").unwrap();
    let charge = chancellor_of_the_spires::model::Counter::Named("charge".into());
    assert_eq!(game.counters_on(vial, &charge), 1);
    pass_until(&mut game, ME, Step::PrecombatMain);
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("priority")
    };
    let activate = actions
        .iter()
        .find_map(|a| match a {
            Action::Activate(x) if x.source == vial => Some(*x),
            _ => None,
        })
        .expect("Vial's ability is offered");
    step(&mut game, Answer::Act(Action::Activate(activate)));
    step(&mut game, Answer::Act(Action::Pass));
    assert!(matches!(
        step(&mut game, Answer::Act(Action::Pass)),
        Decision::May { .. }
    ));
    let Decision::PutFromHand { legal, .. } = step(&mut game, Answer::May(true)) else {
        panic!("a card to choose")
    };
    let lackey = in_hand(&game, ME, "Goblin Lackey").unwrap();
    assert!(legal.contains(&lackey));
    let bears = in_hand(&game, ME, "Grizzly Bears").unwrap();
    assert!(!legal.contains(&bears));
    step(&mut game, Answer::Pick(Some(lackey)));
    assert!(creature(&game, "Goblin Lackey").is_some());
}

/// CR 503.1a, 701.20a, 119.3: Dark Confidant's "At the beginning of your
/// upkeep, reveal the top card of your library and put that card into your
/// hand. You lose life equal to its mana value." A deck file's cards are
/// known, so nothing is asked to name the card revealed.
#[test]
fn dark_confidant_reveals_the_top_card_into_hand_and_costs_its_mana_value() {
    let mut game = deal(
        &[("Swamp", 30), ("Dark Confidant", 30)],
        &[("Forest", 60)],
        &[("Swamp", 2), ("Dark Confidant", 1)],
    );
    play(&mut game, "Swamp");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Dark Confidant", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::Upkeep);
    pass_until(&mut game, ME, Step::Upkeep);
    let top = game.player(ME).library()[0];
    let mv = if game.card(top).unwrap().name == "Dark Confidant" {
        2
    } else {
        0
    };
    let hand = game.player(ME).hand().len();
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand + 1);
    assert!(game.player(ME).hand().contains(&top));
    assert_eq!(game.player(ME).life(), 20 - mv);
}

/// CR 508.1m, 701.20a: Goblin Guide's "Whenever this creature attacks,
/// defending player reveals the top card of their library. If it's a land
/// card, that player puts it into their hand."
#[test]
fn goblin_guide_shows_the_defender_s_top_card_and_a_land_goes_to_hand() {
    for (theirs, to_hand) in [("Forest", true), ("Grizzly Bears", false)] {
        let mut game = deal(
            &[("Mountain", 30), ("Goblin Guide", 30)],
            &[(theirs, 60)],
            &[("Mountain", 1), ("Goblin Guide", 1)],
        );
        play(&mut game, "Mountain");
        cast(&mut game, "Goblin Guide", None);
        resolve(&mut game);
        let guide = creature(&game, "Goblin Guide").unwrap();
        to_attackers(&mut game);
        let top = game.player(THEM).library()[0];
        let hand = game.player(THEM).hand().len();
        step(&mut game, Answer::Attack(vec![(guide, THEM)]));
        resolve(&mut game);
        assert_eq!(game.player(THEM).hand().contains(&top), to_hand, "{theirs}");
        assert_eq!(game.player(THEM).hand().len(), hand + usize::from(to_hand));
    }
}

/// CR 401.4: Impulse looks at the top four, puts the one chosen into my
/// hand and the other three on the bottom in the order I give.
#[test]
fn impulse_takes_one_of_four_and_bottoms_the_rest_in_order() {
    let mut game = deal(
        &[("Island", 30), ("Impulse", 30)],
        &[("Forest", 60)],
        &[("Island", 2), ("Impulse", 1)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    cast(&mut game, "Impulse", None);
    let four: Vec<CardId> = game.player(ME).library().iter().take(4).copied().collect();
    let Decision::ChooseAmong { cards, n, .. } = resolve(&mut game) else {
        panic!("a choice of one")
    };
    assert_eq!((cards.clone(), n), (four.clone(), 1));
    assert!(game.clone().step(Answer::Choose(vec![])).is_err());
    let Decision::Arrange { cards, top, .. } = step(&mut game, Answer::Choose(vec![four[2]]))
    else {
        panic!("an order for the rest")
    };
    assert_eq!((cards, top), (vec![four[0], four[1], four[3]], false));
    step(&mut game, Answer::Arrange(vec![four[3], four[0], four[1]]));
    assert!(game.player(ME).hand().contains(&four[2]));
    let library = game.player(ME).library();
    let bottom: Vec<CardId> = library.iter().skip(library.len() - 3).copied().collect();
    assert_eq!(bottom, vec![four[3], four[0], four[1]]);
    assert_eq!(game.bottomed(), 3);
}

/// CR 103.6, 103.6a, 702.11c, 115.4: after the mulligans, Leyline of
/// Sanctity in my opening hand may begin the game on the battlefield;
/// then I have hexproof, and their Duress can't target me.
#[test]
fn leyline_of_sanctity_begins_on_the_battlefield_and_gives_its_player_hexproof() {
    let mut found = None;
    for seed in 0..200 {
        let game = Game::start(Setup {
            format: Format::Constructed,
            decks: vec![
                deck(&[("Leyline of Sanctity", 20), ("Plains", 40)]),
                deck(&[("Swamp", 40), ("Duress", 20)]),
            ],
            mulligans: false,
            seed,
        })
        .unwrap();
        if named(&game, game.player(THEM).hand(), "Duress") > 0 {
            found = Some(game);
            break;
        }
    }
    let mut game = found.expect("a seed");
    let Decision::OpeningHand { player, cards } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(player, ME);
    assert!(!cards.is_empty());
    assert!(game
        .clone()
        .step(Answer::Begin(vec![cards[0], cards[0]]))
        .is_err());
    step(&mut game, Answer::Begin(vec![cards[0]]));
    assert_eq!(game.turn(), 1);
    assert!(creature(&game, "Leyline of Sanctity").is_some());
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Swamp");
    let duress = in_hand(&game, THEM, "Duress").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("priority")
    };
    assert!(
        !actions.contains(&Action::Cast(duress)),
        "Duress's only legal target is themselves, and it targets an opponent"
    );
}

/// CR 611.3a: Wild Nacatl's "This creature gets +1/+1 as long as you
/// control a Mountain" applies once a Mountain is played, read from its
/// Oracle text.
#[test]
fn wild_nacatl_grows_while_its_controller_controls_a_mountain() {
    let mut game = deal(
        &[("Forest", 20), ("Mountain", 20), ("Wild Nacatl", 20)],
        &[("Forest", 60)],
        &[("Forest", 1), ("Mountain", 1), ("Wild Nacatl", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Wild Nacatl", None);
    resolve(&mut game);
    let cat = creature(&game, "Wild Nacatl").unwrap();
    assert_eq!(pt_of(&game, cat), pt(1, 1));
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    assert_eq!(pt_of(&game, cat), pt(2, 2));
}

/// CR 701.13a, 608.2h: Swords to Plowshares exiles its target, and "its
/// controller gains life equal to its power" reads the creature as it
/// last existed.
#[test]
fn swords_to_plowshares_exiles_and_its_controller_gains_its_power() {
    let mut game = deal_both(
        &[("Plains", 40), ("Swords to Plowshares", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Plains", 1), ("Swords to Plowshares", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    cast(
        &mut game,
        "Swords to Plowshares",
        Some(vec![Target::Object(bears)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Grizzly Bears").is_none());
    assert_eq!(named(&game, game.exile(), "Grizzly Bears"), 1);
    assert_eq!(game.player(THEM).life(), 22);
}

/// CR 701.6a, 115.5: Counterspell targets a spell on the stack, never
/// itself, and the spell countered goes to its owner's graveyard unresolved.
#[test]
fn counterspell_counters_a_spell_on_the_stack() {
    let mut game = deal_both(
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Island", 40), ("Counterspell", 20)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
        &[("Island", 2), ("Counterspell", 1)],
    );
    play(&mut game, "Forest");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Island");
    my_next_main(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Island");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    let before = named(&game, game.player(ME).graveyard(), "Grizzly Bears");
    cast(&mut game, "Grizzly Bears", None);
    let bears = game.stack().iter().find_map(|o| match o {
        StackObject::Spell { card, .. } => Some(*card),
        _ => None,
    });
    step(&mut game, Answer::Act(Action::Pass));
    let counter = in_hand(&game, THEM, "Counterspell").unwrap();
    let Decision::Targets { slots, .. } = step(&mut game, Answer::Act(Action::Cast(counter)))
    else {
        panic!("Counterspell's target")
    };
    assert_eq!(slots, vec![vec![Target::Spell(bears.unwrap())]]);
    let mut d = step(
        &mut game,
        Answer::Targets(vec![Target::Spell(bears.unwrap())]),
    );
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert!(game.stack().is_empty());
    assert!(creature(&game, "Grizzly Bears").is_none());
    assert_eq!(
        named(&game, game.player(ME).graveyard(), "Grizzly Bears"),
        before + 1
    );
}

/// CR 700.4, 603.6c, 603.10a: Festering Goblin's "When this creature
/// dies, target creature gets -1/-1 until end of turn" triggers though it
/// is gone, looking back at it as it last existed: Bolted, it shrinks my
/// other Festering Goblin, which dies too.
#[test]
fn festering_goblin_triggers_as_it_dies() {
    let mut game = deal_both(
        &[("Swamp", 40), ("Festering Goblin", 20)],
        &[("Mountain", 40), ("Lightning Bolt", 20)],
        &[("Swamp", 2), ("Festering Goblin", 2)],
        &[("Mountain", 1), ("Lightning Bolt", 1)],
    );
    play(&mut game, "Swamp");
    cast(&mut game, "Festering Goblin", None);
    resolve(&mut game);
    let first = creature(&game, "Festering Goblin").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Festering Goblin", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Object(first)]),
    );
    let Decision::Targets { player, slots, .. } = resolve(&mut game) else {
        panic!("the dies trigger's target")
    };
    assert_eq!(player, ME);
    let second = creature(&game, "Festering Goblin").unwrap();
    assert_eq!(slots, vec![vec![Target::Object(second)]]);
    step(&mut game, Answer::Targets(vec![Target::Object(second)]));
    resolve(&mut game);
    assert!(creature(&game, "Festering Goblin").is_none());
}

/// CR 608.2d, 701.23a, 701.24a: Path to Exile exiles its target, and its
/// controller, not the caster, may search their library for a basic land
/// card, put it onto the battlefield tapped, and shuffle.
#[test]
fn path_to_exile_lets_the_creature_s_controller_search_for_a_basic_land() {
    let mut game = deal_both(
        &[("Plains", 40), ("Path to Exile", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Plains", 1), ("Path to Exile", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    cast(
        &mut game,
        "Path to Exile",
        Some(vec![Target::Object(bears)]),
    );
    let Decision::May { player, .. } = resolve(&mut game) else {
        panic!("their may")
    };
    assert_eq!(player, THEM);
    assert_eq!(named(&game, game.exile(), "Grizzly Bears"), 1);
    let Decision::Search { player, known, .. } = step(&mut game, Answer::May(true)) else {
        panic!("their search")
    };
    assert_eq!(player, THEM);
    let lands = game.permanents().len();
    step(&mut game, Answer::Find(Found::Card(known[0])));
    assert_eq!(game.permanents().len(), lands + 1);
    let (_, forest) = game
        .permanents()
        .iter()
        .find(|(_, p)| p.card == known[0])
        .unwrap();
    assert!(forest.tapped);
    assert_eq!(
        game.characteristics()
            .values()
            .filter(|c| c.controller == THEM)
            .count(),
        3
    );
}

/// CR 111.1–111.4, 111.7, 704.5d: Khalni Garden's "create a 0/1 green
/// Plant creature token" makes a token its controller owns, named Plant
/// Token; Bolted, it is put into the graveyard and ceases to exist there.
#[test]
fn khalni_garden_makes_a_plant_token_that_ceases_to_exist_off_the_battlefield() {
    let mut game = deal_both(
        &[("Khalni Garden", 30), ("Forest", 30)],
        &[("Mountain", 40), ("Lightning Bolt", 20)],
        &[("Khalni Garden", 1)],
        &[("Mountain", 1), ("Lightning Bolt", 1)],
    );
    play(&mut game, "Khalni Garden");
    resolve(&mut game);
    let plant = creature(&game, "Plant Token").expect("a Plant token");
    let chars = &game.characteristics()[&plant];
    assert_eq!(chars.pt, Some(pt(0, 1)));
    assert!(chars
        .colors
        .contains(&chancellor_of_the_spires::model::Color::Green));
    assert_eq!(chars.controller, ME);
    let graveyard = game.player(ME).graveyard().len();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Object(plant)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Plant Token").is_none());
    assert_eq!(game.player(ME).graveyard().len(), graveyard);
    assert_eq!(game.created(), 0, "a new turn");
}

/// CR 702.16b, 702.16e, 702.16f: Silver Knight's protection from red. A
/// red spell can't target it, a red creature can't block it, and the
/// damage a red creature it blocks would deal it is prevented.
#[test]
fn protection_from_red_stops_targets_blocks_and_damage() {
    let mut game = deal_both(
        &[("Plains", 40), ("Silver Knight", 20)],
        &[("Mountain", 40), ("Hill Giant", 10), ("Lightning Bolt", 10)],
        &[("Plains", 2), ("Silver Knight", 1)],
        &[("Mountain", 4), ("Hill Giant", 1), ("Lightning Bolt", 1)],
    );
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Silver Knight", None);
    resolve(&mut game);
    let knight = creature(&game, "Silver Knight").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    let bolt = in_hand(&game, THEM, "Lightning Bolt").unwrap();
    let Decision::Targets { slots, .. } = step(&mut game, Answer::Act(Action::Cast(bolt))) else {
        panic!("Bolt targets")
    };
    assert!(!slots[0].contains(&Target::Object(knight)));
    assert!(slots[0].contains(&Target::Player(ME)));
    let mut d = step(&mut game, Answer::Targets(vec![Target::Player(ME)]));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 17);
    for _ in 0..2 {
        pass_until(&mut game, THEM, Step::Upkeep);
        pass_until(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Mountain");
    }
    cast_as(&mut game, THEM, "Hill Giant", None);
    resolve(&mut game);
    let giant = creature(&game, "Hill Giant").unwrap();
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(knight, THEM)]));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    assert!(game.step(Answer::Block(vec![(giant, knight)])).is_err());
    step(&mut game, Answer::Block(vec![]));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
    // Until it untaps in my untap step.
    my_next_main(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(giant, ME)]));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    step(&mut game, Answer::Block(vec![(knight, giant)]));
    while game.step_now() < Step::EndOfCombat {
        step(&mut game, Answer::Act(Action::Pass));
    }
    assert_eq!(game.permanents()[&knight].damage, 0, "prevented");
    assert_eq!(game.permanents()[&giant].damage, 2, "first strike");
}

/// What casting this card would cost now, the cast taken back (CR 733.1).
fn owed_for(game: &mut Game, p: PlayerId, name: &str) -> String {
    let c = in_hand(game, p, name).expect("in hand");
    let mut d = step(game, Answer::Act(Action::Cast(c)));
    if let Decision::Targets { slots, .. } = &d {
        let t = slots.iter().map(|s| s[0]).collect();
        d = step(game, Answer::Targets(t));
    }
    let Decision::Pay { owed, .. } = d else {
        panic!("{d:?}")
    };
    step(game, Answer::Pay(Payment::Cancel));
    owed.to_string()
}

/// CR 601.2f, 118.7a: Goblin Warchief's "Goblin spells you cast cost {1}
/// less" takes from the generic component only, and Thalia's
/// "Noncreature spells cost {1} more" taxes my spells as hers.
#[test]
fn cost_changes_add_and_take_generic_mana() {
    let mut game = deal_both(
        &[
            ("Mountain", 30),
            ("Goblin Warchief", 10),
            ("Goblin Piker", 10),
            ("Lightning Bolt", 10),
        ],
        &[("Plains", 40), ("Thalia, Guardian of Thraben", 20)],
        &[
            ("Mountain", 3),
            ("Goblin Warchief", 2),
            ("Goblin Piker", 1),
            ("Lightning Bolt", 1),
        ],
        &[("Plains", 2), ("Thalia, Guardian of Thraben", 1)],
    );
    assert_eq!(owed_for(&mut game, ME, "Lightning Bolt"), "{R}");
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Plains");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Mountain");
    assert_eq!(owed_for(&mut game, ME, "Goblin Piker"), "{1}{R}");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Plains");
    cast_as(&mut game, THEM, "Thalia, Guardian of Thraben", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    assert_eq!(owed_for(&mut game, ME, "Lightning Bolt"), "{1}{R}");
    cast(&mut game, "Goblin Warchief", None);
    resolve(&mut game);
    assert_eq!(owed_for(&mut game, ME, "Goblin Piker"), "{R}");
    assert_eq!(owed_for(&mut game, ME, "Goblin Warchief"), "{R}{R}");
    assert_eq!(owed_for(&mut game, ME, "Lightning Bolt"), "{1}{R}");
}

/// CR 608.2h, 506.4: Goblin Piledriver's attack trigger counts the other
/// attacking Goblins as it resolves, +2/+0 for each.
#[test]
fn goblin_piledriver_grows_for_each_other_attacking_goblin() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Goblin Piledriver", 15),
            ("Goblin Piker", 15),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 3),
            ("Goblin Piledriver", 1),
            ("Goblin Piker", 1),
        ],
    );
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Goblin Piledriver", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Goblin Piker", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let driver = creature(&game, "Goblin Piledriver").unwrap();
    let piker = creature(&game, "Goblin Piker").unwrap();
    to_attackers(&mut game);
    step(
        &mut game,
        Answer::Attack(vec![(driver, THEM), (piker, THEM)]),
    );
    resolve(&mut game);
    assert_eq!(pt_of(&game, driver), pt(3, 2));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 15);
    my_next_main(&mut game);
    assert_eq!(pt_of(&game, driver), pt(1, 2), "until end of turn");
}

/// My first activated ability `source` offers, activated.
fn activate(game: &mut Game, source: chancellor_of_the_spires::model::ObjectId) -> Decision {
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let a = actions
        .iter()
        .find(|a| matches!(a, Action::Activate(x) if x.source == source))
        .copied()
        .expect("offered");
    step(game, Answer::Act(a))
}

/// CR 602.2b, 601.2c, 701.21a: Mogg Fanatic's "Sacrifice this creature:
/// It deals 1 damage to any target" chooses its target as it is
/// activated, and deals the damage as it last existed.
#[test]
fn mogg_fanatic_sacrifices_itself_to_deal_one_damage_to_a_target() {
    let mut game = deal(
        &[("Mountain", 40), ("Mogg Fanatic", 20)],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Mogg Fanatic", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Mogg Fanatic", None);
    resolve(&mut game);
    let fanatic = creature(&game, "Mogg Fanatic").unwrap();
    let Decision::Targets { slots, .. } = activate(&mut game, fanatic) else {
        panic!("a target")
    };
    assert!(slots[0].contains(&Target::Player(THEM)));
    step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    assert!(
        !game.permanents().contains_key(&fanatic),
        "sacrificed as a cost"
    );
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 19);
}

/// CR 602.2b, 601.2g–h, 701.26a: Rishadan Port's "{1}, {T}: Tap target
/// land" has its target chosen, then its {1} paid from another land.
#[test]
fn rishadan_port_pays_one_to_tap_a_land() {
    let mut game = deal_both(
        &[("Rishadan Port", 20), ("Mountain", 40)],
        &[("Forest", 60)],
        &[("Rishadan Port", 1), ("Mountain", 1)],
        &[("Forest", 1)],
    );
    play(&mut game, "Rishadan Port");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Mountain");
    let port = creature(&game, "Rishadan Port").unwrap();
    let forest = creature(&game, "Forest").unwrap();
    let Decision::Targets { slots, .. } = activate(&mut game, port) else {
        panic!("a target")
    };
    assert!(slots[0].contains(&Target::Object(forest)));
    let mut d = step(&mut game, Answer::Targets(vec![Target::Object(forest)]));
    let Decision::Pay { owed, .. } = &d else {
        panic!("{d:?}")
    };
    assert_eq!(owed.to_string(), "{1}");
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    assert!(game.permanents()[&port].tapped);
    resolve(&mut game);
    assert!(game.permanents()[&forest].tapped);
}

/// CR 205.1b, 613.4b, 514.2: Mishra's Factory becomes a 2/2 Assembly-
/// Worker artifact creature until end of turn, still a land.
#[test]
fn mishras_factory_becomes_a_creature_that_is_still_a_land_until_end_of_turn() {
    use chancellor_of_the_spires::model::CardType;
    let mut game = deal(
        &[("Mishra's Factory", 20), ("Mountain", 40)],
        &[("Forest", 60)],
        &[("Mishra's Factory", 1), ("Mountain", 1)],
    );
    play(&mut game, "Mishra's Factory");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let factory = creature(&game, "Mishra's Factory").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!()
    };
    let becomes = actions
        .iter()
        .find(|a| matches!(a, Action::Activate(x) if x.source == factory))
        .copied()
        .expect("{1}: becomes a creature");
    let mut d = step(&mut game, Answer::Act(becomes));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| {
                actions
                    .iter()
                    .find(|a| matches!(a, Payment::Mana(m) if m.source != factory))
            })
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    let now = &game.characteristics()[&factory];
    assert!(now.is(CardType::Land) && now.is(CardType::Artifact) && now.is(CardType::Creature));
    assert_eq!(now.pt, Some(pt(2, 2)));
    let (can, _) = to_attackers(&mut game);
    assert!(can.contains(&factory), "a land since my turn began");
    step(&mut game, Answer::Attack(vec![(factory, THEM)]));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
    pass_until(&mut game, THEM, Step::Upkeep);
    let now = &game.characteristics()[&factory];
    assert!(!now.is(CardType::Creature) && now.pt.is_none());
}

/// CR 605.3a, 601.2g–h: Gruul Signet's "{1}, {T}: Add {R}{G}" is offered
/// once the pool can pay its {1}, and pays it from the pool, so two
/// Mountains and the Signet cast Grizzly Bears.
#[test]
fn a_signet_spends_mana_from_the_pool_to_add_two() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Gruul Signet", 15),
            ("Grizzly Bears", 15),
        ],
        &[("Forest", 60)],
        &[("Mountain", 2), ("Gruul Signet", 1), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Gruul Signet", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let signet = creature(&game, "Gruul Signet").unwrap();
    let bears = in_hand(&game, ME, "Grizzly Bears").unwrap();
    let Decision::Pay { actions, .. } = step(&mut game, Answer::Act(Action::Cast(bears))) else {
        panic!()
    };
    let by_signet = |actions: &[Payment]| {
        actions
            .iter()
            .find(|a| matches!(a, Payment::Mana(m) if m.source == signet))
            .copied()
    };
    assert_eq!(
        by_signet(&actions),
        None,
        "nothing in the pool to pay {{1}}"
    );
    let mountain = actions
        .iter()
        .find(|a| matches!(a, Payment::Mana(_)))
        .copied()
        .unwrap();
    let Decision::Pay { actions, .. } = step(&mut game, Answer::Pay(mountain)) else {
        panic!()
    };
    let signet_mana = by_signet(&actions).expect("offered with {R} in the pool");
    let Decision::Pay { actions, .. } = step(&mut game, Answer::Pay(signet_mana)) else {
        panic!()
    };
    assert_eq!(game.player(ME).pool().to_string(), "{R}{G}");
    let spend = actions
        .iter()
        .find(|a| matches!(a, Payment::Spend(_)))
        .copied()
        .unwrap();
    step(&mut game, Answer::Pay(spend));
    resolve(&mut game);
    assert!(creature(&game, "Grizzly Bears").is_some());
}

/// CR 601.2i, 603.2: Eidolon of the Great Revel triggers on any player's
/// spell with mana value 3 or less, and deals 2 damage to its caster.
#[test]
fn eidolon_of_the_great_revel_damages_whoever_casts_a_cheap_spell() {
    let mut game = deal_both(
        &[("Mountain", 40), ("Eidolon of the Great Revel", 20)],
        &[("Mountain", 40), ("Lightning Bolt", 20)],
        &[("Mountain", 2), ("Eidolon of the Great Revel", 1)],
        &[("Mountain", 2), ("Lightning Bolt", 1)],
    );
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Mountain");
    cast(&mut game, "Eidolon of the Great Revel", None);
    resolve(&mut game);
    assert_eq!(
        game.player(ME).life(),
        20,
        "not on the battlefield as it was cast"
    );
    pass_until(&mut game, THEM, Step::PrecombatMain);
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Player(ME)]),
    );
    // The trigger goes on the stack above the Bolt and resolves first.
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 17);
}

/// CR 702.18a, 702.12b: Argothian Enchantress's shroud keeps a Bolt from
/// targeting it, and Darksteel Myr's indestructible keeps lethal damage
/// from destroying it.
#[test]
fn shroud_stops_targets_and_indestructible_survives_lethal_damage() {
    let mut game = deal_both(
        &[
            ("Forest", 20),
            ("Mountain", 20),
            ("Argothian Enchantress", 10),
            ("Darksteel Myr", 10),
        ],
        &[("Mountain", 40), ("Lightning Bolt", 20)],
        &[
            ("Forest", 2),
            ("Mountain", 1),
            ("Argothian Enchantress", 1),
            ("Darksteel Myr", 1),
        ],
        &[("Mountain", 3), ("Lightning Bolt", 2)],
    );
    play(&mut game, "Forest");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    cast(&mut game, "Argothian Enchantress", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Mountain");
    cast(&mut game, "Darksteel Myr", None);
    resolve(&mut game);
    let enchantress = creature(&game, "Argothian Enchantress").unwrap();
    let myr = creature(&game, "Darksteel Myr").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    let bolt = in_hand(&game, THEM, "Lightning Bolt").unwrap();
    let Decision::Targets { slots, .. } = step(&mut game, Answer::Act(Action::Cast(bolt))) else {
        panic!("Bolt targets")
    };
    assert!(!slots[0].contains(&Target::Object(enchantress)));
    assert!(slots[0].contains(&Target::Object(myr)));
    let mut d = step(&mut game, Answer::Targets(vec![Target::Object(myr)]));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert_eq!(game.permanents()[&myr].damage, 3);
    assert!(game.permanents().contains_key(&myr), "indestructible");
}

/// CR 702.28b: Soltari Priest's shadow: Grizzly Bears can't block it.
#[test]
fn a_creature_with_shadow_is_blocked_only_by_shadow() {
    let mut game = deal_both(
        &[("Plains", 40), ("Soltari Priest", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Plains", 2), ("Soltari Priest", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Plains");
    cast(&mut game, "Soltari Priest", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    pass_until(&mut game, ME, Step::PrecombatMain);
    let priest = creature(&game, "Soltari Priest").unwrap();
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(priest, THEM)]));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    assert!(game.step(Answer::Block(vec![(bears, priest)])).is_err());
}

/// CR 106.3, 608.2c: Dark Ritual's "Add {B}{B}{B}" puts the mana in its
/// controller's pool as it resolves.
#[test]
fn dark_ritual_adds_three_black_mana() {
    let mut game = deal(
        &[("Swamp", 40), ("Dark Ritual", 20)],
        &[("Forest", 60)],
        &[("Swamp", 1), ("Dark Ritual", 1)],
    );
    play(&mut game, "Swamp");
    cast(&mut game, "Dark Ritual", None);
    resolve(&mut game);
    assert_eq!(game.player(ME).pool().to_string(), "{B}{B}{B}");
}

/// CR 608.2c, 120.3: Thalia's Lieutenant puts a +1/+1 counter on each
/// other Human I control as it enters, and Pyroclasm deals 2 damage to
/// each creature at once.
#[test]
fn an_instruction_acts_on_each_permanent_a_filter_matches() {
    let mut game = deal_both(
        &[
            ("Plains", 30),
            ("Champion of the Parish", 15),
            ("Thalia's Lieutenant", 15),
        ],
        &[("Mountain", 40), ("Pyroclasm", 20)],
        &[
            ("Plains", 2),
            ("Champion of the Parish", 1),
            ("Thalia's Lieutenant", 1),
        ],
        &[("Mountain", 2), ("Pyroclasm", 1)],
    );
    play(&mut game, "Plains");
    cast(&mut game, "Champion of the Parish", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Plains");
    cast(&mut game, "Thalia's Lieutenant", None);
    resolve(&mut game);
    // Both triggers: Champion's for another Human, the Lieutenant's own.
    for _ in 0..4 {
        match game.decision() {
            Decision::Order { triggers, .. } => {
                step(&mut game, Answer::Order((0..triggers.len()).collect()));
            }
            _ if !game.stack().is_empty() => {
                resolve(&mut game);
            }
            _ => break,
        }
    }
    let champion = creature(&game, "Champion of the Parish").unwrap();
    let lieutenant = creature(&game, "Thalia's Lieutenant").unwrap();
    assert_eq!(pt_of(&game, champion), pt(3, 3));
    assert_eq!(pt_of(&game, lieutenant), pt(1, 1));
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(&mut game, THEM, "Pyroclasm", None);
    resolve(&mut game);
    assert!(game.permanents().contains_key(&champion));
    assert!(!game.permanents().contains_key(&lieutenant));
}

/// CR 602.5: Fanatic of Rhonas's "{T}: Add {G}{G}{G}{G}. Activate only if
/// you control a creature with power 4 or greater" is offered only once
/// Giant Growth makes it a 4/7.
#[test]
fn a_mana_ability_limited_by_what_you_control_is_offered_only_then() {
    let mut game = deal(
        &[
            ("Forest", 30),
            ("Fanatic of Rhonas", 15),
            ("Giant Growth", 15),
        ],
        &[("Mountain", 60)],
        &[("Forest", 4), ("Fanatic of Rhonas", 1), ("Giant Growth", 1)],
    );
    for _ in 0..3 {
        play(&mut game, "Forest");
        my_next_main(&mut game);
    }
    play(&mut game, "Forest");
    cast(&mut game, "Fanatic of Rhonas", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let fanatic = creature(&game, "Fanatic of Rhonas").unwrap();
    let adds = |game: &Game| -> Vec<String> {
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!()
        };
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Mana(m) if m.source == fanatic => Some(m.adds.to_string()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(adds(&game), vec!["{G}"]);
    cast(
        &mut game,
        "Giant Growth",
        Some(vec![Target::Object(fanatic)]),
    );
    resolve(&mut game);
    assert_eq!(adds(&game), vec!["{G}", "{G}{G}{G}{G}"]);
}
