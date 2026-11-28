//! Hand-written logs, in the shape of PDBot's exports, replayed through
//! the engine (`replay`). The players are alice and bob; no real log is
//! ever committed.

use std::path::Path;
use std::sync::OnceLock;

use chancellor_of_the_spires::deck::{self, CardText, Oracle, TwoFaced};
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

/// A face of a two-faced card, as Scryfall has it.
fn face(name: &str, type_line: &str, cost: &str, text: &str, colors: &[Color]) -> CardText {
    CardText {
        name: name.into(),
        type_line: type_line.into(),
        mana_cost: cost.into(),
        oracle: Oracle::OneFace(text.into()),
        mana_value: cost.matches('{').count() as u32,
        colors: colors.to_vec(),
        pt: None,
        identity: colors.to_vec(),
    }
}

/// A modal double-faced card, whose faces the committed index, built
/// before faces kept their text, joins.
fn pelakka() -> CardText {
    CardText::two_faced(
        "Pelakka Predation // Pelakka Caverns".into(),
        TwoFaced::Modal,
        CardText {
            mana_value: 3,
            ..face(
                "Pelakka Predation",
                "Sorcery",
                "{2}{B}",
                "Target opponent reveals their hand. You choose a card from it with mana value 3 or greater. That player discards that card.",
                &[Color::Black],
            )
        },
        face(
            "Pelakka Caverns",
            "Land",
            "",
            "This land enters tapped.\n{T}: Add {B}.",
            &[],
        ),
    )
    .unwrap()
}

/// A transforming double-faced card, likewise; its back face's mana
/// value is its front's (CR 712.8e).
fn delver() -> CardText {
    CardText::two_faced(
        "Delver of Secrets // Insectile Aberration".into(),
        TwoFaced::Transforming,
        CardText {
            pt: Some(pt(1, 1)),
            ..face(
                "Delver of Secrets",
                "Creature — Human Wizard",
                "{U}",
                "At the beginning of your upkeep, look at the top card of your library. You may reveal that card. If an instant or sorcery card is revealed this way, transform this creature.",
                &[Color::Blue],
            )
        },
        CardText {
            pt: Some(pt(3, 2)),
            mana_value: 1,
            ..face(
                "Insectile Aberration",
                "Creature — Human Insect",
                "",
                "Flying",
                &[Color::Blue],
            )
        },
    )
    .unwrap()
}

/// An adventurer card, likewise.
fn scalding_viper() -> CardText {
    CardText::two_faced(
        "Scalding Viper // Steam Clean".into(),
        TwoFaced::Adventure,
        CardText {
            pt: Some(pt(2, 1)),
            ..face(
                "Scalding Viper",
                "Creature — Elemental Snake",
                "{1}{R}",
                "Whenever an opponent casts a spell with mana value 3 or less, this creature deals 1 damage to that player.",
                &[Color::Red],
            )
        },
        face(
            "Steam Clean",
            "Sorcery — Adventure",
            "{1}{U}",
            "Return target nonland permanent to its owner's hand.",
            &[Color::Blue],
        ),
    )
    .unwrap()
}

/// The real cards the logs here name beyond these, from the card index.
const INDEXED: &[&str] = &[
    "Mountain",
    "Outcaster Trailblazer",
    "Swamp",
    "Wooded Foothills",
    "Raging Goblin",
    "Stomping Ground",
    "Plains",
    "Duress",
    "Fact or Fiction",
    "Accumulated Knowledge",
    "Impulse",
    "Champion of the Parish",
    "Elite Vanguard",
    "Goblin Lackey",
    "Aether Vial",
    "Leyline of Sanctity",
    "Lightning Bolt",
    "Gravecrawler",
    "Marang River Prowler",
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
    "Burst Lightning",
    "Gush",
    "Coral Eel",
    "Exalted Angel",
    "Solitary Confinement",
    "Cloudpost",
    "Vesuva",
    "Bronze Sable",
    "Grim Lavamancer",
    "Fireblast",
    "Survival of the Fittest",
    "Rishadan Port",
    "Psychatog",
    "Rift Bolt",
    "Search for Tomorrow",
    "Phyrexian Dreadnought",
    "Stifle",
    "Triskelion",
    "Quicksilver, Brash Blur",
    "Goblin Matron",
    "Goblin Piledriver",
    "Argothian Enchantress",
    "Enchantress's Presence",
    "Wild Growth",
    "Elephant Grass",
    "Eternal Dragon",
    "Wasteland",
    "Tempting Wurm",
    "Show and Tell",
    "Elvish Visionary",
    "Liliana of the Veil",
    "Escape to the Wilds",
    "Diabolic Edict",
    "Ulamog's Crusher",
    "Nulldrifter",
    "Basking Rootwalla",
    "Arrogant Wurm",
    "Careful Study",
    "Commune with Dinosaurs",
    "Quirion Dryad",
    "Accumulated Knowledge",
    "Plague Belcher",
    "Dread Wanderer",
    "Wonder",
    "Quirion Ranger",
    "Wirewood Symbiote",
    "Gaea's Cradle",
    "Masticore",
    "Centaur Courser",
    "Wall of Blossoms",
    "Yavimaya, Cradle of Growth",
    "Squee, Goblin Nabob",
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
        "Forest" | "Island" | "Plains" => Some(CardText::basic(name)),
        "Grizzly Bears" => Some(bears()),
        "Pelakka Predation" | "Pelakka Caverns" => Some(pelakka()),
        "Scalding Viper" | "Steam Clean" => Some(scalding_viper()),
        "Delver of Secrets" | "Insectile Aberration" => Some(delver()),
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
/// game need {R} and {G} alike and she plays a Mountain and a Forest, so
/// the first basic tried is a Mountain, with which Grizzly Bears on Island
/// can't be paid for; the replay goes back to the search, finds a Forest
/// instead, and the rest follows.
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
        Turn 3: bob
        bob plays [Island].
        Turn 4: alice
        alice plays [Forest].
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
    assert_eq!(on_battlefield(&game, "Forest"), 2);
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 1);
    assert_eq!(on_battlefield(&game, "Raging Goblin"), 1);
}

/// CR 701.23a: of the basics a search may find, a colour the player's
/// spells need that no land the log shows them playing makes is tried
/// first: Alice's Mountain makes red and nothing she plays makes green,
/// so her Wooded Foothills finds the Forest with no going back.
#[test]
fn a_fetchland_finds_first_a_colour_no_land_played_makes() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Wooded Foothills].
        alice activates an ability of [Wooded Foothills] ( Search your library for a Mountain or Forest card, put it onto the battlefield, then shuffle.).
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Mountain].
        alice casts [Raging Goblin].
        Turn 2: bob
        bob plays [Island].
        Turn 3: alice
        alice plays [Mountain].
        alice casts [Grizzly Bears].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    assert!(
        r.trace.iter().any(|t| t.did.starts_with("found Forest")),
        "{:#?}",
        r.trace
    );
    assert!(
        !r.trace.iter().any(|t| t.did.starts_with("chose again")),
        "{:#?}",
        r.trace
    );
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

/// CR 509.1a: each blocker blocks one attacker, so both Raging Goblins
/// may block one of alice's two Grizzly Bears, and the log prints that as
/// it prints one on each. Only the Bears that died leaves Grim Lavamancer
/// a second card to exile, so the replay goes back to the blocks for it.
#[test]
fn two_blockers_may_block_one_of_two_attackers_of_a_name() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Grizzly Bears].
        Turn 2: bob
        bob draws a card.
        bob plays [Mountain].
        bob casts [Raging Goblin].
        bob casts [Raging Goblin].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Grim Lavamancer].
        alice casts [Grizzly Bears].
        Turn 3: bob
        bob draws a card.
        Turn 4: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Lightning Bolt] targeting bob.
        bob is being attacked by [Grizzly Bears] and [Grizzly Bears]
        [Raging Goblin] blocks [Grizzly Bears].
        [Raging Goblin] blocks [Grizzly Bears].
        bob exiles [Grizzly Bears] and [Lightning Bolt] with with [Grim Lavamancer]'s ability.
        alice activates an ability of [Grim Lavamancer] targeting bob ( Grim Lavamancer deals 2 damage to any target.).
        Turn 4: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(
        r.trace
            .iter()
            .any(|t| t.did == "chose again: read the blocks of creatures of one name another way"),
        "{:?}",
        r.trace
    );
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 1);
    assert_eq!(on_battlefield(&game, "Raging Goblin"), 0);
    assert_eq!(game.player(BOB).life(), 20 - 3 - 2 - 2);
}

