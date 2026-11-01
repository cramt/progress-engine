//! Hand-written logs, in the shape of PDBot's exports, replayed through
//! the engine (`replay`). The players are alice and bob; no real log is
//! ever committed.

use chancellor_of_the_spires::deck::{CardText, Oracle};
use chancellor_of_the_spires::log;
use chancellor_of_the_spires::model::{pt, Color, PlayerId};
use chancellor_of_the_spires::replay::{replay, Divergence, End, Replay, SetupStop, Stop};

const ALICE: PlayerId = PlayerId(0);
const BOB: PlayerId = PlayerId(1);

fn bears() -> CardText {
    CardText {
        name: "Grizzly Bears".into(),
        type_line: "Creature — Bear".into(),
        mana_cost: "{1}{G}".into(),
        oracle: Oracle::OneFace(String::new()),
        mana_value: 2,
        colors: vec![Color::Green],
        pt: Some(pt(2, 2)),
        identity: vec![Color::Green],
    }
}

fn cards(name: &str) -> Option<CardText> {
    match name {
        "Forest" | "Island" => Some(CardText::basic(name)),
        "Grizzly Bears" => Some(bears()),
        _ => None,
    }
}

/// A one-game match: the export's header, then `lines`, each given a
/// timestamp.
fn run(lines: &str) -> (log::Game, Replay) {
    let body: String = lines
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            if l.starts_with("Winner:") {
                format!("{l}\n")
            } else {
                format!("[12:00:00] {l}\n")
            }
        })
        .collect();
    let text = format!("Freeform\ntest\nTourney\nalice,bob\n== Game 1 (1) ==\n[Time] 0\n{body}");
    let m = log::parse(&text).unwrap();
    let g = m.games[0].clone();
    assert_eq!(g.unrecognised().count(), 0, "{:?}", g.entries);
    let r = replay(&m, &g, &mut cards);
    (g, r)
}

/// Two turns each of land drops and a vanilla creature, with bob's
/// mulligan to six and the draw-step lines newer logs print, to alice's
/// concession: every line followed, and the engine ends where the log does.
#[test]
fn land_drops_and_a_vanilla_creature_replay_to_the_end() {
    let (_, r) = run("
        alice chooses to play first.
        bob mulligans to six cards.
        alice begins the game with seven cards in hand.
        bob puts a card on the bottom of their library and begins the game with six cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Grizzly Bears].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.turn(), 4);
    assert_eq!(game.active(), BOB);
    // CR 103.4: constructed starts at 20.
    assert_eq!(game.player(ALICE).life(), 20);
    // Seven, less the land and the Bears, and one drawn on turn 3.
    assert_eq!(game.player(ALICE).hand().len(), 7 - 1 - 1 + 1 - 1);
    // Six after the mulligan, a draw and a land each turn.
    assert_eq!(game.player(BOB).hand().len(), 6);
    assert_eq!(game.player(BOB).library().len(), 60 - 6 - 2);
    let bears: Vec<_> = game
        .permanents()
        .values()
        .filter(|p| game.card(p.card).is_some_and(|c| c.name == "Grizzly Bears"))
        .collect();
    assert_eq!(bears.len(), 1);
    // Both Forests paid for it.
    let forests_tapped = game
        .permanents()
        .values()
        .filter(|p| p.tapped && game.card(p.card).is_some_and(|c| c.name == "Forest"))
        .count();
    assert_eq!(forests_tapped, 2);
    // Every card still in a hand or library is one the log never showed.
    assert!(game
        .player(BOB)
        .hand()
        .iter()
        .all(|c| game.card(*c).is_none()));
}

/// CR 305.2: a second land in a turn. Magic Online wouldn't log one, so
/// a log that has it (here, written by hand) must stop the replay there.
#[test]
fn a_second_land_in_a_turn_stops_the_replay() {
    let (g, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        alice plays [Forest].
        Turn 1: bob
    ");
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert_eq!(g.entries[*entry].text, "alice plays [Forest].");
    assert_eq!(*entry, 3);
    assert_eq!(
        *stop,
        Stop::Diverged(Divergence::NeverOffered {
            what: "play Forest".into()
        })
    );
}

/// CR 601.2g–h: Grizzly Bears off one Forest can't be paid for.
#[test]
fn a_spell_the_player_cant_afford_stops_the_replay() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        alice casts [Grizzly Bears].
    ");
    assert_eq!(
        r.end.stop(),
        Some(&Stop::Diverged(Divergence::Unpayable {
            card: "Grizzly Bears".into()
        }))
    );
}

/// The turn a line is in has to be the engine's: bob can't play a land
/// in alice's turn.
#[test]
fn a_land_in_the_opponents_turn_stops_the_replay() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        bob plays [Island].
    ");
    assert!(
        matches!(
            r.end.stop(),
            Some(Stop::Diverged(Divergence::NeverOffered { .. }))
        ),
        "{:?}",
        r.end
    );
}

/// A card the engine has no lowering for is refused by name, and a line
/// the replay doesn't follow yet is named by its kind.
#[test]
fn refusals_and_unfollowed_lines_are_named() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        alice casts [Lightning Bolt] targeting bob.
    ");
    assert_eq!(
        r.end.stop(),
        Some(&Stop::UnknownCard("Lightning Bolt".into()))
    );
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        bob is being attacked by [Grizzly Bears]
    ");
    assert_eq!(
        r.end.stop().map(Stop::kind),
        Some("unfollowed: Attacked".to_string())
    );
}

/// The London mulligan (CR 103.5) puts one card on the bottom per
/// mulligan; a log that keeps six without bottoming one isn't London.
#[test]
fn a_mulligan_without_bottoming_is_not_london() {
    let (_, r) = run("
        alice chooses to play first.
        alice mulligans to six cards.
        alice begins the game with six cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
    ");
    assert_eq!(r.end.stop(), Some(&Stop::Setup(SetupStop::NotLondon)));
    assert!(r.game.is_none());
}

/// A card named from a hidden one in the same main phase it is cast in
/// is offered there: the engine is asked again once it is named.
#[test]
fn a_card_named_in_the_main_phase_is_cast_in_it() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Forest].
        alice casts [Grizzly Bears].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(
        game.step_now(),
        chancellor_of_the_spires::game::Step::PrecombatMain
    );
}
