//! Hand-written logs, in the shape of PDBot's exports, replayed through
//! the engine (`replay`). The players are alice and bob; no real log is
//! ever committed.

use std::path::Path;
use std::sync::OnceLock;

use chancellor_of_the_spires::deck::{self, CardText, Oracle};
use chancellor_of_the_spires::log;
use chancellor_of_the_spires::model::{pt, Color, PlayerId};
use chancellor_of_the_spires::replay::{
    replay, Divergence, End, Replay, SetupStop, Stop, UnseenLand,
};
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
    "Mishra's Factory",
    "Cascade Bluffs",
    "Daring Apprentice",
    "Warmth",
    "Eidolon of the Great Revel",
    "Enlightened Tutor",
    "Aura of Silence",
    "Lonely Sandbar",
    "Mox Diamond",
    "Portent",
    "Cabal Therapy",
    "Typhoid Rats",
    "Dark Ritual",
    "Wall of Roots",
    "Llanowar Elves",
    "City of Brass",
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
    run_with(lines, &mut cards, &[])
}

/// As `run`, with these cards and the lands a search may find that the log
/// never names.
fn run_with(
    lines: &str,
    cards: &mut dyn FnMut(&str) -> Option<CardText>,
    unseen: &[UnseenLand],
) -> (log::Game, Replay) {
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
    let r = replay(&m, &g, cards, unseen);
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

/// CR 602.2: of Mishra's Factory's abilities, the one the log's line
/// prints the text of, though the parser reads that text as its target's
/// note; and its "{1}: becomes a 2/2" paid from another land, so the
/// Factory is untapped to block.
#[test]
fn an_activation_line_names_which_of_a_cards_abilities_by_its_text() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice casts [Raging Goblin].
        Turn 1: bob
        bob plays [Mishra's Factory].
        Turn 2: alice
        alice plays [Mountain].
        Turn 2: bob
        bob plays [Island].
        Turn 3: alice
        bob activates an ability of [Mishra's Factory] ( Mishra's Factory becomes a 2/2 Assembly-Worker artifact creature until end of turn. It's still a ...).
        bob is being attacked by [Raging Goblin]
        [Mishra's Factory] blocks [Raging Goblin].
        bob activates an ability of [Mishra's Factory] targeting [Mishra's Factory] ( Target Assembly-Worker creature gets +1/+1 until end of turn.).
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
}

/// CR 605.3a: the payment solver plans a filter land's "{U/R}, {T}: Add
/// {U}{U}" after the Mountain whose {R} it spends, so two Mountains and
/// Cascade Bluffs cast a {1}{U}{U} spell.
#[test]
fn a_payment_filters_mana_through_a_filter_land() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob plays [Forest].
        Turn 2: alice
        alice plays [Cascade Bluffs].
        Turn 2: bob
        bob plays [Forest].
        Turn 3: alice
        alice plays [Mountain].
        alice casts [Daring Apprentice].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
}

/// CR 601.2i, 603.2: a spell cast while another is on the stack may have
/// been cast in response, and the log never says: Warmth's "Whenever an
/// opponent casts a red spell" would have triggered on the Bolt had Warmth
/// resolved first, and no line shows it did, so the Bolt answered Warmth.
/// Eidolon's trigger, which a line does show, is followed as cast after.
#[test]
fn a_spell_is_cast_in_response_when_the_triggers_after_say_so() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Plains].
        Turn 1: bob
        bob plays [Mountain].
        Turn 2: alice
        alice plays [Plains].
        alice casts [Warmth].
        bob casts [Lightning Bolt] targeting alice.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.stack().len(), 2, "the Bolt above Warmth");
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob plays [Mountain].
        Turn 2: alice
        alice plays [Mountain].
        alice casts [Eidolon of the Great Revel].
        bob casts [Lightning Bolt] targeting alice.
        bob puts triggered ability from [Eidolon of the Great Revel] onto the stack (Whenever a player casts a spell with mana value 3 or less, Eidolon of the Great Revel deals 2 damage to that player.).
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
}