/// A block taken back before blockers are declared (Magic Online prints
/// the click and its undoing) is no block: the Bears that "blocks" and
/// then "stops blocking" never does, and the Goblin's damage is bob's.
#[test]
fn a_block_taken_back_is_no_block() {
    let (_, r) = run("
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
        [Grizzly Bears] stops blocking.
        Turn 3: bob
        alice has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(game.player(BOB).life(), 18);
    assert_eq!(on_battlefield(&game, "Raging Goblin"), 1);
}

/// Elephant Grass's tax on attacking its controller, never printed paid
/// (CR 508.1h): Raging Goblin attacks for free before it is cast and for
/// {2} after, both Mountains tapped to pay; with one Mountain the attack
/// is never offered. Its cumulative upkeep, also never printed paid, is
/// paid from the age counter line, and the Grass stays (CR 702.24a).
#[test]
fn elephant_grass_taxes_an_attack_and_its_upkeep_is_paid() {
    let lines = "
        alice chooses to play first.
        Turn 1: alice
        alice plays [Mountain].
        alice casts [Raging Goblin].
        bob is being attacked by [Raging Goblin]
        Turn 1: bob
        bob plays [Forest].
        bob casts [Elephant Grass].
        Turn 2: alice
        alice plays [Mountain].
        bob is being attacked by [Raging Goblin]
        Turn 2: bob
        bob puts triggered ability from [Elephant Grass] onto the stack (Cumulative upkeep {1} (At the beginning of your upkeep, if this permanent is on the battlefield,...).
        bob puts an age counter on [Elephant Grass].
        bob plays [Forest].
        Turn 3: alice
        alice has conceded from the game.
    ";
    let (_, r) = run(lines);
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(r.trace.iter().any(|t| t.did.contains("paid {2} to attack")));
    let game = r.game.unwrap();
    assert_eq!(game.player(BOB).life(), 18);
    let grass = game
        .permanents()
        .iter()
        .find(|(_, p)| {
            game.card(p.card)
                .is_some_and(|t| t.name == "Elephant Grass")
        })
        .map(|(id, _)| *id)
        .expect("kept");
    assert_eq!(
        game.counters_on(
            grass,
            &chancellor_of_the_spires::model::Counter::named("age")
        ),
        1
    );
    let (_, r) = run(&lines.replace(
        "Turn 2: alice\n        alice plays [Mountain].",
        "Turn 2: alice",
    ));
    assert!(
        matches!(
            r.end.stop(),
            Some(Stop::Diverged(Divergence::NeverOffered { .. }))
        ),
        "{:?}",
        r.end
    );
}

/// Eternal Dragon's plainscycling (CR 702.29e), its Plains named by the
/// line revealing it, cycled in response to Rishadan Port's ability on the
/// Swamp that pays for it: resolving the ability first leaves only the
/// Wasteland to pay with, which the next line activates.
#[test]
fn typecycling_in_response_keeps_a_land_untapped() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Swamp].
        Turn 1: bob
        bob plays [Rishadan Port].
        Turn 2: alice
        alice plays [Plains].
        Turn 2: bob
        bob plays [Mountain].
        Turn 3: alice
        alice plays [Wasteland].
        Turn 3: bob
        bob plays [Mountain].
        Turn 4: alice
        bob activates an ability of [Rishadan Port] targeting [Swamp] ( Tap target land.).
        alice discards [Eternal Dragon].
        alice cycles [Eternal Dragon].
        alice reveals [Plains].
        alice plays [Plains].
        alice activates an ability of [Wasteland] targeting [Rishadan Port] ( Destroy target nonbasic land.).
        Turn 4: bob
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    let graveyard: Vec<String> = game
        .player(BOB)
        .graveyard()
        .iter()
        .filter_map(|c| game.card(*c).map(|t| t.name.clone()))
        .collect();
    assert_eq!(graveyard, ["Rishadan Port"]);
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

/// Tempting Wurm's opponent's cards from their hand (CR 107.1c): no line
/// says which bob put onto the battlefield, so the replay names them from
/// the lines after it, the Grizzly Bears attacking in bob's next turn that
/// no line casts.
fn tempting_wurm(bob_attacks_with: &str, turns_later: bool) -> String {
    let later = if turns_later {
        "Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        Turn 3: bob"
    } else {
        "Turn 2: bob"
    };
    let wurm = "alice puts triggered ability from [Tempting Wurm] onto the stack (When Tempting Wurm enters the battlefield, each opponent may put any number of artifact, creature,...).";
    format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Tempting Wurm].
        {wurm}
        {later}
        bob draws a card.
        alice is being attacked by [{bob_attacks_with}]
        alice has conceded from the game.
    "
    )
}

#[test]
fn tempting_wurm_s_cards_are_read_from_the_lines_after_it() {
    let (_, r) = run(&tempting_wurm("Grizzly Bears", false));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.unwrap();
    let bears = game
        .permanents()
        .values()
        .filter(|p| game.card(p.card).is_some_and(|t| t.name == "Grizzly Bears"))
        .count();
    assert_eq!(bears, 1);
}

/// A card bob attacks with in his next turn that the engine refuses is
/// refused where the Wurm's trigger puts it, never taken for none put:
/// that way would leave the attack unexplained and report it a divergence.
#[test]
fn tempting_wurm_s_refused_card_stops_at_the_trigger() {
    let (_, r) = run(&tempting_wurm("Ulamog's Crusher", false));
    let End::Stopped { stop, .. } = &r.end else {
        panic!("{:?}", r.end)
    };
    assert!(
        matches!(stop, Stop::Refused { card, .. } if card == "Ulamog's Crusher"),
        "{stop:?}"
    );
}

/// A card only a line after bob's next turn shows may have come some
/// other way, so none put is still tried, last: the replay follows the
/// trigger line and the turns after it, and stops only at the attack.
#[test]
fn tempting_wurm_tries_none_for_a_card_shown_turns_later() {
    let (g, r) = run(&tempting_wurm("Ulamog's Crusher", true));
    let End::Stopped { entry, .. } = &r.end else {
        panic!("{:?}", r.end)
    };
    let attack = g
        .entries
        .iter()
        .position(|e| format!("{:?}", e.event).contains("Attacked"))
        .expect("the attack line");
    assert_eq!(*entry, attack, "{:?}", r.end);
}

/// Show and Tell's two choosers (CR 101.4): alice chooses first, then
/// bob, and what both chose enters at once, so alice's choice is read
/// from the trigger lines only once bob has chosen too. Each `put` names
/// whose Elvish Visionary goes in; its trigger and draw are the lines.
fn show_and_tell(put: &[&str]) -> String {
    let lines: String = put
        .iter()
        .map(|who| {
            format!(
                "{who} puts triggered ability from [Elvish Visionary] onto the stack (When Elvish Visionary enters, draw a card.).
                "
            )
        })
        // Put on the stack in APNAP order, bob's resolves first (CR 603.3b).
        .chain(
            put.iter()
                .rev()
                .map(|who| format!("{who} draws a card with [Elvish Visionary]'s ability.\n")),
        )
        .collect();
    format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Show and Tell].
        {lines}
        alice has conceded from the game.
    "
    )
}

fn visionaries(r: &Replay) -> Vec<String> {
    let game = r.game.as_ref().unwrap();
    let chars = game.characteristics();
    let mut whose: Vec<String> = game
        .permanents()
        .iter()
        .filter(|(_, p)| {
            game.card(p.card)
                .is_some_and(|t| t.name == "Elvish Visionary")
        })
        .map(|(id, _)| {
            if chars[id].controller == ALICE {
                "alice".to_string()
            } else {
                "bob".to_string()
            }
        })
        .collect();
    whose.sort();
    whose
}

#[test]
fn show_and_tell_s_first_chooser_is_read_from_the_triggers_after_both_chose() {
    for put in [&["alice"][..], &["bob"], &["alice", "bob"]] {
        let (_, r) = run(&show_and_tell(put));
        assert_eq!(r.end, End::Finished, "{put:?}: {:?}", r.trace);
        assert_eq!(visionaries(&r), put, "{put:?}");
    }
}

/// None put stays a way when a name shown in bob's next turn is one the
/// Wurm can't have put (a planeswalker; Aether Vial, reanimation or a
/// later Show and Tell would be the same): the replay follows the trigger
/// and stops where the line shows it, not at the Wurm.
#[test]
fn tempting_wurm_tries_none_when_the_name_shown_soon_could_have_come_another_way() {
    let wurm = "alice puts triggered ability from [Tempting Wurm] onto the stack (When Tempting Wurm enters the battlefield, each opponent may put any number of artifact, creature,...).";
    let (g, r) = run(&format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Tempting Wurm].
        {wurm}
        Turn 2: bob
        bob draws a card.
        bob activates an ability of [Liliana of the Veil] ( Each player discards a card.).
        alice has conceded from the game.
    "
    ));
    let End::Stopped { entry, .. } = &r.end else {
        panic!("{:?}", r.end)
    };
    let liliana = g
        .entries
        .iter()
        .position(|e| format!("{:?}", e.event).contains("Liliana"))
        .expect("the Liliana line");
    assert_eq!(*entry, liliana, "{:?}", r.end);
}

/// Escape to the Wilds (CR 611.2a, 305.2): alice plays a second land the
/// turn she casts it, from exile or from her hand, which the log never
/// says. Bob's Duress then shows her hand: `kept` lands of it.
fn escape_to_the_wilds(kept: usize) -> String {
    let hand = vec!["[Mountain]"; kept].join(", ");
    format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice draws a card.
        alice plays [Forest].
        Turn 4: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 5: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Escape to the Wilds].
        alice plays [Forest].
        Turn 5: bob
        bob draws a card.
        bob casts [Duress] targeting alice.
        alice reveals {kept} cards with [Duress]: {hand}.
        alice has conceded from the game.
    "
    )
}

/// The second land comes from exile first: five cards are left in her
/// hand, as Duress shows.
#[test]
fn escape_to_the_wilds_s_exiled_land_is_played_first() {
    let (_, r) = run(&escape_to_the_wilds(5));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert_eq!(r.game.unwrap().exile().len(), 4);
}

