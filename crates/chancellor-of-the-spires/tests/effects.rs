//! Spells' and triggered abilities' effects, run through the game: targets
//! chosen on casting and checked again on resolution, a hand revealed and
//! a card chosen from it, damage, scry, draw, an enters trigger; and
//! combat, with what attacking triggers.

use std::path::Path;
use std::sync::OnceLock;

use chancellor_of_the_spires::deck::{self, CardText, DeckCards, Oracle, Side, TwoFaced};
use chancellor_of_the_spires::game::*;
use chancellor_of_the_spires::model::{pt, CardType, Color, PlayerId};
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
    two_faced(name).unwrap_or_else(|| deck::card(name, index()).unwrap_or_else(|e| panic!("{e}")))
}

/// Cards with two faces, whose faces' own text the committed index,
/// built before faces kept it, joins; as Scryfall has them.
fn two_faced(name: &str) -> Option<CardText> {
    let face = |name: &str, type_line: &str, cost: &str, text: &str, mv, color, pt| CardText {
        name: name.into(),
        type_line: type_line.into(),
        mana_cost: cost.into(),
        oracle: Oracle::OneFace(text.into()),
        mana_value: mv,
        colors: Vec::from_iter(color),
        pt,
        identity: Vec::from_iter(color),
    };
    let (layout, front, back) = match name {
        "Pelakka Predation // Pelakka Caverns" => (
            TwoFaced::Modal,
            face(
                "Pelakka Predation",
                "Sorcery",
                "{2}{B}",
                "Target opponent reveals their hand. You choose a card from it with mana value 3 or greater. That player discards that card.",
                3,
                Some(Color::Black),
                None,
            ),
            face(
                "Pelakka Caverns",
                "Land",
                "",
                "This land enters tapped.\n{T}: Add {B}.",
                0,
                None,
                None,
            ),
        ),
        "Scalding Viper // Steam Clean" => (
            TwoFaced::Adventure,
            face(
                "Scalding Viper",
                "Creature — Elemental Snake",
                "{1}{R}",
                "Whenever an opponent casts a spell with mana value 3 or less, this creature deals 1 damage to that player.",
                2,
                Some(Color::Red),
                Some(pt(2, 1)),
            ),
            face(
                "Steam Clean",
                "Sorcery — Adventure",
                "{1}{U}",
                "Return target nonland permanent to its owner's hand.",
                2,
                Some(Color::Blue),
                None,
            ),
        ),
        "Delver of Secrets // Insectile Aberration" => (
            TwoFaced::Transforming,
            face(
                "Delver of Secrets",
                "Creature — Human Wizard",
                "{U}",
                "At the beginning of your upkeep, look at the top card of your library. You may reveal that card. If an instant or sorcery card is revealed this way, transform this creature.",
                1,
                Some(Color::Blue),
                Some(pt(1, 1)),
            ),
            // Its back face's mana value is its front's (CR 712.8e).
            face(
                "Insectile Aberration",
                "Creature — Human Insect",
                "",
                "Flying",
                1,
                Some(Color::Blue),
                Some(pt(3, 2)),
            ),
        ),
        _ => return None,
    };
    CardText::two_faced(name.into(), layout, front, back)
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
        Some(StackObject::Trigger { object, controller: ME, .. }) if *object == Some(champion)
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

/// CR 508.3a, 603.3d: Hired Claw's "Whenever you attack with one or more
/// Lizards" triggers once as it attacks, and deals the opponent it
/// targets 1 damage.
#[test]
fn hired_claw_triggers_when_its_controller_attacks_with_a_lizard() {
    let mut game = deal(
        &[("Mountain", 40), ("Hired Claw", 20)],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Hired Claw", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Hired Claw", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let claw = creature(&game, "Hired Claw").unwrap();
    to_attackers(&mut game);
    let d = step(&mut game, Answer::Attack(vec![(claw, THEM)]));
    let Decision::Targets { slots, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!(slots, vec![vec![Target::Player(THEM)]]);
    step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 19);
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
}

/// CR 603.7a–c: Portent's "Draw a card at the beginning of the next
/// turn's upkeep" is a delayed trigger: it waits for the opponent's
/// upkeep, then triggers once.
#[test]
fn portent_draws_at_the_next_turns_upkeep() {
    let mut game = deal(
        &[("Island", 40), ("Portent", 20)],
        &[("Forest", 60)],
        &[("Island", 1), ("Portent", 1)],
    );
    play(&mut game, "Island");
    cast(&mut game, "Portent", Some(vec![Target::Player(ME)]));
    step(&mut game, Answer::Act(Action::Pass));
    let mut d = step(&mut game, Answer::Act(Action::Pass));
    loop {
        d = match d {
            Decision::Arrange { cards, .. } => step(&mut game, Answer::Arrange(cards)),
            Decision::May { .. } => step(&mut game, Answer::May(false)),
            _ => break,
        };
    }
    let hand = game.player(ME).hand().len();
    pass_until(&mut game, THEM, Step::Upkeep);
    assert_eq!(game.stack().len(), 1, "the delayed trigger");
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand + 1);
    pass_until(&mut game, ME, Step::PrecombatMain);
    pass_until(&mut game, THEM, Step::Upkeep);
    assert!(game.stack().is_empty(), "it triggers once");
}

/// CR 701.20a, 401.4: Goblin Ringleader reveals the top four cards of my
/// library, puts the Goblins among them into my hand and the rest on the
/// bottom in the order I choose.
#[test]
fn goblin_ringleader_takes_the_goblins_it_reveals() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Goblin Ringleader", 10),
            ("Goblin Piker", 20),
        ],
        &[("Forest", 60)],
        &[("Mountain", 4), ("Goblin Ringleader", 1)],
    );
    for _ in 0..3 {
        play(&mut game, "Mountain");
        my_next_main(&mut game);
    }
    play(&mut game, "Mountain");
    let top: Vec<CardId> = game.player(ME).library().iter().take(4).copied().collect();
    let goblins: Vec<CardId> = top
        .iter()
        .copied()
        .filter(|c| game.card(*c).unwrap().type_line.contains("Goblin"))
        .collect();
    cast(&mut game, "Goblin Ringleader", None);
    resolve(&mut game);
    let mut d = resolve(&mut game);
    if let Decision::Arrange { cards, .. } = d {
        assert_eq!(cards.len(), 4 - goblins.len());
        d = step(&mut game, Answer::Arrange(cards));
    }
    let _ = d;
    for g in &goblins {
        assert!(game.player(ME).hand().contains(g));
    }
    let library = game.player(ME).library();
    for c in top.iter().filter(|c| !goblins.contains(c)) {
        assert!(
            library.iter().rev().take(4).any(|x| x == c),
            "on the bottom"
        );
    }
}

/// CR 201.4a, 701.20a, 701.9a, 702.34a, 118.9, 601.2h: Cabal Therapy
/// names a nonland card, a land's name refused; their hand is revealed and
/// every card of that name discarded. From my graveyard it is cast again
/// for its flashback cost, sacrificing a creature as the rest of its cost
/// is paid, and exiled as it resolves rather than put back.
#[test]
fn cabal_therapy_names_a_card_and_flashes_back_for_a_creature() {
    let mut game = deal_both(
        &[("Swamp", 20), ("Cabal Therapy", 20), ("Typhoid Rats", 20)],
        &[("Forest", 30), ("Grizzly Bears", 30)],
        &[("Swamp", 1), ("Cabal Therapy", 1), ("Typhoid Rats", 1)],
        &[("Grizzly Bears", 1)],
    );
    play(&mut game, "Swamp");
    // No creature to sacrifice, so no flashback offered yet.
    cast(&mut game, "Cabal Therapy", Some(vec![Target::Player(THEM)]));
    let Decision::ChooseName { player, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(player, ME);
    assert!(game
        .step(Answer::Name(Box::new(CardText::basic("Forest"))))
        .is_err());
    let bears = named(&game, game.player(THEM).hand(), "Grizzly Bears");
    assert!(bears > 0);
    step(&mut game, Answer::Name(Box::new(card("Grizzly Bears"))));
    assert_eq!(named(&game, game.player(THEM).hand(), "Grizzly Bears"), 0);
    assert_eq!(
        named(&game, game.player(THEM).graveyard(), "Grizzly Bears"),
        bears
    );
    assert_eq!(game.discarded(), bears as u32);
    let therapy = game.player(ME).graveyard()[0];
    assert_eq!(game.card(therapy).unwrap().name, "Cabal Therapy");
    let flashback = |game: &Game| match game.decision() {
        Decision::Priority { actions, .. } => actions
            .into_iter()
            .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == therapy)),
        _ => None,
    };
    assert_eq!(flashback(&game), None);

    // With a creature on the battlefield, it is offered from the graveyard.
    my_next_main(&mut game);
    let rats = in_hand(&game, ME, "Typhoid Rats").expect("dealt");
    step(&mut game, Answer::Act(Action::Cast(rats)));
    while let Decision::Pay { actions, .. } = game.decision() {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    let Some(fb) = flashback(&game) else {
        panic!("flashback offered: {:?}", game.decision())
    };
    step(&mut game, Answer::Act(fb));
    step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    // Its flashback cost has no mana: {0}, paid from the empty pool.
    let Decision::Pay { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    step(&mut game, Answer::Pay(Payment::Spend(ManaPool::default())));
    assert!(actions.contains(&Payment::Spend(ManaPool::default())));
    let Decision::Sacrifice { count, legal, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((count, legal.len()), (1, 1));
    step(&mut game, Answer::Sacrifice(legal.clone()));
    assert_eq!(named(&game, game.player(ME).graveyard(), "Typhoid Rats"), 1);
    let Decision::ChooseName { .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    step(&mut game, Answer::Name(Box::new(card("Grizzly Bears"))));
    // Exiled, not back in the graveyard (CR 702.34a).
    assert!(game.exile().contains(&therapy));
    assert!(!game.player(ME).graveyard().contains(&therapy));
}

/// CR 702.149a: Hopeful Initiate's training triggers when it attacks with
/// a creature of greater power, Watchwolf, and puts a +1/+1 counter on it;
/// attacking alone, it doesn't.
#[test]
fn training_grows_a_creature_attacking_beside_a_bigger_one() {
    let mut game = deal(
        &[
            ("Plains", 20),
            ("Forest", 20),
            ("Hopeful Initiate", 10),
            ("Watchwolf", 10),
        ],
        &[("Forest", 60)],
        &[
            ("Plains", 1),
            ("Forest", 1),
            ("Hopeful Initiate", 1),
            ("Watchwolf", 1),
        ],
    );
    play(&mut game, "Plains");
    cast(&mut game, "Hopeful Initiate", None);
    resolve(&mut game);
    let initiate = creature(&game, "Hopeful Initiate").unwrap();
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(initiate, THEM)]));
    assert!(game.stack().is_empty(), "alone, no training");
    through_combat(&mut game);
    pass_until(&mut game, ME, Step::PostcombatMain);
    play(&mut game, "Forest");
    cast(&mut game, "Watchwolf", None);
    resolve(&mut game);
    let wolf = creature(&game, "Watchwolf").unwrap();
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(
        &mut game,
        Answer::Attack(vec![(initiate, THEM), (wolf, THEM)]),
    );
    assert_eq!(game.stack().len(), 1);
    resolve(&mut game);
    assert_eq!(pt_of(&game, initiate), pt(2, 3));
}

