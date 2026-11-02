//! Hand-written logs, in the shape of PDBot's exports, replayed through
//! the engine (`replay`). The players are alice and bob; no real log is
//! ever committed.

use std::path::Path;
use std::sync::OnceLock;

use chancellor_of_the_spires::deck::{self, CardText, Oracle};
use chancellor_of_the_spires::log;
use chancellor_of_the_spires::model::{pt, Color, PlayerId};
use chancellor_of_the_spires::replay::{replay, Divergence, End, Replay, SetupStop, Stop};
use chip_scryfall::index::IndexFile;

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

/// The real cards the logs here name beyond these, from the card index.
const INDEXED: &[&str] = &[
    "Mountain",
    "Swamp",
    "Wooded Foothills",
    "Raging Goblin",
    "Stomping Ground",
    "Plains",
    "Duress",
    "Champion of the Parish",
    "Elite Vanguard",
    "Goblin Lackey",
    "Aether Vial",
    "Leyline of Sanctity",
    "Lightning Bolt",
    "Swords to Plowshares",
];

fn index() -> &'static IndexFile {
    static INDEX: OnceLock<IndexFile> = OnceLock::new();
    INDEX.get_or_init(|| {
        IndexFile::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../decks/index.jsonl"))
            .expect("decks/index.jsonl")
    })
}

fn cards(name: &str) -> Option<CardText> {
    match name {
        "Forest" | "Island" => Some(CardText::basic(name)),
        "Grizzly Bears" => Some(bears()),
        n if INDEXED.contains(&n) => Some(deck::card(n, index()).unwrap()),
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
        alice casts [Chain Lightning] targeting bob.
    ");
    assert_eq!(
        r.end.stop(),
        Some(&Stop::UnknownCard("Chain Lightning".into()))
    );
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        bob is being attacked by [Grizzly Bears]
    ");
    // No creature to attack with.
    assert_eq!(
        r.end.stop().map(Stop::kind),
        Some("diverged: NeverOffered".to_string())
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

fn on_battlefield(game: &chancellor_of_the_spires::game::Game, name: &str) -> usize {
    game.permanents()
        .values()
        .filter(|p| game.card(p.card).is_some_and(|c| c.name == name))
        .count()
}

/// CR 602.2, 701.23, 701.24: a fetchland activated, its search answered by
/// the replay since the log never says what was found. Alice's spells this
/// game need {R} and {G} alike, so the first basic tried is a Mountain,
/// with which Grizzly Bears on Island can't be paid for; the replay goes
/// back to the search, finds a Forest instead, and the rest follows.
#[test]
fn a_fetchland_finds_what_later_spells_need() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Wooded Foothills].
        alice activates an ability of [Wooded Foothills] ( Search your library for a Mountain or Forest card, put it onto the battlefield, then shuffle.).
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Island].
        alice casts [Grizzly Bears].
        Turn 2: bob
        bob plays [Island].
        Turn 3: alice
        alice plays [Mountain].
        alice casts [Raging Goblin].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    assert!(
        r.trace
            .iter()
            .any(|t| t.did.starts_with("chose again: found Forest")),
        "{:#?}",
        r.trace
    );
    let game = r.game.unwrap();
    // CR 119.4: the fetch's life.
    assert_eq!(game.player(ALICE).life(), 19);
    assert_eq!(on_battlefield(&game, "Wooded Foothills"), 0);
    assert_eq!(on_battlefield(&game, "Forest"), 1);
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 1);
    // Cast, and still on the stack when bob concedes.
    assert_eq!(game.stack().len(), 1);
}

/// CR 701.23b: when the log says the search failed, it found nothing, and
/// Grizzly Bears off one Island can't be paid for.
#[test]
fn a_search_the_log_says_failed_finds_nothing() {
    let (g, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Wooded Foothills].
        alice activates an ability of [Wooded Foothills] ( Search your library for a Mountain or Forest card, put it onto the battlefield, then shuffle.).
        alice fails to find a Mountain or Forest card.
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Island].
        alice casts [Grizzly Bears].
    ");
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert_eq!(g.entries[*entry].text, "alice casts [Grizzly Bears].");
    assert_eq!(
        *stop,
        Stop::Diverged(Divergence::Unpayable {
            card: "Grizzly Bears".into()
        })
    );
}