/// Duress shows four cards, one fewer than the exiled land leaves: the
/// replay goes back and takes the land from her hand.
#[test]
fn escape_to_the_wilds_s_land_from_hand_is_the_way_back() {
    let (_, r) = run(&escape_to_the_wilds(4));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(
        r.trace
            .iter()
            .any(|t| t.did == "took Forest from their hand"),
        "{:?}",
        r.trace
    );
    assert_eq!(r.game.unwrap().exile().len(), 5);
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

/// Two Aether Vials trigger at alice's upkeep, and the log shows the
/// first go on the stack: bob conceding then leaves the second unshown,
/// put on after it, while a game going on would have shown it: the
/// second is a trigger the log doesn't have, the stop a lone one gets.
fn two_vials(then: &str) -> String {
    let vial = "alice puts triggered ability from [Aether Vial] onto the stack (At the beginning of your upkeep, you may put a charge counter on Aether Vial.).";
    format!(
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
        alice chooses not to use [Aether Vial]'s ability.
        alice draws a card.
        alice plays [Mountain].
        alice casts [Aether Vial].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        {vial}
        {then}
    "
    )
}

#[test]
fn triggers_a_concession_leaves_unshown_go_on_the_stack_after_those_shown() {
    let (_, r) = run(&two_vials("bob has conceded from the game."));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let (_, r) = run(&two_vials(
        "alice draws a card.\n        bob has conceded from the game.",
    ));
    let End::Stopped { stop, .. } = &r.end else {
        panic!("{:?}", r.end)
    };
    assert!(
        matches!(stop, Stop::Diverged(Divergence::UnloggedTrigger { card }) if card == "Aether Vial"),
        "{stop:?}"
    );
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

/// CR 608.2d, 701.23a: Goblin Matron's "you may search your library for
/// a Goblin card, reveal that card": the line revealing it shows the
/// "may" taken and names the card found, put into the hand.
#[test]
fn a_reveal_line_takes_the_may_of_a_search_that_reveals() {
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
        alice plays [Mountain].
        Turn 2: bob
        bob plays [Forest].
        Turn 3: alice
        alice plays [Mountain].
        alice casts [Goblin Matron].
        alice puts triggered ability from [Goblin Matron] onto the stack (When Goblin Matron enters, you may search your library for a Goblin card, reveal that card, put it...).
        alice reveals [Goblin Piledriver].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert!(game
        .player(ALICE)
        .hand()
        .iter()
        .any(|c| game.card(*c).is_some_and(|t| t.name == "Goblin Piledriver")));
}

/// CR 405.5, 608.2: a "draws a card with [X]'s ability" line is the
/// topmost of X's triggers resolving, and only it: of two Argothian
/// Enchantress triggers with Enchantress's Presence's between them, the
/// first line draws one card, not all three.
#[test]
fn a_draw_line_resolves_one_trigger_of_its_name() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob plays [Island].
        Turn 2: alice
        alice plays [Forest].
        alice casts [Argothian Enchantress].
        Turn 2: bob
        bob plays [Island].
        Turn 3: alice
        alice plays [Forest].
        alice casts [Enchantress's Presence].
        alice puts triggered ability from [Argothian Enchantress] onto the stack (Whenever you cast an enchantment spell, draw a card.).
        alice draws a card with [Argothian Enchantress]'s ability.
        Turn 3: bob
        bob plays [Island].
        Turn 4: alice
        alice plays [Forest].
        alice casts [Argothian Enchantress].
        Turn 4: bob
        bob plays [Island].
        Turn 5: alice
        alice plays [Forest].
        alice casts [Wild Growth] targeting [Forest].
        alice puts triggered ability from [Argothian Enchantress] onto the stack (Whenever you cast an enchantment spell, draw a card.).
        alice puts triggered ability from [Enchantress's Presence] onto the stack (Whenever you cast an enchantment spell, draw a card.).
        alice puts triggered ability from [Argothian Enchantress] onto the stack (Whenever you cast an enchantment spell, draw a card.).
        alice draws a card with [Argothian Enchantress]'s ability.
        alice draws a card with [Enchantress's Presence]'s ability.
        alice draws a card with [Argothian Enchantress]'s ability.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
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

/// The log's "from the graveyard" is flashback's or, for Gravecrawler, its
/// own "You may cast this card from your graveyard as long as you control
/// a Zombie" (CR 601.3, 113.6f): Bolted there, it is cast from there for
/// {B} once another Gravecrawler is on the battlefield, and not before.
fn gravecrawler_bolted(then: &str) -> (log::Game, Replay) {
    run(&format!(
        "
        alice chooses to play first.
        Turn 1: alice
        alice plays [Swamp].
        alice casts [Gravecrawler].
        Turn 1: bob
        bob plays [Mountain].
        bob casts [Lightning Bolt] targeting [Gravecrawler].
        Turn 2: alice
        alice plays [Swamp].
        {then}
        bob has conceded from the game.
    "
    ))
}

#[test]
fn gravecrawler_is_cast_from_the_graveyard_beside_a_zombie() {
    let (_, r) = gravecrawler_bolted(
        "alice casts [Gravecrawler].
        alice casts [Gravecrawler] from the graveyard.",
    );
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.unwrap();
    // The one from the graveyard is on the stack as bob concedes.
    assert_eq!(on_battlefield(&game, "Gravecrawler"), 1);
    assert_eq!(game.stack().len(), 1);
    assert!(game.player(ALICE).graveyard().is_empty());
}

#[test]
fn gravecrawler_isnt_cast_from_the_graveyard_with_no_zombie() {
    let (_, r) = gravecrawler_bolted("alice casts [Gravecrawler] from the graveyard.");
    // The engine never offers it: the log disagrees with the rules.
    assert!(
        matches!(
            &r.end,
            End::Stopped {
                stop: Stop::Diverged(Divergence::NeverOffered { what }),
                ..
            } if what.contains("Gravecrawler")
        ),
        "{:?} {:#?}",
        r.end,
        r.trace
    );
}

/// A graveyard permission the engine can't run is refused by its own
/// node, not filed as a cast by some cost it lacks: Marang River
/// Prowler's "a black or green permanent".
#[test]
fn a_graveyard_permission_the_engine_cant_run_is_refused() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice plays [Swamp].
        alice casts [Marang River Prowler] from the graveyard.
        bob has conceded from the game.
    ");
    let End::Stopped {
        stop: Stop::Refused { card, .. },
        ..
    } = &r.end
    else {
        panic!("{:?}", r.end)
    };
    assert_eq!(card, "Marang River Prowler");
}

/// A card with flashback and its own permission to be cast from its
/// graveyard: "from the graveyard" doesn't say which, and flashback exiles
/// it (CR 702.34a), so the replay stops by name rather than pick one.
#[test]
fn flashback_or_a_permission_is_a_choice_the_log_doesnt_show() {
    let crawler = || {
        let mut c = face(
            "Test Crawler",
            "Creature — Zombie",
            "{B}",
            "Flashback {B}\nYou may cast this card from your graveyard as long as you control a Zombie.",
            &[Color::Black],
        );
        c.pt = Some(pt(1, 1));
        c
    };
    let (_, r) = run_with(
        "
        alice chooses to play first.
        Turn 1: alice
        alice plays [Swamp].
        alice casts [Test Crawler] from the graveyard.
        bob has conceded from the game.
    ",
        &mut |n| match n {
            "Test Crawler" => Some(crawler()),
            n => cards(n),
        },
        &[],
    );
    let End::Stopped {
        stop: Stop::Unfollowed { event, card },
        ..
    } = &r.end
    else {
        panic!("{:?}", r.end)
    };
    assert_eq!(card.as_deref(), Some("Test Crawler"));
    assert!(event.contains("flashback or its own permission"), "{event}");
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

/// Accumulated Knowledge's {1} paid by City of Brass would trigger it
/// beside Quirion Dryad, and the log shows Dryad's trigger alone: City's
/// is one it never shows, so the replay goes back and pays with Mox
/// Diamond, as it does for a lone City of Brass trigger the log doesn't
/// show.
#[test]
fn a_trigger_ordered_beside_one_the_log_shows_but_never_shown_is_paid_otherwise() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        Turn 3: bob
        bob draws a card.
        bob plays [Forest].
        Turn 4: alice
        alice draws a card.
        alice plays [City of Brass].
        alice casts [Quirion Dryad].
        alice casts [Mox Diamond].
        alice discards [Forest].
        alice casts [Accumulated Knowledge].
        alice puts triggered ability from [Quirion Dryad] onto the stack (Whenever you cast a spell that's white, blue, black, or red, put a +1/+1 counter on Quirion Dryad.).
        alice puts a +1/+1 counter on [Quirion Dryad].
        alice draws a card with [Accumulated Knowledge].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(game.player(ALICE).life(), 20, "City of Brass never tapped");
}

/// CR 704.5f, 608.2h: Plague Belcher's two -1/-1 counters leave Dread
/// Wanderer dead before anyone has priority; it had them as it last
/// existed, so the counters line is followed there, and Belcher's trigger
/// on its dying is the next line's.
#[test]
fn counters_that_kill_the_creature_are_on_it_as_it_last_existed() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Swamp].
        alice casts [Dread Wanderer].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Swamp].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        alice plays [Swamp].
        alice casts [Plague Belcher].
        alice puts a triggered ability from [Plague Belcher] onto the stack (When this creature enters, put two -1/-1 counters on target creature you control.) targeting [Dread Wanderer].
        alice puts two -1/-1 counters on [Dread Wanderer].
        alice puts a triggered ability from [Plague Belcher] onto the stack (Whenever another Zombie you control dies, each opponent loses 1 life.).
        Turn 3: bob
        bob draws a card.
        alice has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.unwrap();
    assert_eq!(game.player(BOB).life(), 19);
}

/// CR 601.2b, 702.33d: "casts [Burst Lightning] with kicker" pays its
/// kicker, all five Mountains, and the plain line after doesn't: 4
/// damage, then 2.
#[test]
fn a_cast_with_kicker_pays_it_and_one_without_does_not() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 3: bob
        bob draws a card.
        bob plays [Forest].
        Turn 4: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 4: bob
        bob draws a card.
        bob plays [Forest].
        Turn 5: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Burst Lightning] with kicker targeting bob.
        Turn 5: bob
        bob draws a card.
        bob plays [Forest].
        Turn 6: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Burst Lightning] targeting bob.
        Turn 6: bob
        bob draws a card.
        bob plays [Forest].
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 20 - 4 - 2);
}