/// CR 603.4: Valakut's "if you control at least five other Mountains" is
/// checked as a Mountain enters, counting the Mountains other than that
/// one: the sixth Mountain triggers it, the fifth doesn't.
#[test]
fn valakut_triggers_only_with_five_other_mountains() {
    let mut game = deal(
        &[("Mountain", 40), ("Valakut, the Molten Pinnacle", 20)],
        &[("Forest", 60)],
        &[("Valakut, the Molten Pinnacle", 1), ("Mountain", 1)],
    );
    play(&mut game, "Valakut, the Molten Pinnacle");
    for turn in 1..=6 {
        my_next_main(&mut game);
        let Some(m) = in_hand(&game, ME, "Mountain") else {
            return;
        };
        step(&mut game, Answer::Act(Action::PlayLand(m)));
        let triggered =
            !game.stack().is_empty() || matches!(game.decision(), Decision::Targets { .. });
        assert_eq!(triggered, turn == 6, "Mountain number {turn}");
    }
    let Decision::Targets { slots, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(slots[0].contains(&Target::Player(THEM)));
    step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    let Decision::May { .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    step(&mut game, Answer::May(true));
    assert_eq!(game.player(THEM).life(), 17);
}

/// Fabled Passage: the basic land found enters tapped, then is untapped
/// only if its controller controls four or more lands as that instruction
/// is followed, the land found among them (CR 608.2c).
#[test]
fn fabled_passage_untaps_the_land_it_finds_with_four_lands() {
    let mut game = deal(
        &[("Forest", 40), ("Fabled Passage", 20)],
        &[("Forest", 60)],
        &[("Forest", 3), ("Fabled Passage", 2)],
    );
    let crack = |game: &mut Game| {
        let passage = creature(game, "Fabled Passage").unwrap();
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!("{:?}", game.decision())
        };
        let a = actions
            .iter()
            .find(|a| matches!(a, Action::Activate(x) if x.source == passage))
            .copied()
            .expect("offered");
        step(game, Answer::Act(a));
        let Decision::Search { known, .. } = resolve(game) else {
            panic!("{:?}", game.decision())
        };
        step(game, Answer::Find(Found::Card(known[0])));
        game.permanents()
            .values()
            .find(|p| p.card == known[0])
            .unwrap()
            .tapped
    };
    play(&mut game, "Fabled Passage");
    assert!(crack(&mut game), "one land: it stays tapped");
    for _ in 0..2 {
        my_next_main(&mut game);
        play(&mut game, "Forest");
    }
    my_next_main(&mut game);
    play(&mut game, "Fabled Passage");
    assert!(!crack(&mut game), "four lands: untapped");
}

/// CR 602.5b, 605.1a, 122.1a: Wall of Roots's "Put a -0/-1 counter on
/// this creature: Add {G}. Activate only once each turn" is a mana
/// ability without {T}, so it is activated the turn it enters, the counter
/// shrinking it, and not again that turn; next turn it is.
#[test]
fn wall_of_roots_adds_mana_once_each_turn_for_a_counter() {
    let mut game = deal(
        &[("Forest", 40), ("Wall of Roots", 20)],
        &[("Forest", 60)],
        &[("Forest", 2), ("Wall of Roots", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Wall of Roots", None);
    resolve(&mut game);
    let wall = creature(&game, "Wall of Roots").unwrap();
    let roots = |game: &Game| match game.decision() {
        Decision::Priority { actions, .. } => actions
            .into_iter()
            .find(|a| matches!(a, Action::Mana(m) if m.source == wall)),
        _ => None,
    };
    let a = roots(&game).expect("offered while summoning sick");
    step(&mut game, Answer::Act(a));
    assert_eq!(game.player(ME).pool().total(), 1);
    assert_eq!(pt_of(&game, wall), pt(0, 4));
    assert_eq!(roots(&game), None);
    my_next_main(&mut game);
    assert!(roots(&game).is_some());
}

/// Razorkin Needlehead's "Whenever an opponent draws a card, this creature
/// deals 1 damage to them": the opponent's draw step draw triggers it, a
/// draw of mine doesn't.
#[test]
fn razorkin_needlehead_punishes_an_opponent_s_draw() {
    let mut game = deal(
        &[("Mountain", 40), ("Razorkin Needlehead", 20)],
        &[("Forest", 60)],
        &[("Mountain", 2), ("Razorkin Needlehead", 1)],
    );
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Razorkin Needlehead", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::Draw);
    assert_eq!(game.stack().len(), 1);
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 19);
    my_next_main(&mut game);
    assert_eq!(game.player(ME).life(), 20);
}

/// CR 605.1a, 701.21a: Skirk Prospector's "Sacrifice a Goblin: Add {R}"
/// is offered once for each Goblin I control, itself among them, and the
/// one chosen goes to the graveyard as the mana is added.
#[test]
fn skirk_prospector_sacrifices_a_goblin_for_mana() {
    let mut game = deal(
        &[("Mountain", 40), ("Skirk Prospector", 20)],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Skirk Prospector", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Skirk Prospector", None);
    resolve(&mut game);
    let skirk = creature(&game, "Skirk Prospector").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let sac: Vec<ManaActivation> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Mana(m) if m.source == skirk => Some(*m),
            _ => None,
        })
        .collect();
    assert_eq!(sac.len(), 1);
    assert_eq!(sac[0].sacrifices, Some(skirk));
    step(&mut game, Answer::Act(Action::Mana(sac[0])));
    assert_eq!(game.player(ME).pool().total(), 1);
    assert_eq!(creature(&game, "Skirk Prospector"), None);
    assert_eq!(
        named(&game, game.player(ME).graveyard(), "Skirk Prospector"),
        1
    );
}

/// Quest for Pure Flame's "Whenever a source you control deals damage to
/// an opponent, you may put a quest counter on this enchantment": my
/// Lightning Bolt at them triggers it, a spell being a source too (CR
/// 120.3); Bolt at my own face doesn't.
#[test]
fn quest_for_pure_flame_counts_damage_my_sources_deal_an_opponent() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Quest for Pure Flame", 10),
            ("Lightning Bolt", 20),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 2),
            ("Quest for Pure Flame", 1),
            ("Lightning Bolt", 2),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Quest for Pure Flame", None);
    resolve(&mut game);
    let quest = creature(&game, "Quest for Pure Flame").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Lightning Bolt", Some(vec![Target::Player(ME)]));
    resolve(&mut game);
    assert!(game.stack().is_empty());
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    resolve(&mut game);
    assert_eq!(game.stack().len(), 1);
    let Decision::May { .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    step(&mut game, Answer::May(true));
    let quest_counter = chancellor_of_the_spires::model::Counter::Named("quest".into());
    assert_eq!(game.counters_on(quest, &quest_counter), 1);
}

