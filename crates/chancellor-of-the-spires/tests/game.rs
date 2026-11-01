//! Scripted games: a seed, the answers given, and the state they end in
//! (ADR-0043). The opponents in a goldfish only ever pass.

use std::path::Path;

use chancellor_of_the_spires::deck::{self, CardText, DeckCards};
use chancellor_of_the_spires::game::*;
use chancellor_of_the_spires::model::PlayerId;
use chip_scryfall::index::IndexFile;

const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

fn loam() -> DeckCards {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let index = IndexFile::open(&root.join("decks/index.jsonl")).expect("decks/index.jsonl");
    let text = std::fs::read_to_string(root.join("decks/loam.deck.toml")).unwrap();
    deck::read(&text, &index).unwrap_or_else(|e| panic!("{e}"))
}

fn forests(n: usize) -> DeckCards {
    DeckCards {
        commander: Vec::new(),
        library: (0..n).map(|_| CardText::basic("Forest")).collect(),
    }
}

fn priority(d: &Decision) -> (PlayerId, &[Action]) {
    match d {
        Decision::Priority { player, actions } => (*player, actions),
        other => panic!("expected priority, got {other:?}"),
    }
}

/// Every player passes until `who` has priority in `step` of their own
/// turn, and discards to hand size with the simplest legal answer, the
/// first cards in hand.
fn pass_until(game: &mut Game, who: PlayerId, step: Step) -> Decision {
    let mut d = game.decision();
    for _ in 0..1000 {
        d = match d {
            Decision::Priority { player, .. }
                if player == who && game.active() == who && game.step_now() == step =>
            {
                return d;
            }
            Decision::Discard { count, hand, .. } => {
                game.step(Answer::Discard(hand[..count].to_vec())).unwrap()
            }
            _ => game.step(Answer::Act(Action::Pass)).unwrap(),
        };
    }
    panic!("never reached {step:?} with {who:?} holding priority: {d:?}")
}

fn act(game: &mut Game, a: Action) -> Decision {
    game.step(Answer::Act(a)).unwrap_or_else(|e| panic!("{e}"))
}

fn land_in_hand(game: &Game, p: PlayerId) -> CardId {
    *game.player(p).hand().iter().next().expect("a card in hand")
}

/// CR 903.6–903.7, 103.8a, 305.1, 305.6, 305.7, 502.3: Loam against a
/// goldfish for three of its turns, from seed 24. Each main phase plays a
/// land from hand, basics first, trying the next when the engine refuses
/// one by name, then taps every land with a mana ability. The first land is
/// Yavimaya, Cradle of Growth, which makes each land a Forest in addition
/// to its other land types (layer 4, CR 613.1d), so the lands after it tap
/// for {G} too.
#[test]
fn loam_goldfished_for_three_turns_plays_a_land_and_taps_for_mana() {
    let mut game = Game::new(vec![loam(), loam()], 24);
    let me = game.player(ME);
    assert_eq!(me.life(), 40);
    assert_eq!(me.hand().len(), 7);
    // The file holds 99 cards, the commander among them.
    assert_eq!(me.library().len(), 98 - 7);
    let commanders: Vec<&str> = game
        .command()
        .iter()
        .filter(|c| game.owner(**c) == ME)
        .map(|c| game.card(*c).name.as_str())
        .collect();
    assert_eq!(commanders, ["Borborygmos and Fblthp"]);

    let mut played = Vec::new();
    let mut pools = Vec::new();
    let mut refused = Vec::new();
    for my_turn in 1..=3u32 {
        let d = pass_until(&mut game, ME, Step::PrecombatMain);
        assert_eq!(game.turn(), 2 * my_turn - 1);
        // Every land I control untapped (CR 502.3).
        assert!(game.permanents().values().all(|p| !p.tapped));
        let mut lands: Vec<CardId> = priority(&d)
            .1
            .iter()
            .filter_map(|a| match a {
                Action::PlayLand(c) => Some(*c),
                _ => None,
            })
            .collect();
        lands.sort_by_key(|c| !game.card(*c).type_line.starts_with("Basic"));
        let land = lands
            .into_iter()
            .find(|c| match game.step(Answer::Act(Action::PlayLand(*c))) {
                Ok(_) => true,
                Err(Rejected::Refused { card, refusal }) => {
                    refused.push(format!("{card}: {refusal}"));
                    false
                }
                Err(e) => panic!("{e}"),
            })
            .expect("a land to play");
        played.push(game.card(land).name.clone());
        assert_eq!(game.player(ME).lands_played(), 1);
        while let Some(tap) = priority(&game.decision())
            .1
            .iter()
            .find(|a| matches!(a, Action::TapForMana { .. }))
            .copied()
        {
            act(&mut game, tap);
        }
        pools.push(game.player(ME).pool().get(Mana::Green));
        act(&mut game, Action::Pass);
    }
    assert_eq!(
        played,
        [
            "Yavimaya, Cradle of Growth",
            "Cephalid Coliseum",
            "Scalding Tarn"
        ]
    );
    assert_eq!(pools, [1, 2, 3], "refused: {refused:?}");
    assert_eq!(game.permanents().len(), 3);
    // The goldfish played nothing.
    assert_eq!(game.player(THEM).lands_played(), 0);
    assert!(game.stack().is_empty());
}