/// CR 614.12a, 119.4: the log never says whether a shock land's 2 life was
/// paid. The replay doesn't pay, so Stomping Ground enters tapped, unless
/// a spell that turn can't be paid for otherwise: here Raging Goblin, cast
/// off it the turn it is played, so the replay goes back and pays.
#[test]
fn a_shock_land_is_paid_for_only_when_what_follows_needs_it() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Stomping Ground].
        alice casts [Raging Goblin].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    assert!(r
        .trace
        .iter()
        .any(|t| t.did == "chose again: paid 2 life for Stomping Ground to enter untapped"));
    assert_eq!(r.game.unwrap().player(ALICE).life(), 18);

    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Stomping Ground].
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice casts [Raging Goblin].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    assert!(r
        .trace
        .iter()
        .any(|t| t.did == "Stomping Ground enters tapped, no life paid"));
    assert_eq!(r.game.unwrap().player(ALICE).life(), 20);
}

/// Duress and Champion of the Parish, as newer logs print them: the
/// target chosen as Duress is cast (CR 601.2c); bob's whole hand revealed
/// (CR 701.20a), which names its hidden cards and must be as many as the
/// engine's; the card alice chose discarded; and Champion's trigger on the
/// stack when the log shows it (CR 603.3), growing it as it resolves.
#[test]
fn a_revealed_hand_and_a_trigger_replay() {
    let lines = "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Swamp].
        alice casts [Duress] targeting bob.
        bob reveals 7 cards with [Duress]: [Plains], [Plains], [Champion of the Parish], [Elite Vanguard], [Duress], [Plains], [Plains].
        bob discards [Duress].
        Turn 1: bob
        bob draws a card.
        bob plays [Plains].
        bob casts [Champion of the Parish].
        Turn 2: alice
        alice draws a card.
        Turn 2: bob
        bob draws a card.
        bob plays [Plains].
        bob casts [Elite Vanguard].
        bob puts triggered ability from [Champion of the Parish] onto the stack (Whenever another Human you control enters, put a +1/+1 counter on this creature.).
        alice has conceded from the game.
    ";
    let (_, r) = run(lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    let names = |cards: &imbl::Vector<chancellor_of_the_spires::game::CardId>| -> Vec<String> {
        cards
            .iter()
            .filter_map(|c| game.card(*c).map(|t| t.name.clone()))
            .collect()
    };
    assert_eq!(names(game.player(BOB).graveyard()), ["Duress"]);
    assert_eq!(names(game.player(ALICE).graveyard()), ["Duress"]);
    let champion = game
        .permanents()
        .iter()
        .find(|(_, p)| {
            game.card(p.card)
                .is_some_and(|t| t.name == "Champion of the Parish")
        })
        .map(|(id, _)| *id)
        .unwrap();
    // On the stack still: alice conceded with it there.
    assert_eq!(game.stack().len(), 1);
    assert_eq!(game.characteristics()[&champion].pt, Some(pt(1, 1)));

    // Without the trigger's line, the engine's trigger is one the log
    // never shows.
    let unlogged = lines.replace(
        "bob puts triggered ability from [Champion of the Parish] onto the stack (Whenever another Human you control enters, put a +1/+1 counter on this creature.).",
        "Turn 3: alice",
    );
    let (_, r) = run(&unlogged);
    assert_eq!(
        r.end.stop(),
        Some(&Stop::Diverged(Divergence::UnloggedTrigger {
            card: "Champion of the Parish".into()
        }))
    );
    // A hand revealed with a card fewer than the engine's diverges.
    let short = lines.replace("[Duress], [Plains], [Plains].", "[Duress], [Plains].");
    let (_, r) = run(&short);
    assert_eq!(
        r.end.stop(),
        Some(&Stop::Diverged(Divergence::HandSize { log: 6, engine: 7 }))
    );
}

/// An attack and a block as the log prints them (CR 508.1, 509.1): Raging
/// Goblin attacks with haste the turn it is cast, unblocked; the next
/// turn Grizzly Bears block it and it dies (CR 510.1c, 704.5g). The
/// damage itself is never printed, so the engine's life total is the
/// check.
#[test]
fn an_attack_and_a_block_replay() {
    let lines = "
        alice chooses to play first.
        Turn 1: alice
        alice plays [Mountain].
        alice casts [Raging Goblin].
        bob is being attacked by [Raging Goblin]
        Turn 1: bob
        bob plays [Forest].
        Turn 2: alice
        Turn 2: bob
        bob plays [Forest].
        bob casts [Grizzly Bears].
        Turn 3: alice
        bob is being attacked by [Raging Goblin]
        [Grizzly Bears] blocks [Raging Goblin].
        Turn 3: bob
        alice has conceded from the game.
    ";
    let (_, r) = run(lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(game.player(BOB).life(), 19);
    let graveyard: Vec<String> = game
        .player(ALICE)
        .graveyard()
        .iter()
        .filter_map(|c| game.card(*c).map(|t| t.name.clone()))
        .collect();
    assert_eq!(graveyard, ["Raging Goblin"]);
    // A block the engine calls illegal: a creature that isn't there.
    let (_, r) = run(&lines.replace("[Grizzly Bears] blocks", "[Mountain] blocks"));
    assert!(matches!(
        r.end.stop(),
        Some(Stop::Diverged(Divergence::NeverOffered { .. }))
    ));
}

/// Goblin Lackey's "may" (CR 603.5, 608.2d): no line says the first was
/// followed, or which card it put onto the battlefield, so the replay
/// follows it and names the card from the next line that shows one of
/// alice's on the battlefield that no line cast, Raging Goblin attacking;
/// the second is declined by its own line.
#[test]
fn goblin_lackey_s_may_is_read_from_the_lines_after_it() {
    let lackey = "alice puts triggered ability from [Goblin Lackey] onto the stack (Whenever this creature deals damage to a player, you may put a Goblin permanent card from your hand onto the battlefield.).";
    let lines = format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice casts [Goblin Lackey].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        bob is being attacked by [Goblin Lackey]
        {lackey}
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        bob is being attacked by [Goblin Lackey] and [Raging Goblin]
        {lackey}
        alice chooses not to use [Goblin Lackey]'s ability.
        bob has conceded from the game.
    "
    );
    let (_, r) = run(&lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    let on: Vec<String> = game
        .permanents()
        .values()
        .filter_map(|p| game.card(p.card).map(|t| t.name.clone()))
        .collect();
    assert_eq!(on.iter().filter(|n| *n == "Raging Goblin").count(), 1);
    assert_eq!(game.player(BOB).life(), 17);
}

/// Aether Vial's upkeep trigger (CR 503.1a): its "may" followed, the
/// counter line checked against the Vial's counters, then declined by its
/// own line the next upkeep.
#[test]
fn aether_vial_s_upkeep_trigger_counts_up_or_is_declined() {
    let vial = "alice puts triggered ability from [Aether Vial] onto the stack (At the beginning of your upkeep, you may put a charge counter on Aether Vial.).";
    let lines = format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice casts [Aether Vial].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        {vial}
        alice puts a charge counter on [Aether Vial].
        alice draws a card.
        alice plays [Mountain].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        {vial}
        alice chooses not to use [Aether Vial]'s ability.
        alice draws a card.
        bob has conceded from the game.
    "
    );
    let (_, r) = run(&lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    let (id, _) = game
        .permanents()
        .iter()
        .find(|(_, p)| game.card(p.card).is_some_and(|t| t.name == "Aether Vial"))
        .unwrap();
    let charge = chancellor_of_the_spires::model::Counter::Named("charge".into());
    assert_eq!(game.counters_on(*id, &charge), 1);
}

/// CR 103.6a: the Leyline the log puts onto the battlefield before the
/// first turn is named in the opening hand as it is kept, and begins the
/// game there.
#[test]
fn a_leyline_begins_the_game_on_the_battlefield() {
    let lines = "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        bob puts [Leyline of Sanctity] onto the battlefield.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob draws a card.
        bob plays [Plains].
        alice has conceded from the game.
    ";
    let (_, r) = run(lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    assert!(game.permanents().values().any(|p| game
        .card(p.card)
        .is_some_and(|t| t.name == "Leyline of Sanctity")));
    assert_eq!(game.player(BOB).hand().len(), 6);
}

/// A spell that targets what a spell on the stack targets is cast in
/// response to it, before it resolves: Swords to Plowshares saves its
/// target from Lightning Bolt, which then leaves the stack with its only
/// target illegal (CR 608.2b), as the log's line says.
#[test]
fn a_response_to_a_spell_targets_the_same_permanent() {
    let lines = "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob draws a card.
        bob plays [Plains].
        bob casts [Elite Vanguard].
        Turn 2: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 2: bob
        bob draws a card.
        bob plays [Plains].
        Turn 3: alice
        alice draws a card.
        alice casts [Lightning Bolt] targeting [Elite Vanguard].
        bob casts [Swords to Plowshares] targeting [Elite Vanguard].
        alice's [Lightning Bolt] is removed from the stack because it has no legal targets.
        bob has conceded from the game.
    ";
    let (_, r) = run(lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(game.player(BOB).life(), 22);
    assert_eq!(game.exile().len(), 1);
}
