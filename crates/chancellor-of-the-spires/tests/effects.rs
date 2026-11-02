//! Spells' and triggered abilities' effects, run through the game: targets
//! chosen on casting and checked again on resolution, a hand revealed and
//! a card chosen from it, damage, scry, draw, and an enters trigger.

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
    for seed in 0..500 {
        let game = Game::start(Setup {
            format: Format::Constructed,
            decks: vec![deck(me), deck(them)],
            mulligans: false,
            seed,
        })
        .unwrap();
        if wants
            .iter()
            .all(|(w, k)| named(&game, game.player(ME).hand(), w) >= *k)
        {
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
    let c = in_hand(game, ME, name).expect("in hand");
    step(game, Answer::Act(Action::PlayLand(c)));
}

/// Cast this card from my hand, choose these targets if it has any, and
/// pay with the first payment that taps what pays it.
fn cast(game: &mut Game, name: &str, targets: Option<Vec<Target>>) {
    let c = in_hand(game, ME, name).expect("in hand");
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
