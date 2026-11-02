//! Scripted games: a seed, the answers given, and the state they end in
//! (ADR-0043). The opponents in a goldfish only ever pass.

use std::path::Path;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use chancellor_of_the_spires::characteristics;
use chancellor_of_the_spires::deck::{self, CardText, DeckCards};
use chancellor_of_the_spires::game::*;
use chancellor_of_the_spires::lower;
use chancellor_of_the_spires::model::{
    pt, Ability, BasicLandType, CardType, Keyword, ObjectId, Op, PlayerId, State,
};
use chip_scryfall::index::IndexFile;

const ME: PlayerId = PlayerId(0);
const THEM: PlayerId = PlayerId(1);

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn index() -> &'static IndexFile {
    static INDEX: OnceLock<IndexFile> = OnceLock::new();
    INDEX.get_or_init(|| {
        IndexFile::open(&root().join("decks/index.jsonl")).expect("decks/index.jsonl")
    })
}

fn loam() -> DeckCards {
    let text = std::fs::read_to_string(root().join("decks/loam.deck.toml")).unwrap();
    deck::read(&text, index()).unwrap_or_else(|e| panic!("{e}"))
}

/// A card as the index knows it.
fn card(name: &str) -> CardText {
    deck::card(name, index()).unwrap_or_else(|e| panic!("{e}"))
}

fn copies(c: &CardText, n: usize) -> Vec<CardText> {
    (0..n).map(|_| c.clone()).collect()
}

fn forests(n: usize) -> DeckCards {
    DeckCards {
        hidden: 0,
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
            Decision::ReturnCommander { .. } => game.step(Answer::CommandZone(true)).unwrap(),
            // The goldfish pilot never attacks, and nothing blocks.
            Decision::Attackers { .. } => game.step(Answer::Attack(vec![])).unwrap(),
            Decision::Blockers { .. } => game.step(Answer::Block(vec![])).unwrap(),
            _ => game.step(Answer::Act(Action::Pass)).unwrap(),
        };
    }
    panic!("never reached {step:?} with {who:?} holding priority: {d:?}")
}

/// Every player passes until my next precombat main phase: this turn's if
/// it hasn't begun, else the next turn's.
fn my_next_main(game: &mut Game) -> Decision {
    if game.active() == ME && game.step_now() >= Step::PrecombatMain {
        let turn = game.turn();
        while game.turn() == turn {
            if !simplest(game) {
                game.step(Answer::Act(Action::Pass)).unwrap();
            }
        }
    }
    pass_until(game, ME, Step::PrecombatMain)
}

fn act(game: &mut Game, a: Action) -> Decision {
    game.step(Answer::Act(a)).unwrap_or_else(|e| panic!("{e}"))
}