/// CR 103.8a: in a two-player game the starting player skips the draw step
/// of their first turn; the other player draws on theirs (CR 504.1).
#[test]
fn the_starting_player_skips_their_first_draw() {
    let mut game = Game::new(vec![forests(20), forests(20)], 1);
    pass_until(&mut game, ME, Step::PrecombatMain);
    assert_eq!(game.player(ME).hand().len(), 7);
    assert_eq!(game.player(ME).library().len(), 13);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    assert_eq!(game.turn(), 2);
    assert_eq!(game.player(THEM).hand().len(), 8);
}

/// CR 103.8c: in any other multiplayer game no one skips it.
#[test]
fn in_a_three_player_game_the_starting_player_draws() {
    let mut game = Game::new(vec![forests(20), forests(20), forests(20)], 1);
    pass_until(&mut game, ME, Step::PrecombatMain);
    assert_eq!(game.player(ME).hand().len(), 8);
}

/// CR 305.2, 305.2a: one land a turn. CR 116.2a, 305.1: only in your own
/// main phase, with priority and the stack empty.
#[test]
fn a_second_land_in_a_turn_is_rejected() {
    let mut game = Game::new(vec![forests(20), forests(20)], 1);
    // Not in upkeep.
    let early = land_in_hand(&game, ME);
    assert_eq!(game.step_now(), Step::Upkeep);
    assert!(matches!(
        game.step(Answer::Act(Action::PlayLand(early))),
        Err(Rejected::Illegal(_))
    ));
    pass_until(&mut game, ME, Step::PrecombatMain);
    let land = land_in_hand(&game, ME);
    act(&mut game, Action::PlayLand(land));
    let second = land_in_hand(&game, ME);
    let d = game.decision();
    assert!(!priority(&d).1.contains(&Action::PlayLand(second)));
    assert!(matches!(
        game.step(Answer::Act(Action::PlayLand(second))),
        Err(Rejected::Illegal(_))
    ));
    assert_eq!(game.permanents().len(), 1);
    assert_eq!(game.player(ME).hand().len(), 6);
    // Nor in the other player's turn, when they have priority.
    let d = pass_until(&mut game, THEM, Step::PrecombatMain);
    assert!(priority(&d)
        .1
        .iter()
        .any(|a| matches!(a, Action::PlayLand(_))));
    act(&mut game, Action::Pass);
    let d = game.decision();
    assert_eq!(priority(&d).0, ME);
    assert!(!priority(&d)
        .1
        .iter()
        .any(|a| matches!(a, Action::PlayLand(_))));
}