/// CR 104.2a, 704.5a: the log stops printing as the game ends, so the
/// seventh Lightning Bolt is never shown resolving; the winner line
/// resolves it, and bob at 0 life loses.
#[test]
fn a_winner_line_resolves_what_ended_the_game() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice casts [Lightning Bolt] targeting bob.
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Lightning Bolt] targeting bob.
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Lightning Bolt] targeting bob.
        Turn 3: bob
        bob draws a card.
        bob plays [Forest].
        Turn 4: alice
        alice draws a card.
        alice casts [Lightning Bolt] targeting bob.
        Turn 4: bob
        bob draws a card.
        bob plays [Forest].
        Turn 5: alice
        alice draws a card.
        alice casts [Lightning Bolt] targeting bob.
        Turn 5: bob
        bob draws a card.
        bob plays [Forest].
        Turn 6: alice
        alice draws a card.
        alice casts [Lightning Bolt] targeting bob.
        Turn 6: bob
        bob draws a card.
        bob plays [Forest].
        Turn 7: alice
        alice draws a card.
        alice casts [Lightning Bolt] targeting bob.
        alice wins the game.
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert_eq!(r.game.expect("set up").player(BOB).life(), 20 - 7 * 3);
}

/// CR 118.9, 601.2g–h: a Gush whose return line comes before its cast
/// line, which says nothing of the cost: cast for the alternative cost,
/// the two Islands tapped for mana first, which pays for the Coral Eel
/// cast after it.
#[test]
fn gush_returns_the_islands_its_line_names_after_tapping_them_for_mana() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        alice returns [Island] and [Island] to its owner's hand with with [Gush]'s ability.
        alice casts [Gush].
        alice draws two cards with [Gush].
        alice casts [Coral Eel].
        alice discards [Island].
        Turn 2: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Island"), 0);
    assert_eq!(on_battlefield(&game, "Coral Eel"), 1);
    // Both back in hand, one discarded to hand size.
    let islands = |cards: &imbl::Vector<chancellor_of_the_spires::game::CardId>| {
        cards
            .iter()
            .filter(|c| game.card(**c).is_some_and(|t| t.name == "Island"))
            .count()
    };
    assert_eq!(islands(game.player(ALICE).hand()), 1);
    assert_eq!(islands(game.player(ALICE).graveyard()), 1);
}

/// CR 702.37c, 708.9, 702.37e: the log hides the card cast face down and
/// names it later. The first is named by the line revealing it as Lightning
/// Bolt kills it; the second, never revealed, by the Exalted Angel that
/// attacks once a line has turned it face up, whose damage trigger the log
/// shows.
#[test]
fn a_card_cast_face_down_is_the_one_a_later_line_names() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Plains].
        Turn 1: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 2: alice
        alice draws a card.
        alice plays [Plains].
        Turn 2: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 3: alice
        alice draws a card.
        alice plays [Plains].
        alice casts a card face down.
        Turn 3: bob
        bob draws a card.
        bob plays [Mountain].
        bob casts [Lightning Bolt] targeting face-down creature #1.
        alice reveals face-down creature #1 as Exalted Angel.
        Turn 4: alice
        alice draws a card.
        alice plays [Plains].
        alice casts a card face down.
        Turn 4: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 5: alice
        alice draws a card.
        alice plays [Plains].
        alice turns face-down creature #2 face up.
        bob is being attacked by [Exalted Angel]
        alice puts triggered ability from [Exalted Angel] onto the stack (Whenever Exalted Angel deals damage, you gain that much life.).
        Turn 5: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Exalted Angel"), 1);
    let angels = game
        .player(ALICE)
        .graveyard()
        .iter()
        .filter(|c| game.card(**c).is_some_and(|t| t.name == "Exalted Angel"))
        .count();
    assert_eq!(angels, 1);
    assert_eq!(game.player(BOB).life(), 16);
    assert_eq!(game.player(ALICE).life(), 24);
}

/// CR 118.12a, 614.10: Solitary Confinement's upkeep trigger is paid with
/// the card the next line discards, and its controller's draw step is
/// skipped as the log says; with no discard line it is sacrificed, and
/// the draw step that follows draws.
#[test]
fn solitary_confinement_is_kept_by_the_discard_line_and_skips_the_draw() {
    let trigger = "puts triggered ability from [Solitary Confinement] onto the stack \
        (At the beginning of your upkeep, sacrifice Solitary Confinement unless you discard a card.).";
    let (_, r) = run(&format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Plains].
        Turn 1: bob
        bob plays [Forest].
        Turn 2: alice
        alice plays [Plains].
        Turn 2: bob
        bob plays [Forest].
        Turn 3: alice
        alice plays [Plains].
        alice casts [Solitary Confinement].
        Turn 3: bob
        bob plays [Forest].
        Turn 4: alice
        alice {trigger}
        alice discards [Forest].
        alice skips their draw step.
        Turn 4: bob
        bob plays [Forest].
        Turn 5: alice
        alice {trigger}
        alice draws a card.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    "
    ));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(r
        .trace
        .iter()
        .any(|t| t.did.contains("as a permanent says (CR 614.10)")));
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Solitary Confinement"), 0);
    let graveyard: Vec<String> = game
        .player(ALICE)
        .graveyard()
        .iter()
        .map(|c| game.card(*c).unwrap().name.clone())
        .collect();
    assert_eq!(graveyard, ["Forest", "Solitary Confinement"]);
}

/// CR 712.12, 715.3, 715.3d: a log names the face of a two-faced card it
/// shows. Pelakka Caverns is played by its land face; Steam Clean is cast
/// as an Adventure, bounces the Bears and goes to exile, and the Scalding
/// Viper a later line casts is that card, cast from exile.
#[test]
fn a_log_names_the_face_played_or_cast_and_an_adventure_is_cast_from_exile() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Pelakka Caverns].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        bob casts [Grizzly Bears].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Steam Clean] targeting [Grizzly Bears].
        alice returns [Grizzly Bears] to its owner's hand with [Steam Clean].
        Turn 3: bob
        bob draws a card.
        bob plays [Forest].
        bob casts [Grizzly Bears].
        Turn 4: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Scalding Viper].
        Turn 4: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(
        on_battlefield(&game, "Pelakka Predation // Pelakka Caverns"),
        1
    );
    assert_eq!(on_battlefield(&game, "Scalding Viper // Steam Clean"), 1);
    assert!(game.exile().is_empty());
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 1);
}

/// CR 701.20a, 701.27a, 701.27f: Delver of Secrets's upkeep trigger,
/// declined where no line reveals the card, and taken where "reveals
/// [Lightning Bolt]." names the top card; the "transforms into" line is
/// the engine's transform, and the attack after it is by Insectile
/// Aberration, the same permanent.
#[test]
fn delver_of_secrets_reveals_the_card_its_line_names_and_transforms() {
    let trigger = "puts triggered ability from [Delver of Secrets] onto the stack \
        (At the beginning of your upkeep, look at the top card of your library. You may reveal that card. I...).";
    let (_, r) = run(&format!(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        alice casts [Delver of Secrets].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice {trigger}
        alice draws a card.
        alice plays [Mountain].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice {trigger}
        alice reveals [Lightning Bolt].
        [Delver of Secrets] transforms into [Insectile Aberration].
        alice draws a card.
        bob is being attacked by [Insectile Aberration]
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    "
    ));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 17);
    let (_, delver) = game
        .permanents()
        .iter()
        .find(|(_, p)| game.face_name(p.card) == Some("Insectile Aberration"))
        .expect("transformed");
    assert_eq!(
        game.side_up(delver.card),
        chancellor_of_the_spires::deck::Side::Back
    );
}

/// CR 707.5: the log never says what Vesuva copied. A copy of alice's
/// Forest, tried first since no line names either land again, pays for
/// the first Bronze Sable but leaves three mana for the next two; the
/// replay goes back and has it enter as a copy of Cloudpost, each of the
/// two Cloudposts then adding {C}{C}.
#[test]
fn vesuva_copies_what_a_later_spell_needs_it_to_have_copied() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Cloudpost].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice plays [Vesuva].
        alice casts [Bronze Sable].
        Turn 3: bob
        bob draws a card.
        bob plays [Island].
        Turn 4: alice
        alice draws a card.
        alice casts [Bronze Sable].
        alice casts [Bronze Sable].
        Turn 4: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(
        r.trace
            .iter()
            .any(|t| t.did == "chose again: Vesuva entered as a copy of Cloudpost (CR 707.5)"),
        "{:?}",
        r.trace
    );
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Cloudpost"), 2);
    assert_eq!(on_battlefield(&game, "Bronze Sable"), 3);
}