/// The goldfish pilot's answer to a shock land: the simplest, not paying,
/// so it enters tapped.
fn enter_unpaid(game: &mut Game) {
    if let Decision::PayOrEnterTapped { .. } = game.decision() {
        game.step(Answer::PayLife(false)).unwrap();
    }
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
        .map(|c| game.card(*c).unwrap().name.as_str())
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
        lands.sort_by_key(|c| !game.card(*c).unwrap().type_line.starts_with("Basic"));
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
        played.push(game.card(land).unwrap().name.clone());
        assert_eq!(game.player(ME).lands_played(), 1);
        while let Some(tap) = priority(&game.decision())
            .1
            .iter()
            .find(|a| matches!(a, Action::Mana(_)))
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
        .find(|a| matches!(a, Action::Mana(m) if m.adds == ManaPool::of([Mana::Green])))
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
            other => panic!("{other:?}"),
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

fn actions(game: &Game) -> Vec<Action> {
    priority(&game.decision()).1.to_vec()
}

fn in_hand(game: &Game, p: PlayerId, name: &str) -> Option<CardId> {
    game.player(p)
        .hand()
        .iter()
        .copied()
        .find(|c| game.card(*c).unwrap().name == name)
}

/// The permanent this card is, if it is one.
fn permanent(game: &Game, c: CardId) -> Option<ObjectId> {
    game.permanents()
        .iter()
        .find(|(_, p)| p.card == c)
        .map(|(id, _)| *id)
}

/// The mana abilities `source` can activate now.
fn mana_of(game: &Game, source: ObjectId) -> Vec<ManaActivation> {
    actions(game)
        .into_iter()
        .filter_map(|a| match a {
            Action::Mana(m) if m.source == source => Some(m),
            _ => None,
        })
        .collect()
}

/// The pilot pays the cost being paid, the simplest way that works: spend
/// from the pool once a payment exists; until then activate a mana
/// ability, one adding a colour the cost still lacks if any, from the
/// source with the fewest choices. If none is left it cancels (CR 733.1).
fn pay(game: &mut Game) -> bool {
    // CR 601.2c: targets first.
    if let Decision::Targets { .. } = game.decision() {
        simplest(game);
    }
    loop {
        let Decision::Pay { owed, actions, .. } = game.decision() else {
            panic!("not paying: {:?}", game.decision());
        };
        if let Some(spend) = actions.iter().find(|a| matches!(a, Payment::Spend(_))) {
            game.step(Answer::Pay(*spend)).unwrap();
            return true;
        }
        let pool = *game.player(game.active()).pool();
        let lacks = |m: Mana| {
            let wanted = owed.shards.iter().filter(|s| **s == Shard::One(m)).count() as u32;
            wanted > pool.get(m)
        };
        let mut choices: BTreeMap<ObjectId, usize> = BTreeMap::new();
        for a in &actions {
            if let Payment::Mana(m) = a {
                *choices.entry(m.source).or_default() += 1;
            }
        }
        let best = actions
            .iter()
            .filter_map(|a| match a {
                Payment::Mana(m) => Some(*m),
                _ => None,
            })
            .min_by_key(|m| {
                let useful = Mana::ALL.iter().any(|c| m.adds.get(*c) > 0 && lacks(*c));
                (!useful, choices[&m.source])
            });
        match best {
            Some(m) => {
                game.step(Answer::Pay(Payment::Mana(m))).unwrap();
            }
            None => {
                game.step(Answer::Pay(Payment::Cancel)).unwrap();
                return false;
            }
        }
    }
}

/// Everyone passes until the stack is empty (CR 405.5).
fn resolve_stack(game: &mut Game) {
    while !game.stack().is_empty() || !matches!(game.decision(), Decision::Priority { .. }) {
        if !simplest(game) {
            game.step(Answer::Act(Action::Pass)).unwrap();
        }
    }
}

/// The pilot's simplest legal answer to a choice an effect asks for,
/// if the game is waiting on one: the first target in each slot, the
/// order triggers were listed in, the first legal card, every card scried
/// back on top, the first cards discarded.
fn simplest(game: &mut Game) -> bool {
    let answer = match game.decision() {
        Decision::ReturnCommander { .. } => Answer::CommandZone(true),
        Decision::Targets { slots, .. } => Answer::Targets(slots.iter().map(|s| s[0]).collect()),
        Decision::Order { triggers, .. } => Answer::Order((0..triggers.len()).collect()),
        Decision::ChooseFromHand { legal, .. } => Answer::Pick(legal.first().copied()),
        Decision::Scry { cards, .. } => Answer::Scry {
            top: cards,
            bottom: vec![],
        },
        Decision::Discard { count, hand, .. } => Answer::Discard(hand[..count].to_vec()),
        Decision::Search { .. } => Answer::Find(Found::Nothing),
        Decision::PayOrEnterTapped { .. } => Answer::PayLife(false),
        Decision::Attackers { .. } => Answer::Attack(vec![]),
        Decision::Blockers { .. } => Answer::Block(vec![]),
        _ => return false,
    };
    game.step(answer).unwrap_or_else(|e| panic!("{e}"));
    true
}

/// Cast `c` and pay for it, leaving it on the stack.
fn cast(game: &mut Game, c: CardId) {
    let d = act(game, Action::Cast(c));
    assert!(matches!(d, Decision::Pay { .. }), "{d:?}");
    assert!(pay(game), "couldn't pay for {}", game.card(c).unwrap().name);
}

/// CR 601.2, 117.1a, 302.1, 608.3a, 117.3b, 302.6: Birds of Paradise, from
/// Loam, cast in my main phase: it goes on the stack and is paid for with
/// the Forest's mana; when both players pass it resolves and enters the
/// battlefield under my control, which the layers see, and I get priority
/// again. Its {T} ability waits for my next turn.
#[test]
fn a_creature_cast_at_sorcery_speed_resolves_onto_the_battlefield() {
    let birds = card("Birds of Paradise");
    let library = [copies(&CardText::basic("Forest"), 20), copies(&birds, 10)].concat();
    let mut game = Game::new(
        vec![
            DeckCards {
                hidden: 0,
                commander: vec![],
                library,
            },
            forests(20),
        ],
        3,
    );
    my_next_main(&mut game);
    let bird = in_hand(&game, ME, "Birds of Paradise").expect("a Birds in hand");
    let forest = in_hand(&game, ME, "Forest").expect("a Forest in hand");
    // Not castable without mana, but the cast can begin and be cancelled.
    let d = act(&mut game, Action::Cast(bird));
    let Decision::Pay {
        player,
        spell,
        owed,
        actions,
    } = d
    else {
        panic!("{d:?}")
    };
    assert_eq!((player, spell, owed.to_string()), (ME, bird, "{G}".into()));
    assert_eq!(actions, [Payment::Cancel]);
    assert_eq!(
        game.stack().len(),
        1,
        "on the stack as it's cast (CR 601.2a)"
    );
    game.step(Answer::Pay(Payment::Cancel)).unwrap();
    assert!(game.stack().is_empty());
    assert!(
        game.player(ME).hand().contains(&bird),
        "back in hand (CR 733.1)"
    );

    act(&mut game, Action::PlayLand(forest));
    cast(&mut game, bird);
    assert_eq!(
        game.stack().iter().collect::<Vec<_>>(),
        [&StackObject::Spell {
            targets: vec![],
            card: bird,
            controller: ME
        }]
    );
    assert_eq!(game.player(ME).pool().total(), 0);
    // I hold priority after casting (CR 601.2i); I pass, they pass.
    assert_eq!(priority(&game.decision()).0, ME);
    act(&mut game, Action::Pass);
    let d = act(&mut game, Action::Pass);
    assert!(game.stack().is_empty());
    assert_eq!(priority(&d).0, ME, "CR 117.3b");
    assert_eq!(game.step_now(), Step::PrecombatMain);
    let id = permanent(&game, bird).expect("on the battlefield");
    let chars = &game.characteristics()[&id];
    assert!(chars.is(CardType::Creature));
    assert_eq!(chars.controller, ME);
    assert_eq!(chars.pt, Some(pt(0, 1)));
    assert!(chars.abilities.contains(&Ability::Keyword(Keyword::Flying)));
    assert!(mana_of(&game, id).is_empty(), "summoning sick (CR 302.6)");
    my_next_main(&mut game);
    let colours: BTreeSet<ManaPool> = mana_of(&game, id).iter().map(|m| m.adds).collect();
    assert_eq!(colours.len(), 5, "one mana of any color");
}

/// CR 117.1a, 304.1: an instant can be cast whenever I have priority, a
/// creature only in my main phase with the stack empty. Beast Within is
/// offered in the other player's upkeep, and refused by name since its
/// effect doesn't run yet; Birds isn't offered, and casting it is illegal.
#[test]
fn a_creature_cant_be_cast_at_instant_speed() {
    let library = [
        copies(&CardText::basic("Forest"), 10),
        copies(&card("Birds of Paradise"), 10),
        copies(&card("Beast Within"), 10),
    ]
    .concat();
    let mut game = Game::new(
        vec![
            DeckCards {
                hidden: 0,
                commander: vec![],
                library,
            },
            forests(20),
        ],
        5,
    );
    let bird = in_hand(&game, ME, "Birds of Paradise").expect("a Birds in hand");
    let beast = in_hand(&game, ME, "Beast Within").expect("a Beast Within in hand");
    pass_until(&mut game, THEM, Step::Upkeep);
    act(&mut game, Action::Pass);
    let d = game.decision();
    assert_eq!(priority(&d).0, ME);
    assert!(priority(&d).1.contains(&Action::Cast(beast)));
    assert!(!priority(&d).1.contains(&Action::Cast(bird)));
    assert!(matches!(
        game.step(Answer::Act(Action::Cast(bird))),
        Err(Rejected::Illegal(_))
    ));
    match game.step(Answer::Act(Action::Cast(beast))) {
        Err(Rejected::Refused { card, refusal }) => {
            assert_eq!(
                (card.as_str(), refusal.node.as_str()),
                ("Beast Within", "Effect::Destroy")
            )
        }
        other => panic!("{other:?}"),
    }
    // In my own main phase, with a spell on the stack, still not.
    my_next_main(&mut game);
    let forest = in_hand(&game, ME, "Forest").expect("a Forest");
    act(&mut game, Action::PlayLand(forest));
    cast(&mut game, bird);
    let other = in_hand(&game, ME, "Birds of Paradise").expect("another Birds");
    assert!(!actions(&game).contains(&Action::Cast(other)));
}

/// CR 614.1d, 614.12, 502.3: Lonely Sandbar enters tapped, so its mana
/// ability can't be activated until it untaps in my next untap step.
/// Argoth enters tapped unless I control a legendary green creature, and I
/// don't.
#[test]
fn a_tapped_land_enters_tapped() {
    for name in ["Lonely Sandbar", "Argoth, Sanctum of Nature"] {
        let mut game = Game::new(
            vec![
                DeckCards {
                    hidden: 0,
                    commander: vec![],
                    library: copies(&card(name), 20),
                },
                forests(20),
            ],
            1,
        );
        my_next_main(&mut game);
        let land = land_in_hand(&game, ME);
        act(&mut game, Action::PlayLand(land));
        let id = permanent(&game, land).expect("on the battlefield");
        assert!(game.permanents()[&id].tapped, "{name}");
        assert!(mana_of(&game, id).is_empty());
        my_next_main(&mut game);
        assert!(!game.permanents()[&id].tapped);
        assert_eq!(mana_of(&game, id).len(), 1, "{name}");
    }
}

/// CR 605.1a, 605.3b, 120.3a, 119.4, 903.4: Karplusan Forest's two mana
/// abilities, the second dealing me 1 damage; Fiery Islet's paying 1 life;
/// Command Tower's adding a colour of my commander's identity, which for
/// Borborygmos and Fblthp is blue, red and green.
#[test]
fn nonbasic_lands_mana_abilities() {
    let library = [
        card("Karplusan Forest"),
        card("Fiery Islet"),
        card("Command Tower"),
    ]
    .iter()
    .flat_map(|c| copies(c, 10))
    .collect();
    let deck = DeckCards {
        hidden: 0,
        commander: vec![card("Borborygmos and Fblthp")],
        library,
    };
    let mut game = Game::new(vec![deck, forests(20)], 2);
    let mut played = BTreeMap::new();
    for _ in 0..3 {
        my_next_main(&mut game);
        let land = ["Karplusan Forest", "Fiery Islet", "Command Tower"]
            .into_iter()
            .filter(|n| !played.contains_key(*n))
            .find_map(|n| in_hand(&game, ME, n))
            .expect("a land not played yet");
        act(&mut game, Action::PlayLand(land));
        played.insert(
            game.card(land).unwrap().name.clone(),
            permanent(&game, land).unwrap(),
        );
    }
    let adds = |game: &Game, name: &str| -> Vec<String> {
        mana_of(game, played[name])
            .iter()
            .map(|m| m.adds.to_string())
            .collect()
    };
    assert_eq!(adds(&game, "Karplusan Forest"), ["{C}", "{R}", "{G}"]);
    assert_eq!(adds(&game, "Fiery Islet"), ["{U}", "{R}"]);
    assert_eq!(adds(&game, "Command Tower"), ["{U}", "{R}", "{G}"]);

    let karplusan = mana_of(&game, played["Karplusan Forest"]);
    act(&mut game, Action::Mana(karplusan[1]));
    assert_eq!(game.player(ME).life(), 39);
    let islet = mana_of(&game, played["Fiery Islet"]);
    act(&mut game, Action::Mana(islet[0]));
    assert_eq!(game.player(ME).life(), 38);
    assert_eq!(game.player(ME).pool().to_string(), "{U}{R}");
    // Tapped, neither has an ability left to activate.
    assert!(mana_of(&game, played["Karplusan Forest"]).is_empty());
    assert!(mana_of(&game, played["Fiery Islet"]).is_empty());
}

/// CR 305.6, 305.7, 613.1d, 613.1f: a basic land type grants its mana
/// ability, so an effect that sets the type changes the ability and one
/// that removes all abilities removes it.
#[test]
fn a_land_that_loses_all_abilities_has_no_mana_ability() {
    let forest = lower::card(&CardText::basic("Forest").as_card().unwrap()).unwrap();
    let mut state = State::default();
    let id = state.enter(forest.chars, forest.statics);
    let intrinsic = |state: &State| -> Vec<Ability> {
        characteristics(state)[&id]
            .abilities
            .iter()
            .copied()
            .filter(|a| matches!(a, Ability::Intrinsic(_)))
            .collect()
    };
    assert_eq!(
        intrinsic(&state),
        [Ability::Intrinsic(BasicLandType::Forest)]
    );
    state.resolve([id], vec![Op::SetBasicLandType(BasicLandType::Mountain)]);
    assert_eq!(
        intrinsic(&state),
        [Ability::Intrinsic(BasicLandType::Mountain)]
    );
    state.resolve([id], vec![Op::LoseAll]);
    assert_eq!(intrinsic(&state), []);
}

/// CR 903.8, 903.9a, 704.5f: Rograkh, Son of Rohgahh, a 0/1 commander
/// costing {0}, is cast from the command zone for {0}. Night of Souls'
/// Betrayal makes it 0/0, so it dies, and I put it back in the command
/// zone. Cast again it costs {2}, dies again, and the third time costs {4},
/// which three Swamps can't pay, so that cast is cancelled.
#[test]
fn the_commander_is_cast_again_with_tax() {
    let night = card("Night of Souls' Betrayal");
    let deck = || DeckCards {
        hidden: 0,
        commander: vec![card("Rograkh, Son of Rohgahh")],
        library: [copies(&CardText::basic("Swamp"), 25), copies(&night, 5)].concat(),
    };
    // A seed whose opening hand holds the Night and three Swamps, so the
    // land drops and the Night come on curve.
    let seed = (0..)
        .find(|s| {
            let game = Game::new(vec![deck(), forests(20)], *s);
            in_hand(&game, ME, "Night of Souls' Betrayal").is_some()
                && game
                    .player(ME)
                    .hand()
                    .iter()
                    .filter(|c| game.card(**c).unwrap().name == "Swamp")
                    .count()
                    >= 3
        })
        .unwrap();
    let mut game = Game::new(vec![deck(), forests(20)], seed);
    let rograkh = game.command()[0];
    let drop_swamp = |game: &mut Game| {
        let swamp = in_hand(game, ME, "Swamp").expect("a Swamp");
        act(game, Action::PlayLand(swamp));
    };

    my_next_main(&mut game);
    drop_swamp(&mut game);
    let d = act(&mut game, Action::Cast(rograkh));
    let Decision::Pay { owed, .. } = &d else {
        panic!("{d:?}")
    };
    assert_eq!(owed.to_string(), "{0}");
    assert!(pay(&mut game));
    resolve_stack(&mut game);
    assert_eq!(game.commander_casts(rograkh), 1);
    let id = permanent(&game, rograkh).expect("on the battlefield");
    assert_eq!(game.characteristics()[&id].pt, Some(pt(0, 1)));

    for _ in 0..3 {
        my_next_main(&mut game);
        drop_swamp(&mut game);
    }
    let night = in_hand(&game, ME, "Night of Souls' Betrayal").unwrap();
    cast(&mut game, night);
    act(&mut game, Action::Pass);
    let d = act(&mut game, Action::Pass);
    // It resolved, Rograkh is 0/0 and dies, and I choose where it goes.
    assert_eq!(
        d,
        Decision::ReturnCommander {
            player: ME,
            card: rograkh
        }
    );
    assert!(game.player(ME).graveyard().contains(&rograkh));
    let d = game.step(Answer::CommandZone(true)).unwrap();
    assert_eq!(priority(&d).0, ME);
    assert!(game.command().contains(&rograkh));

    my_next_main(&mut game);
    drop_swamp(&mut game);
    let d = act(&mut game, Action::Cast(rograkh));
    let Decision::Pay { owed, .. } = &d else {
        panic!("{d:?}")
    };
    assert_eq!(owed.to_string(), "{2}");
    assert!(pay(&mut game));
    resolve_stack(&mut game);
    assert_eq!(game.commander_casts(rograkh), 2);
    assert!(game.command().contains(&rograkh));
    let d = act(&mut game, Action::Cast(rograkh));
    let Decision::Pay { owed, .. } = &d else {
        panic!("{d:?}")
    };
    assert_eq!(owed.to_string(), "{4}");
    assert!(!pay(&mut game), "three Swamps left");
    assert!(game.command().contains(&rograkh));
    assert_eq!(game.commander_casts(rograkh), 2);
    // Cancelling reversed the Swamps' mana abilities too (CR 733.1).
    assert_eq!(game.player(ME).pool().total(), 0);
    let untapped_swamps = game
        .permanents()
        .values()
        .filter(|p| !p.tapped && game.card(p.card).unwrap().name == "Swamp")
        .count();
    assert_eq!(untapped_swamps, 3);
}

/// One of my main phases as the goldfish pilot plays it: a land, the first
/// in hand the engine runs, basics last since nonbasics fix colours; then
/// every spell it can cast and pay for, each resolved before the next.
/// Returns what it played and cast, and adds what was refused.
fn pilot_main_phase(game: &mut Game, refused: &mut BTreeSet<String>) -> Vec<String> {
    let mut did = Vec::new();
    let mut lands: Vec<CardId> = actions(game)
        .iter()
        .filter_map(|a| match a {
            Action::PlayLand(c) => Some(*c),
            _ => None,
        })
        .collect();
    lands.sort_by_key(|c| game.card(*c).unwrap().type_line.starts_with("Basic"));
    for land in lands {
        match game.step(Answer::Act(Action::PlayLand(land))) {
            Ok(_) => {
                enter_unpaid(game);
                did.push(game.card(land).unwrap().name.clone());
                break;
            }
            Err(Rejected::Refused { card, refusal }) => {
                refused.insert(format!("{card}: {refusal}"));
            }
            Err(e) => panic!("{e}"),
        }
    }
    let mut tried = BTreeSet::new();
    while let Some(c) = actions(game).iter().find_map(|a| match a {
        Action::Cast(c) if !tried.contains(c) => Some(*c),
        _ => None,
    }) {
        tried.insert(c);
        match game.step(Answer::Act(Action::Cast(c))) {
            Ok(_) => {
                if pay(game) {
                    resolve_stack(game);
                    did.push(format!("cast {}", game.card(c).unwrap().name));
                }
            }
            Err(Rejected::Refused { card, refusal }) => {
                refused.insert(format!("{card}: {refusal}"));
            }
            Err(e) => panic!("{e}"),
        }
    }
    did
}

/// Loam against a goldfish for six of its turns from seed 24, played by
/// `pilot_main_phase`: lands, then whatever it can cast. Everything the
/// engine refuses is refused by name.
#[test]
fn loam_goldfished_casts_what_it_can() {
    let mut game = Game::new(vec![loam(), loam()], 24);
    let mut refused = BTreeSet::new();
    let mut turns = Vec::new();
    for _ in 0..6 {
        my_next_main(&mut game);
        turns.push(pilot_main_phase(&mut game, &mut refused));
    }
    for (t, did) in turns.iter().enumerate() {
        eprintln!("turn {}: {did:?}", 2 * t + 1);
    }
    for r in &refused {
        eprintln!("refused {r}");
    }
    let did: Vec<Vec<&str>> = turns
        .iter()
        .map(|t| t.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(
        did,
        [
            vec!["Yavimaya, Cradle of Growth"],
            vec!["Cephalid Coliseum", "cast Trade Routes"],
            vec!["Scalding Tarn"],
            // Deep Analysis draws two (CR 121.2); its flashback is
            // carried, never offered.
            vec!["Mountain", "cast Deep Analysis"],
            vec!["Command Beacon"],
            // Unpaid, it enters tapped (CR 614.12a).
            vec!["Breeding Pool"],
        ]
    );
    // Trade Routes' {U} and Deep Analysis's came from Cephalid Coliseum,
    // which dealt me 1 damage each time (CR 120.3a).
    assert_eq!(game.player(ME).life(), 38);
    assert_eq!(game.permanents().len(), 7);
    assert_eq!(game.player(THEM).lands_played(), 0);
    assert!(refused
        .iter()
        .any(|r| r.starts_with("Borborygmos and Fblthp")));
}

/// How many of Loam's distinct cards this engine plays (a land) or casts
/// and resolves whole, each in a game of its own: a land from a hand of
/// it, any other card as the commander from the command zone, with Forests,
/// Islands and Mountains to pay for it, for up to twelve of my turns. The
/// rest are counted by the node refused. Run with `--nocapture` to read
/// them.
#[test]
fn loam_coverage() {
    let deck = loam();
    let mut seen = BTreeSet::new();
    let cards: Vec<CardText> = deck
        .commander
        .iter()
        .chain(&deck.library)
        .filter(|c| seen.insert(c.name.clone()))
        .cloned()
        .collect();
    let basics: Vec<CardText> = ["Forest", "Island", "Mountain"]
        .iter()
        .flat_map(|n| copies(&CardText::basic(n), 10))
        .collect();
    let mut runs = Vec::new();
    let mut refused: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &cards {
        let land = c.type_line.split("//").next().unwrap().contains("Land");
        let mine = if land {
            DeckCards {
                hidden: 0,
                commander: vec![],
                library: copies(c, 10),
            }
        } else {
            DeckCards {
                hidden: 0,
                commander: vec![c.clone()],
                library: basics.clone(),
            }
        };
        let mut game = Game::new(vec![mine, forests(30)], 7);
        let target = if land {
            game.player(ME).hand()[0]
        } else {
            game.command()[0]
        };
        if let Some(r) = game.refusal(target) {
            refused.entry(r.node).or_default().push(c.name.clone());
            continue;
        }
        let mut done = false;
        for _ in 0..12 {
            my_next_main(&mut game);
            if land {
                act(&mut game, Action::PlayLand(target));
                enter_unpaid(&mut game);
                done = permanent(&game, target).is_some();
                break;
            }
            let lands: Vec<CardId> = actions(&game)
                .iter()
                .filter_map(|a| match a {
                    Action::PlayLand(l) => Some(*l),
                    _ => None,
                })
                .collect();
            // A colour I have fewest of.
            let have = |n: &str| {
                game.permanents()
                    .values()
                    .filter(|p| game.card(p.card).unwrap().name == n)
                    .count()
            };
            if let Some(l) = lands
                .iter()
                .min_by_key(|l| have(&game.card(**l).unwrap().name))
            {
                act(&mut game, Action::PlayLand(*l));
                enter_unpaid(&mut game);
            }
            if !actions(&game).contains(&Action::Cast(target)) {
                continue;
            }
            act(&mut game, Action::Cast(target));
            if pay(&mut game) {
                resolve_stack(&mut game);
                done = permanent(&game, target).is_some()
                    || game.player(ME).graveyard().contains(&target);
                break;
            }
        }
        assert!(done, "{} ran but wasn't played or cast", c.name);
        runs.push((c.name.clone(), game.dormant(target).to_vec()));
    }
    let whole: Vec<&str> = runs
        .iter()
        .filter(|(_, d)| d.is_empty())
        .map(|(n, _)| n.as_str())
        .collect();
    eprintln!(
        "{} of Loam's {} distinct cards are played or cast and resolved; {} with every ability running",
        runs.len(),
        cards.len(),
        whole.len()
    );
    eprintln!("  whole: {}", whole.join(", "));
    for (name, dormant) in runs.iter().filter(|(_, d)| !d.is_empty()) {
        eprintln!(
            "  {name}, carrying {} not activatable yet",
            dormant.join(", ")
        );
    }
    let mut top: Vec<_> = refused.into_iter().collect();
    top.sort_by_key(|(_, names)| std::cmp::Reverse(names.len()));
    for (node, names) in &top {
        eprintln!("{:>4}  {node}: {}", names.len(), names.join(", "));
    }
    assert_eq!(cards.len(), 90);
    // The README's number: a change that moves it updates it there.
    assert_eq!((runs.len(), whole.len()), (35, 15));
}

fn hidden(n: usize) -> DeckCards {
    DeckCards {
        hidden: n,
        ..DeckCards::default()
    }
}

fn constructed(decks: Vec<DeckCards>, mulligans: bool) -> Game {
    Game::start(Setup {
        format: Format::Constructed,
        decks,
        mulligans,
        seed: 1,
    })
    .unwrap()
}

/// A hidden card has no text to read, so it can't be played or cast and
/// isn't offered; named, it is an ordinary card, and it is named once.
/// CR 103.4: a constructed game starts at 20 life.
#[test]
fn a_hidden_card_is_offered_only_once_it_is_named() {
    let mut game = constructed(vec![hidden(20), hidden(20)], false);
    assert_eq!(game.player(ME).life(), 20);
    let d = pass_until(&mut game, ME, Step::PrecombatMain);
    assert_eq!(priority(&d).1, [Action::Pass]);
    let c = game.player(ME).hand()[0];
    assert!(game.card(c).is_none());
    assert_eq!(game.refusal(c).unwrap().node, "a hidden card");
    game.reveal(c, CardText::basic("Forest")).unwrap();
    assert_eq!(
        game.reveal(c, CardText::basic("Island")),
        Err(AlreadyKnown("Forest".into()))
    );
    assert!(actions(&game).contains(&Action::PlayLand(c)));
    act(&mut game, Action::PlayLand(c));
    assert_eq!(game.permanents().len(), 1);
}

/// Only Commander has commanders (CR 903.3).
#[test]
fn a_constructed_game_has_no_commander() {
    let deck = DeckCards {
        commander: vec![card("Forest")],
        ..DeckCards::default()
    };
    let setup = Setup {
        format: Format::Constructed,
        decks: vec![deck, forests(20)],
        mulligans: false,
        seed: 1,
    };
    assert_eq!(
        Game::start(setup).err(),
        Some(SetupError::CommanderOutsideCommander)
    );
}

/// CR 103.5: the starting player declares first, then the other; those
/// taking a mulligan draw seven anew and put one card on the bottom for
/// each mulligan taken, and declare again until everyone keeps. Then the
/// first turn begins.
#[test]
fn the_london_mulligan() {
    let mut game = constructed(vec![forests(20), forests(20)], true);
    assert_eq!(game.decision(), Decision::Mulligan { player: ME });
    game.step(Answer::Mulligan(true)).unwrap();
    assert_eq!(game.decision(), Decision::Mulligan { player: THEM });
    game.step(Answer::Mulligan(false)).unwrap();
    let Decision::Bottom {
        player,
        count,
        hand,
    } = game.decision()
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((player, count, hand.len()), (ME, 1, 7));
    // Two cards for one mulligan is not an answer.
    assert!(game.step(Answer::Bottom(hand[..2].to_vec())).is_err());
    game.step(Answer::Bottom(vec![hand[0]])).unwrap();
    assert_eq!(game.player(ME).library().back(), Some(&hand[0]));
    // Only the player who hasn't kept declares again.
    assert_eq!(game.decision(), Decision::Mulligan { player: ME });
    game.step(Answer::Mulligan(true)).unwrap();
    let Decision::Bottom { count, hand, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(count, 2);
    game.step(Answer::Bottom(hand[..2].to_vec())).unwrap();
    game.step(Answer::Mulligan(false)).unwrap();
    assert_eq!(game.turn(), 1);
    assert_eq!(game.player(ME).hand().len(), 5);
    assert_eq!(game.player(ME).mulligans(), 2);
    assert_eq!(game.player(THEM).hand().len(), 7);
    assert_eq!(game.player(ME).library().len(), 15);
}

/// Name the first hidden card in `p`'s hand.
fn name_in_hand(game: &mut Game, p: PlayerId, name: &str) -> CardId {
    let c = *game
        .player(p)
        .hand()
        .iter()
        .find(|c| game.card(**c).is_none())
        .expect("a hidden card in hand");
    game.reveal(c, card(name)).unwrap();
    c
}

/// CR 605.1a, 605.3b, 608.2h: Gaea's Cradle's "Add {G} for each creature
/// you control" is a mana ability, and the amount is counted as it is
/// activated: none with no creature, then {G} once Llanowar Elves is mine.
#[test]
fn gaeas_cradle_adds_mana_for_each_creature() {
    let mut game = constructed(vec![hidden(20), forests(20)], false);
    my_next_main(&mut game);
    let cradle = name_in_hand(&mut game, ME, "Gaea's Cradle");
    act(&mut game, Action::PlayLand(cradle));
    let cradle = permanent(&game, cradle).unwrap();
    let adds =
        |game: &Game| -> Vec<ManaPool> { mana_of(game, cradle).iter().map(|m| m.adds).collect() };
    assert_eq!(adds(&game), [ManaPool::default()]);

    my_next_main(&mut game);
    let forest = name_in_hand(&mut game, ME, "Forest");
    act(&mut game, Action::PlayLand(forest));
    let elves = name_in_hand(&mut game, ME, "Llanowar Elves");
    cast(&mut game, elves);
    resolve_stack(&mut game);
    assert!(permanent(&game, elves).is_some());
    assert_eq!(adds(&game), [ManaPool::of([Mana::Green])]);
}

/// Pass until the stack is empty and a decision other than priority, or
/// my priority again, comes up.
fn resolve_to_decision(game: &mut Game) -> Decision {
    while !game.stack().is_empty() {
        if !matches!(game.decision(), Decision::Priority { .. }) {
            break;
        }
        game.step(Answer::Act(Action::Pass)).unwrap();
    }
    game.decision()
}

fn the_activation(game: &Game, source: ObjectId) -> Activation {
    actions(game)
        .into_iter()
        .find_map(|a| match a {
            Action::Activate(a) if a.source == source => Some(a),
            _ => None,
        })
        .expect("an activation offered")
}

/// CR 602.2, 119.4, 701.21a, 405.1, 608.2c, 701.23a, 701.24a, 117.3b: Wooded
/// Foothills' ability is activated by paying its costs (it is tapped and
/// sacrificed, I pay 1 life) and goes on the stack; when it resolves I search
/// my hidden library. The card found must be a Mountain or Forest card: a
/// hidden one named Island or Lightning Bolt isn't found, nor is one the
/// engine refuses. A Forest is, and enters untapped, the library one card
/// smaller, and the active player gets priority.
#[test]
fn a_fetchland_searches_the_library_for_a_land_it_matches() {
    let mut game = constructed(vec![hidden(20), forests(20)], false);
    my_next_main(&mut game);
    let fetch = name_in_hand(&mut game, ME, "Wooded Foothills");
    act(&mut game, Action::PlayLand(fetch));
    let source = permanent(&game, fetch).unwrap();
    assert!(mana_of(&game, source).is_empty(), "not a mana ability");
    let a = the_activation(&game, source);
    act(&mut game, Action::Activate(a));
    assert_eq!(game.player(ME).life(), 19);
    assert_eq!(permanent(&game, fetch), None);
    assert!(game.player(ME).graveyard().contains(&fetch));
    assert_eq!(game.stack().len(), 1);
    let library = game.player(ME).library().len();
    let Decision::Search {
        player,
        known,
        hidden,
        ..
    } = resolve_to_decision(&mut game)
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((player, known.len(), hidden), (ME, 0, library));
    let c = game.player(ME).library()[0];
    for wrong in ["Island", "Lightning Bolt"] {
        let r = game.step(Answer::Find(Found::Hidden(c, Box::new(card(wrong)))));
        assert!(matches!(r, Err(Rejected::Illegal(_))), "{wrong}: {r:?}");
    }
    assert!(game.card(c).is_none(), "a refused answer names nothing");
    let d = game
        .step(Answer::Find(Found::Hidden(c, Box::new(card("Forest")))))
        .unwrap();
    assert_eq!(priority(&d).0, ME);
    let forest = permanent(&game, c).expect("the Forest entered");
    assert!(!game.permanents()[&forest].tapped);
    assert_eq!(game.player(ME).library().len(), library - 1);
    assert_eq!(mana_of(&game, forest).len(), 1);
}

/// CR 701.23b: a search for a card with a stated quality may find nothing.
/// Evolving Wilds puts what it finds onto the battlefield tapped.
#[test]
fn a_search_may_find_nothing_and_evolving_wilds_puts_its_land_in_tapped() {
    for find in [false, true] {
        let mut game = constructed(vec![hidden(20), forests(20)], false);
        my_next_main(&mut game);
        let wilds = name_in_hand(&mut game, ME, "Evolving Wilds");
        act(&mut game, Action::PlayLand(wilds));
        let source = permanent(&game, wilds).unwrap();
        let a = the_activation(&game, source);
        act(&mut game, Action::Activate(a));
        assert_eq!(game.player(ME).life(), 20);
        assert!(matches!(
            resolve_to_decision(&mut game),
            Decision::Search { .. }
        ));
        let c = game.player(ME).library()[0];
        let found = if find {
            Found::Hidden(c, Box::new(card("Swamp")))
        } else {
            Found::Nothing
        };
        game.step(Answer::Find(found)).unwrap();
        assert!(game.stack().is_empty());
        match permanent(&game, c) {
            Some(swamp) => {
                assert!(find);
                assert!(game.permanents()[&swamp].tapped);
            }
            None => assert!(!find),
        }
    }
}

/// Name a hidden card in my hand and play it, in my next main phase.
fn play_named(game: &mut Game, name: &str) -> ObjectId {
    my_next_main(game);
    let c = name_in_hand(game, ME, name);
    act(game, Action::PlayLand(c));
    permanent(game, c).expect("on the battlefield")
}

fn tapped(game: &Game, id: ObjectId) -> bool {
    game.permanents()[&id].tapped
}

/// CR 614.1c, 614.12a, 119.4: as a shock land enters I choose, before it
/// is on the battlefield, to pay 2 life or have it enter tapped.
#[test]
fn a_shock_land_enters_untapped_for_2_life_or_tapped() {
    for pay in [true, false] {
        let mut game = constructed(vec![hidden(20), forests(20)], false);
        my_next_main(&mut game);
        let c = name_in_hand(&mut game, ME, "Stomping Ground");
        let d = act(&mut game, Action::PlayLand(c));
        assert_eq!(
            d,
            Decision::PayOrEnterTapped {
                player: ME,
                card: c,
                life: 2
            }
        );
        assert_eq!(permanent(&game, c), None, "chosen before it enters");
        let d = game.step(Answer::PayLife(pay)).unwrap();
        assert_eq!(priority(&d).0, ME);
        let id = permanent(&game, c).unwrap();
        assert_eq!(tapped(&game, id), !pay);
        assert_eq!(game.player(ME).life(), if pay { 18 } else { 20 });
    }
}

/// CR 608.2c, 614.12a: a shock land a fetchland finds asks its choice as
/// it enters, partway through the ability's resolution, and the ability
/// goes on after it: the library shuffled and the active player given
/// priority.
#[test]
fn a_shock_land_found_by_a_fetchland_asks_as_it_enters() {
    let mut game = constructed(vec![hidden(20), forests(20)], false);
    let fetch = play_named(&mut game, "Wooded Foothills");
    let a = the_activation(&game, fetch);
    act(&mut game, Action::Activate(a));
    assert!(matches!(
        resolve_to_decision(&mut game),
        Decision::Search { .. }
    ));
    let c = game.player(ME).library()[0];
    let d = game
        .step(Answer::Find(Found::Hidden(
            c,
            Box::new(card("Stomping Ground")),
        )))
        .unwrap();
    assert!(matches!(d, Decision::PayOrEnterTapped { card, .. } if card == c));
    let d = game.step(Answer::PayLife(true)).unwrap();
    assert_eq!(priority(&d).0, ME);
    assert!(game.stack().is_empty());
    assert_eq!(game.player(ME).life(), 17);
    assert!(!tapped(&game, permanent(&game, c).unwrap()));
}
/// CR 614.1d, 614.12: Blackcleave Cliffs enters tapped unless I control
/// two or fewer other lands, counted as it enters, not counting itself.
#[test]
fn a_fast_land_enters_tapped_after_three_other_lands() {
    let mut game = constructed(vec![hidden(30), forests(30)], false);
    let first = play_named(&mut game, "Blackcleave Cliffs");
    assert!(!tapped(&game, first), "no other land");
    play_named(&mut game, "Swamp");
    let third = play_named(&mut game, "Blackcleave Cliffs");
    assert!(!tapped(&game, third), "two other lands");
    let fourth = play_named(&mut game, "Blackcleave Cliffs");
    assert!(tapped(&game, fourth), "three other lands");
}

/// CR 614.1d, 614.12: Prairie Stream enters tapped unless I control two or
/// more basic lands; a nonbasic land doesn't count.
#[test]
fn a_battle_land_enters_tapped_until_two_basics() {
    let mut game = constructed(vec![hidden(30), forests(30)], false);
    play_named(&mut game, "Plains");
    let one = play_named(&mut game, "Prairie Stream");
    assert!(tapped(&game, one), "one basic land");
    play_named(&mut game, "Island");
    let two = play_named(&mut game, "Prairie Stream");
    assert!(!tapped(&game, two), "two basic lands");
}

/// CR 614.1d, 614.12: Clifftop Retreat enters tapped unless I control a
/// Mountain or a Plains: either alternative will do, a land with the type
/// counting whatever its name.
#[test]
fn a_check_land_enters_untapped_with_either_land_type() {
    for other in ["Mountain", "Plains", "Stomping Ground", "Island"] {
        let mut game = constructed(vec![hidden(30), forests(30)], false);
        let alone = play_named(&mut game, "Clifftop Retreat");
        assert!(tapped(&game, alone), "nothing else");
        my_next_main(&mut game);
        let c = name_in_hand(&mut game, ME, other);
        act(&mut game, Action::PlayLand(c));
        if let Decision::PayOrEnterTapped { .. } = game.decision() {
            game.step(Answer::PayLife(false)).unwrap();
        }
        let retreat = play_named(&mut game, "Clifftop Retreat");
        assert_eq!(tapped(&game, retreat), other == "Island", "{other}");
    }
}

/// CR 614.1d, 614.12: Cave of the Frost Dragon enters tapped if I control
/// two or more other lands.
#[test]
fn a_land_enters_tapped_if_a_condition_holds() {
    let mut game = constructed(vec![hidden(30), forests(30)], false);
    let first = play_named(&mut game, "Cave of the Frost Dragon");
    assert!(!tapped(&game, first));
    let second = play_named(&mut game, "Cave of the Frost Dragon");
    assert!(!tapped(&game, second), "one other land");
    let third = play_named(&mut game, "Cave of the Frost Dragon");
    assert!(tapped(&game, third), "two other lands");
}