/// CR 106.4, 500.5: mana left in a pool empties as the step ends, here the
/// precombat main phase into the beginning of combat.
#[test]
fn mana_empties_between_steps() {
    let mut game = Game::new(vec![forests(20), forests(20)], 1);
    pass_until(&mut game, ME, Step::PrecombatMain);
    let land = land_in_hand(&game, ME);
    act(&mut game, Action::PlayLand(land));
    let d = game.decision();
    let tap = *priority(&d)
        .1
        .iter()
        .find(|a| {
            matches!(
                a,
                Action::TapForMana {
                    adds: Mana::Green,
                    ..
                }
            )
        })
        .expect("the Forest's intrinsic ability, CR 305.6");
    act(&mut game, tap);
    assert_eq!(game.player(ME).pool().get(Mana::Green), 1);
    // Tapped, it has no ability to activate.
    let d = game.decision();
    assert!(!priority(&d).1.contains(&tap));
    // Passing doesn't empty it (CR 117.3d); the step ending does.
    act(&mut game, Action::Pass);
    assert_eq!(game.player(ME).pool().get(Mana::Green), 1);
    act(&mut game, Action::Pass);
    assert_eq!(game.step_now(), Step::BeginningOfCombat);
    assert_eq!(game.player(ME).pool().total(), 0);
}

/// CR 121.4, 704.5b: a player who drew from an empty library loses when a
/// player would next receive priority, and the other wins (CR 104.2a).
#[test]
fn drawing_from_an_empty_library_loses_the_game() {
    let mut game = Game::new(vec![forests(7), forests(20)], 1);
    assert!(game.player(ME).library().is_empty());
    let mut d = game.decision();
    for _ in 0..100 {
        d = match d {
            Decision::Over(_) => break,
            Decision::Discard { count, hand, .. } => {
                game.step(Answer::Discard(hand[..count].to_vec())).unwrap()
            }
            Decision::Priority { .. } => game.step(Answer::Act(Action::Pass)).unwrap(),
        };
    }
    assert_eq!(d, Decision::Over(Outcome::Won(THEM)));
    // My third turn's draw step, the first I didn't skip.
    assert_eq!(game.turn(), 3);
    assert_eq!(game.step_now(), Step::Draw);
    assert!(game.player(ME).has_left());
    assert!(matches!(
        game.step(Answer::Act(Action::Pass)),
        Err(Rejected::Illegal(_))
    ));
}

/// CR 514.1: in cleanup the active player discards down to seven, and
/// chooses what.
#[test]
fn cleanup_discards_down_to_seven_as_a_decision() {
    let mut game = Game::new(vec![forests(20), forests(20)], 1);
    pass_until(&mut game, THEM, Step::End);
    act(&mut game, Action::Pass);
    let d = act(&mut game, Action::Pass);
    assert_eq!(game.step_now(), Step::Cleanup);
    let Decision::Discard {
        player,
        count,
        hand,
    } = d
    else {
        panic!("expected a discard, got {d:?}");
    };
    assert_eq!((player, count, hand.len()), (THEM, 1, 8));
    for wrong in [vec![], vec![hand[0], hand[0]], vec![hand[0], hand[1]]] {
        assert!(matches!(
            game.step(Answer::Discard(wrong)),
            Err(Rejected::Illegal(_))
        ));
    }
    let d = game.step(Answer::Discard(vec![hand[3]])).unwrap();
    assert_eq!(
        game.player(THEM).graveyard().iter().collect::<Vec<_>>(),
        [&hand[3]]
    );
    assert_eq!(game.player(THEM).hand().len(), 7);
    // The next turn is mine, and begins in upkeep after the untap step.
    assert_eq!((game.turn(), game.step_now()), (3, Step::Upkeep));
    assert_eq!(priority(&d).0, ME);
}