/// CR 118.9, 601.2g–h, 602.2b: Fireblast cast by sacrificing alice's two
/// Mountains, tapped for mana first since the log never shows them
/// tapped, and Grim Lavamancer activated by exiling those Mountains from
/// her graveyard, its {R} that mana. Magic Online names bob as the one
/// exiling; the activation's line says it is alice's.
#[test]
fn grim_lavamancer_exiles_the_cards_its_line_names_and_spends_what_fireblast_floated() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice casts [Grim Lavamancer].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Fireblast] targeting bob.
        bob exiles [Mountain] and [Mountain] with with [Grim Lavamancer]'s ability.
        alice activates an ability of [Grim Lavamancer] targeting bob ( Grim Lavamancer deals 2 damage to any target.).
        Turn 2: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 20 - 4 - 2);
    assert!(game
        .player(ALICE)
        .graveyard()
        .iter()
        .all(|c| game.card(*c).is_some_and(|t| t.name != "Mountain")));
    assert_eq!(on_battlefield(&game, "Mountain"), 0);
}

/// CR 602.2b, 701.9a: "discards [Llanowar Elves]." then Survival of the
/// Fittest's activation: the discard is its cost, the card the line names.
#[test]
fn survival_of_the_fittest_discards_the_card_the_line_before_names() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Survival of the Fittest].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice discards [Llanowar Elves].
        alice activates an ability of [Survival of the Fittest] ( Search your library for a creature card, reveal that card, put it into your hand, then shuffle.).
        alice reveals [Wall of Roots].
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    let named = |cards: &imbl::Vector<chancellor_of_the_spires::game::CardId>, n: &str| {
        cards
            .iter()
            .filter(|c| game.card(**c).is_some_and(|t| t.name == n))
            .count()
    };
    assert_eq!(named(game.player(ALICE).graveyard(), "Llanowar Elves"), 1);
    assert_eq!(named(game.player(ALICE).hand(), "Wall of Roots"), 1);
}

/// CR 117.7, 602.2: Rishadan Port's ability targets alice's Mountain in
/// her upkeep, and she answers it: Wooded Foothills finds a Mountain,
/// Lightning Bolt is cast with it, and Grim Lavamancer, exiling Wooded
/// Foothills and Bolt once Bolt has resolved, is paid by the first
/// Mountain before the Port taps it. A land the log never names
/// (Stomping Ground, which can enter untapped) is tried only once the
/// replay has tried answering the Port.
#[test]
fn grim_lavamancer_answers_rishadan_port_before_a_land_the_log_never_names() {
    let unseen = [UnseenLand {
        name: "Stomping Ground".into(),
        makes: [false, false, false, true, true],
    }];
    let (_, r) = run_with(
        "
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice casts [Grim Lavamancer].
        Turn 1: bob
        bob draws a card.
        bob plays [Rishadan Port].
        Turn 2: alice
        alice draws a card.
        alice plays [Wooded Foothills].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        bob activates an ability of [Rishadan Port] targeting [Mountain] ( Tap target land.).
        alice activates an ability of [Wooded Foothills] ( Search your library for a Mountain or Forest card, put it onto the battlefield, then shuffle.).
        alice casts [Lightning Bolt] targeting bob.
        alice exiles [Wooded Foothills] and [Lightning Bolt] with with [Grim Lavamancer]'s ability.
        alice activates an ability of [Grim Lavamancer] targeting bob ( Grim Lavamancer deals 2 damage to any target.).
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
    ",
        &mut cards,
        &unseen,
    );
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    assert!(r.unseen.is_empty(), "{:?}", r.unseen);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 20 - 3 - 2);
    assert_eq!(on_battlefield(&game, "Mountain"), 2);
}

/// Psychatog has two abilities the log prints alike, "Psychatog gets
/// +1/+1 until end of turn."; the one activated is the one whose cost the
/// line before shows paid, an exile from the graveyard or a discard.
#[test]
fn psychatog_s_ability_is_the_one_whose_cost_the_line_before_paid() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Swamp].
        Turn 1: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Psychatog].
        Turn 3: bob
        bob draws a card.
        bob plays [Mountain].
        bob casts [Lightning Bolt] targeting alice.
        bob casts [Lightning Bolt] targeting alice.
        Turn 4: alice
        alice draws a card.
        alice discards [Grizzly Bears].
        alice activates an ability of [Psychatog] ( Psychatog gets +1/+1 until end of turn.).
        alice discards [Island].
        alice activates an ability of [Psychatog] ( Psychatog gets +1/+1 until end of turn.).
        alice exiles [Island] and [Grizzly Bears] with with [Psychatog]'s ability.
        alice activates an ability of [Psychatog] ( Psychatog gets +1/+1 until end of turn.).
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.expect("set up");
    // The Bears and the Island discarded, then exiled.
    assert!(game.player(ALICE).graveyard().is_empty());
}

/// CR 702.62a, 116.2f: Rift Bolt suspended for {R}, its time counter put
/// on it, then at alice's next upkeep its trigger removes the counter and
/// the last one's trigger casts it, its target the line's; Search for
/// Tomorrow's two counters come off over two upkeeps, the second casting
/// it, its "without paying its mana cost with suspend" said or not.
#[test]
fn suspended_cards_lose_a_time_counter_each_upkeep_and_are_cast_as_the_last_goes() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        alice exiles [Rift Bolt] with 1 time counter.
        alice puts a time counter on [Rift Bolt].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice puts triggered ability from [Rift Bolt] onto the stack (Suspend 1{R} (At the beginning of your upkeep, remove a time counter.)).
        alice removes a time counter from [Rift Bolt].
        alice puts triggered ability from [Rift Bolt] onto the stack (Suspend 1{R} (When the last time counter is removed from the suspended card, you may cast it w...).
        alice casts [Rift Bolt] targeting bob.
        alice draws a card.
        alice plays [Forest].
        alice exiles [Search for Tomorrow] with 2 time counters.
        alice puts two time counters on [Search for Tomorrow].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice puts triggered ability from [Search for Tomorrow] onto the stack (Suspend 2{G} (At the beginning of your upkeep, remove a time counter.)).
        alice removes a time counter from [Search for Tomorrow].
        alice draws a card.
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice puts triggered ability from [Search for Tomorrow] onto the stack (Suspend 2{G} (At the beginning of your upkeep, remove a time counter.)).
        alice removes a time counter from [Search for Tomorrow].
        alice puts triggered ability from [Search for Tomorrow] onto the stack (Suspend 2{G} (When the last time counter is removed from the suspended card, you may cast it w...).
        alice casts [Search for Tomorrow] without paying its mana cost with suspend.
        alice draws a card.
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 17);
    assert!(game.suspended().is_empty());
    // Mountain, Forest and the basic Search for Tomorrow found.
    assert_eq!(game.permanents().len(), 3 + 3);
}

/// CR 701.6a, 113.1c: "casts [Stifle] targeting Triggered ability from
/// Phyrexian Dreadnought." aims at the trigger on the stack, which is
/// countered, so the Dreadnought stays without anything sacrificed.
#[test]
fn stifle_counters_the_trigger_its_line_names() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Phyrexian Dreadnought].
        alice puts triggered ability from [Phyrexian Dreadnought] onto the stack (When Phyrexian Dreadnought enters the battlefield, sacrifice it unless you sacrifice any number of...).
        alice casts [Stifle] targeting Triggered ability from Phyrexian Dreadnought.
        Turn 2: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Phyrexian Dreadnought"), 1);
}

/// CR 602.2b: "removes a +1/+1 counter from [Triskelion]." is the cost of
/// the activation the next line shows, taken off as it is paid.
#[test]
fn triskelion_removes_the_counter_its_activation_line_pays() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 4: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 5: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 5: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 6: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Triskelion].
        alice removes a +1/+1 counter from [Triskelion].
        alice activates an ability of [Triskelion] targeting bob ( Triskelion deals 1 damage to any target.).
        Turn 6: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 19);
}

/// CR 702.193a, 122.1b: Quicksilver cast and powered up for {4} the turn
/// it entered, after it is declared as an attacker, and "puts a +1/+1 and
/// a double strike counter on [Quicksilver, Brash Blur]." is its ability
/// resolving: double strike, 2/2, deals 4.
#[test]
fn quicksilver_powers_up_in_combat_and_strikes_twice() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Mountain].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice draws a card.
        alice plays [Mountain].
        Turn 4: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 5: alice
        alice draws a card.
        alice plays [Mountain].
        alice casts [Quicksilver, Brash Blur].
        bob is being attacked by [Quicksilver, Brash Blur]
        alice activates an ability of [Quicksilver, Brash Blur] (Put a +1/+1 counter and a double strike counter on Quicksilver.)
        alice puts a +1/+1 and a double strike counter on [Quicksilver, Brash Blur].
        Turn 5: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(game.player(BOB).life(), 16);
}

/// CR 702.170a, 116.2k: "[Outcaster Trailblazer] becomes plotted." names
/// nobody, so it is the turn's player's, plot being a special action only
/// in its owner's main phase; a later turn's "casts [Outcaster
/// Trailblazer] without paying its mana cost." casts it from exile (CR
/// 702.170d).
#[test]
fn a_plotted_card_is_the_turns_players_and_cast_free_later() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Forest].
        [Outcaster Trailblazer] becomes plotted.
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice draws a card.
        alice casts [Outcaster Trailblazer] without paying its mana cost.
        alice puts a triggered ability from [Outcaster Trailblazer] onto the stack (When this creature enters, add one mana of any color.).
        alice plays [Forest].
        bob has conceded from the game.
    ");
    assert_eq!(r.end, End::Finished, "{:#?}", r.trace);
    let game = r.game.expect("set up");
    assert!(game.plotted().next().is_none());
    assert_eq!(game.permanents().len(), 4 + 3 + 1);
}