/// CR 507.1: Luminarch Aspirant's "At the beginning of combat on your
/// turn, put a +1/+1 counter on target creature you control" triggers in
/// my beginning of combat step, not theirs.
#[test]
fn luminarch_aspirant_grows_a_creature_at_the_beginning_of_my_combat() {
    let mut game = deal(
        &[("Plains", 40), ("Luminarch Aspirant", 20)],
        &[("Forest", 60)],
        &[("Plains", 2), ("Luminarch Aspirant", 1)],
    );
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Luminarch Aspirant", None);
    resolve(&mut game);
    let aspirant = creature(&game, "Luminarch Aspirant").unwrap();
    step(&mut game, Answer::Act(Action::Pass));
    let d = step(&mut game, Answer::Act(Action::Pass));
    let Decision::Targets { slots, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!(slots, vec![vec![Target::Object(aspirant)]]);
    step(&mut game, Answer::Targets(vec![Target::Object(aspirant)]));
    resolve(&mut game);
    assert_eq!(pt_of(&game, aspirant), pt(2, 2));
    pass_until(&mut game, THEM, Step::PostcombatMain);
    assert_eq!(pt_of(&game, aspirant), pt(2, 2));
}

/// City of Brass's "Whenever this land becomes tapped, it deals 1 damage to
/// you" triggers when it is tapped for mana (CR 701.26a), and Outcaster
/// Trailblazer's "add one mana of any color" is the colour chosen as it
/// resolves (CR 106.3).
#[test]
fn city_of_brass_hurts_when_tapped_and_a_trigger_adds_mana_of_a_colour_chosen() {
    let mut game = deal(
        &[("City of Brass", 30), ("Outcaster Trailblazer", 30)],
        &[("Forest", 60)],
        &[("City of Brass", 3), ("Outcaster Trailblazer", 1)],
    );
    play(&mut game, "City of Brass");
    let city = creature(&game, "City of Brass").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let tap = actions
        .iter()
        .find(|a| matches!(a, Action::Mana(m) if m.source == city))
        .copied()
        .unwrap();
    step(&mut game, Answer::Act(tap));
    assert_eq!(game.stack().len(), 1);
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 19);
    for _ in 0..2 {
        my_next_main(&mut game);
        play(&mut game, "City of Brass");
    }
    let trailblazer = in_hand(&game, ME, "Outcaster Trailblazer").unwrap();
    step(&mut game, Answer::Act(Action::Cast(trailblazer)));
    while let Decision::Pay { actions, .. } = game.decision() {
        let green = game.player(ME).pool().get(Mana::Green) > 0;
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| {
                actions
                    .iter()
                    .find(|a| matches!(a, Payment::Mana(m) if green || m.adds.get(Mana::Green) > 0))
            })
            .copied()
            .expect("a way to pay");
        step(&mut game, Answer::Pay(pay));
    }
    // The three Cities' triggers, then the Trailblazer, then its own.
    loop {
        match game.decision() {
            Decision::Order { triggers, .. } => {
                step(&mut game, Answer::Order((0..triggers.len()).collect()));
            }
            Decision::Priority { .. } if !game.stack().is_empty() => {
                resolve(&mut game);
            }
            _ => break,
        }
    }
    assert_eq!(game.player(ME).life(), 16);
    let Decision::ChooseMana { options, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(options.len(), 5);
    step(&mut game, Answer::Mana(options[1]));
    assert_eq!(game.player(ME).pool().total(), 1);
}

