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
    if name == UNREADABLE {
        return unreadable();
    }
    two_faced(name).unwrap_or_else(|| deck::card(name, index()).unwrap_or_else(|e| panic!("{e}")))
}

/// A card of a type no rule names, whose characteristics the layers can't
/// read: whether a filter matches it is unknown.
const UNREADABLE: &str = "Unreadable Gizmo";

fn unreadable() -> CardText {
    CardText {
        name: UNREADABLE.into(),
        type_line: "Gizmo".into(),
        mana_cost: "{1}".into(),
        oracle: Oracle::OneFace(String::new()),
        mana_value: 1,
        colors: Vec::new(),
        pt: None,
        identity: Vec::new(),
    }
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
        "Bonecrusher Giant // Stomp" => (
            TwoFaced::Adventure,
            face(
                "Bonecrusher Giant",
                "Creature — Giant",
                "{2}{R}",
                "Whenever this creature becomes the target of a spell, this creature deals 2 damage to that spell's controller.",
                3,
                Some(Color::Red),
                Some(pt(4, 3)),
            ),
            face(
                "Stomp",
                "Instant — Adventure",
                "{1}{R}",
                "Damage can't be prevented this turn. Stomp deals 2 damage to any target.",
                2,
                Some(Color::Red),
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
            // Nothing begins the game on the battlefield unless a test
            // says so (CR 103.6).
            Decision::OpeningHand { .. } => {
                step(game, Answer::Begin(vec![]));
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
    cast_card(game, c, targets)
}

/// Cast this card, wherever it is, as `cast_as` does.
fn cast_card(game: &mut Game, c: CardId, targets: Option<Vec<Target>>) {
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

/// CR 510.2, 603.2, 701.9b: Hypnotic Specter's combat damage to an
/// opponent triggers, and that player discards a card at random: chance's
/// pick, which the decision says, of their hand.
#[test]
fn hypnotic_specter_makes_the_opponent_it_damages_discard_at_random() {
    let mut game = deal(
        &[("Swamp", 40), ("Hypnotic Specter", 20)],
        &[("Forest", 60)],
        &[("Swamp", 3), ("Hypnotic Specter", 1)],
    );
    play(&mut game, "Swamp");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Hypnotic Specter", None);
    resolve(&mut game);
    let specter = creature(&game, "Hypnotic Specter").unwrap();
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(specter, THEM)]));
    let hand = game.player(THEM).hand().clone();
    let d = (0..20)
        .find_map(|_| match game.decision() {
            d @ Decision::Discard { .. } => Some(d),
            Decision::Blockers { .. } => {
                step(&mut game, Answer::Block(vec![]));
                None
            }
            _ => {
                step(&mut game, Answer::Act(Action::Pass));
                None
            }
        })
        .expect("a discard");
    let Decision::Discard {
        player,
        count,
        random,
        hand: from,
    } = d
    else {
        unreachable!()
    };
    assert_eq!((player, count, random), (THEM, 1, true));
    assert_eq!(from, hand.iter().copied().collect::<Vec<_>>());
    assert_eq!(game.player(THEM).life(), 18);
    step(&mut game, Answer::Discard(vec![from[0]]));
    assert!(game.player(THEM).graveyard().contains(&from[0]));
}

/// CR 113.6m, 108.4a, 503.1a: Squee's upkeep trigger works from my
/// graveyard, where its effect moves it out of, and nowhere else: in my
/// hand it does nothing, and discarded to hand size it triggers at my next
/// upkeep and may return to my hand.
#[test]
fn squee_returns_from_the_graveyard_at_my_upkeep() {
    let mut game = deal(
        &[("Squee, Goblin Nabob", 60)],
        &[("Island", 60)],
        &[("Squee, Goblin Nabob", 7)],
    );
    my_next_main(&mut game);
    assert!(game.stack().is_empty(), "nothing triggers from the hand");
    assert_eq!(game.player(ME).hand().len(), 8);
    pass_until(&mut game, THEM, Step::Upkeep);
    let squee = game.player(ME).graveyard()[0];
    assert_eq!(game.player(ME).hand().len(), 7);
    pass_until(&mut game, ME, Step::Upkeep);
    assert_eq!(game.stack().len(), 1);
    let d = resolve(&mut game);
    assert!(matches!(d, Decision::May { player: ME, .. }), "{d:?}");
    step(&mut game, Answer::May(true));
    assert!(game.player(ME).hand().contains(&squee));
    assert!(game.player(ME).graveyard().is_empty());
}

/// CR 113.6b: Anger's haste works while it is in my graveyard, and the
/// engine doesn't run it, so once it is discarded there the game refuses
/// to go on rather than play on without it.
#[test]
fn anger_in_a_graveyard_is_refused() {
    let mut game = deal(&[("Anger", 60)], &[("Island", 60)], &[("Anger", 7)]);
    my_next_main(&mut game);
    let refused = (0..200).find_map(|_| {
        let answer = match game.decision() {
            Decision::Discard { count, hand, .. } => Answer::Discard(hand[..count].to_vec()),
            Decision::Attackers { .. } => Answer::Attack(vec![]),
            _ => Answer::Act(Action::Pass),
        };
        game.step(answer).err()
    });
    assert!(
        game.player(ME).graveyard().len() == 1,
        "discarded to hand size"
    );
    match refused {
        Some(Rejected::Refused { card, .. }) => assert_eq!(card, "Anger"),
        other => panic!("{other:?}"),
    }
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

/// CR 702.39a, 509.1c, 615.1a: Deftblade Elite attacks and provokes the
/// Grizzly Bears, which then must block it, no block at all being
/// illegal; its "{1}{W}: Prevent all combat damage that would be dealt to
/// and dealt by this creature this turn" leaves both alive.
#[test]
fn provoke_makes_its_target_block_and_deftblade_prevents_the_damage() {
    let mut game = deal_both(
        &[("Plains", 40), ("Deftblade Elite", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Plains", 3), ("Deftblade Elite", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    cast(&mut game, "Deftblade Elite", None);
    resolve(&mut game);
    let elite = creature(&game, "Deftblade Elite").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Plains");
    to_attackers(&mut game);
    let d = step(&mut game, Answer::Attack(vec![(elite, THEM)]));
    let Decision::Targets { player, slots, .. } = d else {
        panic!("provoke's target: {d:?}")
    };
    assert_eq!(player, ME);
    assert_eq!(
        slots,
        vec![vec![Target::Object(bears)]],
        "a creature THEM controls"
    );
    step(&mut game, Answer::Targets(vec![Target::Object(bears)]));
    let d = resolve(&mut game);
    assert!(matches!(d, Decision::May { .. }), "{d:?}");
    step(&mut game, Answer::May(true));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    assert!(game.step(Answer::Block(vec![])).is_err(), "it must block");
    step(&mut game, Answer::Block(vec![(bears, elite)]));
    let mut d = activate(&mut game, elite);
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap_or_else(|| panic!("a way to pay: {actions:?}"));
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    through_combat(&mut game);
    assert!(game.permanents().contains_key(&elite));
    assert!(game.permanents().contains_key(&bears));
    assert_eq!(game.permanents()[&bears].damage, 0);
}

/// CR 509.3c, 603.2c, 613.1b, 506.4, 302.6: Goblin Cadets' "Whenever this
/// creature blocks or becomes blocked, target opponent gains control of
/// it" triggers once as Grizzly Bears block it; resolving, its controller's
/// opponent controls it, it leaves combat, so neither deals the other
/// damage, and it is sick under its new controller.
#[test]
fn goblin_cadets_changes_control_when_it_becomes_blocked() {
    let mut game = deal_both(
        &[("Mountain", 40), ("Goblin Cadets", 20)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Mountain", 1), ("Goblin Cadets", 1)],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Goblin Cadets", None);
    resolve(&mut game);
    let cadets = creature(&game, "Goblin Cadets").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    pass_until(&mut game, THEM, Step::Upkeep);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(cadets, THEM)]));
    loop {
        if let Decision::Blockers { .. } = game.decision() {
            break;
        }
        step(&mut game, Answer::Act(Action::Pass));
    }
    let d = step(&mut game, Answer::Block(vec![(bears, cadets)]));
    let Decision::Targets { player, .. } = d else {
        panic!("a target opponent: {d:?}")
    };
    assert_eq!(player, ME);
    step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    assert_eq!(
        game.pending().len() + game.stack().len(),
        1,
        "it triggers once"
    );
    resolve(&mut game);
    assert_eq!(game.characteristics()[&cadets].controller, THEM);
    assert!(game.attacks().is_empty(), "removed from combat");
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 20);
    assert!(game.permanents().contains_key(&bears));
    assert!(game.permanents().contains_key(&cadets));
    assert_eq!(game.permanents()[&cadets].since, game.turn());
}

/// CR 305.6, 613.1d: Prismatic Omen's "Lands you control are every basic
/// land type in addition to their other types" makes my Forests Mountains
/// and Islands too, with each type's mana ability, and leaves their lands
/// alone.
#[test]
fn prismatic_omen_makes_my_lands_every_basic_land_type() {
    use chancellor_of_the_spires::model::{Ability, BasicLandType, Subtype};
    let mut game = deal_both(
        &[("Forest", 40), ("Prismatic Omen", 20)],
        &[("Forest", 60)],
        &[("Forest", 2), ("Prismatic Omen", 1)],
        &[("Forest", 1)],
    );
    play(&mut game, "Forest");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Prismatic Omen", None);
    resolve(&mut game);
    let chars = game.characteristics();
    let lands: Vec<_> = game
        .permanents()
        .iter()
        .filter(|(_, p)| game.card(p.card).unwrap().name == "Forest")
        .map(|(id, _)| (*id, chars[id].controller))
        .collect();
    assert_eq!(lands.len(), 3);
    for (id, controller) in lands {
        let c = &chars[&id];
        let mine = controller == ME;
        for t in [BasicLandType::Mountain, BasicLandType::Island] {
            assert_eq!(c.subtypes.contains(&Subtype::BasicLand(t)), mine);
            assert_eq!(c.abilities.contains(&Ability::Intrinsic(t)), mine);
        }
        assert!(c
            .abilities
            .contains(&Ability::Intrinsic(BasicLandType::Forest)));
    }
}

/// CR 603.6a: Hour of Promise puts two Valakuts onto the battlefield at
/// once, Mountains under Prismatic Omen, so each sees both enter: four
/// triggers, with five Forests and the other Valakut each time.
#[test]
fn two_permanents_entering_at_once_each_see_the_other() {
    let valakut = "Valakut, the Molten Pinnacle";
    let mut game = deal(
        &[
            ("Forest", 30),
            ("Prismatic Omen", 10),
            ("Hour of Promise", 10),
            (valakut, 10),
        ],
        &[("Forest", 60)],
        &[("Forest", 5), ("Prismatic Omen", 1), ("Hour of Promise", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Prismatic Omen", None);
    resolve(&mut game);
    for _ in 0..3 {
        my_next_main(&mut game);
        play(&mut game, "Forest");
    }
    // Each Forest taps for any colour now; green pays all of it.
    let hour = in_hand(&game, ME, "Hour of Promise").unwrap();
    let mut d = step(&mut game, Answer::Act(Action::Cast(hour)));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| {
                actions.iter().find(|a| {
                    matches!(a, Payment::Mana(m)
                        if m.ability == chancellor_of_the_spires::model::Ability::Intrinsic(
                            chancellor_of_the_spires::model::BasicLandType::Forest))
                })
            })
            .copied()
            .expect("a way to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    let mut d = resolve(&mut game);
    for _ in 0..2 {
        let Decision::Search { known, .. } = d else {
            panic!("{d:?}")
        };
        let v = known
            .iter()
            .copied()
            .find(|c| game.card(*c).is_some_and(|t| t.name == valakut))
            .expect("a Valakut in the library");
        d = step(&mut game, Answer::Find(Found::Card(v)));
    }
    let mut triggers = 0;
    loop {
        d = match game.decision() {
            Decision::Targets { .. } => {
                triggers += 1;
                step(&mut game, Answer::Targets(vec![Target::Player(THEM)]))
            }
            Decision::Order { triggers: t, .. } => {
                step(&mut game, Answer::Order((0..t.len()).collect()))
            }
            _ => break,
        };
    }
    let _ = d;
    assert_eq!(triggers, 4);
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

/// CR 603.2: Brineborn Cutthroat's "Whenever you cast a spell during an
/// opponent's turn" says whose turn as part of its event. A Lightning Bolt
/// cast in my own main phase doesn't trigger it, one cast in their upkeep
/// does, and the counter makes it 3/2.
#[test]
fn a_cast_trigger_during_an_opponents_turn() {
    let mut game = deal(
        &[
            ("Island", 20),
            ("Mountain", 20),
            ("Brineborn Cutthroat", 10),
            ("Lightning Bolt", 10),
        ],
        &[("Forest", 60)],
        &[
            ("Island", 1),
            ("Mountain", 2),
            ("Brineborn Cutthroat", 1),
            ("Lightning Bolt", 2),
        ],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Brineborn Cutthroat", None);
    resolve(&mut game);
    let cutthroat = creature(&game, "Brineborn Cutthroat").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    assert_eq!(game.stack().len(), 1, "my own turn: no trigger");
    resolve(&mut game);
    assert_eq!(pt_of(&game, cutthroat), pt(2, 1));
    pass_until(&mut game, THEM, Step::Upkeep);
    step(&mut game, Answer::Act(Action::Pass));
    assert!(matches!(
        game.decision(),
        Decision::Priority { player: ME, .. }
    ));
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    assert_eq!(game.stack().len(), 2, "their turn: it triggers");
    resolve(&mut game);
    assert_eq!(pt_of(&game, cutthroat), pt(3, 2));
}

/// CR 603.2, 105.2: Quirion Dryad's "Whenever you cast a spell that's
/// white, blue, black, or red" triggers on a red Lightning Bolt and not on
/// a green Giant Growth.
#[test]
fn a_cast_trigger_on_a_spell_that_is_one_of_its_colours() {
    let mut game = deal(
        &[
            ("Forest", 20),
            ("Mountain", 20),
            ("Quirion Dryad", 10),
            ("Lightning Bolt", 5),
            ("Giant Growth", 5),
        ],
        &[("Island", 60)],
        &[
            ("Forest", 2),
            ("Mountain", 1),
            ("Quirion Dryad", 1),
            ("Lightning Bolt", 1),
            ("Giant Growth", 1),
        ],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Quirion Dryad", None);
    resolve(&mut game);
    let dryad = creature(&game, "Quirion Dryad").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Giant Growth", Some(vec![Target::Object(dryad)]));
    assert_eq!(game.stack().len(), 1, "green: no trigger");
    resolve(&mut game);
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    assert_eq!(game.stack().len(), 2, "red: it triggers");
    resolve(&mut game);
    resolve(&mut game);
    assert_eq!(pt_of(&game, dryad), pt(5, 5));
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

/// CR 107.1c, 101.4, 110.2a: Tempting Wurm's enters trigger has its
/// opponent choose any number of the artifact, creature, enchantment and
/// land cards of their hand, none included; an instant isn't one. What
/// they choose enters under their control, all at once, so Soul Warden
/// sees the Grizzly Bears beside it enter (CR 603.6a).
#[test]
fn tempting_wurm_lets_its_opponent_put_permanents_from_their_hand() {
    let mut game = deal_both(
        &[("Forest", 30), ("Tempting Wurm", 30)],
        &[
            ("Island", 15),
            ("Grizzly Bears", 15),
            ("Soul Warden", 15),
            ("Lightning Bolt", 15),
        ],
        &[("Forest", 2), ("Tempting Wurm", 1)],
        &[
            ("Island", 1),
            ("Grizzly Bears", 1),
            ("Soul Warden", 1),
            ("Lightning Bolt", 1),
        ],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Tempting Wurm", None);
    resolve(&mut game);
    let d = resolve(&mut game);
    let Decision::ChooseMatching {
        player,
        cards,
        up_to,
        total_mana_value,
        ..
    } = d
    else {
        panic!("{d:?}")
    };
    assert_eq!(player, THEM);
    assert_eq!(cards.len(), game.player(THEM).hand().len());
    assert_eq!((up_to as usize, total_mana_value), (cards.len(), None));
    let theirs = |name: &str| in_hand(&game, THEM, name).expect("in their hand");
    let (island, bears, warden, bolt) = (
        theirs("Island"),
        theirs("Grizzly Bears"),
        theirs("Soul Warden"),
        theirs("Lightning Bolt"),
    );
    assert!(matches!(
        game.step(Answer::Finds(vec![Found::Card(bears), Found::Card(bolt)])),
        Err(Rejected::Illegal(_))
    ));
    let mut none = game.clone();
    step(&mut none, Answer::Finds(vec![]));
    assert!(creature(&none, "Grizzly Bears").is_none());
    step(
        &mut game,
        Answer::Finds(vec![
            Found::Card(island),
            Found::Card(bears),
            Found::Card(warden),
        ]),
    );
    let chars = game.characteristics();
    for name in ["Island", "Grizzly Bears", "Soul Warden"] {
        let id = creature(&game, name).unwrap_or_else(|| panic!("{name} entered"));
        assert_eq!(chars[&id].controller, THEM, "{name}");
    }
    assert!(in_hand(&game, THEM, "Lightning Bolt").is_some());
    assert_eq!(
        chars[&creature(&game, "Tempting Wurm").unwrap()].controller,
        ME
    );
    // Soul Warden's trigger, for the Bears only: it doesn't see itself.
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 21);
    assert!(game.stack().is_empty());
}

/// A card whose characteristics can't be read may or may not be a
/// permanent card, so Tempting Wurm's opponent choosing it is refused by
/// name, never rejected as one the filter doesn't match.
#[test]
fn a_card_the_layers_cant_read_is_refused_not_unmatched() {
    let mut game = deal_both(
        &[("Forest", 30), ("Tempting Wurm", 30)],
        &[("Island", 30), (UNREADABLE, 30)],
        &[("Forest", 2), ("Tempting Wurm", 1)],
        &[(UNREADABLE, 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Tempting Wurm", None);
    resolve(&mut game);
    let d = resolve(&mut game);
    assert!(
        matches!(d, Decision::ChooseMatching { player: THEM, .. }),
        "{d:?}"
    );
    let gizmo = in_hand(&game, THEM, UNREADABLE).expect("in their hand");
    let refused = game.step(Answer::Finds(vec![Found::Card(gizmo)]));
    assert!(
        matches!(&refused, Err(Rejected::Refused { card, .. }) if card == UNREADABLE),
        "{refused:?}"
    );
}

/// CR 101.4, 110.2a, 608.2d: Show and Tell has each player, the active
/// player first, choose up to one artifact, creature, enchantment or land
/// card of their hand; two is too many and none is the "may" declined.
/// What is chosen enters once both have chosen, under the control of the
/// player who put it.
#[test]
fn show_and_tell_has_each_player_put_up_to_one_card_from_their_hand() {
    let mut game = deal_both(
        &[("Island", 30), ("Show and Tell", 15), ("Grizzly Bears", 15)],
        &[("Forest", 30), ("Craw Wurm", 30)],
        &[("Island", 3), ("Show and Tell", 1), ("Grizzly Bears", 2)],
        &[("Craw Wurm", 1)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    cast(&mut game, "Show and Tell", None);
    let d = resolve(&mut game);
    let Decision::ChooseMatching { player, up_to, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!((player, up_to), (ME, 1));
    let bears: Vec<CardId> = game
        .player(ME)
        .hand()
        .iter()
        .copied()
        .filter(|c| game.card(*c).is_some_and(|t| t.name == "Grizzly Bears"))
        .collect();
    assert!(matches!(
        game.step(Answer::Finds(
            bears.iter().map(|c| Found::Card(*c)).collect()
        )),
        Err(Rejected::Illegal(_))
    ));
    let d = step(&mut game, Answer::Finds(vec![Found::Card(bears[0])]));
    let Decision::ChooseMatching { player, up_to, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!((player, up_to), (THEM, 1));
    assert!(
        creature(&game, "Grizzly Bears").is_none(),
        "not before both choose"
    );
    step(&mut game, Answer::Finds(vec![]));
    let chars = game.characteristics();
    assert_eq!(
        chars[&creature(&game, "Grizzly Bears").unwrap()].controller,
        ME
    );
    assert!(creature(&game, "Craw Wurm").is_none());
    assert!(in_hand(&game, THEM, "Craw Wurm").is_some());
    assert!(in_hand(&game, ME, "Grizzly Bears").is_some());
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

/// CR 700.3: Fact or Fiction reveals the top five, the opponent separates
/// them into two piles, either of which may be empty (CR 700.3d), and I
/// put the one I choose into my hand and the other into my graveyard.
/// Each card goes into exactly one pile (CR 700.3a).
#[test]
fn fact_or_fiction_an_opponent_separates_the_five_and_i_take_a_pile() {
    let mut game = deal(
        &[("Island", 30), ("Fact or Fiction", 30)],
        &[("Forest", 60)],
        &[("Island", 4), ("Fact or Fiction", 1)],
    );
    for _ in 0..3 {
        play(&mut game, "Island");
        my_next_main(&mut game);
    }
    play(&mut game, "Island");
    cast(&mut game, "Fact or Fiction", None);
    let five: Vec<CardId> = game.player(ME).library().iter().take(5).copied().collect();
    let Decision::SeparatePiles { player, cards } = resolve(&mut game) else {
        panic!("piles to separate")
    };
    assert_eq!((player, cards), (THEM, five.clone()));
    let mine = in_hand(&game, ME, "Island").unwrap();
    assert!(game.clone().step(Answer::Piles(vec![mine])).is_err());
    assert!(game
        .clone()
        .step(Answer::Piles(vec![five[0], five[0]]))
        .is_err());
    let Decision::ChoosePile { player, piles } = step(&mut game, Answer::Piles(vec![five[3]]))
    else {
        panic!("a pile to choose")
    };
    let rest = vec![five[0], five[1], five[2], five[4]];
    assert_eq!((player, piles.clone()), (ME, [vec![five[3]], rest.clone()]));
    let hand = game.player(ME).hand().len();
    step(&mut game, Answer::Pile(1));
    assert_eq!(game.player(ME).hand().len(), hand + 4);
    assert!(rest.iter().all(|c| game.player(ME).hand().contains(c)));
    assert!(game.player(ME).graveyard().contains(&five[3]));
}

/// CR 401.4: Sleight of Hand looks at the top two, puts the one chosen
/// into my hand and the other on the bottom, no order to give for one.
#[test]
fn sleight_of_hand_takes_one_of_two_and_bottoms_the_other() {
    let mut game = deal(
        &[("Island", 30), ("Sleight of Hand", 30)],
        &[("Forest", 60)],
        &[("Island", 1), ("Sleight of Hand", 1)],
    );
    play(&mut game, "Island");
    cast(&mut game, "Sleight of Hand", None);
    let two: Vec<CardId> = game.player(ME).library().iter().take(2).copied().collect();
    let Decision::ChooseAmong { cards, n, .. } = resolve(&mut game) else {
        panic!("a choice of one")
    };
    assert_eq!((cards, n), (two.clone(), 1));
    step(&mut game, Answer::Choose(vec![two[1]]));
    assert!(game.player(ME).hand().contains(&two[1]));
    assert_eq!(game.player(ME).library().back(), Some(&two[0]));
    assert_eq!(game.bottomed(), 1);
}

/// CR 400.3: Strategic Planning looks at the top three, puts the one
/// chosen into my hand and the other two into my graveyard.
#[test]
fn strategic_planning_takes_one_of_three_and_puts_the_rest_in_the_graveyard() {
    let mut game = deal(
        &[("Island", 30), ("Strategic Planning", 30)],
        &[("Forest", 60)],
        &[("Island", 2), ("Strategic Planning", 1)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    cast(&mut game, "Strategic Planning", None);
    let three: Vec<CardId> = game.player(ME).library().iter().take(3).copied().collect();
    let Decision::ChooseAmong { cards, n, .. } = resolve(&mut game) else {
        panic!("a choice of one")
    };
    assert_eq!((cards, n), (three.clone(), 1));
    step(&mut game, Answer::Choose(vec![three[0]]));
    assert!(game.player(ME).hand().contains(&three[0]));
    let graveyard = game.player(ME).graveyard();
    assert!(graveyard.contains(&three[1]) && graveyard.contains(&three[2]));
    assert!(!game.player(ME).library().contains(&three[1]));
    assert_eq!(game.bottomed(), 0);
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

/// Void (CR 207.2c): Plasma Bolt deals 2 damage, or 3 instead once a
/// nonland permanent has left the battlefield this turn, Mogg Fanatic
/// sacrificed (CR 701.21a); a land played doesn't count, nor does the
/// turn before.
#[test]
fn plasma_bolt_deals_three_once_a_nonland_permanent_left_this_turn() {
    let mut game = deal(
        &[("Mountain", 20), ("Plasma Bolt", 20), ("Mogg Fanatic", 20)],
        &[("Forest", 60)],
        &[("Mountain", 3), ("Plasma Bolt", 2), ("Mogg Fanatic", 2)],
    );
    let sacrifice = |game: &mut Game| {
        cast(game, "Mogg Fanatic", None);
        resolve(game);
        let fanatic = creature(game, "Mogg Fanatic").unwrap();
        activate(game, fanatic);
        step(game, Answer::Targets(vec![Target::Player(THEM)]));
        resolve(game);
    };
    play(&mut game, "Mountain");
    sacrifice(&mut game);
    assert_eq!(game.player(THEM).life(), 19);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Plasma Bolt", Some(vec![Target::Player(THEM)]));
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 17, "the Fanatic left last turn");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    sacrifice(&mut game);
    assert_eq!(game.player(THEM).life(), 16);
    cast(&mut game, "Plasma Bolt", Some(vec![Target::Player(THEM)]));
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 13, "the Fanatic left this turn");
}

/// Void counts a nonland permanent only: a land that left the
/// battlefield this turn, Evolving Wilds sacrificed for its search (CR
/// 701.21a), leaves Plasma Bolt at 2 damage.
#[test]
fn plasma_bolt_deals_two_when_only_a_land_left_this_turn() {
    let mut game = deal(
        &[
            ("Mountain", 20),
            ("Evolving Wilds", 20),
            ("Plasma Bolt", 20),
        ],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Evolving Wilds", 1), ("Plasma Bolt", 1)],
    );
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Evolving Wilds");
    let wilds = creature(&game, "Evolving Wilds").expect("on the battlefield");
    activate(&mut game, wilds);
    assert!(matches!(resolve(&mut game), Decision::Search { .. }));
    step(&mut game, Answer::Find(Found::Nothing));
    assert!(creature(&game, "Evolving Wilds").is_none(), "sacrificed");
    cast(&mut game, "Plasma Bolt", Some(vec![Target::Player(THEM)]));
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 18, "only a land left");
}

/// Void's "or a spell was warped this turn" never holds in a game the
/// engine runs, because it refuses every card with warp (CR 702.185c), so
/// Plasma Bolt's lowering reads only the other half.
#[test]
fn a_warp_card_is_refused() {
    // A vanilla creature but for warp, so nothing else refuses it first.
    let warp = CardText {
        oracle: Oracle::OneFace("Warp {R}".into()),
        ..card("Raging Goblin")
    };
    let (refused, _) = Game::reads(&warp);
    assert_eq!(refused.expect("refused").node, "keyword warp");
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

/// CR 602.2b, 601.2h, 701.21a: Siege-Gang Commander's "{1}{R}, Sacrifice
/// a Goblin: It deals 2 damage to any target" takes its target, then its
/// mana, then a Goblin of its controller's, itself one of them; with no
/// Goblin left it can't be activated.
#[test]
fn siege_gang_commander_sacrifices_a_goblin_for_two_damage() {
    let mut game = deal(
        &[("Mountain", 56), ("Siege-Gang Commander", 4)],
        &[("Forest", 60)],
        &[("Mountain", 6), ("Siege-Gang Commander", 1)],
    );
    play(&mut game, "Mountain");
    for _ in 0..4 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    cast(&mut game, "Siege-Gang Commander", None);
    resolve(&mut game);
    resolve(&mut game);
    for _ in 0..2 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    let sgc = creature(&game, "Siege-Gang Commander").unwrap();
    let goblins: Vec<_> = game
        .characteristics()
        .iter()
        .filter(|(_, c)| c.controller == ME && c.is(CardType::Creature))
        .map(|(o, _)| *o)
        .collect();
    assert_eq!(goblins.len(), 4, "itself and three tokens");
    let Decision::Targets { .. } = activate(&mut game, sgc) else {
        panic!("a target: {:?}", game.decision())
    };
    let mut d = step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    let Decision::Sacrifice { count, legal, .. } = d else {
        panic!("a sacrifice: {d:?}")
    };
    assert_eq!(count, 1);
    assert_eq!(
        legal
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        goblins.iter().copied().collect(),
        "any Goblin of mine, itself too"
    );
    assert!(
        game.clone().step(Answer::Sacrifice(vec![])).is_err(),
        "one Goblin, not none"
    );
    step(&mut game, Answer::Sacrifice(vec![sgc]));
    assert!(
        !game.permanents().contains_key(&sgc),
        "sacrificed as a cost"
    );
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 18, "as it last existed");
}

/// CR 601.2h, 733.1: paying Siege-Gang Commander's {1}{R} with Skirk
/// Prospector's "Sacrifice a Goblin: Add {R}", the Prospector can't
/// sacrifice the last Goblin the ability's own cost needs, which would
/// leave that cost unpaid and the activation reversed.
#[test]
fn a_mana_ability_cant_sacrifice_what_the_cost_still_needs() {
    let mut game = deal(
        &[
            ("Mountain", 52),
            ("Siege-Gang Commander", 4),
            ("Skirk Prospector", 4),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 5),
            ("Siege-Gang Commander", 1),
            ("Skirk Prospector", 1),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Skirk Prospector", None);
    resolve(&mut game);
    // Five for the Commander, one for the ability's {1}.
    for _ in 0..5 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    // Paid with Mountains, not the Prospector.
    let c = in_hand(&game, ME, "Siege-Gang Commander").unwrap();
    let mut d = step(&mut game, Answer::Act(Action::Cast(c)));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| {
                actions
                    .iter()
                    .find(|a| matches!(a, Payment::Mana(m) if m.sacrifices.is_none()))
            })
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    // Its enters trigger waits on the stack: no tokens yet.
    let sgc = creature(&game, "Siege-Gang Commander").unwrap();
    let skirk = creature(&game, "Skirk Prospector").unwrap();
    let Decision::Targets { .. } = activate(&mut game, sgc) else {
        panic!("a target: {:?}", game.decision())
    };
    let d = step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    let Decision::Pay { actions, .. } = d else {
        panic!("{d:?}")
    };
    let skirk_sacrificing = |actions: &[Payment], o| {
        actions
            .iter()
            .find(|a| matches!(a, Payment::Mana(m) if m.source == skirk && m.sacrifices == Some(o)))
            .copied()
    };
    let first = skirk_sacrificing(&actions, sgc).expect("two Goblins: one may go for mana");
    let Decision::Pay { actions, .. } = step(&mut game, Answer::Pay(first)) else {
        panic!("{:?}", game.decision())
    };
    assert!(
        skirk_sacrificing(&actions, skirk).is_none(),
        "the last Goblin is the cost's"
    );
    let mut d = game.decision();
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| {
                actions
                    .iter()
                    .find(|a| matches!(a, Payment::Mana(m) if m.sacrifices.is_none()))
            })
            .copied()
            .unwrap();
        d = step(&mut game, Answer::Pay(pay));
    }
    let Decision::Sacrifice { legal, .. } = d else {
        panic!("a sacrifice: {d:?}")
    };
    assert_eq!(legal, vec![skirk]);
}

/// CR 602.2b, 701.21a: Ravenous Baloth's "Sacrifice a Beast: You gain 4
/// life", with no mana or target, asks for the Beast straight away.
#[test]
fn ravenous_baloth_sacrifices_a_beast_for_four_life() {
    let mut game = deal(
        &[("Forest", 40), ("Ravenous Baloth", 20)],
        &[("Forest", 60)],
        &[("Forest", 4), ("Ravenous Baloth", 1)],
    );
    play(&mut game, "Forest");
    for _ in 0..3 {
        my_next_main(&mut game);
        play(&mut game, "Forest");
    }
    cast(&mut game, "Ravenous Baloth", None);
    resolve(&mut game);
    let baloth = creature(&game, "Ravenous Baloth").unwrap();
    let Decision::Sacrifice { count, legal, .. } = activate(&mut game, baloth) else {
        panic!("a sacrifice: {:?}", game.decision())
    };
    assert_eq!((count, legal), (1, vec![baloth]));
    step(&mut game, Answer::Sacrifice(vec![baloth]));
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 24);
}

/// CR 602.2b, 118.3: Triskelion enters with three +1/+1 counters (CR
/// 614.1c), and "Remove a +1/+1 counter from this creature: It deals 1
/// damage to any target" takes one off as each activation is paid; with
/// none left it can't be activated.
#[test]
fn triskelion_removes_a_counter_for_each_damage() {
    use chancellor_of_the_spires::model::Counter;
    let mut game = deal(
        &[("Mountain", 40), ("Triskelion", 20)],
        &[("Forest", 60)],
        &[("Mountain", 6), ("Triskelion", 1)],
    );
    play(&mut game, "Mountain");
    for _ in 0..5 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    cast(&mut game, "Triskelion", None);
    resolve(&mut game);
    let trisk = creature(&game, "Triskelion").unwrap();
    assert_eq!(game.counters_on(trisk, &Counter::PlusOne), 3);
    for left in [2, 1, 0] {
        let Decision::Targets { .. } = activate(&mut game, trisk) else {
            panic!("a target: {:?}", game.decision())
        };
        assert_eq!(game.counters_on(trisk, &Counter::PlusOne), left, "paid");
        step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
        resolve(&mut game);
    }
    assert_eq!(game.player(THEM).life(), 17);
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Activate(x) if x.source == trisk)),
        "no counter left to remove"
    );
}

/// CR 702.193a, 702.193b: Quicksilver's power-up costs {4}{R} less its
/// {R} the turn it entered, so {4}; its "+1/+1 counter and a double
/// strike counter" give it double strike (CR 122.1b, 613.1f), and it is
/// activated only once.
#[test]
fn quicksilver_powers_up_once_for_less_the_turn_it_entered() {
    use chancellor_of_the_spires::model::{Counter, Keyword};
    let mut game = deal(
        &[("Mountain", 40), ("Quicksilver, Brash Blur", 20)],
        &[("Forest", 60)],
        &[("Mountain", 5), ("Quicksilver, Brash Blur", 1)],
    );
    play(&mut game, "Mountain");
    for _ in 0..4 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    cast(&mut game, "Quicksilver, Brash Blur", None);
    resolve(&mut game);
    let qs = creature(&game, "Quicksilver, Brash Blur").unwrap();
    let mut d = activate(&mut game, qs);
    let Decision::Pay { owed, .. } = &d else {
        panic!("{d:?}")
    };
    assert_eq!((owed.generic, owed.shards.len()), (4, 0), "{owed:?}");
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
    assert_eq!(game.counters_on(qs, &Counter::PlusOne), 1);
    assert_eq!(
        game.counters_on(qs, &Counter::Keyword(Keyword::DoubleStrike)),
        1
    );
    let chars = &game.characteristics()[&qs];
    assert!(chars.has(Keyword::DoubleStrike));
    assert_eq!((pt_of(&game, qs).power, pt_of(&game, qs).toughness), (2, 2));
    my_next_main(&mut game);
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a, Action::Activate(x) if x.source == qs)),
        "only once"
    );
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

/// CR 106.1a: Black Lotus's "Add three mana of any one color" is three of
/// one color chosen, five ways, never a mix.
#[test]
fn three_mana_of_any_one_color() {
    let mut game = deal(
        &[("Black Lotus", 30), ("Island", 30)],
        &[("Mountain", 60)],
        &[("Black Lotus", 1), ("Island", 1)],
    );
    cast(&mut game, "Black Lotus", None);
    resolve(&mut game);
    let lotus = game
        .permanents()
        .iter()
        .find(|(_, p)| game.card(p.card).is_some_and(|t| t.name == "Black Lotus"))
        .map(|(id, _)| *id)
        .unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!()
    };
    let adds: Vec<String> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Mana(m) if m.source == lotus => Some(m.adds.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        adds,
        vec![
            "{W}{W}{W}",
            "{U}{U}{U}",
            "{B}{B}{B}",
            "{R}{R}{R}",
            "{G}{G}{G}"
        ]
    );
}

/// CR 305.1, 614.15: River of Tears adds {U}, or {B} instead once I've
/// played a land this turn: itself, the turn it's played, and the Island
/// played the turn after, but not before that Island.
#[test]
fn river_of_tears_adds_black_once_a_land_is_played() {
    let mut game = deal(
        &[("River of Tears", 30), ("Island", 30)],
        &[("Mountain", 60)],
        &[("River of Tears", 1), ("Island", 1)],
    );
    play(&mut game, "River of Tears");
    let river = game
        .permanents()
        .iter()
        .find(|(_, p)| {
            game.card(p.card)
                .is_some_and(|t| t.name == "River of Tears")
        })
        .map(|(id, _)| *id)
        .unwrap();
    let adds = |game: &Game| -> Vec<String> {
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!()
        };
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Mana(m) if m.source == river => Some(m.adds.to_string()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(adds(&game), vec!["{B}"]);
    my_next_main(&mut game);
    assert_eq!(adds(&game), vec!["{U}"]);
    play(&mut game, "Island");
    assert_eq!(adds(&game), vec!["{B}"]);
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

/// Quest for Pure Flame's "Remove four quest counters from this
/// enchantment and sacrifice it: If any source you control would deal
/// damage to a permanent or player this turn, it deals double that damage
/// to that permanent or player instead": a replacement effect (CR 614.1a)
/// on my sources' damage to anyone, me too, which ends in cleanup (CR
/// 514.2).
#[test]
fn quest_for_pure_flame_doubles_my_sources_damage_this_turn() {
    let mut game = deal(
        &[
            ("Mountain", 6),
            ("Quest for Pure Flame", 2),
            ("Lightning Bolt", 52),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 3),
            ("Quest for Pure Flame", 1),
            ("Lightning Bolt", 3),
        ],
    );
    let bolt_them = |game: &mut Game| {
        cast(game, "Lightning Bolt", Some(vec![Target::Player(THEM)]));
        resolve(game);
        if let Decision::May { .. } = resolve(game) {
            step(game, Answer::May(true));
        }
    };
    play(&mut game, "Mountain");
    cast(&mut game, "Quest for Pure Flame", None);
    resolve(&mut game);
    let quest = creature(&game, "Quest for Pure Flame").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    bolt_them(&mut game);
    bolt_them(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    bolt_them(&mut game);
    bolt_them(&mut game);
    let quest_counter = chancellor_of_the_spires::model::Counter::Named("quest".into());
    assert_eq!(game.counters_on(quest, &quest_counter), 4);
    assert_eq!(game.player(THEM).life(), 8);
    activate(&mut game, quest);
    resolve(&mut game);
    assert_eq!(creature(&game, "Quest for Pure Flame"), None, "sacrificed");
    cast(&mut game, "Lightning Bolt", Some(vec![Target::Player(ME)]));
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 14, "my own face, doubled");
    my_next_main(&mut game);
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 5, "ended in cleanup");
}

/// Quest for Pure Flame doubles only damage from sources its controller
/// controls: their Lightning Bolt, a spell they cast and so control (CR
/// 112.2), deals me 3.
#[test]
fn quest_for_pure_flame_leaves_an_opponents_spell_alone() {
    let mut game = deal_both(
        &[
            ("Mountain", 6),
            ("Quest for Pure Flame", 2),
            ("Lightning Bolt", 52),
        ],
        &[("Mountain", 30), ("Lightning Bolt", 30)],
        &[
            ("Mountain", 3),
            ("Quest for Pure Flame", 1),
            ("Lightning Bolt", 3),
        ],
        &[("Mountain", 2), ("Lightning Bolt", 1)],
    );
    let bolt_them = |game: &mut Game| {
        cast(game, "Lightning Bolt", Some(vec![Target::Player(THEM)]));
        resolve(game);
        if let Decision::May { .. } = resolve(game) {
            step(game, Answer::May(true));
        }
    };
    let their_turn = |game: &mut Game| {
        pass_until(game, THEM, Step::PrecombatMain);
        play_as(game, THEM, "Mountain");
        pass_until(game, ME, Step::PrecombatMain);
    };
    play(&mut game, "Mountain");
    cast(&mut game, "Quest for Pure Flame", None);
    resolve(&mut game);
    let quest = creature(&game, "Quest for Pure Flame").unwrap();
    their_turn(&mut game);
    play(&mut game, "Mountain");
    bolt_them(&mut game);
    bolt_them(&mut game);
    their_turn(&mut game);
    play(&mut game, "Mountain");
    bolt_them(&mut game);
    bolt_them(&mut game);
    activate(&mut game, quest);
    resolve(&mut game);
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    step(&mut game, Answer::Act(Action::Pass));
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Player(ME)]),
    );
    resolve(&mut game);
    assert_eq!(
        game.player(ME).life(),
        17,
        "their source isn't one I control"
    );
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 2, "mine is doubled");
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
        let leak = in_hand(&game, ME, "Mana Leak").unwrap();
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
            assert!(game.countered().is_empty());
        } else {
            step(&mut game, Answer::Pay(Payment::Cancel));
            assert!(game.player(THEM).graveyard().contains(&bears));
            assert!(game.stack().is_empty());
            // CR 701.6a: the record the replay checks a log's
            // "counters [Grizzly Bears] with [Mana Leak]" against.
            assert_eq!(
                game.countered().iter().copied().collect::<Vec<_>>(),
                vec![(bears, leak)]
            );
        }
    }
}

/// As `pass_until`, declining every "may" on the way.
fn pass_until_declining(game: &mut Game, who: PlayerId, s: Step) {
    for _ in 0..1000 {
        match game.decision() {
            Decision::May { .. } => {
                step(game, Answer::May(false));
            }
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

/// CR 608.2h, 118.12a: Circular Logic counters the spell unless its
/// controller pays {1} for each card in Circular Logic's controller's
/// graveyard, counted as it resolves, with Circular Logic itself still on
/// the stack (CR 608.2n).
#[test]
fn circular_logic_asks_one_for_each_card_in_its_controllers_graveyard() {
    for pays in [false, true] {
        let mut game = deal_both(
            &[("Island", 40), ("Circular Logic", 20)],
            &[("Forest", 40), ("Grizzly Bears", 20)],
            &[("Island", 2), ("Circular Logic", 1)],
            &[("Forest", 5), ("Grizzly Bears", 1)],
        );
        play(&mut game, "Island");
        let mut islands = 1;
        // Three Islands and no more, so the cleanup discards the rest.
        for _ in 0..2 {
            pass_until_declining(&mut game, THEM, Step::PrecombatMain);
            if in_hand(&game, THEM, "Forest").is_some() {
                play_as(&mut game, THEM, "Forest");
            }
            // A Circular Logic the cleanup discards is exiled by its
            // madness (CR 702.35a), and not cast: it goes to the graveyard.
            pass_until_declining(&mut game, THEM, Step::Upkeep);
            pass_until_declining(&mut game, ME, Step::PrecombatMain);
            if islands < 3 && in_hand(&game, ME, "Island").is_some() {
                play(&mut game, "Island");
                islands += 1;
            }
        }
        pass_until_declining(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Forest");
        let mine = game.player(ME).graveyard().len() as u32;
        assert!(mine > 0, "the cleanup discarded");
        assert_ne!(mine, game.player(THEM).graveyard().len() as u32);
        let bears = in_hand(&game, THEM, "Grizzly Bears").unwrap();
        cast_as(&mut game, THEM, "Grizzly Bears", None);
        step(&mut game, Answer::Act(Action::Pass));
        cast(
            &mut game,
            "Circular Logic",
            Some(vec![Target::Spell(bears)]),
        );
        let Decision::PayUnless {
            player,
            owed,
            actions,
            ..
        } = resolve(&mut game)
        else {
            panic!("{:?}", game.decision())
        };
        assert_eq!((player, owed.generic), (THEM, mine));
        if pays {
            let mut actions = actions;
            loop {
                let pay = actions
                    .iter()
                    .find(|a| matches!(a, Payment::Spend(_)))
                    .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
                    .copied()
                    .expect("Forests untapped");
                match step(&mut game, Answer::Pay(pay)) {
                    Decision::PayUnless { actions: a, .. } => actions = a,
                    _ => break,
                }
            }
            resolve(&mut game);
            assert!(creature(&game, "Grizzly Bears").is_some());
        } else {
            step(&mut game, Answer::Pay(Payment::Cancel));
            assert!(game.player(THEM).graveyard().contains(&bears));
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

/// CR 701.6a, 113.1c: Stifle counters Phyrexian Dreadnought's enters
/// trigger, an ability on the stack, so it is never sacrificed; with
/// nothing on the stack it has nothing to target and isn't offered (CR
/// 601.2c).
#[test]
fn stifle_counters_phyrexian_dreadnoughts_trigger() {
    let mut game = deal(
        &[
            ("Island", 30),
            ("Phyrexian Dreadnought", 15),
            ("Stifle", 15),
        ],
        &[("Mountain", 60)],
        &[("Island", 2), ("Phyrexian Dreadnought", 1), ("Stifle", 1)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    let stifle = in_hand(&game, ME, "Stifle").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(
        !actions.contains(&Action::Cast(stifle)),
        "nothing to target"
    );
    cast(&mut game, "Phyrexian Dreadnought", None);
    resolve(&mut game);
    let Some(StackObject::Trigger { seq, .. }) = game.stack().back().cloned() else {
        panic!("its enters trigger: {:?}", game.stack())
    };
    let Decision::Targets { slots, .. } =
        step(&mut game.clone(), Answer::Act(Action::Cast(stifle)))
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(slots, vec![vec![Target::Ability(seq)]]);
    cast(&mut game, "Stifle", Some(vec![Target::Ability(seq)]));
    resolve(&mut game);
    assert!(game.stack().is_empty(), "the trigger is countered");
    assert!(game.player(ME).graveyard().contains(&stifle));
    assert!(creature(&game, "Phyrexian Dreadnought").is_some());
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

/// CR 602.2b, 601.2h, 701.9a: Survival of the Fittest's "{G}, Discard a
/// creature card" discards one chosen among the creature cards in hand as
/// it is paid, then its mana; the search finds a creature card into hand.
#[test]
fn survival_of_the_fittest_discards_a_creature_card_to_search_for_one() {
    let mut game = deal(
        &[
            ("Forest", 30),
            ("Survival of the Fittest", 10),
            ("Llanowar Elves", 20),
        ],
        &[("Swamp", 60)],
        &[
            ("Forest", 2),
            ("Survival of the Fittest", 1),
            ("Llanowar Elves", 1),
        ],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Survival of the Fittest", None);
    resolve(&mut game);
    my_next_main(&mut game);
    let survival = creature(&game, "Survival of the Fittest").expect("on the battlefield");
    let elves = in_hand(&game, ME, "Llanowar Elves").expect("dealt");
    let Decision::PayCards {
        count, legal, how, ..
    } = activate(&mut game, survival)
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(
        (count, how),
        (1, chancellor_of_the_spires::lower::PaidWith::Discard)
    );
    // Only creature cards: no Forest.
    assert!(legal
        .iter()
        .all(|c| game.card(*c).is_some_and(|t| t.name == "Llanowar Elves")));
    let mut d = step(&mut game, Answer::Choose(vec![elves]));
    assert!(game.player(ME).graveyard().contains(&elves));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a Forest");
        d = step(&mut game, Answer::Pay(pay));
    }
    let hand = game.player(ME).hand().len();
    let Decision::Search { known, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    let found = known
        .iter()
        .copied()
        .find(|c| game.card(*c).is_some_and(|t| t.name == "Llanowar Elves"))
        .expect("an Elves in the library");
    step(&mut game, Answer::Find(Found::Card(found)));
    assert_eq!(game.player(ME).hand().len(), hand + 1);
    assert!(game.player(ME).hand().contains(&found));
}

/// CR 602.2b, 601.2h: Grim Lavamancer's "{R}, {T}, Exile two cards from
/// your graveyard" isn't offered with one card there; with two it exiles
/// them as it is paid and deals 2 damage to its target.
#[test]
fn grim_lavamancer_exiles_two_cards_from_the_graveyard_to_deal_two_damage() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Grim Lavamancer", 10),
            ("Lightning Bolt", 20),
        ],
        &[("Swamp", 60)],
        &[
            ("Mountain", 2),
            ("Grim Lavamancer", 1),
            ("Lightning Bolt", 2),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Grim Lavamancer", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let lavamancer = creature(&game, "Grim Lavamancer").expect("on the battlefield");
    let offered = |game: &Game| {
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!("{:?}", game.decision())
        };
        actions
            .iter()
            .any(|a| matches!(a, Action::Activate(x) if x.source == lavamancer))
    };
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    resolve(&mut game);
    assert_eq!(game.player(ME).graveyard().len(), 1);
    assert!(!offered(&game), "one card in the graveyard isn't two");
    my_next_main(&mut game);
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Player(THEM)]),
    );
    resolve(&mut game);
    assert!(offered(&game));
    let Decision::Targets { .. } = activate(&mut game, lavamancer) else {
        panic!("{:?}", game.decision())
    };
    let Decision::PayCards { count, legal, .. } =
        step(&mut game, Answer::Targets(vec![Target::Player(THEM)]))
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!((count, legal.len()), (2, 2));
    let mut d = step(&mut game, Answer::Choose(legal));
    assert!(game.player(ME).graveyard().is_empty());
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a Mountain");
        d = step(&mut game, Answer::Pay(pay));
    }
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 20 - 3 - 3 - 2);
}

/// CR 702.62a, 116.2f: Rift Bolt suspended for {R} is exiled with a time
/// counter, without the stack; at my next upkeep the counter comes off,
/// and as the last does I may cast it without paying its mana cost,
/// choosing its target then.
#[test]
fn rift_bolt_suspended_is_cast_free_at_my_next_upkeep() {
    let mut game = deal(
        &[("Mountain", 40), ("Rift Bolt", 20)],
        &[("Swamp", 60)],
        &[("Mountain", 1), ("Rift Bolt", 1)],
    );
    play(&mut game, "Mountain");
    let bolt = in_hand(&game, ME, "Rift Bolt").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(actions.contains(&Action::Suspend(bolt)));
    let mut d = step(&mut game, Answer::Act(Action::Suspend(bolt)));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a Mountain");
        d = step(&mut game, Answer::Pay(pay));
    }
    assert!(game.stack().is_empty(), "a special action");
    assert_eq!(game.suspended().get(&bolt), Some(&1));
    assert!(game.exile().contains(&bolt));
    pass_until(&mut game, THEM, Step::Upkeep);
    // My upkeep: the counter's trigger, then the last one's "may".
    for _ in 0..40 {
        match game.decision() {
            Decision::May { card, .. } => {
                assert_eq!(card, bolt);
                break;
            }
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
    assert!(game.may_cast_suspended());
    let Decision::Targets { .. } = step(&mut game, Answer::May(true)) else {
        panic!("{:?}", game.decision())
    };
    let mut d = step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .copied()
            .expect("nothing to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    assert!(game.suspended().is_empty());
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 17);
    assert!(game.player(ME).graveyard().contains(&bolt));
}

/// CR 702.24a: cumulative upkeep puts an age counter on its permanent at
/// each of its controller's upkeeps, then asks the cost once for each age
/// counter: Elephant Grass's {1} paid in the first upkeep, {2} owed in the
/// second, and declined, sacrificing it.
#[test]
fn cumulative_upkeep_asks_its_cost_for_each_age_counter() {
    let mut game = deal(
        &[("Forest", 40), ("Elephant Grass", 20)],
        &[("Forest", 60)],
        &[("Forest", 1), ("Elephant Grass", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Elephant Grass", None);
    resolve(&mut game);
    let grass = creature(&game, "Elephant Grass").unwrap();
    pass_until(&mut game, THEM, Step::Upkeep);
    pass_until(&mut game, ME, Step::Upkeep);
    assert_eq!(game.stack().len(), 1, "cumulative upkeep triggers");
    let Decision::PayUnless { owed, actions, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(owed.to_string(), "{1}");
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
    assert!(game.permanents().contains_key(&grass));
    pass_until(&mut game, THEM, Step::Upkeep);
    pass_until(&mut game, ME, Step::Upkeep);
    let Decision::PayUnless { owed, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(owed.to_string(), "{2}", "two age counters");
    step(&mut game, Answer::Pay(Payment::Cancel));
    assert!(!game.permanents().contains_key(&grass));
}

/// CR 508.1c, 508.1h–j: Elephant Grass's "Black creatures can't attack
/// you" makes an attack by Walking Corpse illegal, and "Nonblack creatures
/// can't attack you unless their controller pays {2} for each creature
/// they control that's attacking you" makes Grizzly Bears's cost {2},
/// paid by tapping two lands, after which it attacks and deals its damage.
#[test]
fn elephant_grass_stops_black_attackers_and_taxes_the_rest() {
    let mut game = deal_both(
        &[
            ("Swamp", 20),
            ("Forest", 20),
            ("Walking Corpse", 10),
            ("Grizzly Bears", 10),
        ],
        &[("Forest", 40), ("Elephant Grass", 20)],
        &[
            ("Swamp", 1),
            ("Forest", 3),
            ("Walking Corpse", 1),
            ("Grizzly Bears", 1),
        ],
        &[("Forest", 3), ("Elephant Grass", 1)],
    );
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    cast(&mut game, "Walking Corpse", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Elephant Grass", None);
    resolve(&mut game);
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    let corpse = creature(&game, "Walking Corpse").unwrap();
    let bears = creature(&game, "Grizzly Bears").unwrap();
    to_attackers(&mut game);
    assert!(
        game.step(Answer::Attack(vec![(corpse, THEM)])).is_err(),
        "a black creature can't attack"
    );
    let Decision::PayAttack { owed, actions, .. } =
        step(&mut game, Answer::Attack(vec![(bears, THEM)]))
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(owed.to_string(), "{2}");
    let mut actions = actions;
    for _ in 0..2 {
        let tap = actions
            .iter()
            .find(|a| matches!(a, Payment::Mana(_)))
            .copied()
            .unwrap();
        let Decision::PayAttack { actions: next, .. } = step(&mut game, Answer::Pay(tap)) else {
            panic!("{:?}", game.decision())
        };
        actions = next;
    }
    let spend = actions
        .iter()
        .find(|a| matches!(a, Payment::Spend(_)))
        .copied()
        .unwrap();
    step(&mut game, Answer::Pay(spend));
    assert_eq!(game.attacks(), vec![(bears, THEM)]);
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
}

/// CR 702.29e: Eternal Dragon's plainscycling discards it for {2} and
/// searches for a Plains card, revealed, which goes to the hand; the
/// search offers only Plains.
#[test]
fn plainscycling_searches_for_a_plains_into_the_hand() {
    let mut game = deal(
        &[("Plains", 20), ("Island", 20), ("Eternal Dragon", 20)],
        &[("Forest", 60)],
        &[("Island", 2), ("Eternal Dragon", 1)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    let dragon = in_hand(&game, ME, "Eternal Dragon").unwrap();
    step(&mut game, Answer::Act(Action::Cycle(dragon)));
    while let Decision::Pay { actions, .. } = game.decision() {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        step(&mut game, Answer::Pay(pay));
    }
    assert!(game.player(ME).graveyard().contains(&dragon));
    let Decision::Search { known, reveals, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    assert!(reveals);
    assert!(!known.is_empty());
    assert!(known
        .iter()
        .all(|c| game.card(*c).is_some_and(|t| t.name == "Plains")));
    let plains = known[0];
    step(&mut game, Answer::Find(Found::Card(plains)));
    assert!(game.player(ME).hand().contains(&plains));
}

/// CR 608.2h: Powder Keg's "Destroy each artifact and creature with mana
/// value equal to the number of fuse counters on this artifact" counts
/// the counters as it resolves, on the Keg as it last existed, sacrificed
/// to pay: one fuse counter destroys Raging Goblin, of mana value 1, and
/// not Grizzly Bears, of 2.
#[test]
fn powder_keg_destroys_what_its_fuse_counters_count() {
    let mut game = deal(
        &[
            ("Mountain", 20),
            ("Forest", 20),
            ("Raging Goblin", 5),
            ("Grizzly Bears", 5),
            ("Powder Keg", 10),
        ],
        &[("Island", 60)],
        &[
            ("Mountain", 1),
            ("Forest", 2),
            ("Raging Goblin", 1),
            ("Grizzly Bears", 1),
            ("Powder Keg", 1),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Powder Keg", None);
    resolve(&mut game);
    let keg_card = to_may(&mut game);
    assert_eq!(game.card(keg_card).unwrap().name, "Powder Keg");
    step(&mut game, Answer::May(true));
    let keg = creature(&game, "Powder Keg").unwrap();
    let fuse = chancellor_of_the_spires::model::Counter::named("fuse");
    assert_eq!(game.counters_on(keg, &fuse), 1);
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("priority")
    };
    let activate = actions
        .iter()
        .find_map(|a| match a {
            Action::Activate(x) if x.source == keg => Some(*x),
            _ => None,
        })
        .expect("the Keg's ability is offered");
    step(&mut game, Answer::Act(Action::Activate(activate)));
    assert!(!game.permanents().contains_key(&keg), "sacrificed to pay");
    resolve(&mut game);
    assert_eq!(creature(&game, "Raging Goblin"), None);
    assert!(creature(&game, "Grizzly Bears").is_some());
}

/// CR 505.1b, 603.4, 115.6, 607.2a: Veteran Survivor's "at the beginning
/// of your second main phase, if this creature is tapped, exile up to one
/// target card from a graveyard" triggers only after it attacked, may
/// target nothing, and the cards it exiles are the ones "as long as there
/// are three or more cards exiled with this creature" counts: with three
/// it is a 5/4 with hexproof. Their graveyard fills with the Islands they
/// discard to hand size.
#[test]
fn veteran_survivor_counts_the_cards_its_own_trigger_exiled() {
    use chancellor_of_the_spires::model::Keyword;
    let mut game = deal(
        &[("Plains", 30), ("Veteran Survivor", 30)],
        &[("Island", 60)],
        &[("Plains", 2), ("Veteran Survivor", 1)],
    );
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Veteran Survivor", None);
    resolve(&mut game);
    let survivor = creature(&game, "Veteran Survivor").unwrap();
    // Not attacking: untapped, so no trigger in the second main phase.
    pass_until(&mut game, ME, Step::PostcombatMain);
    assert!(game.stack().is_empty());
    // Attack, then pass to the second main phase's trigger.
    let attack_and_aim = |game: &mut Game, pick: &dyn Fn(&[Target]) -> Target| {
        my_next_main(game);
        while !matches!(game.decision(), Decision::Attackers { .. }) {
            step(game, Answer::Act(Action::Pass));
        }
        step(game, Answer::Attack(vec![(survivor, THEM)]));
        loop {
            match game.decision() {
                Decision::Targets { slots, .. } => {
                    assert_eq!(game.step_now(), Step::PostcombatMain);
                    let t = pick(&slots[0]);
                    step(game, Answer::Targets(vec![t]));
                    return resolve(game);
                }
                Decision::Blockers { .. } => {
                    step(game, Answer::Block(vec![]));
                }
                _ => {
                    step(game, Answer::Act(Action::Pass));
                }
            }
        }
    };
    let first_card = |slots: &[Target]| {
        assert!(slots.contains(&Target::Nothing));
        *slots
            .iter()
            .find(|t| matches!(t, Target::Card(_)))
            .expect("an Island in their graveyard")
    };
    // Up to one: none chosen exiles nothing.
    attack_and_aim(&mut game, &|_| Target::Nothing);
    assert!(game.exile().is_empty());
    for k in 1..=3 {
        assert_eq!(game.characteristics()[&survivor].pt, Some(pt(2, 1)));
        attack_and_aim(&mut game, &first_card);
        assert_eq!(game.exile().len(), k);
    }
    let chars = &game.characteristics()[&survivor];
    assert_eq!(chars.pt, Some(pt(5, 4)));
    assert!(chars.has(Keyword::Hexproof));
}

/// CR 115.1, 202.3: Smother's "target creature with mana value 3 or
/// less" offers their Dusk Legion Zealot, of mana value 2, and not my
/// Hollow Dogs, of 5, and destroys what it targets.
#[test]
fn smother_targets_only_a_creature_of_mana_value_3_or_less() {
    let mut game = deal_both(
        &[
            ("Swamp", 30),
            ("Dark Ritual", 10),
            ("Hollow Dogs", 10),
            ("Smother", 10),
        ],
        &[("Swamp", 40), ("Dusk Legion Zealot", 20)],
        &[
            ("Swamp", 2),
            ("Dark Ritual", 2),
            ("Hollow Dogs", 1),
            ("Smother", 1),
        ],
        &[("Swamp", 2), ("Dusk Legion Zealot", 1)],
    );
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Swamp");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Swamp");
    for _ in 0..2 {
        cast(&mut game, "Dark Ritual", None);
        resolve(&mut game);
    }
    cast(&mut game, "Hollow Dogs", None);
    resolve(&mut game);
    let dogs = creature(&game, "Hollow Dogs").expect("cast off two rituals");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Swamp");
    cast_as(&mut game, THEM, "Dusk Legion Zealot", None);
    resolve(&mut game);
    let zealot = creature(&game, "Dusk Legion Zealot").expect("theirs");
    my_next_main(&mut game);
    let smother = in_hand(&game, ME, "Smother").unwrap();
    let Decision::Targets { slots, .. } = step(&mut game, Answer::Act(Action::Cast(smother)))
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(slots, vec![vec![Target::Object(zealot)]]);
    assert!(!slots[0].contains(&Target::Object(dogs)));
    let mut d = step(&mut game, Answer::Targets(vec![Target::Object(zealot)]));
    while let Decision::Pay { actions, .. } = d {
        d = step(&mut game, Answer::Pay(actions[0]));
    }
    resolve(&mut game);
    assert_eq!(creature(&game, "Dusk Legion Zealot"), None);
    assert!(creature(&game, "Hollow Dogs").is_some());
}

/// CR 120.2b, 120.3a: Price of Progress deals each player damage equal
/// to twice the nonbasic lands that player controls, each counted for
/// themselves: my one Rishadan Port is 2 to me, their two are 4 to them.
#[test]
fn price_of_progress_counts_each_players_own_nonbasic_lands() {
    let mut game = deal_both(
        &[
            ("Mountain", 20),
            ("Rishadan Port", 20),
            ("Price of Progress", 20),
        ],
        &[("Rishadan Port", 40), ("Forest", 20)],
        &[
            ("Mountain", 1),
            ("Rishadan Port", 1),
            ("Price of Progress", 1),
        ],
        &[("Rishadan Port", 2)],
    );
    play(&mut game, "Rishadan Port");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Rishadan Port");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Rishadan Port");
    pass_until(&mut game, ME, Step::PrecombatMain);
    let (mine, theirs) = (game.player(ME).life(), game.player(THEM).life());
    cast(&mut game, "Price of Progress", None);
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), mine - 2);
    assert_eq!(game.player(THEM).life(), theirs - 4);
}

/// CR 702.8a, 601.2f, 101.2, 701.6a: with Cunning Nightbonder out, my
/// Ambush Viper, a spell with flash, costs {G} rather than {1}{G}, and
/// their Counterspell doesn't counter it.
#[test]
fn cunning_nightbonder_makes_flash_spells_cheaper_and_uncounterable() {
    let mut game = deal_both(
        &[
            ("Island", 20),
            ("Forest", 20),
            ("Cunning Nightbonder", 10),
            ("Ambush Viper", 10),
        ],
        &[("Island", 40), ("Counterspell", 20)],
        &[
            ("Island", 2),
            ("Forest", 1),
            ("Cunning Nightbonder", 1),
            ("Ambush Viper", 1),
        ],
        &[("Island", 2), ("Counterspell", 1)],
    );
    play(&mut game, "Island");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Island");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Island");
    cast(&mut game, "Cunning Nightbonder", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Island");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    let viper = in_hand(&game, ME, "Ambush Viper").unwrap();
    let Decision::Pay { owed, .. } = game.clone().step(Answer::Act(Action::Cast(viper))).unwrap()
    else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(owed.generic, 0, "{{1}}{{G}} less {{1}}");
    cast(&mut game, "Ambush Viper", None);
    // I pass priority to them with the Viper on the stack.
    step(&mut game, Answer::Act(Action::Pass));
    cast_as(
        &mut game,
        THEM,
        "Counterspell",
        Some(vec![Target::Spell(viper)]),
    );
    resolve(&mut game);
    assert!(game
        .stack()
        .iter()
        .any(|o| matches!(o, StackObject::Spell { card, .. } if *card == viper)));
    resolve(&mut game);
    assert!(creature(&game, "Ambush Viper").is_some());
}

/// CR 601.2b, 700.2a: Red Elemental Blast's mode is chosen as it is cast,
/// before its target; with no blue spell on the stack only "Destroy
/// target blue permanent" can be chosen, and it destroys their Flying
/// Men.
#[test]
fn red_elemental_blast_offers_only_a_mode_with_a_legal_target() {
    let mut game = deal_both(
        &[("Mountain", 30), ("Red Elemental Blast", 30)],
        &[("Island", 40), ("Flying Men", 20)],
        &[("Mountain", 1), ("Red Elemental Blast", 1)],
        &[("Island", 1), ("Flying Men", 1)],
    );
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Island");
    cast_as(&mut game, THEM, "Flying Men", None);
    resolve(&mut game);
    let men = creature(&game, "Flying Men").expect("theirs");
    pass_until(&mut game, ME, Step::PrecombatMain);
    let blast = in_hand(&game, ME, "Red Elemental Blast").unwrap();
    let Decision::Mode { modes, .. } = step(&mut game, Answer::Act(Action::Cast(blast))) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(modes, vec![1]);
    assert!(game.clone().step(Answer::Mode(0)).is_err());
    let Decision::Targets { slots, .. } = step(&mut game, Answer::Mode(1)) else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(slots, vec![vec![Target::Object(men)]]);
    let mut d = step(&mut game, Answer::Targets(vec![Target::Object(men)]));
    while let Decision::Pay { actions, .. } = d {
        d = step(&mut game, Answer::Pay(actions[0]));
    }
    resolve(&mut game);
    assert_eq!(creature(&game, "Flying Men"), None);
}

/// CR 608.2c, 700.2a: Pyroblast's "Destroy target permanent if it's
/// blue" may target any permanent, and does nothing to one that isn't
/// blue as it resolves: their Island stays, their Flying Men don't.
#[test]
fn pyroblast_destroys_its_target_only_if_it_is_blue() {
    let mut game = deal_both(
        &[("Mountain", 30), ("Pyroblast", 30)],
        &[("Island", 40), ("Flying Men", 20)],
        &[("Mountain", 1), ("Pyroblast", 2)],
        &[("Island", 1), ("Flying Men", 1)],
    );
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Island");
    cast_as(&mut game, THEM, "Flying Men", None);
    resolve(&mut game);
    let island = creature(&game, "Island").expect("their land");
    let men = creature(&game, "Flying Men").expect("theirs");
    let blast = |game: &mut Game, at: chancellor_of_the_spires::model::ObjectId| {
        pass_until(game, ME, Step::PrecombatMain);
        let pyro = in_hand(game, ME, "Pyroblast").unwrap();
        let Decision::Mode { modes, .. } = step(game, Answer::Act(Action::Cast(pyro))) else {
            panic!("{:?}", game.decision())
        };
        assert_eq!(modes, vec![1], "no spell on the stack to counter");
        let Decision::Targets { slots, .. } = step(game, Answer::Mode(1)) else {
            panic!("{:?}", game.decision())
        };
        assert!(slots[0].contains(&Target::Object(at)));
        let mut d = step(game, Answer::Targets(vec![Target::Object(at)]));
        while let Decision::Pay { actions, .. } = d {
            d = step(game, Answer::Pay(actions[0]));
        }
        resolve(game);
    };
    blast(&mut game, island);
    assert!(game.permanents().contains_key(&island), "not blue");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    blast(&mut game, men);
    assert_eq!(creature(&game, "Flying Men"), None);
}

/// CR 604.3, 613.4a, 613.4c, 205.2a: Tarmogoyf's power is the number of
/// card types among cards in all graveyards, its toughness that plus 1,
/// in layer 7a, before Giant Growth's +3/+3 in 7c: their discarded Island
/// makes it 1/2, and Giant Growth in my graveyard once it has resolved
/// adds instant, so 2/3 and 5/6 with the pump.
#[test]
fn tarmogoyf_counts_card_types_in_graveyards_before_a_pump() {
    let mut game = deal(
        &[("Forest", 30), ("Tarmogoyf", 15), ("Giant Growth", 15)],
        &[("Island", 60)],
        &[("Forest", 2), ("Tarmogoyf", 1), ("Giant Growth", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Tarmogoyf", None);
    resolve(&mut game);
    let goyf = creature(&game, "Tarmogoyf").unwrap();
    let theirs = game.player(THEM).graveyard().len();
    assert!(theirs > 0, "they discarded to hand size");
    assert_eq!(game.characteristics()[&goyf].pt, Some(pt(1, 2)));
    my_next_main(&mut game);
    cast(&mut game, "Giant Growth", Some(vec![Target::Object(goyf)]));
    resolve(&mut game);
    assert_eq!(game.characteristics()[&goyf].pt, Some(pt(5, 6)));
}

/// A card in a graveyard the engine can't see (Strategic Planning put two
/// cards of my library into my graveyard, never named) could be one whose
/// ability works there (CR 113.6b), and Tarmogoyf would count it (CR
/// 604.3): the game refuses to go on rather than read it as nothing.
#[test]
fn a_hidden_card_in_a_graveyard_is_refused() {
    let mut game = planned(
        &[
            ("Forest", 5),
            ("Island", 5),
            ("Tarmogoyf", 5),
            ("Strategic Planning", 5),
        ],
        false,
    );
    assert!(
        game.player(ME)
            .graveyard()
            .iter()
            .any(|c| game.card(*c).is_none()),
        "a hidden card in my graveyard"
    );
    match game.step(Answer::Act(Action::Pass)) {
        Err(Rejected::Refused { refusal, .. }) => {
            assert_eq!(refusal.node, "a hidden card in a graveyard")
        }
        other => panic!("{other:?}"),
    }
}

/// Strategic Planning on the second turn, the card kept a known one where
/// there is one, so two hidden cards go to my graveyard where they can.
fn planned(known: &[(&'static str, usize)], presume: bool) -> Game {
    let mut game = (0..2000)
        .map(|seed| {
            Game::start(Setup {
                format: Format::Constructed,
                decks: vec![
                    DeckCards {
                        hidden: 40,
                        ..deck(known)
                    },
                    deck(&[("Island", 60)]),
                ],
                mulligans: false,
                seed,
            })
            .unwrap()
        })
        .find(|g| {
            known
                .iter()
                .all(|(n, _)| named(g, g.player(ME).hand(), n) >= 1)
        })
        .expect("a seed deals one of each");
    if presume {
        game.presume_graveyard_inert();
    }
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Island");
    cast(&mut game, "Strategic Planning", None);
    let three: Vec<CardId> = game.player(ME).library().iter().take(3).copied().collect();
    resolve(&mut game);
    let mut by_hidden = three.clone();
    by_hidden.sort_by_key(|c| game.card(*c).is_none());
    step(&mut game, Answer::Choose(vec![by_hidden[0]]));
    game
}

/// A caller may presume a card no one named in a graveyard has no ability
/// that works there (CR 113.6b, 113.6m), as a replay does, whose log would
/// show one working: the game goes on, keeping each card it presumed, and
/// naming one later as a card with such an ability (Squee's return at its
/// owner's upkeep) is refused, as is a count of the graveyard by name.
#[test]
fn a_hidden_card_in_a_graveyard_presumed_inert_goes_on() {
    let mut game = planned(
        &[
            ("Forest", 5),
            ("Island", 5),
            ("Accumulated Knowledge", 5),
            ("Strategic Planning", 5),
        ],
        true,
    );
    let hidden: Vec<CardId> = game
        .player(ME)
        .graveyard()
        .iter()
        .copied()
        .filter(|c| game.card(*c).is_none())
        .collect();
    assert!(!hidden.is_empty(), "a hidden card in my graveyard");
    assert!(hidden.iter().all(|c| game.presumed().contains(c)));
    assert!(matches!(
        game.clone().reveal(hidden[0], card("Squee, Goblin Nabob")),
        Err(Unnameable::WorksInGraveyard(_))
    ));
    assert!(matches!(
        game.clone().reveal(hidden[0], card("Anger")),
        Err(Unnameable::WorksInGraveyard(_))
    ));
    assert_eq!(game.clone().reveal(hidden[0], card("Island")), Ok(()));
    // Accumulated Knowledge counts the cards of its name in all graveyards,
    // and can't count one it can't see.
    my_next_main(&mut game);
    cast(&mut game, "Accumulated Knowledge", None);
    step(&mut game, Answer::Act(Action::Pass));
    match game.step(Answer::Act(Action::Pass)) {
        Err(Rejected::Refused { card, refusal }) => {
            assert_eq!(card, "Accumulated Knowledge");
            assert_eq!(refusal.node, "Amount::CardsInGraveyards");
        }
        other => panic!("{other:?}"),
    }
}

/// Once its owner has shown Anger, whose haste works from a graveyard
/// unseen by the log (CR 113.6b), a hidden card in that graveyard may be
/// another Anger: the presumption doesn't hold, and the game refuses.
#[test]
fn a_hidden_card_isnt_presumed_inert_beside_a_shown_graveyard_static() {
    let mut game = planned(
        &[
            ("Forest", 5),
            ("Island", 5),
            ("Anger", 5),
            ("Strategic Planning", 5),
        ],
        true,
    );
    match game.step(Answer::Act(Action::Pass)) {
        Err(Rejected::Refused { card, refusal }) => {
            assert_eq!(card, "a hidden card");
            assert_eq!(
                refusal.node,
                "a hidden card in a graveyard beside a graveyard static"
            );
            assert!(refusal.why.contains("113.6b"), "{}", refusal.why);
        }
        other => panic!("{other:?}"),
    }
}

/// CR 702.14c: Goblin King gives my Raging Goblin mountainwalk, so while
/// they control a Mountain their Raging Goblin can't block it.
#[test]
fn mountainwalk_cant_be_blocked_while_they_control_a_mountain() {
    let mut game = deal_both(
        &[("Mountain", 30), ("Goblin King", 15), ("Raging Goblin", 15)],
        &[("Mountain", 40), ("Raging Goblin", 20)],
        &[("Mountain", 4), ("Goblin King", 1), ("Raging Goblin", 1)],
        &[("Mountain", 1), ("Raging Goblin", 1)],
    );
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(&mut game, THEM, "Raging Goblin", None);
    resolve(&mut game);
    let theirs = creature(&game, "Raging Goblin").expect("theirs");
    pass_until(&mut game, ME, Step::PrecombatMain);
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Goblin King", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    let mine = game
        .permanents()
        .iter()
        .find(|(id, p)| {
            **id != theirs && game.card(p.card).is_some_and(|t| t.name == "Raging Goblin")
        })
        .map(|(id, _)| *id)
        .expect("mine");
    while !matches!(game.decision(), Decision::Attackers { .. }) {
        step(&mut game, Answer::Act(Action::Pass));
    }
    step(&mut game, Answer::Attack(vec![(mine, THEM)]));
    while !matches!(game.decision(), Decision::Blockers { .. }) {
        step(&mut game, Answer::Act(Action::Pass));
    }
    let Decision::Blockers { can, .. } = game.decision() else {
        unreachable!("waited for it")
    };
    assert!(can.contains(&theirs), "untapped, it could block another");
    assert!(game
        .clone()
        .step(Answer::Block(vec![(theirs, mine)]))
        .is_err());
    step(&mut game, Answer::Block(vec![]));
}

/// CR 508.3d, 508.4: Adeline's "Whenever you attack" triggers as Raging
/// Goblin attacks, and its token enters tapped and attacking the
/// opponent, never declared, so Adeline doesn't trigger again; combat
/// deals both creatures' damage.
#[test]
fn adeline_makes_a_token_attacking_each_opponent_when_you_attack() {
    let mut game = deal(
        &[
            ("Plains", 30),
            ("Mountain", 10),
            ("Adeline, Resplendent Cathar", 10),
            ("Raging Goblin", 10),
        ],
        &[("Forest", 60)],
        &[
            ("Plains", 3),
            ("Mountain", 1),
            ("Adeline, Resplendent Cathar", 1),
            ("Raging Goblin", 1),
        ],
    );
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Adeline, Resplendent Cathar", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    let goblin = creature(&game, "Raging Goblin").unwrap();
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(goblin, THEM)]));
    assert_eq!(game.stack().len(), 1, "one trigger, for the attack");
    resolve(&mut game);
    assert!(game.stack().is_empty(), "the token never attacked");
    let token = creature(&game, "Human Token").expect("the token");
    assert!(game.permanents()[&token].tapped);
    assert!(game.attacks().contains(&(token, THEM)));
    through_combat(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
}

/// CR 702.21a: Flowering of the White Tree gives Isamaru ward {1}, so
/// their Lightning Bolt targeting it triggers ward, and the Bolt is
/// countered unless they pay {1}; paid, it resolves and Isamaru, 4/3,
/// dies. My own spell targeting it wouldn't trigger it.
#[test]
fn ward_counters_an_opponents_spell_unless_they_pay() {
    for pays in [false, true] {
        let mut game = deal_both(
            &[
                ("Plains", 40),
                ("Isamaru, Hound of Konda", 10),
                ("Flowering of the White Tree", 10),
            ],
            &[("Mountain", 40), ("Lightning Bolt", 20)],
            &[
                ("Plains", 2),
                ("Isamaru, Hound of Konda", 1),
                ("Flowering of the White Tree", 1),
            ],
            &[("Mountain", 2), ("Lightning Bolt", 1)],
        );
        play(&mut game, "Plains");
        cast(&mut game, "Isamaru, Hound of Konda", None);
        resolve(&mut game);
        pass_until(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Mountain");
        my_next_main(&mut game);
        play(&mut game, "Plains");
        cast(&mut game, "Flowering of the White Tree", None);
        resolve(&mut game);
        let isamaru = creature(&game, "Isamaru, Hound of Konda").unwrap();
        assert_eq!(game.characteristics()[&isamaru].pt, Some(pt(4, 3)));
        pass_until(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Mountain");
        let bolt = in_hand(&game, THEM, "Lightning Bolt").unwrap();
        cast_as(
            &mut game,
            THEM,
            "Lightning Bolt",
            Some(vec![Target::Object(isamaru)]),
        );
        assert_eq!(game.stack().len(), 2, "ward triggered over the Bolt");
        let Decision::PayUnless {
            player,
            owed,
            actions,
            ..
        } = resolve(&mut game)
        else {
            panic!("{:?}", game.decision())
        };
        assert_eq!((player, owed.generic), (THEM, 1));
        if pays {
            let mut actions = actions;
            loop {
                let pay = actions
                    .iter()
                    .find(|a| matches!(a, Payment::Spend(_)))
                    .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
                    .copied()
                    .expect("a Mountain untapped");
                match step(&mut game, Answer::Pay(pay)) {
                    Decision::PayUnless { actions: a, .. } => actions = a,
                    _ => break,
                }
            }
            resolve(&mut game);
            assert!(creature(&game, "Isamaru, Hound of Konda").is_none());
        } else {
            step(&mut game, Answer::Pay(Payment::Cancel));
            assert!(game.player(THEM).graveyard().contains(&bolt));
            assert!(game.stack().is_empty());
            assert!(creature(&game, "Isamaru, Hound of Konda").is_some());
        }
    }
}

/// CR 702.170a–b, 116.2k: Outcaster Trailblazer plotted for {2}{G} in my
/// main phase is exiled without the stack; it can't be cast from exile the
/// turn it became plotted, and on my next turn it is cast without paying
/// its mana cost (CR 702.170d).
#[test]
fn a_plotted_card_is_cast_free_on_a_later_turn() {
    let mut game = deal(
        &[("Forest", 40), ("Outcaster Trailblazer", 20)],
        &[("Swamp", 60)],
        &[("Forest", 3), ("Outcaster Trailblazer", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    let trailblazer = in_hand(&game, ME, "Outcaster Trailblazer").unwrap();
    let mut d = step(&mut game, Answer::Act(Action::Plot(trailblazer)));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a Forest");
        d = step(&mut game, Answer::Pay(pay));
    }
    assert!(game.stack().is_empty(), "a special action");
    assert!(game.exile().contains(&trailblazer));
    assert!(game.plotted().any(|(c, _)| c == trailblazer));
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(
        !actions.contains(&Action::CastPlotted(trailblazer)),
        "not the turn it became plotted"
    );
    my_next_main(&mut game);
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert!(actions.contains(&Action::CastPlotted(trailblazer)));
    let mut d = step(&mut game, Answer::Act(Action::CastPlotted(trailblazer)));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .copied()
            .expect("nothing to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    assert!(game.plotted().next().is_none());
    assert!(!game.exile().contains(&trailblazer));
    resolve(&mut game);
    assert!(creature(&game, "Outcaster Trailblazer").is_some());
}

/// CR 707.9a–b, 707.5: Phantasmal Image entering as a copy of Mogg
/// Fanatic is a Mogg Fanatic that is an Illusion too and has "When this
/// creature becomes the target of a spell or ability, sacrifice it."; the
/// Fanatic it copied aiming at it makes it trigger, and it is sacrificed
/// before the damage (CR 603.2c, 603.3).
#[test]
fn phantasmal_image_copies_and_is_sacrificed_as_it_becomes_a_target() {
    let mut game = deal(
        &[
            ("Mountain", 20),
            ("Island", 20),
            ("Mogg Fanatic", 10),
            ("Phantasmal Image", 10),
        ],
        &[("Forest", 60)],
        &[
            ("Mountain", 1),
            ("Island", 1),
            ("Mogg Fanatic", 1),
            ("Phantasmal Image", 1),
        ],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Mogg Fanatic", None);
    resolve(&mut game);
    let fanatic = creature(&game, "Mogg Fanatic").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Island");
    let image = in_hand(&game, ME, "Phantasmal Image").unwrap();
    cast(&mut game, "Phantasmal Image", None);
    let d = resolve(&mut game);
    let Decision::EnterAsCopy { of, may, .. } = d else {
        panic!("{d:?}")
    };
    assert!(may);
    assert_eq!(of, vec![fanatic]);
    step(&mut game, Answer::Copy(Some(fanatic)));
    let copy = permanent_of(&game, image);
    assert_eq!(game.card(image).unwrap().name, "Mogg Fanatic");
    let now = &game.characteristics()[&copy];
    let has = |t: &str| now.subtypes.iter().any(|s| format!("{s:?}").contains(t));
    assert!(has("Goblin") && has("Illusion"), "{:?}", now.subtypes);
    let Decision::Targets { .. } = activate(&mut game, fanatic) else {
        panic!("a target")
    };
    step(&mut game, Answer::Targets(vec![Target::Object(copy)]));
    assert_eq!(
        game.stack().len(),
        2,
        "the Fanatic's ability and the Image's trigger"
    );
    resolve(&mut game);
    assert!(!game.permanents().contains_key(&copy), "sacrificed");
    assert!(game.player(ME).graveyard().contains(&image));
    assert_eq!(game.card(image).unwrap().name, "Phantasmal Image");
}

/// CR 701.13a, 406.2: Tormod's Crypt, sacrificed as its cost, exiles every
/// card in the target player's graveyard, itself among them by then.
#[test]
fn tormods_crypt_exiles_a_graveyard_and_itself_in_it() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Mogg Fanatic", 15),
            ("Tormod's Crypt", 15),
        ],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Mogg Fanatic", 1), ("Tormod's Crypt", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Mogg Fanatic", None);
    resolve(&mut game);
    let fanatic = creature(&game, "Mogg Fanatic").unwrap();
    let Decision::Targets { .. } = activate(&mut game, fanatic) else {
        panic!("a target")
    };
    step(&mut game, Answer::Targets(vec![Target::Player(THEM)]));
    resolve(&mut game);
    assert_eq!(game.player(ME).graveyard().len(), 1);
    cast(&mut game, "Tormod's Crypt", None);
    resolve(&mut game);
    let crypt = creature(&game, "Tormod's Crypt").unwrap();
    let Decision::Targets { slots, .. } = activate(&mut game, crypt) else {
        panic!("a target")
    };
    assert!(slots[0].contains(&Target::Player(ME)));
    step(&mut game, Answer::Targets(vec![Target::Player(ME)]));
    assert_eq!(game.player(ME).graveyard().len(), 2, "the Crypt sacrificed");
    resolve(&mut game);
    assert!(game.player(ME).graveyard().is_empty());
    assert_eq!(game.exile().len(), 2);
}

/// CR 107.3i, 608.2h, 503.1a: Graveborn Muse's "At the beginning of your
/// upkeep, you draw X cards and lose X life, where X is the number of
/// Zombies you control", the Muse a Zombie itself, so two with Walking
/// Corpse.
#[test]
fn graveborn_muse_draws_and_costs_life_for_each_zombie() {
    let mut game = deal(
        &[
            ("Swamp", 30),
            ("Graveborn Muse", 15),
            ("Walking Corpse", 15),
        ],
        &[("Forest", 60)],
        &[("Swamp", 5), ("Graveborn Muse", 1), ("Walking Corpse", 1)],
    );
    for _ in 0..3 {
        play(&mut game, "Swamp");
        my_next_main(&mut game);
    }
    play(&mut game, "Swamp");
    cast(&mut game, "Graveborn Muse", None);
    resolve(&mut game);
    my_next_main(&mut game);
    assert_eq!(game.player(ME).life(), 19, "one Zombie at this upkeep");
    play(&mut game, "Swamp");
    cast(&mut game, "Walking Corpse", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::Upkeep);
    pass_until(&mut game, ME, Step::Upkeep);
    let hand = game.player(ME).hand().len();
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand + 2);
    assert_eq!(game.player(ME).life(), 17);
}

/// CR 101.4, 701.13a: Yarok's Fenlurker's "When this creature enters,
/// each opponent exiles a card from their hand": the opponent chooses
/// which, and nothing of mine is asked.
#[test]
fn yaroks_fenlurker_has_each_opponent_exile_a_card_of_their_choosing() {
    let mut game = deal(
        &[("Swamp", 30), ("Yarok's Fenlurker", 30)],
        &[("Forest", 60)],
        &[("Swamp", 2), ("Yarok's Fenlurker", 1)],
    );
    play(&mut game, "Swamp");
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Yarok's Fenlurker", None);
    let mine = game.player(ME).hand().len();
    let theirs = game.player(THEM).hand().len();
    resolve(&mut game);
    resolve(&mut game);
    let Decision::ExileFromHand { player, hand } = game.decision() else {
        panic!("a card to exile, not {:?}", game.decision())
    };
    assert_eq!(player, THEM);
    assert_eq!(hand.len(), theirs);
    assert!(
        game.clone()
            .step(Answer::Pick(Some(game.player(ME).hand()[0])))
            .is_err(),
        "only a card of their own hand"
    );
    assert!(
        game.clone().step(Answer::Pick(None)).is_err(),
        "a card, with one in hand"
    );
    step(&mut game, Answer::Pick(Some(hand[0])));
    assert_eq!(game.player(THEM).hand().len(), theirs - 1);
    assert_eq!(game.player(ME).hand().len(), mine);
    assert!(game.exile().contains(&hand[0]));
}

/// CR 508.1c: Topiary Stomper's "This creature can't attack or block
/// unless you control seven or more lands", with six it can't, with seven
/// it can.
#[test]
fn topiary_stomper_attacks_only_with_seven_lands() {
    let mut game = deal(
        &[("Forest", 50), ("Topiary Stomper", 10)],
        &[("Forest", 60)],
        &[("Forest", 6), ("Topiary Stomper", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Topiary Stomper", None);
    while !matches!(game.decision(), Decision::Search { .. }) {
        step(&mut game, Answer::Act(Action::Pass));
    }
    let Decision::Search { known, .. } = game.decision() else {
        unreachable!()
    };
    step(&mut game, Answer::Find(Found::Card(known[0])));
    let stomper = creature(&game, "Topiary Stomper").unwrap();
    // With no creature that can attack, the engine asks for no
    // declaration (CR 508.1a).
    for lands in [5, 6] {
        my_next_main(&mut game);
        play(&mut game, "Forest");
        while game.step_now() != Step::End {
            assert!(
                !matches!(game.decision(), Decision::Attackers { .. }),
                "an attack with {lands} lands"
            );
            step(&mut game, Answer::Act(Action::Pass));
        }
    }
    my_next_main(&mut game);
    play(&mut game, "Forest");
    let (can, _) = to_attackers(&mut game);
    assert_eq!(can, vec![stomper], "with seven lands");
}

/// CR 509.1b: Topiary Stomper's "can't attack or block unless you control
/// seven or more lands" keeps it from blocking with four: no block of
/// Raging Goblin is asked for, and the Goblin deals its damage. Seven
/// lands lifting it is the attack test's, the same check.
#[test]
fn topiary_stomper_cant_block_with_four_lands() {
    let mut game = deal_both(
        &[("Forest", 50), ("Topiary Stomper", 10)],
        &[("Mountain", 50), ("Raging Goblin", 10)],
        &[("Forest", 3), ("Topiary Stomper", 1)],
        &[("Mountain", 1), ("Raging Goblin", 1)],
    );
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Topiary Stomper", None);
    while !matches!(game.decision(), Decision::Search { .. }) {
        step(&mut game, Answer::Act(Action::Pass));
    }
    let Decision::Search { known, .. } = game.decision() else {
        unreachable!()
    };
    step(&mut game, Answer::Find(Found::Card(known[0])));
    let stomper = creature(&game, "Topiary Stomper").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Mountain");
    cast_as(&mut game, THEM, "Raging Goblin", None);
    resolve(&mut game);
    let goblin = creature(&game, "Raging Goblin").unwrap();
    let life = game.player(ME).life();
    while !matches!(game.decision(), Decision::Attackers { .. }) {
        step(&mut game, Answer::Act(Action::Pass));
    }
    step(&mut game, Answer::Attack(vec![(goblin, ME)]));
    // With no creature that can block, the engine asks for no
    // declaration (CR 509.1a).
    while game.step_now() <= Step::EndOfCombat {
        assert!(
            !matches!(game.decision(), Decision::Blockers { .. }),
            "a block with four lands"
        );
        step(&mut game, Answer::Act(Action::Pass));
    }
    assert!(game.permanents().contains_key(&stomper));
    assert_eq!(game.player(ME).life(), life - 1);
}

/// CR 608.2d, 701.26b: Frantic Search's "Untap up to three lands" asks
/// its caster, as it resolves after the discard, which of the tapped lands
/// to untap: no more than three, none twice and none untapped, and those
/// chosen untap.
#[test]
fn frantic_search_untaps_up_to_three_tapped_lands() {
    let mut game = deal(
        &[("Island", 50), ("Frantic Search", 10)],
        &[("Island", 60)],
        &[("Island", 4), ("Frantic Search", 1)],
    );
    play(&mut game, "Island");
    for _ in 0..3 {
        my_next_main(&mut game);
        play(&mut game, "Island");
    }
    cast(&mut game, "Frantic Search", None);
    let Decision::Discard { hand, count, .. } = resolve(&mut game) else {
        panic!("the discard first")
    };
    let d = step(&mut game, Answer::Discard(hand[..count].to_vec()));
    let Decision::UntapUpTo { player, n, legal } = d else {
        panic!("{d:?}")
    };
    assert_eq!(
        (player, n, legal.len()),
        (ME, 3, 3),
        "the three tapped Islands"
    );
    let untapped = game
        .permanents()
        .iter()
        .find(|(_, p)| !p.tapped)
        .map(|(id, _)| *id)
        .expect("the fourth Island");
    assert!(game.clone().step(Answer::Untap(vec![untapped])).is_err());
    assert!(game
        .clone()
        .step(Answer::Untap(vec![legal[0], legal[0]]))
        .is_err());
    step(&mut game, Answer::Untap(legal[..2].to_vec()));
    assert!(!game.permanents()[&legal[0]].tapped);
    assert!(!game.permanents()[&legal[1]].tapped);
    assert!(game.permanents()[&legal[2]].tapped);
}

/// The payments offered once `name` is cast and every untapped land I
/// control is tapped for mana.
fn cast_tapping_all(game: &mut Game, name: &str, targets: Option<Vec<Target>>) -> Vec<Payment> {
    let c = in_hand(game, ME, name).expect("in hand");
    let mut d = step(game, Answer::Act(Action::Cast(c)));
    if let Some(t) = targets {
        d = step(game, Answer::Targets(t));
    }
    loop {
        let Decision::Pay { actions, .. } = d else {
            panic!("{d:?}")
        };
        let Some(tap) = actions.iter().find(|a| matches!(a, Payment::Mana(_))) else {
            return actions;
        };
        d = step(game, Answer::Pay(*tap));
    }
}

/// CR 107.4f, 119.4, 601.2h: a Phyrexian symbol is paid with one mana of
/// its colour or 2 life. Vault Skirge's {1}{B/P} with one Swamp is paid
/// only with 2 life; Dismember's {1}{B/P}{B/P} with two is paid with 2
/// life and both, or 4 life and one, the other left in the pool.
#[test]
fn phyrexian_mana_is_paid_with_its_colour_or_two_life() {
    let mut game = deal(
        &[("Swamp", 30), ("Vault Skirge", 15), ("Dismember", 15)],
        &[("Forest", 60)],
        &[("Swamp", 2), ("Vault Skirge", 1), ("Dismember", 1)],
    );
    play(&mut game, "Swamp");
    let offered = cast_tapping_all(&mut game, "Vault Skirge", None);
    let b = ManaPool::of([Mana::Black]);
    assert!(
        !offered.iter().any(|a| matches!(a, Payment::Spend(_))),
        "{offered:?}"
    );
    assert!(
        offered.contains(&Payment::SpendPayingLife(b, 2)),
        "{offered:?}"
    );
    step(&mut game, Answer::Pay(Payment::SpendPayingLife(b, 2)));
    assert_eq!(game.player(ME).life(), 18);
    resolve(&mut game);
    let skirge = creature(&game, "Vault Skirge").expect("resolved");

    my_next_main(&mut game);
    play(&mut game, "Swamp");
    let offered = cast_tapping_all(&mut game, "Dismember", Some(vec![Target::Object(skirge)]));
    let bb = ManaPool::of([Mana::Black, Mana::Black]);
    assert!(
        offered.contains(&Payment::SpendPayingLife(bb, 2)),
        "{offered:?}"
    );
    assert!(
        offered.contains(&Payment::SpendPayingLife(b, 4)),
        "{offered:?}"
    );
    step(&mut game, Answer::Pay(Payment::SpendPayingLife(b, 4)));
    assert_eq!(game.player(ME).life(), 14);
    assert_eq!(*game.player(ME).pool(), b, "one {{B}} left");
    resolve(&mut game);
    assert!(creature(&game, "Vault Skirge").is_none(), "-5/-5");
}

/// CR 610.3, 610.3c: Banisher Priest exiles their Grizzly Bears until it
/// leaves the battlefield; bolted, it dies and the Bears return under
/// their owner's control.
#[test]
fn banisher_priest_exiles_until_it_leaves_the_battlefield() {
    let mut game = deal_both(
        &[
            ("Plains", 30),
            ("Mountain", 10),
            ("Banisher Priest", 10),
            ("Lightning Bolt", 10),
        ],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[
            ("Plains", 3),
            ("Mountain", 1),
            ("Banisher Priest", 1),
            ("Lightning Bolt", 1),
        ],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    let card = game.permanents()[&bears].card;
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(&mut game, "Banisher Priest", None);
    let d = resolve(&mut game);
    let Decision::Targets { slots, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!(slots, vec![vec![Target::Object(bears)]]);
    step(&mut game, Answer::Targets(vec![Target::Object(bears)]));
    resolve(&mut game);
    assert!(creature(&game, "Grizzly Bears").is_none());
    let priest = creature(&game, "Banisher Priest").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Object(priest)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Banisher Priest").is_none());
    let back = creature(&game, "Grizzly Bears").expect("returned");
    assert_eq!(game.permanents()[&back].card, card);
    assert_eq!(game.characteristics()[&back].controller, THEM);
}

/// CR 610.3, 303.4c: Sheltered by Ghosts exiles their Grizzly Bears until
/// it leaves the battlefield; the creature it enchants bolted, the Aura
/// is put into the graveyard as a state-based action (CR 704.5m) and the
/// Bears return under their owner's control.
#[test]
fn sheltered_by_ghosts_returns_what_it_exiled_when_its_creature_dies() {
    let mut game = deal_both(
        &[
            ("Plains", 20),
            ("Mountain", 20),
            ("Raging Goblin", 7),
            ("Sheltered by Ghosts", 7),
            ("Lightning Bolt", 6),
        ],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[
            ("Plains", 2),
            ("Mountain", 2),
            ("Raging Goblin", 1),
            ("Sheltered by Ghosts", 1),
            ("Lightning Bolt", 1),
        ],
        &[("Forest", 2), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    let goblin = creature(&game, "Raging Goblin").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Plains");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    let card = game.permanents()[&bears].card;
    my_next_main(&mut game);
    play(&mut game, "Plains");
    cast(
        &mut game,
        "Sheltered by Ghosts",
        Some(vec![Target::Object(goblin)]),
    );
    let d = resolve(&mut game);
    let Decision::Targets { slots, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!(slots, vec![vec![Target::Object(bears)]]);
    step(&mut game, Answer::Targets(vec![Target::Object(bears)]));
    resolve(&mut game);
    assert!(creature(&game, "Grizzly Bears").is_none(), "exiled");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(
        &mut game,
        "Lightning Bolt",
        Some(vec![Target::Object(goblin)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Raging Goblin").is_none());
    assert!(
        creature(&game, "Sheltered by Ghosts").is_none(),
        "the Aura is gone"
    );
    let back = creature(&game, "Grizzly Bears").expect("returned");
    assert_eq!(game.permanents()[&back].card, card);
    assert_eq!(game.characteristics()[&back].controller, THEM);
}

/// CR 702.190a, 509.1h, 601.2h: Michelangelo's Technique's sneak is
/// offered only in its caster's declare blockers step, returning an
/// unblocked attacking creature of theirs to hand rather than paying its
/// mana cost. Resolving, it looks at the top eight cards and puts up to
/// two creature cards of total mana value 6 or less onto the battlefield
/// (CR 202.3): a land, three cards or a total of 8 aren't legal choices.
/// The rest go on the bottom.
#[test]
fn michelangelos_technique_sneaks_in_for_an_unblocked_attacker() {
    let mut game = deal(
        &[
            ("Forest", 24),
            ("Grizzly Bears", 12),
            ("Craw Wurm", 12),
            ("Michelangelo's Technique", 12),
        ],
        &[("Island", 60)],
        &[
            ("Forest", 4),
            ("Grizzly Bears", 1),
            ("Michelangelo's Technique", 1),
        ],
    );
    let technique = in_hand(&game, ME, "Michelangelo's Technique").unwrap();
    let sneak = |game: &Game| {
        let Decision::Priority { actions, .. } = game.decision() else {
            return None;
        };
        actions
            .into_iter()
            .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == technique))
    };
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Grizzly Bears", None);
    resolve(&mut game);
    let bears = creature(&game, "Grizzly Bears").unwrap();
    my_next_main(&mut game);
    play(&mut game, "Forest");
    my_next_main(&mut game);
    play(&mut game, "Forest");
    assert!(sneak(&game).is_none(), "not in a main phase");
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(bears, THEM)]));
    assert!(sneak(&game).is_none(), "not before blockers are declared");
    while game.step_now() < Step::DeclareBlockers {
        match game.decision() {
            Decision::Blockers { .. } => step(&mut game, Answer::Block(vec![])),
            _ => step(&mut game, Answer::Act(Action::Pass)),
        };
    }
    let alt = sneak(&game).expect("offered in my declare blockers step");
    let mut d = step(&mut game, Answer::Act(alt));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("four Forests pay {3}{G}");
        d = step(&mut game, Answer::Pay(pay));
    }
    let Decision::ReturnToHand { count, legal, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!((count, legal.clone()), (1, vec![bears]));
    step(&mut game, Answer::ReturnToHand(legal));
    assert!(in_hand(&game, ME, "Grizzly Bears").is_some());
    let library = game.player(ME).library().len();
    let d = resolve(&mut game);
    let Decision::ChooseMatching {
        cards,
        up_to,
        total_mana_value,
        ..
    } = d
    else {
        panic!("{d:?}")
    };
    assert_eq!((cards.len(), up_to, total_mana_value), (8, 2, Some(6)));
    let of = |name: &str| -> Vec<CardId> {
        cards
            .iter()
            .copied()
            .filter(|c| game.card(*c).is_some_and(|t| t.name == name))
            .collect()
    };
    let (forests, bears_seen, wurms) = (of("Forest"), of("Grizzly Bears"), of("Craw Wurm"));
    let illegal = |game: &mut Game, chosen: &[CardId]| {
        let finds = chosen.iter().map(|c| Found::Card(*c)).collect();
        assert!(
            matches!(game.step(Answer::Finds(finds)), Err(Rejected::Illegal(_))),
            "{chosen:?}"
        );
    };
    let forest = *forests.first().expect("a Forest among the eight");
    let wurm = *wurms.first().expect("a Craw Wurm among the eight");
    let bear = *bears_seen.first().expect("a Grizzly Bears among the eight");
    illegal(&mut game, &[forest]);
    illegal(&mut game, &[wurm, bear]);
    if let [a, b, ..] = bears_seen.as_slice() {
        illegal(&mut game, &[*a, *b, wurm]);
    }
    step(&mut game, Answer::Finds(vec![Found::Card(wurm)]));
    assert!(creature(&game, "Craw Wurm").is_some());
    // The other seven on the bottom, the library one card shorter.
    let lib = game.player(ME).library();
    assert_eq!(lib.len(), library - 1);
    let mut bottom: Vec<CardId> = lib.iter().rev().take(7).copied().collect();
    let mut rest: Vec<CardId> = cards.into_iter().filter(|c| *c != wurm).collect();
    bottom.sort();
    rest.sort();
    assert_eq!(bottom, rest);
}

/// CR 700.2c, 608.2c: Archdruid's Charm's first mode finds a creature or
/// land card, put onto the battlefield tapped if it's a land card and
/// otherwise into the hand; casting it chooses no target, though another
/// mode has two.
#[test]
fn archdruids_charm_puts_a_land_found_onto_the_battlefield_and_a_creature_in_hand() {
    let mut game = deal(
        &[
            ("Forest", 30),
            ("Grizzly Bears", 15),
            ("Archdruid's Charm", 15),
        ],
        &[("Island", 60)],
        &[("Forest", 3), ("Archdruid's Charm", 2)],
    );
    let charm = |game: &mut Game, find: &str| -> CardId {
        let c = in_hand(game, ME, "Archdruid's Charm").unwrap();
        let Decision::Mode { modes, .. } = step(game, Answer::Act(Action::Cast(c))) else {
            panic!("{:?}", game.decision())
        };
        assert!(modes.contains(&0), "{modes:?}");
        let mut d = step(game, Answer::Mode(0));
        while let Decision::Pay { actions, .. } = d {
            d = step(game, Answer::Pay(actions[0]));
        }
        let Decision::Search { known, .. } = resolve(game) else {
            panic!("{:?}", game.decision())
        };
        let found = *known
            .iter()
            .find(|k| game.card(**k).is_some_and(|t| t.name == find))
            .expect("in the library");
        step(game, Answer::Find(Found::Card(found)));
        found
    };
    play(&mut game, "Forest");
    for _ in 0..2 {
        my_next_main(&mut game);
        play(&mut game, "Forest");
    }
    let forest = charm(&mut game, "Forest");
    let entered = game
        .permanents()
        .values()
        .find(|p| p.card == forest)
        .expect("a land card: onto the battlefield");
    assert!(entered.tapped);
    my_next_main(&mut game);
    let bears = charm(&mut game, "Grizzly Bears");
    assert!(
        game.player(ME).hand().contains(&bears),
        "otherwise: the hand"
    );
    assert!(!game.permanents().values().any(|p| p.card == bears));
}

/// CR 608.2c: Coiling Oracle's "reveal the top card of your library. If
/// it's a land card, put it onto the battlefield. Otherwise, put that card
/// into your hand": the "otherwise" only when the "if" didn't hold, each
/// card it reveals going to just one of the two places.
#[test]
fn coiling_oracle_puts_a_land_onto_the_battlefield_and_otherwise_into_hand() {
    let mut game = deal(
        &[("Forest", 15), ("Island", 15), ("Coiling Oracle", 30)],
        &[("Island", 60)],
        &[("Forest", 1), ("Island", 1), ("Coiling Oracle", 2)],
    );
    play(&mut game, "Forest");
    for _ in 0..2 {
        my_next_main(&mut game);
        if in_hand(&game, ME, "Island").is_some() {
            play(&mut game, "Island");
        }
        let top = *game.player(ME).library().front().expect("a library");
        cast(&mut game, "Coiling Oracle", None);
        resolve(&mut game);
        resolve(&mut game);
        let land = game.card(top).expect("revealed").type_line.contains("Land");
        let entered = game.permanents().values().any(|p| p.card == top);
        let held = game.player(ME).hand().contains(&top);
        assert_eq!((entered, held), (land, !land), "one place, by what it is");
    }
}

/// CR 118.9, 107.3a, 601.2h: Blazing Shoal cast by exiling a red card
/// with mana value X from the hand rather than paying its mana cost: X is
/// that card's mana value, 12 for Volcanic Salvo, so Raging Goblin gets
/// +12/+0.
#[test]
fn blazing_shoal_exiles_a_red_card_from_hand_and_its_mana_value_is_x() {
    let mut game = deal(
        &[
            ("Mountain", 30),
            ("Raging Goblin", 10),
            ("Blazing Shoal", 10),
            ("Volcanic Salvo", 10),
        ],
        &[("Island", 60)],
        &[
            ("Mountain", 1),
            ("Raging Goblin", 1),
            ("Blazing Shoal", 1),
            ("Volcanic Salvo", 1),
        ],
    );
    let shoal = in_hand(&game, ME, "Blazing Shoal").unwrap();
    let salvo = in_hand(&game, ME, "Volcanic Salvo").unwrap();
    play(&mut game, "Mountain");
    cast(&mut game, "Raging Goblin", None);
    resolve(&mut game);
    let goblin = creature(&game, "Raging Goblin").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let alt = actions
        .into_iter()
        .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == shoal))
        .expect("offered");
    let mut d = step(&mut game, Answer::Act(alt));
    if let Decision::Targets { .. } = d {
        d = step(&mut game, Answer::Targets(vec![Target::Object(goblin)]));
    }
    if let Decision::Pay { .. } = d {
        d = step(&mut game, Answer::Pay(Payment::Spend(ManaPool::default())));
    }
    let Decision::PayCards { count, legal, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!(count, 1);
    assert!(legal.contains(&salvo));
    assert!(!legal.contains(&shoal), "on the stack");
    step(&mut game, Answer::Choose(vec![salvo]));
    assert!(game.exile().contains(&salvo));
    resolve(&mut game);
    assert_eq!(game.characteristics()[&goblin].pt, Some(pt(13, 1)));
}

/// CR 107.3a, 704.5f: Sickening Shoal's "-X/-X", X the mana value of the
/// black card it exiled, Grave Titan's 6: their Grizzly Bears dies.
#[test]
fn sickening_shoal_gives_minus_x_minus_x_for_the_card_it_exiled() {
    let mut game = deal_both(
        &[("Swamp", 40), ("Sickening Shoal", 10), ("Grave Titan", 10)],
        &[("Forest", 40), ("Grizzly Bears", 20)],
        &[("Swamp", 3), ("Sickening Shoal", 1), ("Grave Titan", 1)],
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
    let shoal = in_hand(&game, ME, "Sickening Shoal").expect("one kept");
    let titan = in_hand(&game, ME, "Grave Titan").expect("one kept");
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let alt = actions
        .iter()
        .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == shoal))
        .copied()
        .expect("offered");
    let mut d = step(&mut game, Answer::Act(alt));
    if let Decision::Targets { .. } = d {
        d = step(&mut game, Answer::Targets(vec![Target::Object(bears)]));
    }
    if let Decision::Pay { .. } = d {
        d = step(&mut game, Answer::Pay(Payment::Spend(ManaPool::default())));
    }
    let Decision::PayCards { legal, .. } = d else {
        panic!("{d:?}")
    };
    assert!(legal.contains(&titan));
    step(&mut game, Answer::Choose(vec![titan]));
    resolve(&mut game);
    assert_eq!(creature(&game, "Grizzly Bears"), None, "2/2 at -6/-6");
}

/// CR 701.9b, 608.2h, 608.2g: Malcolm's combat damage puts a chorus
/// counter on it, draws and discards the card its controller chooses; the
/// cast of that card waits on four chorus counters, which three hits don't
/// reach; at the fourth it may be declined, and taken it refuses by name,
/// since this engine doesn't cast a card while an instruction resolves.
#[test]
fn malcolm_discards_the_card_chosen_and_refuses_the_free_cast_at_four_counters() {
    let mut game = deal(
        &[("Island", 40), ("Malcolm, Alluring Scoundrel", 20)],
        &[("Forest", 60)],
        &[("Island", 2), ("Malcolm, Alluring Scoundrel", 1)],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    cast(&mut game, "Malcolm, Alluring Scoundrel", None);
    resolve(&mut game);
    let malcolm = creature(&game, "Malcolm, Alluring Scoundrel").unwrap();
    for hit in 1..=4u32 {
        my_next_main(&mut game);
        to_attackers(&mut game);
        step(&mut game, Answer::Attack(vec![(malcolm, THEM)]));
        let d = (0..20)
            .find_map(|_| match game.decision() {
                d @ Decision::Discard { .. } => Some(d),
                Decision::Blockers { .. } => {
                    step(&mut game, Answer::Block(vec![]));
                    None
                }
                _ => {
                    step(&mut game, Answer::Act(Action::Pass));
                    None
                }
            })
            .expect("a discard");
        let Decision::Discard {
            player,
            count,
            random,
            hand,
        } = d
        else {
            unreachable!()
        };
        assert_eq!((player, count, random), (ME, 1, false));
        assert_eq!(
            game.counters_on(
                malcolm,
                &chancellor_of_the_spires::model::Counter::Named("chorus".into())
            ),
            hit
        );
        let card = *hand.last().unwrap();
        match game.step(Answer::Discard(vec![card])) {
            Ok(_) if hit < 4 => {
                assert!(game.player(ME).graveyard().contains(&card));
                assert!(game.stack().is_empty());
                through_combat(&mut game);
            }
            Ok(Decision::May { player: ME, .. }) if hit == 4 => {
                assert!(game.player(ME).graveyard().contains(&card));
                let mut declined = game.clone();
                declined.step(Answer::May(false)).unwrap();
                assert!(declined.player(ME).graveyard().contains(&card));
                match game.step(Answer::May(true)) {
                    Err(Rejected::Refused { card, refusal }) => {
                        assert_eq!(card, "Malcolm, Alluring Scoundrel");
                        assert_eq!(refusal.node, "CastWithoutPaying");
                    }
                    r => panic!("{r:?}"),
                }
                return;
            }
            r => panic!("hit {hit}: {r:?}"),
        }
    }
    panic!("the fourth hit refuses");
}

/// Whether `source` offers an ability other than a mana ability now.
fn activatable(game: &Game, source: chancellor_of_the_spires::model::ObjectId) -> bool {
    match game.decision() {
        Decision::Priority { actions, .. } => actions
            .iter()
            .any(|a| matches!(a, Action::Activate(x) if x.source == source)),
        _ => false,
    }
}

/// CR 602.5: Weathered Wayfarer's "Activate only if an opponent controls
/// more lands than you" is offered only once they have more.
#[test]
fn weathered_wayfarer_searches_only_while_an_opponent_has_more_lands() {
    let mut game = deal_both(
        &[("Plains", 50), ("Weathered Wayfarer", 10)],
        &[("Forest", 60)],
        &[("Plains", 1), ("Weathered Wayfarer", 1)],
        &[("Forest", 2)],
    );
    play(&mut game, "Plains");
    cast(&mut game, "Weathered Wayfarer", None);
    resolve(&mut game);
    let wayfarer = creature(&game, "Weathered Wayfarer").unwrap();
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    assert!(!activatable(&game, wayfarer), "one land each");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    my_next_main(&mut game);
    assert!(activatable(&game, wayfarer), "two lands to my one");
}

/// CR 602.5, 207.2c: Barbarian Ring's threshold ability is offered only
/// with seven or more cards in its controller's graveyard.
#[test]
fn barbarian_ring_deals_its_damage_only_at_threshold() {
    let mut game = deal(
        &[("Mountain", 50), ("Barbarian Ring", 10)],
        &[("Forest", 60)],
        &[("Mountain", 1), ("Barbarian Ring", 1)],
    );
    play(&mut game, "Barbarian Ring");
    let ring = game
        .permanents()
        .iter()
        .find(|(_, p)| {
            game.card(p.card)
                .is_some_and(|t| t.name == "Barbarian Ring")
        })
        .map(|(o, _)| *o)
        .unwrap();
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    for _ in 0..20 {
        let cards = game.player(ME).graveyard().len();
        assert_eq!(
            activatable(&game, ring),
            cards >= 7,
            "{cards} in the graveyard"
        );
        if cards >= 7 {
            return;
        }
        my_next_main(&mut game);
    }
    panic!("never reached threshold");
}

/// Pays what an activation owes with the first payment offered each time.
fn pay_all(game: &mut Game, mut d: Decision) -> Decision {
    while let Decision::Pay { actions, .. } = &d {
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

fn untapped_lands(game: &Game, p: PlayerId) -> usize {
    game.permanents()
        .values()
        .filter(|perm| {
            game.owner(perm.card) == p
                && !perm.tapped
                && game
                    .card(perm.card)
                    .is_some_and(|t| ["Forest", "Swamp", "Mountain"].contains(&t.name.as_str()))
        })
        .count()
}

/// CR 602.2b, 107.3a: Pernicious Deed's "{X}, Sacrifice this
/// enchantment" announces X as it is activated, costs X mana, and
/// destroys each artifact, creature and enchantment of mana value X or
/// less: Llanowar Elves at 1, not Grizzly Bears at 2.
#[test]
fn pernicious_deed_destroys_up_to_the_x_announced_as_it_was_activated() {
    let mut game = deal_both(
        &[("Forest", 25), ("Swamp", 25), ("Pernicious Deed", 10)],
        &[
            ("Forest", 30),
            ("Llanowar Elves", 15),
            ("Grizzly Bears", 15),
        ],
        &[("Forest", 2), ("Swamp", 1), ("Pernicious Deed", 1)],
        &[("Forest", 2), ("Llanowar Elves", 1), ("Grizzly Bears", 1)],
    );
    play(&mut game, "Forest");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Llanowar Elves", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Forest");
    cast_as(&mut game, THEM, "Grizzly Bears", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Forest");
    cast(&mut game, "Pernicious Deed", None);
    resolve(&mut game);
    pass_until(&mut game, THEM, Step::PrecombatMain);
    my_next_main(&mut game);
    if let Some(land) = ["Forest", "Swamp"]
        .into_iter()
        .find(|l| in_hand(&game, ME, l).is_some())
    {
        play(&mut game, land);
    }
    let lands = untapped_lands(&game, ME);
    let deed = creature(&game, "Pernicious Deed").unwrap();
    let d = activate(&mut game, deed);
    assert!(matches!(d, Decision::AnnounceX { .. }), "{d:?}");
    let d = step(&mut game, Answer::X(1));
    pay_all(&mut game, d);
    assert_eq!(untapped_lands(&game, ME), lands - 1, "X is one mana");
    resolve(&mut game);
    assert!(creature(&game, "Llanowar Elves").is_none(), "mana value 1");
    assert!(creature(&game, "Grizzly Bears").is_some(), "mana value 2");
    assert!(creature(&game, "Pernicious Deed").is_none(), "sacrificed");
}

/// CR 107.3a, 107.3i: Blast Zone's "{X}{X}, {T}: Put X charge counters on
/// this land" costs X twice over for the one X announced.
#[test]
fn blast_zone_pays_its_x_twice_and_puts_x_charge_counters() {
    let mut game = deal(
        &[("Mountain", 40), ("Blast Zone", 20)],
        &[("Forest", 60)],
        &[("Mountain", 2), ("Blast Zone", 1)],
    );
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Blast Zone");
    let zone = creature(&game, "Blast Zone").unwrap();
    let charge = chancellor_of_the_spires::model::Counter::Named("charge".into());
    assert_eq!(game.counters_on(zone, &charge), 1, "enters with one");
    let d = activate(&mut game, zone);
    assert!(matches!(d, Decision::AnnounceX { .. }), "{d:?}");
    let d = step(&mut game, Answer::X(1));
    pay_all(&mut game, d);
    assert_eq!(untapped_lands(&game, ME), 0, "{{X}}{{X}} at 1 is two mana");
    resolve(&mut game);
    assert_eq!(game.counters_on(zone, &charge), 2);
}

/// CR 110.2a, 608.2h: Reanimate puts a creature card from a graveyard onto
/// the battlefield under its caster's control, whoever owns it, and its
/// caster loses life equal to its mana value: 6 for an opponent's Craw
/// Wurm.
#[test]
fn reanimate_returns_a_creature_card_and_costs_its_mana_value_in_life() {
    let mut game = deal(
        &[("Swamp", 30), ("Reanimate", 30)],
        &[("Craw Wurm", 60)],
        &[("Swamp", 1), ("Reanimate", 1)],
    );
    play(&mut game, "Swamp");
    // They play no land, so their hand overflows into their graveyard.
    my_next_main(&mut game);
    let wurm = game
        .player(THEM)
        .graveyard()
        .iter()
        .copied()
        .find(|c| game.card(*c).is_some_and(|t| t.name == "Craw Wurm"))
        .expect("discarded to hand size");
    let life = game.player(ME).life();
    cast(&mut game, "Reanimate", Some(vec![Target::Card(wurm)]));
    resolve(&mut game);
    let id = creature(&game, "Craw Wurm").expect("on the battlefield");
    assert_eq!(game.characteristics()[&id].controller, ME);
    assert_eq!(game.owner(wurm), THEM);
    assert!(!game.player(THEM).graveyard().contains(&wurm));
    assert_eq!(game.player(ME).life(), life - 6);
}

/// CR 400.3, 608.2h: Condemn puts the attacking creature on the bottom of
/// its owner's library, and its controller gains life equal to its
/// toughness as it last existed: 1 for Savannah Lions.
#[test]
fn condemn_bottoms_an_attacker_and_its_controller_gains_its_toughness() {
    let mut game = deal_both(
        &[("Plains", 40), ("Savannah Lions", 20)],
        &[("Plains", 40), ("Condemn", 20)],
        &[("Plains", 1), ("Savannah Lions", 1)],
        &[("Plains", 1), ("Condemn", 1)],
    );
    play(&mut game, "Plains");
    cast(&mut game, "Savannah Lions", None);
    resolve(&mut game);
    let lions = creature(&game, "Savannah Lions").unwrap();
    let card = game.permanents()[&lions].card;
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Plains");
    my_next_main(&mut game);
    // CR 508.1k: not yet attacking, so no target for it.
    step(&mut game, Answer::Act(Action::Pass));
    let Decision::Priority { player, actions } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    assert_eq!(player, THEM);
    assert!(
        !actions.iter().any(|a| matches!(a, Action::Cast(c)
            if game.card(*c).is_some_and(|t| t.name == "Condemn"))),
        "Condemn offered with no attacking creature"
    );
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(lions, THEM)]));
    while !matches!(game.decision(), Decision::Priority { player, .. } if player == THEM) {
        step(&mut game, Answer::Act(Action::Pass));
    }
    let life = game.player(ME).life();
    cast_as(
        &mut game,
        THEM,
        "Condemn",
        Some(vec![Target::Object(lions)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Savannah Lions").is_none());
    assert_eq!(game.player(ME).library().back(), Some(&card));
    assert_eq!(game.player(ME).life(), life + 1);
}

const GIANT: &str = "Bonecrusher Giant // Stomp";

/// The Giant on my battlefield by turn three, Silver Knight (protection
/// from red, first strike) on theirs, and my fifth Mountain played.
fn giant_against_the_knight() -> (
    Game,
    chancellor_of_the_spires::model::ObjectId,
    chancellor_of_the_spires::model::ObjectId,
) {
    let mut game = deal_both(
        &[("Mountain", 40), (GIANT, 20)],
        &[("Plains", 40), ("Silver Knight", 20)],
        &[("Mountain", 5), (GIANT, 2)],
        &[("Plains", 2), ("Silver Knight", 1)],
    );
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Plains");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Plains");
    cast_as(&mut game, THEM, "Silver Knight", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, GIANT, None);
    resolve(&mut game);
    for _ in 0..2 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    let giant = creature(&game, GIANT).unwrap();
    let knight = creature(&game, "Silver Knight").unwrap();
    (game, giant, knight)
}

/// CR 615.12, 702.16e, 514.2: after Stomp's "Damage can't be prevented
/// this turn", Silver Knight's protection from red still applies but
/// prevents nothing, so blocking Bonecrusher Giant it is dealt the
/// Giant's 4 and dies.
#[test]
fn stomp_lets_damage_through_protection_this_turn() {
    let (mut game, giant, knight) = giant_against_the_knight();
    let stomp = in_hand(&game, ME, GIANT).unwrap();
    let d = step(&mut game, Answer::Act(Action::CastBack(stomp)));
    let d = match d {
        Decision::Targets { .. } => step(&mut game, Answer::Targets(vec![Target::Player(THEM)])),
        d => panic!("{d:?}"),
    };
    pay(&mut game, d);
    resolve(&mut game);
    assert_eq!(game.player(THEM).life(), 18);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(giant, THEM)]));
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
    assert!(!game.permanents().contains_key(&knight), "not prevented");
    assert_eq!(game.permanents()[&giant].damage, 2, "first strike");
}

/// CR 603.2, 115.1: Bonecrusher Giant becoming the target of a spell, my
/// own Stomp, deals 2 damage to that spell's controller, me; then Stomp
/// deals it 2.
#[test]
fn bonecrusher_giant_damages_the_controller_of_a_spell_targeting_it() {
    let (mut game, giant, _) = giant_against_the_knight();
    let stomp = in_hand(&game, ME, GIANT).unwrap();
    let d = step(&mut game, Answer::Act(Action::CastBack(stomp)));
    let d = match d {
        Decision::Targets { .. } => step(&mut game, Answer::Targets(vec![Target::Object(giant)])),
        d => panic!("{d:?}"),
    };
    pay(&mut game, d);
    assert_eq!(game.stack().len(), 2, "Stomp and the Giant's trigger");
    resolve(&mut game);
    assert_eq!(game.player(ME).life(), 18);
    resolve(&mut game);
    assert_eq!(game.permanents()[&giant].damage, 2);
}

/// CR 608.2c, 608.2h: Ghastly Demise destroys its target only if the
/// target's toughness is at most the number of cards in my graveyard, as
/// it resolves. The first, with my graveyard empty, leaves Savannah Lions
/// (toughness 1); the second, with the first in it, destroys them.
#[test]
fn ghastly_demise_destroys_only_what_my_graveyard_outnumbers() {
    let mut game = deal_both(
        &[("Swamp", 40), ("Ghastly Demise", 20)],
        &[("Plains", 40), ("Savannah Lions", 20)],
        &[("Swamp", 2), ("Ghastly Demise", 2)],
        &[("Plains", 1), ("Savannah Lions", 1)],
    );
    play(&mut game, "Swamp");
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Plains");
    cast_as(&mut game, THEM, "Savannah Lions", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    let lions = creature(&game, "Savannah Lions").unwrap();
    cast(
        &mut game,
        "Ghastly Demise",
        Some(vec![Target::Object(lions)]),
    );
    resolve(&mut game);
    assert_eq!(game.player(ME).graveyard().len(), 1);
    assert!(creature(&game, "Savannah Lions").is_some());
    cast(
        &mut game,
        "Ghastly Demise",
        Some(vec![Target::Object(lions)]),
    );
    resolve(&mut game);
    assert!(creature(&game, "Savannah Lions").is_none());
}

/// CR 702.33d, 608.2c: Overload destroys an artifact with mana value 2 or
/// less, or 5 or less if it was kicked. Su-Chi, mana value 4, survives it
/// unkicked and is destroyed by it kicked.
#[test]
fn overload_kicked_destroys_a_bigger_artifact() {
    let mut game = deal(
        &[("Mountain", 40), ("Overload", 10), ("Su-Chi", 10)],
        &[("Forest", 60)],
        &[("Mountain", 5), ("Overload", 1), ("Su-Chi", 1)],
    );
    play(&mut game, "Mountain");
    for _ in 0..3 {
        my_next_main(&mut game);
        play(&mut game, "Mountain");
    }
    cast(&mut game, "Su-Chi", None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    let su_chi = creature(&game, "Su-Chi").unwrap();
    let overload = in_hand(&game, ME, "Overload").unwrap();
    for (kicked, survives) in [(false, true), (true, false)] {
        let mut game = game.clone();
        let d = step(&mut game, Answer::Act(Action::Cast(overload)));
        assert!(matches!(d, Decision::Kicker { .. }), "{d:?}");
        step(&mut game, Answer::Kick(kicked));
        let mut d = step(&mut game, Answer::Targets(vec![Target::Object(su_chi)]));
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
        assert_eq!(
            creature(&game, "Su-Chi").is_some(),
            survives,
            "kicked {kicked}"
        );
    }
}

const RAUBAHN: &str = "Raubahn, Bull of Ala Mhigo";

/// CR 702.21a–b, 119.4: Raubahn's "Ward—Pay life equal to Raubahn's
/// power" counters their Lightning Bolt unless they pay 2 life, its power
/// as the ward resolves; paid, the Bolt resolves and Raubahn, 2/2, dies.
#[test]
fn raubahns_ward_asks_life_equal_to_its_power() {
    for pays in [false, true] {
        let mut game = deal_both(
            &[("Mountain", 40), (RAUBAHN, 20)],
            &[("Mountain", 40), ("Lightning Bolt", 20)],
            &[("Mountain", 2), (RAUBAHN, 1)],
            &[("Mountain", 2), ("Lightning Bolt", 1)],
        );
        play(&mut game, "Mountain");
        pass_until(&mut game, THEM, Step::PrecombatMain);
        play_as(&mut game, THEM, "Mountain");
        my_next_main(&mut game);
        play(&mut game, "Mountain");
        cast(&mut game, RAUBAHN, None);
        resolve(&mut game);
        let raubahn = creature(&game, RAUBAHN).unwrap();
        pass_until(&mut game, THEM, Step::PrecombatMain);
        let bolt = in_hand(&game, THEM, "Lightning Bolt").unwrap();
        cast_as(
            &mut game,
            THEM,
            "Lightning Bolt",
            Some(vec![Target::Object(raubahn)]),
        );
        assert_eq!(game.stack().len(), 2, "ward triggered over the Bolt");
        let Decision::PayLifeOr { player, life, .. } = resolve(&mut game) else {
            panic!("{:?}", game.decision())
        };
        assert_eq!((player, life), (THEM, 2));
        step(&mut game, Answer::PayLife(pays));
        if pays {
            assert_eq!(game.player(THEM).life(), 18);
            resolve(&mut game);
            assert!(creature(&game, RAUBAHN).is_none());
        } else {
            assert_eq!(game.player(THEM).life(), 20);
            assert!(game.player(THEM).graveyard().contains(&bolt));
            assert!(game.stack().is_empty());
            assert!(creature(&game, RAUBAHN).is_some());
        }
    }
}

/// CR 701.3a, 115.6: Raubahn attacking attaches up to one target
/// Equipment I control, Bonesplitter, to target attacking creature,
/// itself, which then has +2/+0.
#[test]
fn raubahn_attacking_attaches_an_equipment_to_an_attacker() {
    let mut game = deal(
        &[("Mountain", 40), (RAUBAHN, 10), ("Bonesplitter", 10)],
        &[("Forest", 60)],
        &[("Mountain", 3), (RAUBAHN, 1), ("Bonesplitter", 1)],
    );
    play(&mut game, "Mountain");
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, RAUBAHN, None);
    resolve(&mut game);
    my_next_main(&mut game);
    play(&mut game, "Mountain");
    cast(&mut game, "Bonesplitter", None);
    resolve(&mut game);
    let raubahn = creature(&game, RAUBAHN).unwrap();
    let splitter = creature(&game, "Bonesplitter").unwrap();
    to_attackers(&mut game);
    let d = step(&mut game, Answer::Attack(vec![(raubahn, THEM)]));
    let Decision::Targets { slots, .. } = d else {
        panic!("{d:?}")
    };
    assert_eq!(
        slots,
        vec![
            vec![Target::Object(splitter), Target::Nothing],
            vec![Target::Object(raubahn)]
        ]
    );
    step(
        &mut game,
        Answer::Targets(vec![Target::Object(splitter), Target::Object(raubahn)]),
    );
    resolve(&mut game);
    assert_eq!(game.characteristics()[&raubahn].pt, Some(pt(4, 2)));
}

/// CR 406.3, 611.2a, 305.2a, 514.2: Escape to the Wilds exiles the top
/// five cards of its caster's library, which they may play until the end
/// of their next turn, and gives them one more land this turn. A land of
/// those five is played as the turn's extra land, a second isn't; a spell
/// of them is cast from exile; and none is offered after their next turn.
#[test]
fn escape_to_the_wilds_lets_its_caster_play_the_exiled_cards_until_their_next_turn_ends() {
    let mut game = deal_both(
        &[
            ("Mountain", 20),
            ("Forest", 20),
            ("Escape to the Wilds", 4),
            ("Grizzly Bears", 16),
        ],
        &[("Forest", 30), ("Craw Wurm", 30)],
        &[("Mountain", 3), ("Forest", 2), ("Escape to the Wilds", 1)],
        &[],
    );
    for land in ["Mountain", "Forest", "Mountain", "Forest"] {
        play(&mut game, land);
        my_next_main(&mut game);
    }
    play(&mut game, "Mountain");
    let top: Vec<CardId> = game.player(ME).library().iter().take(5).copied().collect();
    cast(&mut game, "Escape to the Wilds", None);
    resolve(&mut game);
    assert!(top.iter().all(|c| game.exile().contains(c)));
    let mut playable: Vec<CardId> = game.may_play(ME).collect();
    playable.sort();
    let mut exiled = top.clone();
    exiled.sort();
    assert_eq!(playable, exiled);
    let is = |game: &Game, c: &CardId, name: &str| game.card(*c).is_some_and(|t| t.name == name);
    let lands: Vec<CardId> = top
        .iter()
        .copied()
        .filter(|c| is(&game, c, "Mountain") || is(&game, c, "Forest"))
        .collect();
    assert!(lands.len() >= 2, "the seed exiles two lands: {lands:?}");
    let offered = |game: &Game| {
        let Decision::Priority { actions, .. } = game.decision() else {
            panic!("{:?}", game.decision())
        };
        actions
    };
    assert!(offered(&game).contains(&Action::PlayLand(lands[0])));
    step(&mut game, Answer::Act(Action::PlayLand(lands[0])));
    assert!(!game.exile().contains(&lands[0]));
    assert!(
        !offered(&game).contains(&Action::PlayLand(lands[1])),
        "two lands this turn are all"
    );
    // Its next turn, the permission still holds, and a land is played
    // from exile as that turn's land.
    my_next_main(&mut game);
    assert!(offered(&game).contains(&Action::PlayLand(lands[1])));
    let bears = *top
        .iter()
        .find(|c| is(&game, c, "Grizzly Bears"))
        .expect("the seed exiles a Grizzly Bears");
    assert!(offered(&game).contains(&Action::Cast(bears)));
    step(&mut game, Answer::Act(Action::PlayLand(lands[1])));
    assert!(
        !offered(&game)
            .iter()
            .any(|a| matches!(a, Action::PlayLand(_))),
        "one land this turn, the extra one ended"
    );
    cast_card(&mut game, bears, None);
    assert!(!game.exile().contains(&bears));
    assert!(!game.may_play(ME).any(|c| c == bears));
    // After it, none is.
    my_next_main(&mut game);
    assert_eq!(game.may_play(ME).count(), 0);
}

/// Pass while a spell or ability is on the stack, or a trigger waits to
/// go on it, stopping at any other decision.
fn settle(game: &mut Game) -> Decision {
    loop {
        match game.decision() {
            Decision::Priority { .. } if !game.stack().is_empty() || !game.pending().is_empty() => {
                step(game, Answer::Act(Action::Pass));
            }
            d => return d,
        }
    }
}

/// CR 701.21a, 101.3: Phyrexian Negator's "Whenever this creature is dealt
/// damage, sacrifice that many permanents". Two Lightning Bolts: the first
/// has its controller choose three of their permanents as it resolves;
/// the second kills it, and they sacrifice as many as they have left, the
/// one Swamp they kept or none.
#[test]
fn phyrexian_negator_has_its_controller_sacrifice_that_many_permanents() {
    for swamps in [3, 4] {
        let mut game = deal_both(
            &[("Swamp", 40), ("Phyrexian Negator", 20)],
            &[("Mountain", 40), ("Lightning Bolt", 20)],
            &[("Swamp", swamps), ("Phyrexian Negator", 1)],
            &[("Mountain", 2), ("Lightning Bolt", 2)],
        );
        play(&mut game, "Swamp");
        for _ in 1..swamps {
            pass_until(&mut game, THEM, Step::PrecombatMain);
            play_as(&mut game, THEM, "Mountain");
            my_next_main(&mut game);
            play(&mut game, "Swamp");
        }
        cast(&mut game, "Phyrexian Negator", None);
        resolve(&mut game);
        let negator = creature(&game, "Phyrexian Negator").unwrap();
        pass_until(&mut game, THEM, Step::PrecombatMain);
        cast_as(
            &mut game,
            THEM,
            "Lightning Bolt",
            Some(vec![Target::Object(negator)]),
        );
        cast_as(
            &mut game,
            THEM,
            "Lightning Bolt",
            Some(vec![Target::Object(negator)]),
        );
        resolve(&mut game);
        let d = settle(&mut game);
        let Decision::Sacrifice {
            player,
            count,
            legal,
            cost,
            ..
        } = d
        else {
            panic!("the first trigger's sacrifice: {d:?}")
        };
        assert_eq!((player, count, cost), (ME, 3, false));
        assert_eq!(legal.len(), swamps + 1, "my Swamps and the Negator");
        assert_eq!(
            game.resolving(),
            Some(game.permanents()[&negator].card),
            "its trigger is resolving"
        );
        let lands: Vec<_> = legal.iter().copied().filter(|o| *o != negator).collect();
        assert!(
            game.clone()
                .step(Answer::Sacrifice(lands[..2].to_vec()))
                .is_err(),
            "exactly three"
        );
        step(&mut game, Answer::Sacrifice(lands[..3].to_vec()));
        let d = settle(&mut game);
        assert!(!game.permanents().contains_key(&negator), "six damage");
        if swamps == 3 {
            assert!(
                matches!(d, Decision::Priority { .. }),
                "nothing left: {d:?}"
            );
        } else {
            let Decision::Sacrifice {
                player,
                count,
                legal,
                ..
            } = d
            else {
                panic!("the second trigger's sacrifice: {d:?}")
            };
            assert_eq!((player, count), (ME, 1), "three asked, one left");
            assert_eq!(legal, vec![lands[3]]);
            step(&mut game, Answer::Sacrifice(legal));
        }
        assert!(game
            .permanents()
            .values()
            .all(|p| game.owner(p.card) == THEM));
    }
}

/// CR 702.86a, 508.5: Eldrazi Ravager's annihilator 1 has the player it
/// attacks sacrifice a permanent of their choice, and still the player it
/// attacked once two Lightning Bolts kill it in response.
#[test]
fn annihilator_has_the_defending_player_sacrifice_a_permanent() {
    let mut game = deal_both(
        &[("Wastes", 40), ("Eldrazi Ravager", 20)],
        &[("Mountain", 40), ("Lightning Bolt", 20)],
        &[("Wastes", 2), ("Eldrazi Ravager", 1)],
        &[("Mountain", 2), ("Lightning Bolt", 2)],
    );
    let wastes = |game: &Game| {
        game.permanents()
            .values()
            .filter(|p| game.card(p.card).unwrap().name == "Wastes")
            .count()
    };
    play(&mut game, "Wastes");
    for _ in 0..30 {
        if wastes(&game) == 6 {
            break;
        }
        pass_until(&mut game, THEM, Step::PrecombatMain);
        if in_hand(&game, THEM, "Mountain").is_some() {
            play_as(&mut game, THEM, "Mountain");
        }
        my_next_main(&mut game);
        if in_hand(&game, ME, "Wastes").is_some() {
            play(&mut game, "Wastes");
        }
    }
    assert_eq!(wastes(&game), 6);
    cast(&mut game, "Eldrazi Ravager", None);
    resolve(&mut game);
    let ravager = creature(&game, "Eldrazi Ravager").unwrap();
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(ravager, THEM)]));
    let d = settle(&mut game);
    let Decision::Sacrifice {
        player,
        count,
        legal,
        cost,
        ..
    } = d
    else {
        panic!("annihilator's sacrifice: {d:?}")
    };
    assert_eq!((player, count, cost), (THEM, 1, false));
    let theirs = |game: &Game| {
        game.permanents()
            .iter()
            .filter(|(_, p)| game.owner(p.card) == THEM)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
    };
    assert_eq!(legal, theirs(&game), "any permanent of theirs");
    let before = legal.len();
    step(&mut game, Answer::Sacrifice(vec![legal[0]]));
    assert_eq!(theirs(&game).len(), before - 1);
    through_combat(&mut game);
    my_next_main(&mut game);
    to_attackers(&mut game);
    step(&mut game, Answer::Attack(vec![(ravager, THEM)]));
    // The trigger on the stack, they kill it in response.
    loop {
        match game.decision() {
            Decision::Priority { player: THEM, .. } if !game.stack().is_empty() => break,
            _ => {
                step(&mut game, Answer::Act(Action::Pass));
            }
        }
    }
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Object(ravager)]),
    );
    cast_as(
        &mut game,
        THEM,
        "Lightning Bolt",
        Some(vec![Target::Object(ravager)]),
    );
    resolve(&mut game);
    resolve(&mut game);
    assert!(!game.permanents().contains_key(&ravager));
    let before = theirs(&game).len();
    let d = settle(&mut game);
    let Decision::Sacrifice { player, legal, .. } = d else {
        panic!("annihilator's sacrifice, its creature gone: {d:?}")
    };
    assert_eq!(player, THEM, "the player it attacked");
    step(&mut game, Answer::Sacrifice(vec![legal[0]]));
    assert_eq!(theirs(&game).len(), before - 1);
}

/// CR 701.21a: Springbloom Druid's "you may sacrifice a land. If you do,
/// search …" is refused by name: a "may"'s sacrifice pays for what
/// follows, and as many lands as there are, none, would not.
#[test]
fn a_may_s_sacrifice_is_refused() {
    let game = deal_both(
        &[("Forest", 40), ("Springbloom Druid", 20)],
        &[("Forest", 60)],
        &[("Springbloom Druid", 1)],
        &[],
    );
    let druid = in_hand(&game, ME, "Springbloom Druid").unwrap();
    let refusal = game.refusal(druid).expect("refused");
    assert_eq!(refusal.node, "sacrificing permanents as a \"may\"");
}

/// CR 702.74a, 113.6k, 603.4: Nulldrifter cast for its evoke cost: its
/// "When you cast this spell, draw two cards" triggers from the stack and
/// resolves before it, and as it enters its evoke trigger sacrifices it.
/// Cast for its mana cost, it draws the same and stays.
#[test]
fn nulldrifter_draws_as_it_is_cast_and_is_sacrificed_when_evoked() {
    let mut game = deal_both(
        &[("Island", 40), ("Nulldrifter", 20)],
        &[("Mountain", 60)],
        &[("Island", 3), ("Nulldrifter", 2)],
        &[],
    );
    let islands = |game: &Game| {
        game.permanents()
            .values()
            .filter(|p| game.card(p.card).unwrap().name == "Island")
            .count()
    };
    let to_islands = |game: &mut Game, n: usize| {
        for _ in 0..30 {
            if islands(game) == n {
                return;
            }
            my_next_main(game);
            if in_hand(game, ME, "Island").is_some() {
                play(game, "Island");
            }
        }
        panic!("never {n} Islands");
    };
    play(&mut game, "Island");
    to_islands(&mut game, 3);
    let drifter = in_hand(&game, ME, "Nulldrifter").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let evoke = actions
        .into_iter()
        .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == drifter))
        .expect("its evoke cost is offered");
    let hand = game.player(ME).hand().len();
    let mut d = step(&mut game, Answer::Act(evoke));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    assert_eq!(islands(&game), 3);
    assert_eq!(
        game.pending().len() + game.stack().len(),
        2,
        "it and its cast trigger"
    );
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand - 1 + 2);
    assert_eq!(game.stack().len(), 1, "Nulldrifter still on the stack");
    resolve(&mut game);
    assert!(creature(&game, "Nulldrifter").is_some(), "it enters");
    assert_eq!(
        game.pending().len() + game.stack().len(),
        1,
        "evoke's trigger"
    );
    settle(&mut game);
    assert!(creature(&game, "Nulldrifter").is_none());
    assert!(game.player(ME).graveyard().contains(&drifter));

    to_islands(&mut game, 7);
    let hand = game.player(ME).hand().len();
    cast(&mut game, "Nulldrifter", None);
    resolve(&mut game);
    assert_eq!(game.player(ME).hand().len(), hand - 1 + 2);
    resolve(&mut game);
    settle(&mut game);
    assert!(
        creature(&game, "Nulldrifter").is_some(),
        "not evoked, it stays"
    );
}

/// CR 400.7, 702.74a: the evoked Nulldrifter is sacrificed, and Reanimate
/// returning it makes a new object, whose evoke cost no one paid: it stays.
#[test]
fn an_evoked_creature_reanimated_isnt_sacrificed_again() {
    let mut game = deal_both(
        &[
            ("Island", 30),
            ("Swamp", 10),
            ("Nulldrifter", 10),
            ("Reanimate", 10),
        ],
        &[("Mountain", 60)],
        &[
            ("Island", 3),
            ("Swamp", 1),
            ("Nulldrifter", 1),
            ("Reanimate", 1),
        ],
        &[],
    );
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    my_next_main(&mut game);
    play(&mut game, "Island");
    let drifter = in_hand(&game, ME, "Nulldrifter").unwrap();
    let Decision::Priority { actions, .. } = game.decision() else {
        panic!("{:?}", game.decision())
    };
    let evoke = actions
        .into_iter()
        .find(|a| matches!(a, Action::CastAlternative(c, _) if *c == drifter))
        .expect("its evoke cost is offered");
    let mut d = step(&mut game, Answer::Act(evoke));
    while let Decision::Pay { actions, .. } = d {
        let pay = actions
            .iter()
            .find(|a| matches!(a, Payment::Spend(_)))
            .or_else(|| actions.iter().find(|a| matches!(a, Payment::Mana(_))))
            .copied()
            .expect("a way to pay");
        d = step(&mut game, Answer::Pay(pay));
    }
    settle(&mut game);
    assert!(
        game.player(ME).graveyard().contains(&drifter),
        "evoked away"
    );
    my_next_main(&mut game);
    play(&mut game, "Swamp");
    cast(&mut game, "Reanimate", Some(vec![Target::Card(drifter)]));
    resolve(&mut game);
    settle(&mut game);
    assert!(
        creature(&game, "Nulldrifter").is_some(),
        "a new object stays"
    );
}

/// CR 702.35a, 608.2g: Careful Study's discard of Basking Rootwalla exiles
/// it instead, and its trigger asks its owner whether to cast it for its
/// madness cost: taken, it is cast from exile and enters; declined, it
/// goes to the graveyard. The Island discarded beside it goes there either
/// way.
#[test]
fn a_card_with_madness_is_exiled_as_it_is_discarded_and_may_be_cast() {
    for take in [true, false] {
        let mut game = deal(
            &[
                ("Island", 30),
                ("Careful Study", 10),
                ("Basking Rootwalla", 20),
            ],
            &[("Island", 60)],
            &[
                ("Island", 2),
                ("Careful Study", 1),
                ("Basking Rootwalla", 1),
            ],
        );
        play(&mut game, "Island");
        cast(&mut game, "Careful Study", None);
        let Decision::Discard { count, .. } = resolve(&mut game) else {
            panic!("the discard")
        };
        assert_eq!(count, 2);
        let walla = in_hand(&game, ME, "Basking Rootwalla").unwrap();
        let island = in_hand(&game, ME, "Island").unwrap();
        step(&mut game, Answer::Discard(vec![walla, island]));
        assert!(game.exile().contains(&walla), "exiled, not discarded");
        assert!(!game.player(ME).graveyard().contains(&walla));
        assert!(game.player(ME).graveyard().contains(&island));
        let d = resolve(&mut game);
        assert_eq!(
            d,
            Decision::May {
                player: ME,
                card: walla
            }
        );
        let d = step(&mut game, Answer::May(take));
        if take {
            assert!(matches!(d, Decision::Pay { .. }), "{d:?}");
            assert!(!game.exile().contains(&walla));
            // Taken back, the cast is the "may" asked again (CR 733.1).
            let d = step(&mut game, Answer::Pay(Payment::Cancel));
            assert_eq!(
                d,
                Decision::May {
                    player: ME,
                    card: walla
                }
            );
            assert!(game.exile().contains(&walla));
            let mut d = step(&mut game, Answer::May(true));
            while let Decision::Pay { actions, .. } = d {
                d = step(&mut game, Answer::Pay(actions[0]));
            }
            assert!(!game.exile().contains(&walla));
            assert_eq!(game.stack().len(), 1, "the Rootwalla spell");
            resolve(&mut game);
            assert!(creature(&game, "Basking Rootwalla").is_some());
        } else {
            assert!(game.exile().is_empty());
            assert!(game.player(ME).graveyard().contains(&walla));
            assert!(creature(&game, "Basking Rootwalla").is_none());
        }
    }
}

/// Careful Study cast and resolved to its discard, these two cards of my
/// hand discarded.
fn study_discarding(game: &mut Game, first: &str, second: &str) -> Result<Decision, Rejected> {
    play(game, "Island");
    cast(game, "Careful Study", None);
    let Decision::Discard { .. } = resolve(game) else {
        panic!("the discard")
    };
    let a = in_hand(game, ME, first).unwrap();
    let b = game
        .player(ME)
        .hand()
        .iter()
        .copied()
        .find(|c| *c != a && game.card(*c).is_some_and(|t| t.name == second))
        .unwrap();
    game.step(Answer::Discard(vec![a, b]))
}

/// CR 514.3a, 702.35a: Basking Rootwalla discarded to hand size in the
/// cleanup step is exiled, and its trigger goes on the stack then, its
/// owner getting priority in that cleanup step.
#[test]
fn a_madness_card_discarded_to_hand_size_triggers_in_the_cleanup_step() {
    let mut game = deal(
        &[("Island", 30), ("Basking Rootwalla", 30)],
        &[("Island", 60)],
        &[("Basking Rootwalla", 1)],
    );
    my_next_main(&mut game);
    let walla = in_hand(&game, ME, "Basking Rootwalla").unwrap();
    let mut d = game.decision();
    for _ in 0..50 {
        if let Decision::Discard { .. } = d {
            break;
        }
        d = step(&mut game, Answer::Act(Action::Pass));
    }
    assert!(matches!(d, Decision::Discard { count: 1, .. }), "{d:?}");
    let d = step(&mut game, Answer::Discard(vec![walla]));
    assert!(game.exile().contains(&walla));
    assert!(matches!(d, Decision::Priority { player: ME, .. }), "{d:?}");
    assert_eq!(game.step_now(), Step::Cleanup);
    assert_eq!(game.active(), ME);
    let d = resolve(&mut game);
    assert_eq!(
        d,
        Decision::May {
            player: ME,
            card: walla
        }
    );
}

/// CR 608.2g, 117.3b: my Rootwalla discarded to their Thoughtseize is cast
/// as its trigger resolves on their turn, and then they, the active
/// player, get priority, not I who cast it.
#[test]
fn after_a_madness_cast_the_active_player_gets_priority() {
    let mut game = deal_both(
        &[("Island", 30), ("Basking Rootwalla", 30)],
        &[("Swamp", 30), ("Thoughtseize", 30)],
        &[("Basking Rootwalla", 1)],
        &[("Swamp", 1), ("Thoughtseize", 1)],
    );
    pass_until(&mut game, THEM, Step::PrecombatMain);
    play_as(&mut game, THEM, "Swamp");
    cast_as(
        &mut game,
        THEM,
        "Thoughtseize",
        Some(vec![Target::Player(ME)]),
    );
    let Decision::ChooseFromHand { .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    let walla = in_hand(&game, ME, "Basking Rootwalla").unwrap();
    step(&mut game, Answer::Pick(Some(walla)));
    assert!(game.exile().contains(&walla));
    let d = resolve(&mut game);
    assert_eq!(
        d,
        Decision::May {
            player: ME,
            card: walla
        }
    );
    let mut d = step(&mut game, Answer::May(true));
    while let Decision::Pay { actions, .. } = d {
        d = step(&mut game, Answer::Pay(actions[0]));
    }
    assert!(
        matches!(d, Decision::Priority { player: THEM, .. }),
        "{d:?}"
    );
    assert_eq!(game.stack().len(), 1, "the Rootwalla spell");
}

/// CR 702.35a: Avacyn's Judgment's "Madness {X}{R}" is a cost the engine
/// can't pay, so the card is refused; discarding it is refused too, since
/// its madness would replace that discard.
#[test]
fn discarding_a_refused_card_with_madness_is_refused() {
    let mut game = deal(
        &[
            ("Island", 30),
            ("Careful Study", 10),
            ("Avacyn's Judgment", 20),
        ],
        &[("Island", 60)],
        &[
            ("Island", 1),
            ("Careful Study", 1),
            ("Avacyn's Judgment", 2),
        ],
    );
    let refused = study_discarding(&mut game, "Avacyn's Judgment", "Avacyn's Judgment");
    match refused {
        Err(Rejected::Refused { card, .. }) => assert_eq!(card, "Avacyn's Judgment"),
        r => panic!("{r:?}"),
    }
}

/// CR 601.2c, 702.35a: Just the Wind discarded with no creature on the
/// battlefield to target can't be cast as its trigger resolves, so it goes to the
/// graveyard even when its owner would cast it.
#[test]
fn a_madness_spell_with_no_legal_target_goes_to_the_graveyard() {
    let mut game = deal(
        &[("Island", 30), ("Careful Study", 10), ("Just the Wind", 20)],
        &[("Island", 60)],
        &[("Island", 2), ("Careful Study", 1), ("Just the Wind", 1)],
    );
    study_discarding(&mut game, "Just the Wind", "Island").unwrap();
    assert_eq!(named(&game, game.exile(), "Just the Wind"), 1);
    let Decision::May { card, .. } = resolve(&mut game) else {
        panic!("{:?}", game.decision())
    };
    step(&mut game, Answer::May(true));
    assert!(game
        .stack()
        .iter()
        .all(|o| !matches!(o, StackObject::Spell { card: c, .. } if *c == card)));
    assert!(game.player(ME).graveyard().contains(&card));
    assert!(!game.exile().contains(&card));
}

/// CR 400.11, 400.11a: Living Wish's card "you own from outside the game"
/// is one no search of a library finds, and the engine holds no cards
/// outside the game (a sideboard's), so the spell is refused by name
/// rather than resolving to nothing. Demonic Tutor's search of the
/// library still runs.
#[test]
fn a_card_from_outside_the_game_is_refused() {
    let game = deal(
        &[("Forest", 30), ("Living Wish", 15), ("Demonic Tutor", 15)],
        &[("Forest", 60)],
        &[("Living Wish", 1), ("Demonic Tutor", 1)],
    );
    let wish = in_hand(&game, ME, "Living Wish").unwrap();
    let refusal = game.refusal(wish).expect("refused");
    assert_eq!(refusal.node, "With a card no instruction finds");
    let tutor = in_hand(&game, ME, "Demonic Tutor").unwrap();
    assert_eq!(game.refusal(tutor), None);
}

/// CR 608.2d, 701.20a, 401.4: Commune with Dinosaurs looks at the top five,
/// and "You may reveal a Dinosaur or land card from among them and put it
/// into your hand" is a choice of up to one card the filter matches, that
/// card revealed: chosen, it goes to the hand and the other four to the
/// bottom; none chosen, the "may" declined, all five go there. A card the
/// filter doesn't match can't be chosen.
#[test]
fn commune_with_dinosaurs_puts_a_land_from_among_five_into_the_hand() {
    let mut game = deal(
        &[
            ("Forest", 30),
            ("Grizzly Bears", 10),
            ("Commune with Dinosaurs", 20),
        ],
        &[("Forest", 60)],
        &[("Forest", 1), ("Commune with Dinosaurs", 1)],
    );
    play(&mut game, "Forest");
    cast(&mut game, "Commune with Dinosaurs", None);
    let top: Vec<CardId> = game.player(ME).library().iter().take(5).copied().collect();
    let d = resolve(&mut game);
    let Decision::ChooseMatching {
        player,
        cards,
        up_to,
        reveals,
        ..
    } = d
    else {
        panic!("{d:?}")
    };
    assert_eq!((player, up_to, reveals), (ME, 1, true));
    assert_eq!(cards, top);
    let named = |game: &Game, n: &str| {
        top.iter()
            .copied()
            .find(|c| game.card(*c).unwrap().name == n)
    };
    let other = top
        .iter()
        .copied()
        .find(|c| game.card(*c).unwrap().name != "Forest")
        .expect("a card not a land among the five");
    assert!(matches!(
        game.clone().step(Answer::Finds(vec![Found::Card(other)])),
        Err(Rejected::Illegal(_))
    ));
    let to_bottom = |game: &mut Game, mut d: Decision| {
        for _ in 0..5 {
            match d {
                Decision::Arrange { cards, .. } => d = step(game, Answer::Arrange(cards)),
                _ => return d,
            }
        }
        d
    };
    let hand = game.player(ME).hand().len();
    let mut none = game.clone();
    let d = step(&mut none, Answer::Finds(vec![]));
    to_bottom(&mut none, d);
    assert_eq!(none.player(ME).hand().len(), hand);
    let library = none.player(ME).library();
    assert!(top
        .iter()
        .all(|c| library.iter().skip(library.len() - 5).any(|x| x == c)));
    let forest = named(&game, "Forest").expect("a Forest among five, of 30 in 60");
    let d = step(&mut game, Answer::Finds(vec![Found::Card(forest)]));
    to_bottom(&mut game, d);
    assert_eq!(game.player(ME).hand().len(), hand + 1);
    assert!(game.player(ME).hand().contains(&forest));
    let library = game.player(ME).library();
    let bottom: Vec<CardId> = library.iter().skip(library.len() - 4).copied().collect();
    assert!(top
        .iter()
        .filter(|c| **c != forest)
        .all(|c| bottom.contains(c)));
}