/// Fact or Fiction's five revealed cards and its piles, which no log
/// prints: the five stay hidden, and the replay tries each size of the
/// pile alice takes, two first. The cards it leaves in her graveyard no one
/// has named, and the replay presumes them to have no ability that works
/// there; taking all five, or none, leaves a hand Duress reveals (CR
/// 701.20a) the wrong size. Accumulated Knowledge then counts the cards of
/// its name in all graveyards, which it can't with hidden ones there, and
/// the engine refuses it rather than count them as none.
#[test]
fn accumulated_knowledge_refuses_to_count_a_graveyard_holding_fact_or_fictions_hidden_pile() {
    let (g, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Fact or Fiction].
        Turn 4: bob
        bob draws a card.
        bob casts [Duress] targeting alice.
        alice reveals 6 cards with [Duress]: [Island], [Island], [Accumulated Knowledge], [Impulse], [Island], [Island].
        alice discards [Impulse].
        Turn 5: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Accumulated Knowledge].
        alice draws a card with [Accumulated Knowledge].
        alice has conceded from the game.
    ");
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert!(
        matches!(stop, Stop::Refused { node, .. } if node == "Amount::CardsInGraveyards"),
        "{stop:?} at {}",
        g.entries[*entry].text
    );
    assert_eq!(
        g.entries[*entry].text,
        "alice draws a card with [Accumulated Knowledge]."
    );
    assert!(r.presumed > 0);
}

/// Fact or Fiction cast at the end of bob's turn, its piles unprinted. A
/// pile of fewer than five leaves hidden cards in alice's graveyard, and
/// Accumulated Knowledge counting them is refused; taking all five gets
/// past it, to a cleanup discarding 3 where the log discards 1. The
/// refusal may be the game's, so it stops the replay, but every way past
/// it diverged, and that divergence is reported beside it (`End::Masked`),
/// never hidden behind it.
#[test]
fn a_divergence_every_way_past_a_refusal_met_is_reported_beside_it() {
    let (g, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice draws a card.
        alice plays [Island].
        Turn 4: bob
        bob draws a card.
        bob plays [Swamp].
        alice casts [Fact or Fiction].
        Turn 5: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Accumulated Knowledge].
        alice draws a card with [Accumulated Knowledge].
        alice discards [Island].
        Turn 5: bob
        bob draws a card.
        bob has conceded from the game.
    ");
    let End::Masked {
        entry,
        stop,
        diverged,
    } = &r.end
    else {
        panic!("{:?}", r.end)
    };
    assert!(
        matches!(stop, Stop::Refused { node, .. } if node == "Amount::CardsInGraveyards"),
        "{stop:?}"
    );
    assert_eq!(
        g.entries[*entry].text,
        "alice draws a card with [Accumulated Knowledge]."
    );
    assert_eq!(g.entries[diverged.entry].text, "alice discards [Island].");
    assert_eq!(
        diverged.divergence,
        Divergence::Discards { log: 1, engine: 3 }
    );
}

/// CR 701.21a: Diabolic Edict's "Target player sacrifices a creature of
/// their choice" is answered by the log's "sacrifices" line, and one
/// naming a permanent the player can't sacrifice to it stops there.
#[test]
fn an_edict_sacrifices_what_the_log_says() {
    let log = |sacrificed: &str| {
        format!(
            "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Grizzly Bears].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        bob casts [Diabolic Edict] targeting alice.
        alice sacrifices [{sacrificed}].
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    "
        )
    };
    let (_, r) = run(&log("Grizzly Bears"));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 0);
    assert_eq!(on_battlefield(&game, "Forest"), 2);
    // A land isn't a creature.
    let (g, r) = run(&log("Forest"));
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert_eq!(g.entries[*entry].text, "alice sacrifices [Forest].");
    assert_eq!(
        *stop,
        Stop::Diverged(Divergence::Sacrificed {
            card: "Forest".into()
        })
    );
}

/// CR 701.21a, 302.6: of three Llanowar Elves the log can't say which
/// one Diabolic Edict took, and the one a later spell needs for mana, the
/// only one not cast this turn, is kept.
#[test]
fn sacrifices_of_one_name_go_back_to_another() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        alice casts [Llanowar Elves].
        bob casts [Diabolic Edict] targeting alice.
        alice sacrifices [Llanowar Elves].
        alice casts [Grizzly Bears].
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Llanowar Elves"), 2);
    assert!(r
        .trace
        .iter()
        .any(|t| t.did.contains("sacrificed others of the same names")));
    assert_eq!(
        game.stack().len(),
        1,
        "the Bears, paid for with the older Elves"
    );
}

/// CR 701.21a: the other seat sacrificing: alice's Diabolic Edict has bob
/// sacrifice the Grizzly Bears the log names.
#[test]
fn an_edict_has_the_opponent_of_its_caster_sacrifice_too() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Swamp].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Swamp].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        bob casts [Grizzly Bears].
        Turn 3: alice
        alice draws a card.
        alice casts [Diabolic Edict] targeting bob.
        bob sacrifices [Grizzly Bears].
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 0);
    assert_eq!(on_battlefield(&game, "Forest"), 2);
}

/// An instruction's sacrifice the log never shows is a stop, never
/// answered as a cost's would be, by the replay's own ranking.
#[test]
fn an_edict_the_log_shows_no_sacrifice_for_stops() {
    let (g, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Grizzly Bears].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        bob casts [Diabolic Edict] targeting alice.
        Turn 3: alice
        alice draws a card.
    ");
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert_eq!(g.entries[*entry].text, "Turn 3: alice");
    assert_eq!(
        *stop,
        Stop::Diverged(Divergence::Unlogged {
            what: "a sacrifice".into()
        })
    );
}

/// CR 702.74a, 113.6k: "casts [Nulldrifter] by paying {2U} with evoke." is
/// cast for its evoke cost; its cast trigger draws two, and its evoke
/// trigger sacrifices it as it enters.
#[test]
fn a_spell_cast_with_evoke_is_sacrificed_as_it_enters() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Nulldrifter] by paying {2U} with evoke.
        alice puts a triggered ability from [Nulldrifter] onto the stack (When you cast this spell, draw two cards.).
        alice draws two cards with [Nulldrifter].
        alice puts a triggered ability from [Nulldrifter] onto the stack (Evoke {2U}).
        Turn 3: bob
        bob draws a card.
        bob plays [Forest].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Nulldrifter"), 0);
    assert_eq!(
        game.player(ALICE)
            .graveyard()
            .iter()
            .filter(|c| game.card(**c).is_some_and(|t| t.name == "Nulldrifter"))
            .count(),
        1
    );
    // Seven, a draw on each of turns 2 and 3, two drawn, less three
    // Islands and Nulldrifter.
    assert_eq!(game.player(ALICE).hand().len(), 7 + 2 + 2 - 3 - 1);
}

/// The other seat evoking, and a cast line that says nothing of how: bob's
/// evoked Nulldrifter is sacrificed, and alice's plain "casts
/// [Nulldrifter]." off three Islands is its mana cost she can't pay, never
/// taken for an evoke the log would have named (CR 702.74a).
#[test]
fn an_evoke_cast_is_only_one_the_log_names() {
    let lines = |alice_casts: bool| {
        let alice = if alice_casts {
            "alice casts [Nulldrifter]."
        } else {
            ""
        };
        format!(
            "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Island].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice plays [Island].
        {alice}
        Turn 3: bob
        bob draws a card.
        bob plays [Island].
        bob casts [Nulldrifter] by paying {{2U}} with evoke.
        bob puts a triggered ability from [Nulldrifter] onto the stack (When you cast this spell, draw two cards.).
        bob draws two cards with [Nulldrifter].
        bob puts a triggered ability from [Nulldrifter] onto the stack (Evoke {{2U}}).
        bob discards [Island].
        Turn 4: alice
        alice draws a card.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    "
        )
    };
    let (_, r) = run(&lines(false));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Nulldrifter"), 0);
    assert_eq!(
        game.player(BOB).graveyard().len(),
        2,
        "Nulldrifter and the Island"
    );
    let (g, r) = run(&lines(true));
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert_eq!(g.entries[*entry].text, "alice casts [Nulldrifter].");
    assert_eq!(
        *stop,
        Stop::Diverged(Divergence::Unpayable {
            card: "Nulldrifter".into()
        })
    );
}

/// CR 708.2: a card cast face down that no line ever names stays hidden,
/// and the face-down spell has no abilities, so nothing reads its text
/// for a trigger on its own casting.
#[test]
fn a_card_cast_face_down_and_never_named_stays_hidden() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Plains].
        Turn 1: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 2: alice
        alice draws a card.
        alice plays [Plains].
        Turn 2: bob
        bob draws a card.
        bob plays [Mountain].
        Turn 3: alice
        alice draws a card.
        alice plays [Plains].
        alice casts a card face down.
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    let down: Vec<_> = game.permanents().values().filter(|p| p.face_down).collect();
    assert_eq!(down.len(), 1);
    assert!(game.card(down[0].card).is_none(), "never named");
}

fn named_in(
    game: &chancellor_of_the_spires::game::Game,
    cards: &imbl::Vector<chancellor_of_the_spires::game::CardId>,
    name: &str,
) -> usize {
    cards
        .iter()
        .filter(|c| game.card(**c).is_some_and(|t| t.name == name))
        .count()
}

/// The lines of a game where alice's Careful Study discards Basking
/// Rootwalla and an Island, madness exiles the Rootwalla, and `then`
/// follows its trigger.
fn careful_study(then: &str) -> String {
    format!(
        "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        alice casts [Careful Study].
        alice draws two cards with [Careful Study].
        alice discards [Basking Rootwalla].
        alice discards [Island].
        alice puts triggered ability from [Basking Rootwalla] onto the stack (Madness {{0}}).
        {then}
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    "
    )
}