/// CR 701.23a: Enlightened Tutor's "Search your library for an artifact
/// or enchantment card, reveal it": the line that reveals it names what
/// the search found, put on top.
#[test]
fn a_search_that_reveals_finds_the_card_the_line_names() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Plains].
        alice casts [Enlightened Tutor].
        alice reveals [Aura of Silence].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    let top = game.player(ALICE).library()[0];
    assert_eq!(
        game.card(top).map(|t| t.name.as_str()),
        Some("Aura of Silence")
    );
}

/// CR 702.29a: "discards [Lonely Sandbar]" then "cycles [Lonely Sandbar]"
/// is its cycling: the discard its cost with {U}, and a card drawn as it
/// resolves.
#[test]
fn a_discard_then_a_cycle_line_cycles_the_card() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        alice discards [Lonely Sandbar].
        alice cycles [Lonely Sandbar].
        Turn 1: bob
        bob plays [Forest].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(
        game.player(ALICE).hand().len(),
        6,
        "seven, less the Island, less the Sandbar, and a draw"
    );
    assert_eq!(game.player(ALICE).graveyard().len(), 1);
}

/// CR 614.1a: Mox Diamond enters only if a land card is discarded
/// instead, which the log's next discard line says; with none, it goes
/// to its owner's graveyard.
#[test]
fn mox_diamond_enters_for_the_land_the_next_line_discards() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice casts [Mox Diamond].
        alice discards [Forest].
        alice casts [Mox Diamond].
        Turn 1: bob
        bob plays [Forest].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    let moxen = game
        .permanents()
        .values()
        .filter(|p| game.card(p.card).is_some_and(|t| t.name == "Mox Diamond"))
        .count();
    assert_eq!(moxen, 1, "the second has no land to discard");
    assert_eq!(
        game.player(ALICE).graveyard().len(),
        2,
        "the Forest and a Mox"
    );
}

/// CR 701.24a, 603.7a: "shuffles their library" after Portent is its "You
/// may have that player shuffle" taken, matched to the engine's shuffle;
/// its delayed draw goes on the stack at the next turn's upkeep.
#[test]
fn a_shuffle_line_is_an_instructions_shuffle() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        alice casts [Portent] targeting alice.
        alice shuffles their library.
        Turn 1: bob
        alice puts triggered ability from [Portent] onto the stack (Draw a card at the beginning of the next turn's upkeep.).
        alice draws a card with [Portent]'s ability.
        bob plays [Forest].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        alice shuffles their library.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert!(
        matches!(r.end, End::Stopped { entry: 6, .. }),
        "no instruction shuffled: {:?}",
        r.end
    );
}

/// A fetchland's find the log never names, where no basic or land the
/// match shows will do (here, no basic is in the card data): a real land
/// of those the game's format allows, as a hypothesis, the one making the
/// colours alice's spells need first, and only one the search's filter
/// matches. The replay says it relied on it.
#[test]
fn a_fetchland_may_find_a_land_the_log_never_names() {
    let land = |name: &str, makes: [bool; 5]| UnseenLand {
        name: name.into(),
        makes,
    };
    let unseen = [
        // Plains Island: not a Mountain or Forest card, though it makes
        // nothing alice needs either.
        land("Tundra", [true, true, false, false, false]),
        land("Volcanic Island", [false, true, false, true, false]),
        land("Taiga", [false, false, false, true, true]),
    ];
    let mut no_basics = |name: &str| match name {
        "Forest" | "Mountain" => None,
        "Tundra" | "Volcanic Island" | "Taiga" => Some(deck::card(name, index()).unwrap()),
        n => cards(n),
    };
    let (_, r) = run_with(
        "
        alice chooses to play first.
        Turn 1: alice
        alice plays [Wooded Foothills].
        alice activates an ability of [Wooded Foothills] ( Search your library for a Mountain or Forest card, put it onto the battlefield, then shuffle.).
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Island].
        alice casts [Grizzly Bears].
        bob has conceded from the game.
    ",
        &mut no_basics,
        &unseen,
    );
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    assert_eq!(r.unseen, ["Taiga"]);
    let game = r.game.unwrap();
    assert_eq!(on_battlefield(&game, "Taiga"), 1);
    assert_eq!(on_battlefield(&game, "Tundra"), 0);
}