/// CR 118.12a: Mana Leak counters the spell unless its controller pays
/// {3}; declining, it is countered, and paying, it resolves.
#[test]
fn mana_leak_counters_unless_its_controller_pays() {
    for pays in [false, true] {
        let mut game = deal_both(
            &[("Island", 40), ("Mana Leak", 20)],
            &[("Forest", 40), ("Grizzly Bears", 20)],
            &[("Island", 2), ("Mana Leak", 1)],
            &[("Forest", 5), ("Grizzly Bears", 1)],
        );
        play(&mut game, "Island");
        for _ in 0..4 {
            pass_until(&mut game, THEM, Step::PrecombatMain);
            play_as(&mut game, THEM, "Forest");
            my_next_main(&mut game);
            if in_hand(&game, ME, "Island").is_some() {
                play(&mut game, "Island");
            }
        }
        pass_until(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Forest");
        let bears = in_hand(&game, THEM, "Grizzly Bears").unwrap();
        cast_as(&mut game, THEM, "Grizzly Bears", None);
        // They pass priority to me with Bears on the stack.
        step(&mut game, Answer::Act(Action::Pass));
        cast(&mut game, "Mana Leak", Some(vec![Target::Spell(bears)]));
        let Decision::PayUnless {
            player,
            owed,
            actions,
            ..
        } = resolve(&mut game)
        else {
            panic!("{:?}", game.decision())
        };
        assert_eq!((player, owed.generic), (THEM, 3));
        if pays {
            let mut actions = actions;
            loop {
                let pay = actions
                    .iter()
                    .find(|a| matches!(a, Payment::Spend(_)))
                    .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
                    .copied()
                    .expect("three Forests untapped");
                let d = step(&mut game, Answer::Pay(pay));
                match d {
                    Decision::PayUnless { actions: a, .. } => actions = a,
                    _ => break,
                }
            }
            resolve(&mut game);
            assert!(creature(&game, "Grizzly Bears").is_some());
        } else {
            step(&mut game, Answer::Pay(Payment::Cancel));
            assert!(game.player(THEM).graveyard().contains(&bears));
            assert!(game.stack().is_empty());
        }
    }
}

/// CR 702.30a: echo triggers in the upkeep after the permanent came under
/// its controller's control, and it is sacrificed unless the echo is paid;
/// a turn later it doesn't trigger again.
#[test]
fn echo_is_paid_or_the_permanent_sacrificed_in_the_next_upkeep() {
    for pays in [false, true] {
        let mut game = deal(
            &[("Mountain", 40), ("Goblin Patrol", 20)],
            &[("Forest", 60)],
            &[("Mountain", 2), ("Goblin Patrol", 1)],
        );
        play(&mut game, "Mountain");
        cast(&mut game, "Goblin Patrol", None);
        resolve(&mut game);
        pass_until(&mut game, ME, Step::Upkeep);
        assert_eq!(game.stack().len(), 1, "echo triggers");
        let Decision::PayUnless { actions, .. } = resolve(&mut game) else {
            panic!("{:?}", game.decision())
        };
        if pays {
            let tap = actions
                .iter()
                .find(|a| matches!(a, Payment::Mana(_)))
                .copied()
                .unwrap();
            let Decision::PayUnless { actions, .. } = step(&mut game, Answer::Pay(tap)) else {
                panic!("{:?}", game.decision())
            };
            let spend = actions
                .iter()
                .find(|a| matches!(a, Payment::Spend(_)))
                .copied()
                .unwrap();
            step(&mut game, Answer::Pay(spend));
            assert!(creature(&game, "Goblin Patrol").is_some());
            my_next_main(&mut game);
            pass_until(&mut game, THEM, Step::Upkeep);
            pass_until(&mut game, ME, Step::Upkeep);
            assert!(game.stack().is_empty(), "no echo a turn later");
        } else {
            step(&mut game, Answer::Pay(Payment::Cancel));
            assert_eq!(creature(&game, "Goblin Patrol"), None);
        }
    }
}

/// CR 118.12a, 701.9a: Masticore's "At the beginning of your upkeep,
/// sacrifice this creature unless you discard a card": a card discarded
/// keeps it, and declining sacrifices it.
#[test]
fn masticore_is_sacrificed_in_the_upkeep_unless_a_card_is_discarded() {
    for discards in [false, true] {
        let mut game = deal(
            &[("Mountain", 40), ("Masticore", 20)],
            &[("Forest", 60)],
            &[("Mountain", 4), ("Masticore", 1)],
        );
        for _ in 0..3 {
            play(&mut game, "Mountain");
            my_next_main(&mut game);
        }
        play(&mut game, "Mountain");
        cast(&mut game, "Masticore", None);
        resolve(&mut game);
        assert!(creature(&game, "Masticore").is_some());
        pass_until(&mut game, THEM, Step::Upkeep);
        pass_until(&mut game, ME, Step::Upkeep);
        assert_eq!(game.stack().len(), 1, "its upkeep trigger");
        let hand = game.player(ME).hand().len();
        let Decision::DiscardUnless {
            player,
            legal,
            unsure,
            ..
        } = resolve(&mut game)
        else {
            panic!("{:?}", game.decision())
        };
        assert_eq!((player, legal.len(), unsure), (ME, hand, 0));
        if discards {
            step(&mut game, Answer::Pick(Some(legal[0])));
            assert!(creature(&game, "Masticore").is_some());
            assert_eq!(game.player(ME).hand().len(), hand - 1);
            assert!(game.player(ME).graveyard().contains(&legal[0]));
        } else {
            step(&mut game, Answer::Pick(None));
            assert_eq!(creature(&game, "Masticore"), None);
            assert_eq!(game.player(ME).hand().len(), hand);
        }
    }
}

/// CR 118.12a, 701.21a: Phyrexian Dreadnought's "sacrifice it unless you
/// sacrifice any number of creatures with total power 12 or greater":
/// Grizzly Bears alone are too little, Dreadnought itself is enough, and
/// sacrificing nothing has it sacrificed.
#[test]
fn phyrexian_dreadnought_is_sacrificed_unless_twelve_power_is() {
    for pays in [false, true] {
        let mut game = deal(
            &[
                ("Forest", 30),
                ("Grizzly Bears", 15),
                ("Phyrexian Dreadnought", 15),
            ],
            &[("Mountain", 60)],
            &[
                ("Forest", 2),
                ("Grizzly Bears", 1),
                ("Phyrexian Dreadnought", 1),
            ],
        );
        play(&mut game, "Forest");
        my_next_main(&mut game);
        play(&mut game, "Forest");
        cast(&mut game, "Grizzly Bears", None);
        resolve(&mut game);
        let bears = creature(&game, "Grizzly Bears").unwrap();
        my_next_main(&mut game);
        cast(&mut game, "Phyrexian Dreadnought", None);
        resolve(&mut game);
        let dreadnought = creature(&game, "Phyrexian Dreadnought").unwrap();
        assert_eq!(game.stack().len(), 1, "its enters trigger");
        let Decision::SacrificeUnless { legal, power, .. } = resolve(&mut game) else {
            panic!("{:?}", game.decision())
        };
        assert_eq!(power, 12);
        assert!(legal.contains(&bears) && legal.contains(&dreadnought));
        assert!(game.clone().step(Answer::Sacrifice(vec![bears])).is_err());
        let paid = if pays { vec![dreadnought] } else { vec![] };
        step(&mut game, Answer::Sacrifice(paid));
        assert_eq!(creature(&game, "Phyrexian Dreadnought"), None);
        assert_eq!(creature(&game, "Grizzly Bears"), Some(bears));
    }
}

/// Solitary Confinement: its upkeep trigger asks for a discard (CR
/// 118.12a); while it is on the battlefield its controller skips their
/// draw step (CR 614.10), can't be targeted (CR 702.18a) and is dealt no
/// damage (CR 615.1a); sacrificed in the upkeep, the draw step is theirs
/// again (CR 614.10).
#[test]
fn solitary_confinement_skips_the_draw_gives_shroud_and_prevents_damage() {
    for discards in [true, false] {
        let mut game = deal_both(
            &[("Plains", 40), ("Solitary Confinement", 20)],
            &[("Mountain", 30), ("Jackal Pup", 15), ("Lightning Bolt", 15)],
            &[("Plains", 3), ("Solitary Confinement", 1)],
            &[("Mountain", 1), ("Jackal Pup", 1), ("Lightning Bolt", 1)],
        );
        play(&mut game, "Plains");
        pass_until(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Mountain");
        cast_as(&mut game, THEM, "Jackal Pup", None);
        resolve(&mut game);
        let pup = creature(&game, "Jackal Pup").unwrap();
        my_next_main(&mut game);
        play(&mut game, "Plains");
        pass_until(&mut game, THEM, Step::PrecombatMain);
        to_attackers(&mut game);
        step(&mut game, Answer::Attack(vec![(pup, ME)]));
        through_combat(&mut game);
        assert_eq!(game.player(ME).life(), 18, "before it");
        my_next_main(&mut game);
        play(&mut game, "Plains");
        cast(&mut game, "Solitary Confinement", None);
        resolve(&mut game);
        assert!(creature(&game, "Solitary Confinement").is_some());
        pass_until(&mut game, THEM, Step::PrecombatMain);
        let bolt = in_hand(&game, THEM, "Lightning Bolt").unwrap();
        let Decision::Targets { slots, .. } =
            step(&mut game.clone(), Answer::Act(Action::Cast(bolt)))
        else {
            panic!("{:?}", game.decision())
        };
        assert!(!slots[0].contains(&Target::Player(ME)), "I have shroud");
        assert!(slots[0].contains(&Target::Player(THEM)));
        to_attackers(&mut game);
        step(&mut game, Answer::Attack(vec![(pup, ME)]));
        through_combat(&mut game);
        assert_eq!(game.player(ME).life(), 18, "its damage prevented");
        pass_until(&mut game, ME, Step::Upkeep);
        let Decision::DiscardUnless { legal, .. } = resolve(&mut game) else {
            panic!("{:?}", game.decision())
        };
        let hand = game.player(ME).hand().len();
        let answer = if discards { Some(legal[0]) } else { None };
        step(&mut game, Answer::Pick(answer));
        pass_until(&mut game, ME, Step::PrecombatMain);
        if discards {
            assert!(creature(&game, "Solitary Confinement").is_some());
            assert_eq!(game.player(ME).hand().len(), hand - 1, "no draw");
        } else {
            assert_eq!(creature(&game, "Solitary Confinement"), None);
            assert_eq!(game.player(ME).hand().len(), hand + 1, "drew");
        }
    }
}

/// CR 702.29a, 603.3: Gempalm Incinerator's "When you cycle this card,
/// you may have it deal X damage to target creature, where X is the
/// number of Goblins on the battlefield" triggers from the graveyard it is
/// discarded to, above the cycling, and counts the Goblins as it resolves.
#[test]
fn gempalm_incinerator_counts_goblins_when_cycled() {
    let mut game = deal_both(
        &[
            ("Mountain", 30),
            ("Gempalm Incinerator", 15),
            ("Raging Goblin", 15),
        ],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[
            ("Mountain", 3),
            ("Gempalm Incinerator", 1),
            ("Raging Goblin", 2),
        ],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let gempalm = in_hand(&game, ME, "Gempalm Incinerator").unwrap();
    step(&mut game, Answer::Act(Action::Cycle(gempalm)));
    while let Decision::Pay { actions, .. } = game.decision() {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        step(&mut game, Answer::Pay(pay));
    }
    let Decision::Targets { slots, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(slots[0].contains(&Target::Object(bears)));
    step(&mut game, Answer::Targets(vec![Target::Object(bears)]));
    assert_eq!(game.stack().len(), 2);
    let Decision::May { .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    step(&mut game, Answer::May(true));
    // Two Goblins, 2 damage: the Bears die.
    assert_eq!(creature(&game, "Grizzly Bears"), None);
}

/// CR 614.1c, 122.6, 118.3: Gemstone Mine enters with three mining
/// counters, each activation takes one off for a mana of any colour, and
/// the one that leaves none sacrifices it.
#[test]
fn gemstone_mine_runs_out_of_mining_counters() {
    let mut game = deal(
        &[("Gemstone Mine", 60)],
        &[("Forest", 60)],
        &[("Gemstone Mine", 1)],
    );
    play(&mut game, "Gemstone Mine");
    let mine = creature(&game, "Gemstone Mine").unwrap();
    let mining = chancellor_of_the_spires::model::Counter::Named("mining".into());
    assert_eq!(game.counters_on(mine, &mining), 3);
    for left in [2, 1, 0] {
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!("{:?}", game.decision())
        };
        let tap = actions
            .iter()
            .find(|a| matches!(a, Action::Mana(m) if m.source == mine))
            .copied()
            .expect("offered while it has a counter and is untapped");
        step(&mut game, Answer::Act(tap));
        if left > 0 {
            assert_eq!(game.counters_on(mine, &mining), left);
            my_next_main(&mut game);
        }
    }
    assert_eq!(creature(&game, "Gemstone Mine"), None);
}

/// CR 305.2a: with Exploration I may play a second land this turn, not a
/// third.
#[test]
fn exploration_lets_me_play_an_additional_land() {
    let mut game = deal(
        &[("Forest", 40), ("Exploration", 20)],
        &[("Forest", 60)],
        &[("Forest", 3), ("Exploration", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Exploration", None);
    resolve(&mut game);
    play(&mut game, "Forest");
    assert!(in_hand(&game, ME, "Forest").is_some());
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(!actions.iter().any(|a| matches!(a, Action::PlayLand(_))));
}

/// CR 602.5, 602.5b: Hired Claw's "{1}{R}: Put a +1/+1 counter on this
/// creature. Activate only if an opponent lost life this turn and only once
/// each turn" isn't offered until they have lost life, then once.
#[test]
fn hired_claw_grows_once_after_an_opponent_lost_life() {
    let mut game = deal(
        &[("Mountain", 40), ("Hired Claw", 10), ("Lightning Bolt", 10)],
        &[("Forest", 60)],
        &[("Mountain", 3), ("Hired Claw", 1), ("Lightning Bolt", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Hired Claw", None);
    resolve(&mut game);
    let claw = creature(&game, "Hired Claw").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let grow = |game: &Game| match game.decision() {
        Decision::Priority { actions, .. } => actions
            .into_iter()
            .find(|a| matches!(a, Action::Activate(x) if x.source == claw)),
        _ => None,
    };
    assert_eq!(grow(&game), None, "no one has lost life");
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    resolve(&mut game);
    let a = grow(&game).expect("they lost life");
    step(&mut game, Answer::Act(a));
    while let Decision::Pay { actions, .. } = game.decision() {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap();
        step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert_eq!(pt_of(&game, claw), pt(2, 3));
    assert_eq!(grow(&game), None, "once each turn");
}

/// CR 601.2f: Temur Battlecrier's "During your turn, spells you cast cost
/// {1} less to cast for each creature you control with power 4 or
/// greater" counts itself, a 4/3, so Grizzly Bears costs {G}.
#[test]
fn temur_battlecrier_discounts_my_spells_for_each_big_creature() {
    let mut game = deal(
        &[
            ("Forest", 15),
            ("Island", 15),
            ("Mountain", 15),
            ("Temur Battlecrier", 8),
            ("Grizzly Bears", 7),
        ],
        &[("Forest", 60)],
        &[
            ("Forest", 2),
            ("Island", 1),
            ("Mountain", 1),
            ("Temur Battlecrier", 1),
            ("Grizzly Bears", 1),
        ],
    );
    assert_eq!(owed_for(&mut game, ME, "Grizzly Bears"), "{1}{G}");
    for land in ["Forest", "Island"] {
        play(&mut game, land);
        my_next_main(&mut game);
    }
    play(&mut game, "Mountain");
    cast(&mut game, "Temur Battlecrier", None);
    resolve(&mut game);
    assert_eq!(owed_for(&mut game, ME, "Grizzly Bears"), "{G}");
}

/// CR 701.9a, 608.2c: Gerrard's Verdict's "Target player discards two
/// cards. You gain 3 life for each land card discarded this way" counts
/// the lands among the two they chose.
#[test]
fn gerrards_verdict_gains_life_for_each_land_discarded() {
    let mut game = deal_both(
        &[("Plains", 20), ("Swamp", 20), ("Gerrard's Verdict", 20)],
        &[("Forest", 30), ("Grizzly Bears", 30)],
        &[("Plains", 1), ("Swamp", 1), ("Gerrard's Verdict", 1)],
        &[("Forest", 1), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(
        &mut game,
        "Gerrard's Verdict",
        Some(vec![Target::Player(THEM)]),
    );
    let Decision::Discard { player, hand, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(player, THEM);
    let forest = *hand
        .iter()
        .find(|c| game.card(**c).unwrap().name == "Forest")
        .unwrap();
    let bears = *hand
        .iter()
        .find(|c| game.card(**c).unwrap().name == "Grizzly Bears")
        .unwrap();
    step(&mut game, Answer::Discard(vec![forest, bears]));
    assert_eq!(game.player(ME).life(), 23);
}

/// CR 601.2b, 702.33a, 702.33d: Burst Lightning's "Kicker {4}" is
/// announced as it is cast, adds {4} to its cost, and "If this spell was
/// kicked, it deals 4 damage instead" has the spell deal 4 to the target
/// it chose, rather than 2.
#[test]
fn burst_lightning_deals_four_if_kicked_and_two_if_not() {
    let mut game = deal(
        &[("Mountain", 40), ("Burst Lightning", 20)],
        &[("Forest", 60)],
        &[("Mountain", 5), ("Burst Lightning", 2)],
    );
    play(&mut game, "Mountain");
    for _ in 0..4 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    let bolt = in_hand(&game, ME, "Burst Lightning").unwrap();
    let kick = |game: &mut Game, kicked: bool| {
        let d = step(game, Answer::Act(Action::Cast(bolt)));
        let Decision::Kicker { cost, .. } = d else {
            panic!("{d:?}")
        };
        assert_eq!(cost.to_string(), "{4}");
        let d = step(game, Answer::Kick(kicked));
        assert!(matches!(d, Decision::Targets { .. }), "{d:?}");
        let Decision::Pay { owed, .. } = step(game, Answer::Targets(vec![Target::Player(THEM)]))
        else {
            panic!()
        };
        owed.to_string()
    };
    let mut unkicked = game.clone();
    assert_eq!(kick(&mut unkicked, false), "{R}");
    let mut kicked = game.clone();
    assert_eq!(kick(&mut kicked, true), "{4}{R}");
    for (game, life) in [(&mut unkicked, 18), (&mut kicked, 16)] {
        while let Decision::Pay { actions, .. } = game.decision() {
            let pay = actions
                .iter()
                .find(|a| matches!(a, Payment::Spend(_)))
                .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
                .copied()
                .unwrap();
            step(game, Answer::Pay(pay));
        }
        resolve(game);
        assert_eq!(game.player(THEM).life(), life);
    }
}

/// CR 303.4a, 608.3c, 605.1b, 605.4a: Wild Growth targets a land as it is
/// cast and enters attached to it, and the land then taps for an
/// additional {G}, added at once; CR 704.5m: when the land is destroyed,
/// the Aura is attached to nothing and goes to the graveyard.
#[test]
fn wild_growth_enchants_a_land_that_then_taps_for_two() {
    let mut game = deal_both(
        &[("Forest", 40), ("Wild Growth", 20)],
        &[("Mountain", 40), ("Stone Rain", 20)],
        &[("Forest", 2), ("Wild Growth", 1)],
        &[("Mountain", 3), ("Stone Rain", 1)],
    );
    play(&mut game, "Forest");
    let forest = creature(&game, "Forest").expect("a Forest");
    let growth = in_hand(&game, ME, "Wild Growth").unwrap();
    let Decision::Targets { slots, .. } = step(&mut game, Answer::Act(Action::Cast(growth))) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(slots, vec![vec![Target::Object(forest)]]);
    let mut d = step(&mut game, Answer::Targets(vec![Target::Object(forest)]));
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
    let aura = creature(&game, "Wild Growth").expect("Wild Growth entered");
    assert_eq!(game.attached_to(aura), Some(forest));
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    // The enchanted Forest makes {G}{G}, the other {G}.
    let mut made: Vec<(chancellor_of_the_spires::model::ObjectId, u32)> = game
        .mana_options(ME)
        .iter()
        .map(|a| (a.source, a.adds.total()))
        .collect();
    made.sort();
    assert_eq!(made.len(), 2);
    assert!(made.contains(&(forest, 2)), "{made:?}");
    let to = game
        .mana_options(ME)
        .into_iter()
        .find(|a| a.source == forest)
        .unwrap();
    step(&mut game, Answer::Act(Action::Mana(to)));
    assert_eq!(game.player(ME).pool().total(), 2);
    // Stone Rain destroys it, and Wild Growth follows (CR 704.5m).
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    pass_until(&mut game, ME, Step::PrecombatMain);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(
        &mut game,
        THEM,
        "Stone Rain",
        Some(vec![Target::Object(forest)]),
    );
    resolve(&mut game);
    assert!(game.permanents().get(&forest).is_none());
    assert!(creature(&game, "Wild Growth").is_none());
    assert_eq!(named(&game, game.player(ME).graveyard(), "Wild Growth"), 1);
}

/// CR 504.1, 504.2, 121.2, 119.4: Sylvan Library's "At the beginning of
/// your draw step, you may draw two additional cards. If you do, choose
/// two cards in your hand drawn this turn. For each of those cards, pay 4
/// life or put the card on top of your library." Taken, it offers the
/// three cards drawn this turn and none from before; the first chosen is
/// paid for and stays, the second goes back on top.
#[test]
fn sylvan_library_keeps_a_card_for_4_life_and_puts_the_other_back() {
    let mut game = deal(
        &[("Forest", 30), ("Sylvan Library", 30)],
        &[("Island", 60)],
        &[("Forest", 2), ("Sylvan Library", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Sylvan Library", None);
    resolve(&mut game);
    let before: Vec<CardId> = game.player(ME).hand().iter().copied().collect();
    let library = to_may(&mut game);
    assert_eq!(game.card(library).unwrap().name, "Sylvan Library");
    assert_eq!((game.active(), game.step_now()), (ME, Step::Draw));
    let Decision::ChooseAmong { cards, n, .. } = step(&mut game, Answer::May(true)) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((cards.len(), n), (3, 2));
    assert!(cards.iter().all(|c| !before.contains(c)));
    let (kept, back) = (cards[0], cards[2]);
    let Decision::PayLifeOr { player, life, .. } =
        step(&mut game, Answer::Choose(vec![kept, back]))
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((player, life), (ME, 4));
    assert!(matches!(
        step(&mut game, Answer::PayLife(true)),
        Decision::PayLifeOr { .. }
    ));
    step(&mut game, Answer::PayLife(false));
    assert_eq!(game.player(ME).life(), 16);
    assert!(game.player(ME).hand().contains(&kept));
    assert!(game.player(ME).hand().contains(&cards[1]));
    assert!(!game.player(ME).hand().contains(&back));
    assert_eq!(game.player(ME).library().front(), Some(&back));
}

/// The activations of `source` offered to me now.
fn equips(game: &Game, source: chancellor_of_the_spires::model::ObjectId) -> Vec<Activation> {
    match game.decision() {
        Decision::Priority { actions, .. } => actions
            .iter()
            .filter_map(|a| match a {
                Action::Activate(x) if x.source == source => Some(*x),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// CR 702.6a, 602.5d, 301.5c, 704.5n: Bonesplitter's "Equip {1}" is
/// "{1}: Attach this permanent to target creature you control. Activate
/// only as a sorcery": not offered with a spell on the stack, it attaches
/// it to Grizzly Bears, which gets +2/+0, and when the Bears die it stays
/// on the battlefield unattached.
#[test]
fn bonesplitter_equips_at_sorcery_speed_and_stays_when_its_creature_dies() {
    let mut game = deal_both(
        &[("Forest", 30), ("Bonesplitter", 15), ("Grizzly Bears", 15)],
        &[("Mountain", 40), ("Lightning Bolt", 20)],
        &[("Forest", 3), ("Bonesplitter", 1), ("Grizzly Bears", 1)],
        &[("Lightning Bolt", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Bonesplitter", None);
    resolve(&mut game);
    let splitter = creature(&game, "Bonesplitter").unwrap();
    assert!(equips(&game, splitter).is_empty(), "no creature to equip");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    assert!(equips(&game, splitter).is_empty(), "the stack isn't empty");
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    my_next_main(&mut game);
    let [equip] = equips(&game, splitter)[..] else {
        panic!("equip is offered")
    };
    let mut d = step(&mut game, Answer::Act(Action::Activate(equip)));
    if matches!(d, Decision::Targets { .. }) {
        d = step(&mut game, Answer::Targets(vec![Target::Object(bears)]));
    }
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert_eq!(game.attached_to(splitter), Some(bears));
    assert_eq!(pt_of(&game, bears), pt(4, 2));
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Object(bears)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Grizzly Bears").is_none());
    assert_eq!(game.attached_to(splitter), None);
    assert!(creature(&game, "Bonesplitter").is_some());
}

/// CR 103.6b, 603.7, 702.10: Chancellor of the Forge revealed from the
/// opening hand stays there, and at the beginning of the first upkeep, the
/// starting player's of turn 1, its delayed trigger creates a 1/1 red
/// Phyrexian Goblin with haste.
#[test]
fn chancellor_of_the_forge_revealed_makes_a_hasty_goblin_at_the_first_upkeep() {
    let mut found = None;
    for seed in 0..200 {
        let game = Game::start(Setup {
            format: Format::Constructed,
            decks: vec![
                deck(&[("Chancellor of the Forge", 20), ("Mountain", 40)]),
                deck(&[("Forest", 60)]),
            ],
            mulligans: false,
            seed,
        })
        .unwrap();
        if named(&game, game.player(ME).hand(), "Chancellor of the Forge") > 0 {
            found = Some(game);
            break;
        }
    }
    let mut game = found.expect("a seed");
    let Decision::OpeningHand { player, cards } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(player, ME);
    step(&mut game, Answer::Begin(vec![cards[0]]));
    assert!(game.player(ME).hand().contains(&cards[0]));
    assert_eq!((game.turn(), game.step_now()), (1, Step::Upkeep));
    assert_eq!(game.stack().len(), 1, "the delayed trigger");
    resolve(&mut game);
    let goblins: Vec<_> = game
        .permanents()
        .iter()
        .filter(|(_, p)| {
            game.card(p.card)
                .is_some_and(|t| t.name.contains("Phyrexian Goblin"))
        })
        .map(|(id, _)| *id)
        .collect();
    let [goblin] = goblins[..] else {
        panic!("one token: {goblins:?}")
    };
    let chars = &game.characteristics()[&goblin];
    assert_eq!(chars.pt, Some(pt(1, 1)));
    assert!(chars.has(chancellor_of_the_spires::model::Keyword::Haste));
}

/// CR 118.9, 701.21a, 701.23a, 701.23d: Flare of Cultivation cast by
/// sacrificing a nontoken green creature rather than paying its mana
/// cost, then "Search your library for up to two basic land cards, reveal
/// those cards, put one onto the battlefield tapped and the other into
/// your hand": the second search can't find the card the first did, and
/// of the two, the one chosen enters tapped and the other goes to hand.
#[test]
fn flare_of_cultivation_sacrifices_an_elf_and_finds_two_lands() {
    let mut game = deal(
        &[
            ("Forest", 20),
            ("Island", 20),
            ("Flare of Cultivation", 10),
            ("Llanowar Elves", 10),
        ],
        &[("Swamp", 60)],
        &[
            ("Forest", 1),
            ("Flare of Cultivation", 1),
            ("Llanowar Elves", 1),
        ],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Llanowar Elves", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let flare = in_hand(&game, ME, "Flare of Cultivation").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let alt = actions
        .into_iter()
        .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == flare))
        .expect("the sacrifice is offered");
    step(&mut game, Answer::Act(alt));
    if matches!(game.decision(), Decision::Pay { .. }) {
        step(&mut game, Answer::Pay(Payment::Spend(ManaPool::default())));
    }
    let Decision::Sacrifice { legal, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(legal, vec![creature(&game, "Llanowar Elves").unwrap()]);
    step(&mut game, Answer::Sacrifice(legal));
    let Decision::Search { known, reveals, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert!(reveals);
    let by_name = |game: &Game, known: &[CardId], n: &str| {
        known
            .iter()
            .copied()
            .find(|c| game.card(*c).is_some_and(|t| t.name == n))
            .unwrap()
    };
    let island = by_name(&game, &known, "Island");
    let Decision::Search { known, .. } = step(&mut game, Answer::Find(Found::Card(island))) else {
        panic!("{:?}", game.decision())
    };
    assert!(!known.contains(&island), "found already");
    assert!(game
        .clone()
        .step(Answer::Find(Found::Card(island)))
        .is_err());
    let forest = by_name(&game, &known, "Forest");
    let Decision::ChooseAmong { cards, n, .. } = step(&mut game, Answer::Find(Found::Card(forest)))
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((cards, n), (vec![island, forest], 1));
    step(&mut game, Answer::Choose(vec![forest]));
    assert!(game.player(ME).hand().contains(&island));
    let entered = game
        .permanents()
        .iter()
        .find(|(_, p)| p.card == forest)
        .map(|(_, p)| p.tapped);
    assert_eq!(entered, Some(true));
    assert_eq!(
        named(&game, game.player(ME).graveyard(), "Llanowar Elves"),
        1
    );
}

/// CR 608.2h, 121.2: Accumulated Knowledge counts the cards named
/// Accumulated Knowledge in all graveyards as it resolves, itself still on
/// the stack and so not one of them: the first draws one, the second two.
#[test]
fn accumulated_knowledge_draws_one_more_for_each_in_a_graveyard() {
    let mut game = deal(
        &[("Island", 30), ("Accumulated Knowledge", 30)],
        &[("Forest", 60)],
        &[("Island", 2), ("Accumulated Knowledge", 2)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    let hand = game.player(ME).hand().len();
    cast(&mut game, "Accumulated Knowledge", None);
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand);
    my_next_main(&mut game);
    let hand = game.player(ME).hand().len();
    cast(&mut game, "Accumulated Knowledge", None);
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand + 1);
    assert_eq!(
        named(&game, game.player(ME).graveyard(), "Accumulated Knowledge"),
        2
    );
}

/// CR 207.2c, 611.3a: threshold, an ability word, so Nimble Mongoose gets +2/+2 while
/// there are seven or more cards in its controller's graveyard, read as
/// the layers apply, here the cards discarded to hand size.
#[test]
fn threshold_grows_nimble_mongoose_from_the_seventh_card_in_the_graveyard() {
    let mut game = deal(
        &[("Forest", 30), ("Nimble Mongoose", 30)],
        &[("Forest", 60)],
        &[("Forest", 1), ("Nimble Mongoose", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Nimble Mongoose", None);
    resolve(&mut game);
    let mongoose = creature(&game, "Nimble Mongoose").unwrap();
    while game.player(ME).graveyard().len() < 6 {
        assert_eq!(pt_of(&game, mongoose), pt(1, 1));
        my_next_main(&mut game);
    }
    assert_eq!(pt_of(&game, mongoose), pt(1, 1));
    my_next_main(&mut game);
    assert_eq!(game.player(ME).graveyard().len(), 7);
    assert_eq!(pt_of(&game, mongoose), pt(3, 3));
}

/// CR 118.9, 601.2h: Gush cast for its alternative cost returns two
/// Islands its caster controls to their owner's hand, a tapped one too,
/// and pays no mana; it isn't offered with fewer than two.
#[test]
fn gush_returns_two_islands_rather_than_pay_its_mana_cost() {
    let mut game = deal(
        &[("Island", 40), ("Gush", 20)],
        &[("Swamp", 60)],
        &[("Island", 2), ("Gush", 1)],
    );
    let gush = in_hand(&game, ME, "Gush").unwrap();
    let offered = |game: &Game| {
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!("{:?}", game.decision())
        };
        actions
            .into_iter()
            .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == gush))
    };
    play(&mut game, "Island");
    assert!(offered(&game).is_none(), "one Island isn't two");
    my_next_main(&mut game);
    play(&mut game, "Island");
    let hand = game.player(ME).hand().len();
    let alt = offered(&game).expect("offered");
    step(&mut game, Answer::Act(alt));
    if matches!(game.decision(), Decision::Pay { .. }) {
        step(&mut game, Answer::Pay(Payment::Spend(ManaPool::default())));
    }
    let Decision::ReturnToHand { count, legal, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((count, legal.len()), (2, 2));
    step(&mut game, Answer::ReturnToHand(legal));
    assert_eq!(game.returned(), 2);
    resolve(&mut game);
    // Gush gone, two Islands back and two cards drawn.
    assert_eq!(game.player(ME).hand().len(), hand - 1 + 2 + 2);
    assert!(named(&game, game.player(ME).hand(), "Island") >= 2);
    assert!(game.permanents().is_empty());
}

/// CR 614.12a, 607.2d, 205.3m: Engineered Plague's creature type is chosen
/// before it enters, only a creature type, and "the chosen type" is that
/// one: Grizzly Bears, a Bear, gets -1/-1.
#[test]
fn engineered_plague_shrinks_the_creature_type_chosen_as_it_enters() {
    let mut game = deal_both(
        &[("Swamp", 40), ("Engineered Plague", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Swamp", 3), ("Engineered Plague", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Engineered Plague", None);
    let Decision::ChooseCreatureType { player, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(player, ME);
    assert!(
        creature(&game, "Engineered Plague").is_none(),
        "not there yet"
    );
    assert!(game.step(Answer::CreatureType("Island".into())).is_err());
    step(&mut game, Answer::CreatureType("Bear".into()));
    assert_eq!(pt_of(&game, bears), pt(1, 1));
}

/// Plague Engineer's "Creatures of the chosen type your opponents control
/// get -1/-1" spares its controller's own.
#[test]
fn plague_engineer_shrinks_only_the_opponents_creatures_of_the_chosen_type() {
    let mut game = deal_both(
        &[
            ("Swamp", 20),
            ("Forest", 10),
            ("Plague Engineer", 15),
            ("Grizzly Bears", 15),
        ],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[
            ("Swamp", 2),
            ("Forest", 1),
            ("Plague Engineer", 1),
            ("Grizzly Bears", 1),
        ],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    let mine = creature(&game, "Grizzly Bears").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let theirs = game
        .permanents()
        .iter()
        .find(|(id, p)| {
            **id != mine && game.card(p.card).is_some_and(|t| t.name == "Grizzly Bears")
        })
        .map(|(id, _)| *id)
        .unwrap();
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Plague Engineer", None);
    resolve(&mut game);
    step(&mut game, Answer::CreatureType("Bear".into()));
    assert_eq!(pt_of(&game, theirs), pt(1, 1));
    assert_eq!(pt_of(&game, mine), pt(2, 2));
}

/// Cast `name` from my hand face down by its morph ability, paying with
/// the first payment as `cast` does: a face-down spell on the stack.
fn cast_face_down(game: &mut Game, name: &str) -> CardId {
    let c = in_hand(game, ME, name).expect("in hand");
    let mut d = step(game, Answer::Act(Action::CastFaceDown(c)));
    while let Decision::Pay { actions, owed, .. } = d {
        assert_eq!(
            owed.to_string(),
            "{3}",
            "CR 702.37c: {{3}} rather than its mana cost"
        );
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(game, Answer::Pay(pay));
    }
    c
}

/// Turn the face-down permanent face up for its morph cost, paying as
/// `cast` does.
fn turn_face_up(game: &mut Game, id: chancellor_of_the_spires::model::ObjectId) -> Decision {
    let mut d = step(game, Answer::Act(Action::TurnFaceUp(id)));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(game, Answer::Pay(pay));
    }
    d
}

fn face_down_of(game: &Game, card: CardId) -> chancellor_of_the_spires::model::ObjectId {
    game.permanents()
        .iter()
        .find(|(_, p)| p.card == card)
        .map(|(id, _)| *id)
        .expect("on the battlefield")
}

/// CR 702.37c, 708.2a, 702.37e, 708.8: Exalted Angel cast face down for
/// {3} is a 2/2 creature with no abilities or mana value, which attacks
/// without its lifelink-like trigger; turned face up for {2}{W}{W} without
/// the stack, it is the same creature, a 4/5 flier that can still attack,
/// and "Whenever this creature deals damage, you gain that much life"
/// gains me the 4 it deals.
#[test]
fn exalted_angel_face_down_is_a_2_2_until_turned_face_up_for_its_morph_cost() {
    let mut game = deal(
        &[("Plains", 40), ("Exalted Angel", 20)],
        &[("Forest", 60)],
        &[("Plains", 4), ("Exalted Angel", 1)],
    );
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    let angel = cast_face_down(&mut game, "Exalted Angel");
    assert!(game.face_down_on_stack(angel));
    resolve(&mut game);
    let id = face_down_of(&game, angel);
    assert!(game.permanents()[&id].face_down);
    let chars = &game.characteristics()[&id];
    assert_eq!(chars.pt, Some(pt(2, 2)));
    assert!(chars.abilities.is_empty() && chars.colors.is_empty() && chars.subtypes.is_empty());
    assert_eq!(chars.mana_value, 0);
    my_next_main(&mut game);
    play(&mut game, "Plains");
    let (can, _) = to_attackers(&mut game);
    assert_eq!(can, vec![id]);
    step(&mut game, Answer::Attack(vec![(id, THEM)]));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
    assert_eq!(game.player(ME).life(), 20, "face down it has no trigger");
    // Postcombat, with four untapped Plains.
    let d = turn_face_up(&mut game, id);
    assert!(matches!(d, Decision::Priority { player: ME, .. }), "{d:?}");
    assert!(
        game.stack().is_empty(),
        "CR 702.37e: it doesn't use the stack"
    );
    assert!(!game.permanents()[&id].face_down);
    assert_eq!(pt_of(&game, id), pt(4, 5));
    my_next_main(&mut game);
    let (can, _) = to_attackers(&mut game);
    assert_eq!(
        can,
        vec![id],
        "the same object, under my control since before"
    );
    step(&mut game, Answer::Attack(vec![(id, THEM)]));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 14);
    assert_eq!(game.player(ME).life(), 24);
}

/// CR 702.37e, 603.2: Nantuko Vigilante's "When this creature is turned
/// face up, destroy target artifact or enchantment" triggers as it is
/// turned face up, and only an artifact or an enchantment is a legal
/// target: Sol Ring is, a Forest isn't.
#[test]
fn nantuko_vigilante_turned_face_up_destroys_an_artifact() {
    let mut game = deal_both(
        &[("Forest", 40), ("Nantuko Vigilante", 20)],
        &[("Forest", 40), ("Sol Ring", 20)],
        &[("Forest", 4), ("Nantuko Vigilante", 1)],
        &[("Forest", 1), ("Sol Ring", 1)],
    );
    play(&mut game, "Forest");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Sol Ring", None);
    resolve(&mut game);
    let ring = creature(&game, "Sol Ring").expect("Sol Ring");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    let vigilante = cast_face_down(&mut game, "Nantuko Vigilante");
    resolve(&mut game);
    let id = face_down_of(&game, vigilante);
    my_next_main(&mut game);
    play(&mut game, "Forest");
    let Decision::Targets { slots, .. } = turn_face_up(&mut game, id) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(slots, vec![vec![Target::Object(ring)]]);
    step(&mut game, Answer::Targets(vec![Target::Object(ring)]));
    resolve(&mut game);
    assert!(creature(&game, "Sol Ring").is_none());
    assert_eq!(pt_of(&game, id), pt(3, 2));
}

/// Pay for the spell being cast with the first payment offered that
/// spends or taps, until it is paid.
fn pay(game: &mut Game, mut d: Decision) {
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(game, Answer::Pay(pay));
    }
}

fn offered(game: &Game) -> Vec<Action> {
    match game.decision() {
        Decision::Priority { actions, .. } => actions,
        d => panic!("{d:?}"),
    }
}

/// CR 712.12, 712.8f, 712.11b–c, 712.8a: a modal double-faced card is
/// played as a land by its land face, which is up on the battlefield
/// and alone has characteristics there; its other face, a sorcery, is
/// cast by itself, and back in a graveyard the card is its front face.
#[test]
fn a_modal_double_faced_card_is_played_or_cast_by_either_face() {
    const PELAKKA: &str = "Pelakka Predation // Pelakka Caverns";
    let mut game = deal_both(
        &[("Swamp", 30), (PELAKKA, 30)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Swamp", 2), (PELAKKA, 2)],
        &[("Grizzly Bears", 1)],
    );
    let first = in_hand(&game, ME, PELAKKA).unwrap();
    let actions = offered(&game);
    // A sorcery face is no land, and a land face is never cast.
    assert!(actions.contains(&Action::PlayBack(first)));
    assert!(!actions.contains(&Action::PlayLand(first)));
    assert!(!actions.contains(&Action::CastBack(first)));
    step(&mut game, Answer::Act(Action::PlayBack(first)));
    let (id, perm) = game
        .permanents()
        .iter()
        .find(|(_, p)| p.card == first)
        .map(|(id, p)| (*id, p.clone()))
        .unwrap();
    assert!(perm.tapped, "Pelakka Caverns enters tapped");
    assert_eq!(game.side_up(first), Side::Back);
    let now = &game.characteristics()[&id];
    assert!(now.is(CardType::Land) && !now.is(CardType::Sorcery));
    assert_eq!(now.mana_value, 0);
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    let second = in_hand(&game, ME, PELAKKA).unwrap();
    assert!(offered(&game).contains(&Action::Cast(second)));
    let d = step(&mut game, Answer::Act(Action::Cast(second)));
    assert!(matches!(d, Decision::Targets { .. }), "{d:?}");
    let d = step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    pay(&mut game, d);
    let Decision::ChooseFromHand { legal, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    // Mana value 3 or greater: a Forest and Grizzly Bears are neither.
    assert!(legal.is_empty());
    step(&mut game, Answer::Pick(None));
    assert_eq!(named(&game, game.player(ME).graveyard(), PELAKKA), 1);
    assert_eq!(game.side_up(second), Side::Front);
}

/// CR 715.3, 715.3b, 715.3d, 715.4: an adventurer card cast as its
/// Adventure has only the Adventure's characteristics and instructions,
/// is exiled as it resolves rather than put into a graveyard, and its
/// caster may cast the creature from exile afterwards, its normal
/// characteristics.
#[test]
fn an_adventure_resolves_into_exile_and_the_creature_is_cast_from_there() {
    const VIPER: &str = "Scalding Viper // Steam Clean";
    let mut game = deal_both(
        &[("Island", 20), ("Mountain", 20), (VIPER, 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Island", 2), ("Mountain", 2), (VIPER, 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Island");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Island");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let held = named(&game, game.player(THEM).hand(), "Grizzly Bears");
    let viper = in_hand(&game, ME, VIPER).unwrap();
    assert!(offered(&game).contains(&Action::CastBack(viper)));
    let d = step(&mut game, Answer::Act(Action::CastBack(viper)));
    // On the stack it is Steam Clean, a sorcery of mana value 2.
    assert_eq!(game.side_up(viper), Side::Back);
    let d = match d {
        Decision::Targets { .. } => step(&mut game, Answer::Targets(vec![Target::Object(bears)])),
        d => panic!("{d:?}"),
    };
    pay(&mut game, d);
    resolve(&mut game);
    assert!(creature(&game, "Grizzly Bears").is_none());
    assert_eq!(
        named(&game, game.player(THEM).hand(), "Grizzly Bears"),
        held + 1
    );
    assert_eq!(named(&game, game.exile(), VIPER), 1);
    assert_eq!(game.adventuring(ME).collect::<Vec<_>>(), vec![viper]);
    assert_eq!(game.side_up(viper), Side::Front);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let actions = offered(&game);
    assert!(actions.contains(&Action::Cast(viper)));
    // Not as an Adventure again from exile (CR 715.3d).
    assert!(!actions.contains(&Action::CastBack(viper)));
    let d = step(&mut game, Answer::Act(Action::Cast(viper)));
    pay(&mut game, d);
    resolve(&mut game);
    let id = creature(&game, VIPER).expect("Scalding Viper on the battlefield");
    assert_eq!(game.characteristics()[&id].pt, Some(pt(2, 1)));
    assert_eq!(game.exile().len(), 0);
    assert_eq!(game.adventuring(ME).count(), 0);
}

/// CR 701.27a, 712.8d–e, 712.18, 613.7g, 701.20a: Delver of Secrets
/// looks at the top card in its controller's upkeep and may reveal it;
/// an instant revealed transforms it into Insectile Aberration, the same
/// permanent with only its back face's characteristics but its front's
/// mana value. A land revealed, or nothing revealed, leaves it as it is.
#[test]
fn delver_of_secrets_transforms_when_it_reveals_an_instant() {
    const DELVER: &str = "Delver of Secrets // Insectile Aberration";
    let mut transformed = 0;
    let mut stayed = 0;
    for seed in 0..16 {
        let Ok(mut game) = Game::start(Setup {
            format: Format::Constructed,
            decks: vec![
                deck(&[("Island", 30), (DELVER, 10), ("Lightning Bolt", 20)]),
                deck(&[("Forest", 60)]),
            ],
            mulligans: false,
            seed,
        }) else {
            continue;
        };
        let hand = game.player(ME).hand().clone();
        if named(&game, &hand, DELVER) == 0 || named(&game, &hand, "Island") == 0 {
            continue;
        }
        pass_until(&mut game, ME, Step::PrecombatMain);
        play(&mut game, "Island");
        cast(&mut game, DELVER, None);
        resolve(&mut game);
        let id = creature(&game, DELVER).unwrap();
        let card = game.permanents()[&id].card;
        pass_until(&mut game, THEM, Step::PrecombatMain);
        let mut top = None;
        loop {
            match game.decision() {
                Decision::May { player, .. } => {
                    assert_eq!(player, ME);
                    top = game.player(ME).library().front().copied();
                    step(&mut game, Answer::May(true));
                }
                Decision::Priority { .. } if game.step_now() == Step::Draw => break,
                Decision::Discard { count, hand, .. } => {
                    step(&mut game, Answer::Discard(hand[..count].to_vec()));
                }
                Decision::Attackers { .. } => {
                    step(&mut game, Answer::Attack(vec![]));
                }
                Decision::Blockers { .. } => {
                    step(&mut game, Answer::Block(vec![]));
                }
                _ => {
                    step(&mut game, Answer::Act(Action::Pass));
                }
            }
        }
        let top = top.expect("its upkeep trigger asked");
        let instant = game.card(top).unwrap().name == "Lightning Bolt";
        let now = &game.characteristics()[&id];
        if instant {
            transformed += 1;
            assert_eq!(game.side_up(card), Side::Back);
            assert_eq!(game.face_name(card), Some("Insectile Aberration"));
            assert_eq!(now.pt, Some(pt(3, 2)));
            assert_eq!(now.mana_value, 1);
            assert!(game.permanents().contains_key(&id), "the same object");
        } else {
            stayed += 1;
            assert_eq!(game.side_up(card), Side::Front);
            assert_eq!(now.pt, Some(pt(1, 1)));
        }
    }
    assert!(transformed > 0 && stayed > 0, "{transformed} {stayed}");
}

/// The mana abilities offered to me now, by what each adds.
fn mana_offered(game: &Game) -> Vec<ManaPool> {
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("priority")
    };
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Mana(m) => Some(m.adds),
            _ => None,
        })
        .collect()
}

fn permanent_of(game: &Game, card: CardId) -> chancellor_of_the_spires::model::ObjectId {
    game.permanents()
        .iter()
        .find(|(_, p)| p.card == card)
        .map(|(id, _)| *id)
        .expect("on the battlefield")
}

/// CR 707.5, 614.1c, 614.12a: Vesuva may enter tapped as a copy of any
/// land on the battlefield, chosen before it enters; as a copy of
/// Cloudpost it is a Locus named Cloudpost, so each Cloudpost counts two
/// Loci. Declined, it enters untapped as itself, with nothing to tap for.
#[test]
fn vesuva_enters_tapped_as_a_copy_of_cloudpost_or_as_itself() {
    for copies in [true, false] {
        let mut game = deal(
            &[("Cloudpost", 20), ("Vesuva", 20), ("Forest", 20)],
            &[("Forest", 60)],
            &[("Cloudpost", 1), ("Vesuva", 1)],
        );
        play(&mut game, "Cloudpost");
        let cloudpost = creature(&game, "Cloudpost").unwrap();
        my_next_main(&mut game);
        let vesuva = in_hand(&game, ME, "Vesuva").unwrap();
        let d = step(&mut game, Answer::Act(Action::PlayLand(vesuva)));
        let Decision::EnterAsCopy {
            player, of, may, ..
        } = d
        else {
            panic!("{d:?}")
        };
        assert_eq!((player, may), (ME, true));
        assert_eq!(of, vec![cloudpost], "only a land, and not itself");
        step(&mut game, Answer::Copy(copies.then_some(cloudpost)));
        let id = permanent_of(&game, vesuva);
        assert_eq!(game.permanents()[&id].tapped, copies);
        let now = &game.characteristics()[&id];
        let locus = now
            .subtypes
            .iter()
            .any(|s| format!("{s:?}").contains("Locus"));
        assert_eq!(locus, copies);
        let name = game.card(vesuva).unwrap().name.clone();
        assert_eq!(name, if copies { "Cloudpost" } else { "Vesuva" });
        my_next_main(&mut game);
        let two = ManaPool::of([Mana::Colorless, Mana::Colorless]);
        let one = ManaPool::of([Mana::Colorless]);
        if copies {
            assert_eq!(mana_offered(&game), vec![two, two], "two Loci");
        } else {
            assert_eq!(mana_offered(&game), vec![one], "Vesuva adds nothing");
        }
    }
}

/// CR 707.5: Vesuva entering as a copy of Glimmerpost has its "When this
/// land enters" trigger, which counts both Loci.
#[test]
fn vesuva_as_glimmerpost_triggers_as_it_enters() {
    let mut game = deal(
        &[("Glimmerpost", 20), ("Vesuva", 20), ("Forest", 20)],
        &[("Forest", 60)],
        &[("Glimmerpost", 1), ("Vesuva", 1)],
    );
    play(&mut game, "Glimmerpost");
    resolve(&mut game);
    my_next_main(&mut game);
    let life = game.player(ME).life();
    let glimmerpost = creature(&game, "Glimmerpost").unwrap();
    let vesuva = in_hand(&game, ME, "Vesuva").unwrap();
    step(&mut game, Answer::Act(Action::PlayLand(vesuva)));
    step(&mut game, Answer::Copy(Some(glimmerpost)));
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), life + 2, "two Loci");
    assert_eq!(game.card(vesuva).unwrap().name, "Glimmerpost");
}

/// CR 707.4, 707.9a, 613.2a: Thespian's Stage becomes a copy of target
/// land, except it has that ability: a Cloudpost, the same permanent, that
/// can still become a copy of something else.
#[test]
fn thespians_stage_becomes_a_copy_of_cloudpost_and_keeps_its_ability() {
    let mut game = deal(
        &[("Cloudpost", 15), ("Thespian's Stage", 15), ("Forest", 30)],
        &[("Forest", 60)],
        &[("Cloudpost", 1), ("Thespian's Stage", 1), ("Forest", 2)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Cloudpost");
    my_next_main(&mut game);
    play(&mut game, "Thespian's Stage");
    let stage = creature(&game, "Thespian's Stage").unwrap();
    let card = game.permanents()[&stage].card;
    let cloudpost = creature(&game, "Cloudpost").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!()
    };
    let becomes = actions
        .iter()
        .find(|a| matches!(a, Action::Activate(x) if x.source == stage))
        .copied()
        .expect("its copy ability");
    let d = step(&mut game, Answer::Act(becomes));
    let Decision::Targets { slots, .. } = d else {
        panic!("{d:?}")
    };
    assert!(slots[0].contains(&Target::Object(cloudpost)));
    let mut d = step(&mut game, Answer::Targets(vec![Target::Object(cloudpost)]));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| {
                actions.iter().find(
                    |a| matches!(a, Payment::Mana(m) if m.source != stage && m.source != cloudpost),
                )
            })
            .copied()
            .expect("a way to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert!(game.permanents().contains_key(&stage), "the same permanent");
    let text = game.card(card).unwrap();
    assert_eq!(text.name, "Cloudpost");
    let Oracle::OneFace(rules) = &text.oracle else {
        panic!()
    };
    assert!(rules.contains("becomes a copy of target land"), "{rules}");
    my_next_main(&mut game);
    let two = ManaPool::of([Mana::Colorless, Mana::Colorless]);
    let offered = mana_offered(&game);
    assert_eq!(
        offered.iter().filter(|m| **m == two).count(),
        2,
        "{offered:?}"
    );
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!()
    };
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Activate(x) if x.source == stage)),
        "it still has the ability that made it a copy"
    );
}