/// CR 702.35a, 608.2g: a discarded Basking Rootwalla is exiled instead,
/// and its trigger resolving, the cast line after it casts the Rootwalla
/// for its madness cost from exile.
#[test]
fn a_card_discarded_with_madness_is_cast_from_exile_by_its_trigger() {
    let (_, r) = run(&careful_study("alice casts [Basking Rootwalla]."));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Basking Rootwalla"), 1);
    let alice = game.player(ALICE);
    assert_eq!(named_in(&game, alice.graveyard(), "Basking Rootwalla"), 0);
    assert_eq!(named_in(&game, alice.graveyard(), "Island"), 1);
    assert_eq!(named_in(&game, alice.graveyard(), "Careful Study"), 1);
    assert!(game.exile().is_empty(), "{:?}", game.exile());
}

/// CR 702.35a: with no line casting it, the Rootwalla madness exiled goes
/// to its owner's graveyard as the trigger resolves.
#[test]
fn a_card_madness_exiled_and_not_cast_goes_to_the_graveyard() {
    let (_, r) = run(&careful_study(""));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Basking Rootwalla"), 0);
    let alice = game.player(ALICE);
    assert_eq!(named_in(&game, alice.graveyard(), "Basking Rootwalla"), 1);
    assert!(game.exile().is_empty(), "{:?}", game.exile());
}

/// CR 702.35a, 608.2g, 603.3b: two madness cards discarded at once, their
/// triggers put on the stack in the log's order: the Rootwalla's, on top,
/// casts it for {0}; it resolves, and Arrogant Wurm's trigger casts the
/// Wurm for {2G}, its mana paid as that trigger resolves.
#[test]
fn two_madness_triggers_cast_each_card_as_each_resolves() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        Turn 3: alice
        alice draws a card.
        alice plays [Forest].
        Turn 3: bob
        bob draws a card.
        bob plays [Forest].
        Turn 4: alice
        alice draws a card.
        alice plays [Island].
        alice casts [Careful Study].
        alice draws two cards with [Careful Study].
        alice discards [Arrogant Wurm].
        alice discards [Basking Rootwalla].
        alice puts triggered ability from [Arrogant Wurm] onto the stack (Madness {2G}).
        alice puts triggered ability from [Basking Rootwalla] onto the stack (Madness {0}).
        alice casts [Basking Rootwalla].
        alice casts [Arrogant Wurm].
        Turn 4: bob
        bob draws a card.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Basking Rootwalla"), 1);
    assert_eq!(on_battlefield(&game, "Arrogant Wurm"), 1);
    assert!(game.exile().is_empty(), "{:?}", game.exile());
}

/// Alice's Commune with Dinosaurs, then `then`, and a Forest played next
/// turn.
fn commune(then: &str) -> String {
    format!(
        "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        alice casts [Commune with Dinosaurs].
        {then}
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    "
    )
}

/// CR 608.2d, 701.20a: the log's "reveals [Forest] with [Commune with
/// Dinosaurs]." names the card chosen from among the five looked at, a
/// hidden card, which goes to the hand; with no such line none is chosen,
/// and all five go to the bottom.
#[test]
fn a_card_revealed_from_among_those_looked_at_is_the_one_chosen() {
    let (_, r) = run(&commune(
        "alice reveals [Forest] with [Commune with Dinosaurs].",
    ));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(
        r.trace
            .iter()
            .any(|t| t.did == "chose Forest from among the cards Commune with Dinosaurs looked at"),
        "{:?}",
        r.trace
    );
    let game = r.game.expect("set up");
    let library = game.player(ALICE).library();
    let hidden_bottom = library
        .iter()
        .skip(library.len() - 4)
        .filter(|c| game.card(**c).is_none())
        .count();
    assert_eq!(
        hidden_bottom, 4,
        "the other four, still hidden, on the bottom"
    );

    let (_, r) = run(&commune(""));
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(
        r.trace.iter().any(|t| t.did
            == "chose none of the cards Commune with Dinosaurs looked at, which no line reveals"),
        "{:?}",
        r.trace
    );
}

/// Two Communes in one turn, the first revealing nothing: the reveal line
/// after the second cast is the second's, so the first chooses none.
#[test]
fn a_reveal_line_belongs_to_the_last_cast_of_its_card_before_it() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Commune with Dinosaurs].
        alice casts [Commune with Dinosaurs].
        alice reveals [Forest] with [Commune with Dinosaurs].
        Turn 2: bob
        bob draws a card.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let did: Vec<(usize, &str)> = r
        .trace
        .iter()
        .filter(|t| t.did.contains("Commune with Dinosaurs looked at"))
        .map(|t| (t.entry, t.did.as_str()))
        .collect();
    assert_eq!(
        did.iter().map(|(_, d)| *d).collect::<Vec<_>>(),
        vec![
            "chose none of the cards Commune with Dinosaurs looked at, which no line reveals",
            "chose Forest from among the cards Commune with Dinosaurs looked at",
        ],
        "{:?}",
        r.trace
    );
}

/// A reveal line naming a card the filter doesn't match is the log's
/// choice and not one the engine offers: a divergence, by name.
#[test]
fn a_revealed_card_the_filter_doesnt_match_diverges() {
    let (g, r) = run(&commune(
        "alice reveals [Grizzly Bears] with [Commune with Dinosaurs].",
    ));
    let End::Stopped { entry, stop } = &r.end else {
        panic!("{:?}", r.trace)
    };
    assert_eq!(
        g.entries[*entry].text,
        "alice reveals [Grizzly Bears] with [Commune with Dinosaurs]."
    );
    assert_eq!(
        *stop,
        Stop::Diverged(Divergence::NeverOffered {
            what: "Grizzly Bears from among the cards Commune with Dinosaurs looked at".into()
        })
    );
}

/// CR 113.6b, 109.5: Wonder discarded to Careful Study gives alice's
/// creatures flying from her graveyard while she controls an Island, and
/// bob's none.
#[test]
fn wonder_in_a_graveyard_gives_its_owners_creatures_flying() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Island].
        alice casts [Careful Study].
        alice draws two cards with [Careful Study].
        alice discards [Wonder].
        alice discards [Island].
        Turn 1: bob
        bob draws a card.
        bob plays [Forest].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 2: bob
        bob draws a card.
        bob plays [Forest].
        bob casts [Grizzly Bears].
        Turn 3: alice
        alice draws a card.
        alice has conceded from the game.
        bob wins the game.
        Winner: bob
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    let flying = |name: &str| {
        let chars = game.characteristics();
        let (id, _) = game
            .permanents()
            .iter()
            .find(|(_, p)| game.card(p.card).is_some_and(|t| t.name == name))
            .expect(name);
        chars[id]
            .abilities
            .contains(&chancellor_of_the_spires::model::Ability::Keyword(
                chancellor_of_the_spires::model::Keyword::Flying,
            ))
    };
    assert_eq!(named_in(&game, game.player(ALICE).graveyard(), "Wonder"), 1);
    assert!(flying("Llanowar Elves"));
    assert!(!flying("Grizzly Bears"));
}

/// CR 602.2b, 601.2h: the log prints Quirion Ranger's cost, "returns
/// [Forest] to its owner's hand", on the line before its activation; the
/// Forest that line names goes back as the ability is activated, and is
/// played again from the hand to pay for Llanowar Elves.
#[test]
fn quirion_ranger_returns_the_forest_its_line_names_as_it_is_activated() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        alice casts [Quirion Ranger].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice returns [Forest] to its owner's hand with with [Quirion Ranger]'s ability.
        alice activates an ability of [Quirion Ranger] targeting [Quirion Ranger] (Return a Forest you control to its owner's hand: Untap target creature.).
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 2: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Forest"), 1);
    assert_eq!(
        on_battlefield(&game, "Llanowar Elves"),
        1,
        "paid by the Forest played again"
    );
}

/// Wirewood Symbiote's line, before its activation, names the Elf it
/// returns: Quirion Ranger, not the Llanowar Elves that entered first
/// (CR 602.2b, 601.2h).
fn symbiote_returns(named: &str) -> Replay {
    run(&format!(
        "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Wirewood Symbiote].
        alice casts [Grizzly Bears].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice casts [Quirion Ranger].
        alice returns [{named}] to its owner's hand with with [Wirewood Symbiote]'s ability.
        alice activates an ability of [Wirewood Symbiote] targeting [Llanowar Elves] (Untap target creature).
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    "
    ))
    .1
}

#[test]
fn wirewood_symbiote_returns_the_elf_its_line_names() {
    let r = symbiote_returns("Quirion Ranger");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Quirion Ranger"), 0);
    assert_eq!(on_battlefield(&game, "Llanowar Elves"), 1);
    assert_eq!(
        named_in(&game, game.player(ALICE).hand(), "Quirion Ranger"),
        1
    );
}

/// A line returning a permanent the cost can't, a Bear for an Elf, is a
/// divergence, not some Elf returned in its place.
#[test]
fn a_return_the_cost_cant_make_diverges() {
    let r = symbiote_returns("Grizzly Bears");
    assert!(
        matches!(
            &r.end,
            End::Stopped {
                stop: Stop::Diverged(Divergence::NeverOffered { what }),
                ..
            } if what.contains("Grizzly Bears")
        ),
        "{:?} {:#?}",
        r.end,
        r.trace
    );
}