/// Cabal Therapy names the card the log's "names" line does (CR 201.4),
/// the hand revealed names bob's hidden cards (CR 701.20a), and each card
/// of that name is discarded, a line apiece. Cast again from the graveyard
/// for its flashback cost, the creature sacrificed is one the log never
/// names, and the spell is exiled (CR 702.34a).
#[test]
fn cabal_therapy_names_and_flashes_back() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Swamp].
        alice casts [Cabal Therapy] targeting bob.
        alice names Grizzly Bears for [Cabal Therapy].
        bob reveals their hand to [Cabal Therapy], containing [Grizzly Bears], [Island], [Island], [Island], [Island], [Island], and [Grizzly Bears].
        bob discards [Grizzly Bears].
        bob discards [Grizzly Bears].
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Swamp].
        alice casts [Typhoid Rats].
        Turn 2: bob
        bob plays [Island].
        Turn 3: alice
        alice casts [Cabal Therapy] with FlashbackSacrifice a creature from the graveyard targeting bob.
        alice names Grizzly Bears for [Cabal Therapy].
        bob reveals their hand to [Cabal Therapy], containing [Island], [Island], [Grizzly Bears], [Island], and [Island].
        bob discards [Grizzly Bears].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(on_battlefield(&game, "Typhoid Rats"), 0);
    assert_eq!(game.exile().len(), 1);
    assert_eq!(game.player(BOB).graveyard().len(), 3);
    assert_eq!(game.player(BOB).hand().len(), 4);
}

/// CR 106.3, 500.5: Dark Ritual's mana stays in the pool through the main
/// phase, so Duress cast after it resolves is paid from it.
#[test]
fn dark_ritual_s_mana_pays_for_the_next_spell() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Swamp].
        alice casts [Dark Ritual].
        alice casts [Duress] targeting bob.
        bob reveals their hand to [Duress], containing [Island], [Island], [Island], [Island], [Island], [Island], and [Island].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
}

/// Mana of the wrong kind in the pool is no reason to stop looking: Dark
/// Ritual's {B}{B}{B} doesn't pay Raging Goblin's {R}, a Mountain does.
#[test]
fn a_pool_of_the_wrong_colour_still_taps_a_land() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Mountain].
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Swamp].
        alice casts [Dark Ritual].
        alice casts [Raging Goblin].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
}

/// A "puts a -0/-1 counter on [Wall of Roots]" line is its mana ability's
/// cost, printed as it is activated (CR 602.2b): the replay activates it
/// then, and the mana floats until Llanowar Elves is cast with it.
#[test]
fn wall_of_roots_counter_line_is_its_mana_ability() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Forest].
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Forest].
        alice casts [Wall of Roots].
        alice puts a -0/-1 counter on [Wall of Roots].
        alice casts [Llanowar Elves].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.unwrap();
    // Cast off the Wall's {G}, and on the stack when bob concedes.
    assert_eq!(game.stack().len(), 1);
    assert_eq!(on_battlefield(&game, "Wall of Roots"), 1);
}

/// City of Brass's "Whenever this land becomes tapped" line with nothing
/// on the stack: it was tapped for mana, which the log never shows, and
/// the {R} it made floats until Raging Goblin is cast (CR 701.26a, 106.4).
#[test]
fn a_city_of_brass_trigger_line_is_it_tapped_for_mana() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [City of Brass].
        alice puts triggered ability from [City of Brass] onto the stack (Whenever City of Brass becomes tapped, it deals 1 damage to you.).
        alice casts [Raging Goblin].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(game.player(ALICE).life(), 19);
}