/// Of two Llanowar Elves, the line names neither apart: the tapped one
/// goes back, so the replay pays for the second Elves with the first, the
/// second summoning sick (CR 302.6), and Grizzly Bears is paid for with
/// the Forests.
#[test]
fn a_return_goes_back_to_another_permanent_of_the_name() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Wirewood Symbiote].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice casts [Llanowar Elves].
        alice returns [Llanowar Elves] to its owner's hand with with [Wirewood Symbiote]'s ability.
        alice activates an ability of [Wirewood Symbiote] targeting [Wirewood Symbiote] (Untap target creature).
        alice casts [Grizzly Bears].
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Llanowar Elves"), 1);
    assert_eq!(on_battlefield(&game, "Grizzly Bears"), 1);
}

/// Turn 3's Wirewood Symbiote returns Quirion Ranger, one of Gaea's
/// Cradle's three creatures; `turn3` is what alice does after.
fn cradle_and(returned: &str, ability: &str, turn3: &str) -> Replay {
    run(&format!(
        "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Gaea's Cradle].
        alice casts [Wirewood Symbiote].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice casts [Quirion Ranger].
        alice returns [{returned}] to its owner's hand with with [{ability}]'s ability.
        alice activates an ability of [{ability}] targeting [Wirewood Symbiote] (Untap target creature).
        {turn3}
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    "
    ))
    .1
}

fn cradle_tapped(game: &chancellor_of_the_spires::game::Game) -> bool {
    game.permanents()
        .values()
        .any(|p| p.tapped && game.card(p.card).is_some_and(|t| t.name == "Gaea's Cradle"))
}

/// CR 605.3a, 608.2h, 106.4: Gaea's Cradle counts Quirion Ranger only
/// while it is on the battlefield, so it is tapped for three before
/// Wirewood Symbiote's cost returns the Ranger, the mana floating; with
/// the Forest or the Elves, whichever didn't pay for the Ranger, that is
/// Masticore's {4}. Tapped after, it makes two, and Masticore is
/// unpayable.
#[test]
fn gaeas_cradle_is_tapped_before_a_return_cost_uncounts_a_creature() {
    let r = cradle_and(
        "Quirion Ranger",
        "Wirewood Symbiote",
        "alice casts [Masticore].",
    );
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Masticore"), 1);
}

/// Of the Ranger's own ability, which returns a Forest: the Cradle doesn't
/// count lands, so nothing taps it, and it is still untapped on bob's
/// turn.
#[test]
fn a_return_cost_cradle_doesnt_count_leaves_it_untapped() {
    let r = cradle_and("Forest", "Quirion Ranger", "");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert!(!cradle_tapped(&game), "{:?}", r.trace);
}

/// Quirion Ranger's {G}, with Gaea's Cradle (counting only Llanowar
/// Elves) played before the Forest: the Forest or the Elves pays, and the
/// Cradle is left to tap later, when it may count more (CR 608.2h). Here
/// it counts the Ranger too, for Centaur Courser's {2}{G} with the one
/// left, and the replay never has to go back.
#[test]
fn gaeas_cradle_pays_last() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Gaea's Cradle].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice casts [Quirion Ranger].
        alice casts [Centaur Courser].
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    assert!(
        !r.trace.iter().any(|t| t.did.contains("again")),
        "{:#?}",
        r.trace
    );
}

/// The way back from tapping Gaea's Cradle first: the mana it floats in
/// the first main phase is lost as combat begins (CR 106.4), and only the
/// Cradle left untapped pays for Wall of Blossoms after the Elves attack.
#[test]
fn gaeas_cradle_is_left_untapped_when_its_mana_would_be_lost() {
    let r = cradle_and(
        "Quirion Ranger",
        "Wirewood Symbiote",
        "bob is being attacked by [Llanowar Elves]
        alice casts [Wall of Blossoms].
        alice puts triggered ability from [Wall of Blossoms] onto the stack (When Wall of Blossoms enters, draw a card.).
        alice draws a card with [Wall of Blossoms]'s ability.",
    );
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Wall of Blossoms"), 1);
}

/// Under Yavimaya, Cradle of Growth, Gaea's Cradle is a Forest too, with
/// a Forest's "{T}: Add {G}" (CR 305.6). Tapped first, it is tapped
/// for its count, three, not the Forest's one: with the two of the Forest,
/// the Elves and Yavimaya left, that is Masticore's {4} and the Ranger's
/// {G} cast again. One, or left untapped for two, isn't.
#[test]
fn gaeas_cradle_under_yavimaya_is_tapped_for_its_count() {
    let (_, r) = run("
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        alice casts [Llanowar Elves].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Gaea's Cradle].
        alice casts [Wirewood Symbiote].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice plays [Yavimaya, Cradle of Growth].
        alice casts [Quirion Ranger].
        alice returns [Quirion Ranger] to its owner's hand with with [Wirewood Symbiote]'s ability.
        alice activates an ability of [Wirewood Symbiote] targeting [Wirewood Symbiote] (Untap target creature).
        alice casts [Masticore].
        alice casts [Quirion Ranger].
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(on_battlefield(&game, "Masticore"), 1);
    assert_eq!(on_battlefield(&game, "Quirion Ranger"), 1);
}

/// CR 113.6m, 608.2d: Squee, discarded to Survival of the Fittest,
/// returns at alice's upkeep by its own trigger, the line saying it did.
/// Its "may" is answered as the trigger resolves, the stack by then
/// empty.
#[test]
fn squee_returns_from_the_graveyard_by_its_upkeep_trigger() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Survival of the Fittest].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice discards [Squee, Goblin Nabob].
        alice activates an ability of [Survival of the Fittest] ( Search your library for a creature card, reveal that card, put it into your hand, then shuffle.).
        alice reveals [Wall of Roots].
        Turn 3: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 4: alice
        alice puts triggered ability from [Squee, Goblin Nabob] onto the stack (At the beginning of your upkeep, you may return Squee, Goblin Nabob from your graveyard to your hand.).
        [Squee, Goblin Nabob] is returned to alice's hand from the graveyard.
        alice draws a card.
        alice plays [Forest].
        Turn 4: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert_eq!(r.end, End::Finished, "{:?}", r.trace);
    let game = r.game.expect("set up");
    assert_eq!(
        named_in(&game, game.player(ALICE).hand(), "Squee, Goblin Nabob"),
        1
    );
    assert_eq!(
        named_in(&game, game.player(ALICE).graveyard(), "Squee, Goblin Nabob"),
        0
    );
}

/// A line returning Squee from the graveyard with no trigger of it before
/// it is unfollowed by name: nothing in the game returned it.
#[test]
fn squee_returned_with_no_trigger_is_unfollowed() {
    let (_, r) = run("
        alice chooses to play first.
        alice begins the game with seven cards in hand.
        bob begins the game with seven cards in hand.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Survival of the Fittest].
        Turn 2: bob
        bob draws a card.
        bob plays [Swamp].
        Turn 3: alice
        alice draws a card.
        alice discards [Squee, Goblin Nabob].
        alice activates an ability of [Survival of the Fittest] ( Search your library for a creature card, reveal that card, put it into your hand, then shuffle.).
        alice reveals [Wall of Roots].
        [Squee, Goblin Nabob] is returned to alice's hand from the graveyard.
        Turn 3: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ");
    assert!(
        matches!(&r.end, End::Stopped { stop: Stop::Unfollowed { card: Some(c), .. }, .. }
            if c == "Squee, Goblin Nabob"),
        "{:?}",
        r.end
    );
}

/// An Elf that taps for one color of mana for each enchantment, as no
/// printed Elf does: Sanctum Weaver's text.
fn elvish_weaver() -> CardText {
    CardText {
        pt: Some(chancellor_of_the_spires::model::pt(1, 1)),
        ..face(
            "Elvish Weaver",
            "Enchantment Creature — Elf",
            "{1}{G}",
            "{T}: Add X mana of any one color, where X is the number of enchantments you control.",
            &[Color::Green],
        )
    }
}

/// Wirewood Symbiote returns a permanent that the Weaver counts, itself.
/// So the Weaver would be tapped first, with its mana floating, but in
/// which color no line says: refused by name, not tapped for its first
/// color.
#[test]
fn a_counting_source_of_several_colors_tapped_before_a_return_is_refused() {
    let (_, r) = run_with(
        "
        alice chooses to play first.
        Turn 1: alice
        alice skips their draw step.
        alice plays [Forest].
        Turn 1: bob
        bob draws a card.
        bob plays [Island].
        Turn 2: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Elvish Weaver].
        Turn 2: bob
        bob draws a card.
        bob plays [Island].
        Turn 3: alice
        alice draws a card.
        alice plays [Forest].
        alice casts [Wirewood Symbiote].
        Turn 3: bob
        bob draws a card.
        bob plays [Island].
        Turn 4: alice
        alice draws a card.
        alice returns [Elvish Weaver] to its owner's hand with with [Wirewood Symbiote]'s ability.
        alice activates an ability of [Wirewood Symbiote] targeting [Wirewood Symbiote] (Untap target creature).
        Turn 4: bob
        bob draws a card.
        bob has conceded from the game.
        alice wins the game.
        Winner: alice
    ",
        &mut |n| match n {
            "Elvish Weaver" => Some(elvish_weaver()),
            n => cards(n),
        },
        &[],
    );
    let End::Stopped { stop, .. } = &r.end else {
        panic!("{:?} {:#?}", r.end, r.trace)
    };
    assert!(
        matches!(stop, Stop::Refused { card, node, .. }
            if card == "Elvish Weaver" && node.contains("of one of several colors")),
        "{stop:?}"
    );
}
