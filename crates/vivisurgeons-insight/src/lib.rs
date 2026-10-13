//! Vivisurgeon's Insight: Oracle text in, a typed ability tree out.
//!
//! A spike. Its question is how much of Magic a grammar can read into a tree
//! with no hole in it, and where the rest breaks; see README.md for the number.

pub mod ast;
pub mod corpus;
pub mod golden;
pub mod lex;
pub mod lint;
pub mod parse;
pub mod unparse;
pub mod vocab;
pub mod walk;

use ast::Ability;

/// One face of a card as Scryfall prints it.
pub struct Face<'a> {
    pub name: &'a str,
    pub type_line: &'a str,
    pub text: &'a str,
}

pub struct Line {
    /// The line as read, with the card's name as `~`.
    pub text: String,
    pub parsed: Result<Ability, Broke>,
    /// An instant or sorcery's paragraph after its first: the same spell's
    /// instructions, followed after the ones before (CR 608.2c), so it may
    /// open with "Then".
    pub continues: bool,
    /// The tree as the line's own words say it, where reading the card's
    /// other lines resolved it further (Swarming Goblins' "Create two of
    /// those tokens" under the row that describes them): the round trip
    /// says this.
    pub said: Option<Ability>,
}

/// Where a line stopped parsing: the words read and the word it could not.
pub struct Broke {
    pub read: String,
    pub at: String,
}

/// Every ability line on one face. A modal spell's bullets belong to the
/// "Choose one —" line above them.
pub fn read_face(face: &Face, card_name: &str) -> Vec<Line> {
    let legendary = face.type_line.contains("Legendary");
    let spell = face.type_line.contains("Instant") || face.type_line.contains("Sorcery");
    let marked = lex::mark_self(face.text, &[face.name, card_name], legendary);
    // "Gideon the Oathless" is "Gideon" in its own text (CR 201.5c): a
    // planeswalker whose name starts with its own subtype is called by it.
    // Only planeswalkers: "Sliver Queen" says "Sliver creature token" of
    // the type. And not before "'s": Nissa Revane names "Nissa's Chosen".
    let short = (face.type_line.contains("Planeswalker") && !face.name.contains(','))
        .then(|| {
            let first = face.name.split(' ').next()?;
            let subtypes = face.type_line.split_once('—')?.1;
            (first != face.name && subtypes.split_whitespace().any(|s| s == first)).then_some(first)
        })
        .flatten();
    let marked = match short {
        Some(first) => lex::mark_short_name(&marked, first),
        None => marked,
    };
    let mut lines: Vec<String> = Vec::new();
    for l in marked.lines() {
        match lines.last_mut() {
            Some(prev) if l.starts_with('•') => {
                prev.push('\n');
                prev.push_str(l);
            }
            _ => lines.push(l.to_string()),
        }
    }
    let mut read: Vec<Line> = lines
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        // A line of nothing but reminder text says nothing to read.
        .filter(|l| !lex::lex(&l.replace('\u{E000}', "~")).is_empty())
        .enumerate()
        .map(|(i, l)| {
            let toks = lex::lex(&l);
            let text = l.replace('\u{E000}', "~");
            let continues = spell && i > 0;
            let parsed =
                parse::parse_line(&toks, face.name, spell, continues).map_err(|at| Broke {
                    read: toks[..at.min(toks.len())]
                        .iter()
                        .map(show)
                        .collect::<Vec<_>>()
                        .join(" "),
                    at: toks.get(at).map(show).unwrap_or_else(|| "<end>".into()),
                });
            Line {
                text,
                parsed,
                continues,
                said: None,
            }
        })
        .collect();
    those_tokens(&mut read);
    read
}

/// "Create two of those tokens" where no tokens were made to be "those":
/// the tokens a row above described (Swarming Goblins' "1—9 | Create a 1/1
/// red Goblin creature token. / 10—19 | Create two of those tokens."), the
/// paragraph before (Gather the Townsfolk), or the branch not taken (Brood
/// Birthing's "Otherwise, create one of those tokens"), or a creation an
/// "instead" replaced (Conqueror's Pledge's "If this spell was kicked, create
/// twelve of those tokens instead", CR 614.1a). It is a `Create` of that
/// description ([those-tokens-described]).
fn those_tokens(lines: &mut [Line]) {
    use ast::{Actor, Effect, Noun, Objects, Player, Quant, Token};
    use walk::{Visitor, Walk};
    struct Those {
        last: Option<Token>,
        // The token this line created, for an "instead" that replaces it.
        here: Option<Token>,
        instead: usize,
        changed: bool,
    }
    impl Visitor for Those {
        fn enter(&mut self, _: &mut Objects) -> bool {
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
        fn effect(&mut self, e: &mut Effect) {
            match e {
                Effect::Create {
                    who: Player::You,
                    token,
                    ..
                } => {
                    self.last = Some(token.clone());
                    self.here = Some(token.clone());
                }
                Effect::Instead { .. } => self.instead += 1,
                Effect::CreateThose {
                    by: Actor::Player(Player::You),
                    n,
                    those,
                } if self.instead > 0
                    && those.quant == Quant::Created
                    && those.filter.kind == Some(Noun::Token)
                    && those.filter.alts.is_empty()
                    && those.filter.posts.is_empty()
                    && those.filter.or.is_none() =>
                {
                    if let Some(token) = self.here.clone() {
                        *e = Effect::Create {
                            who: Player::You,
                            n: n.clone(),
                            token,
                        };
                        self.changed = true;
                    }
                }
                Effect::CreateThose {
                    by: Actor::Player(Player::You),
                    n,
                    those,
                } if those.quant == Quant::Antecedent
                    && those.filter.kind == Some(Noun::Token)
                    && those.filter.alts.is_empty()
                    && those.filter.posts.is_empty()
                    && those.filter.or.is_none() =>
                {
                    if let Some(token) = self.last.clone() {
                        *e = Effect::Create {
                            who: Player::You,
                            n: n.clone(),
                            token,
                        };
                        self.changed = true;
                    }
                }
                _ => {}
            }
        }
        fn effect_done(&mut self, e: &mut Effect) {
            if matches!(e, Effect::Instead { .. }) {
                self.instead -= 1;
            }
        }
    }
    let mut v = Those {
        last: None,
        here: None,
        instead: 0,
        changed: false,
    };
    for l in lines.iter_mut() {
        let Ok(a) = &mut l.parsed else { continue };
        let before = a.clone();
        v.changed = false;
        v.here = None;
        a.walk(&mut v);
        if v.changed {
            l.said = Some(before);
        }
    }
}
impl Line {
    /// Whether the line's tree renders back into the line's own words; `None`
    /// for a line that did not parse.
    pub fn round_trip(&self) -> Option<Result<(), unparse::Diff>> {
        let a = self.parsed.as_ref().ok()?;
        Some(unparse::compare(
            &unparse::paragraph(self.said.as_ref().unwrap_or(a), self.continues),
            &unparse::line_tokens(&self.text),
        ))
    }
}

pub fn show(t: &lex::Tok) -> String {
    use lex::Tok::*;
    match t {
        Word(w) => w.clone(),
        Num(n) => n.to_string(),
        Sym(s) => format!("{{{s}}}"),
        Signed(s) => format!("{s:?}"),
        Pt(_) => "<pt>".into(),
        SelfRef => "~".into(),
        Comma => ",".into(),
        Period => ".".into(),
        Colon => ":".into(),
        Semi => ";".into(),
        Dash => "—".into(),
        Bullet => "•".into(),
        Quote => "\"".into(),
        Possessive => "'s".into(),
        Other(c) => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::*;

    fn read(name: &str, type_line: &str, text: &str) -> Vec<Ability> {
        read_face(
            &Face {
                name,
                type_line,
                text,
            },
            name,
        )
        .into_iter()
        .map(|l| match l.parsed {
            Ok(a) => a,
            Err(b) => panic!("{}: broke at {:?} after {:?}", l.text, b.at, b.read),
        })
        .collect()
    }

    #[test]
    fn its_namesake_reads_as_draw_three_then_proliferate() {
        let a = read(
            "Vivisurgeon's Insight",
            "Sorcery",
            "Draw three cards. Proliferate. (Choose any number of permanents and/or players, then give each another counter of each kind already there.)",
        );
        assert_eq!(
            a,
            vec![Ability::Spell(vec![
                Effect::Draw {
                    who: Player::You,
                    n: Amount::N(3)
                },
                Effect::Action {
                    by: None,
                    name: "proliferate".into(),
                    n: None
                },
            ])]
        );
    }

    #[test]
    fn a_mana_dork_is_a_keyword_free_tap_ability() {
        let a = read("Llanowar Elves", "Creature — Elf Druid", "{T}: Add {G}.");
        assert!(matches!(
            &a[0],
            Ability::Activated { cost, effect, .. }
                if cost == &vec![Cost::Tap]
                && effect == &vec![Effect::AddMana(ManaAdd::Symbols(Mana(vec!["G".into()])))]
        ));
    }

    #[test]
    fn a_copy_s_choice_of_new_targets_belongs_to_each_copy() {
        // CR 707.10c: the choice is made as each copy is put on the stack,
        // so it sits on the Copy inside the loop, with no "may" after it.
        let a = read(
            "Thunderclap Drake",
            "Creature — Drake",
            "{2}{U}, Sacrifice this creature: When you next cast an instant or sorcery spell this turn, copy it for each time you've cast your commander from the command zone this game. You may choose new targets for the copies.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        let [Effect::DelayedNext { then, .. }] = effect.as_slice() else {
            panic!("{effect:?}")
        };
        let [Effect::Repeat { then, .. }] = then.as_slice() else {
            panic!("{then:?}")
        };
        assert!(
            matches!(
                then.as_slice(),
                [Effect::Copy {
                    may_choose_new_targets: true,
                    ..
                }]
            ),
            "{then:?}"
        );
    }

    #[test]
    fn a_new_templated_enters_trigger_reads_this_creature_as_itself() {
        let a = read(
            "Some Bear",
            "Creature — Bear",
            "When this creature enters, you gain 3 life.",
        );
        assert!(matches!(
            &a[0],
            Ability::Triggered { trigger: Trigger::When(e), .. }
                if matches!(&**e, Event::Enters { what, .. } if what.quant == Quant::SelfRef)
        ));
    }

    #[test]
    fn command_towers_mana_is_any_color_constrained_by_the_commander() {
        let a = read(
            "Command Tower",
            "Land",
            "{T}: Add one mana of any color in your commander's color identity.",
        );
        assert!(matches!(
            &a[0],
            Ability::Activated { effect, .. } if effect == &vec![Effect::AddMana(ManaAdd::Of {
                n: Amount::N(1),
                pick: Pick::AnyColor,
                within: Some(Palette::CommanderIdentity),
            })]
        ));
    }

    #[test]
    fn when_you_do_is_a_reflexive_trigger_not_a_condition() {
        let a = read(
            "Some Witch",
            "Creature — Human Warlock",
            "Whenever this creature attacks, you may sacrifice a creature. When you do, draw a card.",
        );
        let Ability::Triggered { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(&effect[1], Effect::WhenYouDo(_)), "{effect:?}");
    }

    #[test]
    fn an_if_after_the_trigger_is_an_intervening_if() {
        let a = read(
            "Some Ring",
            "Artifact",
            "When this artifact enters, if you cast it, draw a card.",
        );
        assert!(matches!(
            &a[0],
            Ability::Triggered { trigger: Trigger::InterveningIf(_, c), .. }
                if matches!(**c, Condition::YouCastIt(_))
        ));
    }

    #[test]
    fn a_trailing_where_binds_x_in_every_clause_of_its_sentence() {
        let a = read(
            "Some Merchant",
            "Creature — Zombie",
            "When this creature enters, each opponent loses X life and you gain X life, where X is your devotion to black.",
        );
        let Ability::Triggered { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        let [Effect::Where { then, x }] = &effect[..] else {
            panic!("{effect:?}")
        };
        assert_eq!(then.len(), 2);
        assert_eq!(
            x,
            &Amount::Devotion {
                who: Player::You,
                to: Palette::Listed(vec![Color::Black])
            }
        );
    }

    #[test]
    fn the_verb_after_may_keeps_the_subject_before_it() {
        let a = read(
            "Some Denial",
            "Instant",
            "Counter target spell. Its controller may draw up to two cards.",
        );
        let Ability::Spell(effect) = &a[0] else {
            panic!("{a:?}")
        };
        let Effect::May { who, then } = &effect[1] else {
            panic!("{effect:?}")
        };
        assert!(matches!(who, Player::ControllerOf(_)));
        assert!(matches!(
            &then[0],
            Effect::Draw {
                who: Player::ControllerOf(_),
                n: Amount::UpTo(_)
            }
        ));
    }

    #[test]
    fn a_counter_replacement_reads_its_event_and_its_bigger_count() {
        let a = read(
            "Some Scales",
            "Enchantment",
            "If one or more +1/+1 counters would be put on a creature you control, that many plus one +1/+1 counters are put on it instead.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        let Static::Replacement { event, instead } = &s[0] else {
            panic!("{s:?}")
        };
        assert!(matches!(**event, Event::CountersPut { by: None, .. }));
        // Said in the passive: put by whoever was putting them, not by "you".
        assert!(matches!(
            &instead[0],
            Effect::CountersArePut { n: Amount::Plus(n, 1), .. } if **n == Amount::ThatMany
        ));
    }

    #[test]
    fn a_tax_on_attacking_you_names_its_payer_and_its_count() {
        let a = read(
            "Some Prison",
            "Enchantment",
            "Creatures can't attack you unless their controller pays {2} for each creature they control that's attacking you.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &s[0],
            Static::Restrict {
                rule: Restriction::UnlessPays { rule, per: Some(_), .. },
                ..
            } if **rule == Restriction::CantAttackPlayer(Player::You)
        ));
    }

    #[test]
    fn mana_for_each_color_binds_that_color_per_color() {
        let a = read(
            "Some Tender",
            "Creature — Elf",
            "{T}: For each color among permanents you control, add one mana of that color.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        let [Effect::ForEachColor {
            among: Palette::ColorsOf(_),
            then,
        }] = &effect[..]
        else {
            panic!("{effect:?}")
        };
        assert_eq!(
            then,
            &vec![Effect::AddMana(ManaAdd::Of {
                n: Amount::N(1),
                pick: Pick::Stated,
                within: Some(Palette::That),
            })]
        );
    }

    #[test]
    fn a_clone_keeps_what_its_except_adds() {
        let a = read(
            "Some Metamorph",
            "Artifact Creature — Phyrexian Shapeshifter",
            "You may have this creature enter as a copy of any artifact or creature on the battlefield, except it's an artifact in addition to its other types.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &s[0],
            Static::EntersAsCopy { may: true, except, .. }
                if except == &vec![Exception::AlsoIs(vec![Adj::Type("artifact".into())])]
        ));
    }

    #[test]
    fn exiling_a_countered_spell_replaces_where_it_would_go() {
        let a = read(
            "Some Negation",
            "Instant",
            "Counter target spell. If that spell is countered this way, exile it instead of putting it into its owner's graveyard.",
        );
        let Ability::Spell(effect) = &a[0] else {
            panic!("{a:?}")
        };
        let Effect::If { cond, then } = &effect[1] else {
            panic!("{effect:?}")
        };
        assert!(matches!(cond, Condition::ThisWay(e) if matches!(**e, Event::Countered(_))));
        assert!(matches!(
            &then[0],
            Effect::InsteadOf { then, of }
                if matches!(**then, Effect::Exile(_))
                && matches!(**of, Effect::Move { to: Zone::OwnersGraveyard, .. })
        ));
    }

    #[test]
    fn on_top_and_on_the_bottom_say_which_end_of_the_library() {
        let a = read(
            "Some Oracle",
            "Sorcery",
            "Look at the top three cards of your library. Put up to one of them on top of your library and the rest on the bottom of your library in a random order.",
        );
        let Ability::Spell(effect) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &effect[1],
            Effect::Move { what, to: Zone::TopOfLibrary(Player::You), .. }
                if what.quant == Quant::UpTo(Amount::N(1))
        ));
        assert!(matches!(
            &effect[2],
            Effect::Move {
                to: Zone::BottomOfLibrary(Player::You),
                order: Some(Order::Random),
                ..
            }
        ));
    }

    #[test]
    fn a_land_that_becomes_a_creature_says_it_is_still_a_land() {
        let a = read(
            "Some Touch",
            "Sorcery",
            "Target land becomes a 2/2 creature that's still a land.",
        );
        assert!(matches!(
            &a[0],
            Ability::Spell(e) if matches!(&e[0], Effect::Becomes { keeps: Keeps::Listed(k), .. }
                if k == &vec![Adj::Type("land".into())])
        ));
    }

    #[test]
    fn token_is_a_kind_of_object_even_last_in_a_list_of_types() {
        let a = read(
            "Some Kitty",
            "Creature — Cat",
            "{2}, Sacrifice another creature or token: Draw a card.",
        );
        let Ability::Activated { cost, .. } = &a[0] else {
            panic!("{a:?}")
        };
        let Cost::Sacrifice(o) = &cost[1] else {
            panic!("{cost:?}")
        };
        assert_eq!(o.filter.alts, vec![vec![Adj::Type("creature".into())]]);
        assert_eq!(
            o.filter.or.as_ref().and_then(|f| f.kind.clone()),
            Some(Noun::Token)
        );
    }

    #[test]
    fn a_leading_for_each_repeats_its_clauses_per_object() {
        let a = read(
            "Some Cathar",
            "Creature — Human Knight",
            "Whenever you attack, for each opponent, create a 1/1 white Human creature token that's tapped and attacking that player or a planeswalker they control.",
        );
        let Ability::Triggered { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        let [Effect::ForEach { then, .. }] = &effect[..] else {
            panic!("{effect:?}")
        };
        assert!(matches!(
            &then[0],
            Effect::Create { token, .. } if token.tapped && matches!(token.attacking, Some(Recipient::Or(_)))
        ));
    }

    #[test]
    fn one_subject_takes_a_list_of_predicates() {
        let a = read(
            "Some Regalia",
            "Artifact — Equipment",
            "Equipped creature has ward {2}, is an Assassin in addition to its other types, and can't be blocked.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(&s[0], Static::Modify { .. }));
        assert!(matches!(&s[1], Static::AlsoIs { .. }));
        assert!(matches!(
            &s[2],
            Static::Restrict {
                rule: Restriction::CantBeBlocked,
                ..
            }
        ));
    }

    #[test]
    fn a_cost_reduction_can_hang_on_a_condition() {
        let a = read(
            "Some Bend",
            "Instant",
            "This spell costs {3} less to cast if you control a creature with power 4 or greater.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &s[0],
            Static::If { cond: Condition::YouControl(_), then } if matches!(then[0], Static::CostChange { less: true, .. })
        ));
    }

    #[test]
    fn a_spree_mode_reads_its_added_cost() {
        let a = read(
            "Some Favor",
            "Instant",
            "+ {1} — Change the target of target spell or ability with a single target.",
        );
        assert!(matches!(
            &a[0],
            Ability::Spree { cost, effect } if cost == &Mana(vec!["1".into()])
                && matches!(effect[0], Effect::ChangeTarget(_))
        ));
    }

    #[test]
    fn a_bonus_with_no_duration_needs_one_said_before_it() {
        let a = read(
            "Some Stampede",
            "Sorcery",
            "Until end of turn, creatures you control gain trample and get +X/+X, where X is the greatest power among creatures you control.",
        );
        assert!(matches!(&a[0], Ability::Spell(e) if matches!(e[0], Effect::During { .. })));
        // Without the leading duration, the first bonus would last forever.
        let lines = read_face(
            &Face {
                name: "Some Veteran",
                type_line: "Creature — Human Soldier",
                text: "Whenever this creature attacks, attacking red creatures get +2/+0 and attacking white creatures get +0/+2 until end of turn.",
            },
            "Some Veteran",
        );
        if let Ok(a) = &lines[0].parsed {
            assert!(!format!("{a:?}").contains("dur: Permanent"), "{a:?}");
        }
    }

    #[test]
    fn an_aura_that_makes_a_new_creature_says_what_it_is_has_and_loses() {
        let a = read(
            "Some Mutation",
            "Enchantment — Aura",
            "Enchanted creature is an Insect artifact creature with base power and toughness 0/1 and has indestructible, and it loses all other abilities, card types, and creature types.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(&s[0], Static::Is { types: TypeSet::Listed(t), .. } if t.len() == 3));
        assert!(matches!(&s[1], Static::BasePt { .. }));
        assert!(matches!(&s[2], Static::Modify { has, .. } if has.len() == 1));
        assert_eq!(
            s[3],
            Static::Loses {
                what: match &s[0] {
                    Static::Is { what, .. } => what.clone(),
                    _ => unreachable!(),
                },
                lost: [
                    Characteristic::Abilities,
                    Characteristic::CardTypes,
                    Characteristic::CreatureTypes
                ]
                .map(|of| Lost::All { other: true, of })
                .to_vec(),
            }
        );
    }

    #[test]
    fn a_damage_doubler_replaces_the_amount_and_keeps_the_recipient() {
        let a = read(
            "Some Tyrant",
            "Creature — Dragon",
            "If a source you control would deal damage to an opponent or a permanent an opponent controls, it deals double that damage instead.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        let Static::Replacement { event, instead } = &s[0] else {
            panic!("{s:?}")
        };
        assert!(matches!(**event, Event::DealsDamageTo { .. }));
        assert!(matches!(
            &instead[0],
            Effect::ThatDamage { n: Amount::Twice(n), to: None, .. } if **n == Amount::ThatDamage
        ));
    }

    #[test]
    fn digging_until_a_card_names_the_verb_and_the_card_it_stops_at() {
        let a = read(
            "Some Conqueror",
            "Creature — Elder Giant",
            "When this creature enters, each player exiles cards from the top of their library until they exile a nonland card.",
        );
        let Ability::Triggered { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &effect[0],
            Effect::UntilFound { who: Player::Each, verb: DigVerb::Exile, zone: Zone::TopOfLibrary(Player::That), until }
                if until.filter.alts == vec![vec![Adj::Non("land".into())]]
        ));
    }

    #[test]
    fn a_repeated_process_is_every_sentence_after_it() {
        let a = read(
            "Some Torment",
            "Sorcery",
            "Repeat the following process X times. Each opponent loses 3 life unless that player sacrifices a nonland permanent of their choice or discards a card.",
        );
        let Ability::Spell(effect) = &a[0] else {
            panic!("{a:?}")
        };
        let [Effect::Repeat {
            times: Amount::X,
            then,
        }] = &effect[..]
        else {
            panic!("{effect:?}")
        };
        assert!(matches!(
            &then[0],
            Effect::Unless { who: Player::That, cost, .. } if matches!(&cost[..], [Cost::OneOf(c)] if c.len() == 2)
        ));
    }

    #[test]
    fn one_player_takes_a_list_of_trigger_verbs_that_ends_in_or() {
        let a = read(
            "Some Pairs",
            "Enchantment",
            "Whenever an opponent attacks you with two or more creatures, draws their second card each turn, or casts their second spell each turn, you draw a card.",
        );
        assert!(matches!(
            &a[0],
            Ability::Triggered { trigger: Trigger::When(e), .. }
                if matches!(&**e, Event::Or(v) if v.len() == 3
                    && matches!(&v[0], Event::AttacksWith { at: Some(Player::You), .. }))
        ));
        // A comma with no "or" after it ends the trigger, not the list.
        let a = read(
            "Some Necropotence",
            "Enchantment",
            "Whenever you discard a card, exile that card from your graveyard.",
        );
        assert!(matches!(
            &a[0],
            Ability::Triggered { trigger: Trigger::When(e), .. } if matches!(**e, Event::Discards { .. })
        ));
    }

    #[test]
    fn a_zone_after_a_compared_amount_belongs_to_the_object_compared() {
        let a = read(
            "Some Vial",
            "Artifact",
            "{T}: You may put a creature card with mana value equal to the number of charge counters on this artifact from your hand onto the battlefield.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        let Effect::May { then, .. } = &effect[0] else {
            panic!("{effect:?}")
        };
        assert!(matches!(
            &then[0],
            Effect::Move { what, .. } if what.filter.posts.last() == Some(&Post::In(Zone::Hand(Player::You)))
        ));
    }

    #[test]
    fn a_damage_clause_after_and_keeps_the_subject_before_it() {
        let a = read(
            "Some Satyr",
            "Creature — Satyr",
            "{1}{R}: This creature gets +1/+0 until end of turn and deals 1 damage to you.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &effect[1],
            Effect::Damage { source: Some(o), .. } if o.quant == Quant::SelfRef
        ));
    }

    #[test]
    fn an_extra_combat_says_which_phase_it_follows() {
        let a = read(
            "Some Assault",
            "Enchantment",
            "{3}{R}{R}: Untap all creatures you control. After this main phase, there is an additional combat phase followed by an additional main phase. Activate only as a sorcery.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert_eq!(
            effect[1],
            Effect::AdditionalPhases {
                after: Some(PhaseKind::Main),
                phases: vec![PhaseKind::Combat, PhaseKind::Main],
            }
        );
    }

    #[test]
    fn each_other_keeps_its_other() {
        let a = read(
            "Some Mimic",
            "Artifact Creature — Shapeshifter",
            "Each other creature you control of the chosen type enters with an additional +1/+1 counter on it.",
        );
        assert!(matches!(
            &a[0],
            Ability::Static(s) if matches!(&s[0], Static::EntersWithCounters { what, n: Amount::Additional(_), .. }
                if what.quant == Quant::Each && what.filter.alts[0][0] == Adj::State(State::Other))
        ));
    }

    #[test]
    fn counters_a_returned_permanent_enters_with_wrap_the_move() {
        let a = read(
            "Some Persist",
            "Sorcery",
            "Return target nonlegendary creature card from your graveyard to the battlefield with a -1/-1 counter on it.",
        );
        // Given as it enters (CR 122.6), so no `PutCounters` after the move.
        assert!(matches!(
            &a[0],
            Ability::Spell(e) if matches!(&e[0], Effect::EntersWith { effect, n: Amount::N(1), .. }
                if matches!(**effect, Effect::Move { to: Zone::Battlefield, .. }))
        ));
    }

    #[test]
    fn after_may_an_objects_verb_is_said_with_have() {
        let a = read(
            "Some Pinnacle",
            "Land",
            "Whenever a Mountain you control enters, if you control at least five other Mountains, you may have this land deal 3 damage to any target.",
        );
        let Ability::Triggered {
            trigger: Trigger::InterveningIf(_, c),
            effect,
            ..
        } = &a[0]
        else {
            panic!("{a:?}")
        };
        assert!(
            matches!(&**c, Condition::YouControl(o) if o.quant == Quant::AtLeast(Amount::N(5)))
        );
        // The land deals the damage; "you" only chooses whether it does.
        assert!(matches!(
            &effect[0],
            Effect::May { who: Player::You, then } if matches!(&then[0], Effect::Damage { source: Some(s), .. } if s.quant == Quant::SelfRef)
        ));
    }

    #[test]
    fn its_graveyard_is_its_owners_and_its_controllers_is_another_zone() {
        let zone_of = |text: &str| {
            let a = read("Some Loch", "Instant", text);
            let Ability::Spell(e) = &a[0] else {
                panic!("{a:?}")
            };
            let Effect::Counter { what, .. } = &e[0] else {
                panic!("{e:?}")
            };
            let [Post::With(Compare {
                value: Amount::CountOf(cards),
                ..
            })] = &what.filter.posts[..]
            else {
                panic!("{what:?}")
            };
            cards.filter.posts.clone()
        };
        let it = Box::new(Objects {
            quant: Quant::Antecedent,
            filter: Filter::default(),
        });
        // CR 400.3: an object's graveyard is only ever its owner's.
        assert_eq!(
            zone_of("Counter target spell with mana value less than or equal to the number of cards in its graveyard."),
            vec![Post::In(Zone::Graveyard(Player::OwnerOf(it.clone())))]
        );
        // A spell or a stolen permanent may be controlled by a player who
        // does not own it, so its controller's graveyard is another zone.
        assert_eq!(
            zone_of("Counter target spell with mana value less than or equal to the number of cards in its controller's graveyard."),
            vec![Post::In(Zone::Graveyard(Player::ControllerOf(it)))]
        );
    }

    #[test]
    fn a_sentence_about_the_reflexive_targets_it_belongs_to_that_trigger() {
        let a = read(
            "Some Guide",
            "Creature — Human Cleric",
            "Whenever you attack, you may pay {E}{E}{E}. When you do, put two +1/+1 counters and a flying counter on target attacking creature. It becomes an Angel in addition to its other types.",
        );
        let Ability::Triggered { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        // "It" is the creature the reflexive trigger targets, so it becomes
        // an Angel when that trigger resolves, not when this one does.
        assert_eq!(effect.len(), 2, "{effect:?}");
        assert!(matches!(&effect[1], Effect::WhenYouDo(then)
            if matches!(&then[..], [Effect::PutCountersEach { counters, .. }, Effect::Becomes { .. }] if counters.len() == 2)));
    }

    #[test]
    fn an_instants_lines_that_are_not_its_effect_get_their_own_kind() {
        // CR 113.6g: it works on the stack, so it is a static, as on a
        // creature, not something the spell does as it resolves.
        let a = read("Some Decay", "Instant", "This spell can't be countered.");
        assert!(matches!(&a[0], Ability::Static(s)
            if matches!(&s[..], [Static::Restrict { rule: Restriction::CantBeCountered, .. }])));
        let a = read(
            "Some Firecraft",
            "Sorcery",
            "Spell mastery — If there are two or more instant and/or sorcery cards in your graveyard, this spell can't be countered.",
        );
        assert!(matches!(&a[0], Ability::Labelled { ability, .. }
            if matches!(&**ability, Ability::Static(s) if matches!(&s[..], [Static::If { .. }]))));
        // A Pact's upkeep trigger is created as it resolves (CR 603.7a),
        // and losing the game is part of it.
        let a = read(
            "Some Pact",
            "Instant",
            "Counter target spell.\nAt the beginning of your next upkeep, pay {3}{U}{U}. If you don't, you lose the game.",
        );
        assert!(matches!(&a[1], Ability::Spell(e)
            if matches!(&e[..], [Effect::Delayed { step: Step::Upkeep(Whose::YourNext), then }]
                if matches!(&then[..], [Effect::Pay { .. }, Effect::IfYouDont(_)]))));
        // A cost said in words after a keyword's dash is the keyword's.
        for (text, cost) in [
            (
                "Buyback—Pay 3 life, Discard a card at random.",
                "DiscardAtRandom",
            ),
            ("Replicate—Pay {E}{E}{E}.", "Energy(3)"),
            ("Kicker—Forage.", "ActionAlone(\"forage\")"),
            (
                "Splice onto Arcane—An opponent gains 5 life.",
                "PlayerGainsLife",
            ),
        ] {
            let a = read("Some Bolt", "Sorcery", text);
            assert!(
                matches!(&a[0], Ability::Keywords(_)) && format!("{a:?}").contains(cost),
                "{text}: {a:?}"
            );
        }
    }

    #[test]
    fn the_sentences_after_a_delayed_trigger_are_its() {
        let a = read(
            "Some Encampment",
            "Land",
            "{2}{W}, {T}: Whenever you attack this turn, create two 1/1 red Warrior creature tokens that are tapped and attacking. Sacrifice them at the beginning of the next end step.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        // The tokens exist only once it has triggered, so each attack makes
        // its own and sacrifices its own.
        let [Effect::During { then, .. }] = &effect[..] else {
            panic!("{effect:?}")
        };
        assert!(matches!(&then[..], [Effect::DelayedWhenever { then, .. }]
            if matches!(&then[..], [Effect::Create { .. }, Effect::Delayed { .. }])));
        // A sentence naming a new target is the spell's, chosen as it is
        // cast; a second "When …" is a trigger of its own.
        let a = read(
            "Some Feeding",
            "Sorcery",
            "Choose target creature an opponent controls. When that creature dies this turn, you gain 3 life. Target creature you control fights that creature.",
        );
        let Ability::Spell(effect) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(
            matches!(
                &effect[..],
                [
                    Effect::Choose(_),
                    Effect::During { .. },
                    Effect::Fight { .. }
                ]
            ),
            "{effect:?}"
        );
        let a = read(
            "Some Mount",
            "Creature — Horse",
            "{T}: Target creature you control gets +1/+1 until end of turn. When this creature leaves the battlefield this turn, sacrifice that creature. When the creature leaves the battlefield this turn, sacrifice this creature.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert_eq!(effect.len(), 3, "{effect:?}");
    }

    #[test]
    fn a_sentence_about_what_an_option_brought_in_is_the_options() {
        let a = read(
            "Some Breach",
            "Instant — Arcane",
            "You may put a creature card from your hand onto the battlefield. That creature gains haste. Sacrifice that creature at the beginning of the next end step.",
        );
        // No creature, no haste and no sacrifice: both are the option's.
        assert!(matches!(&a[0], Ability::Spell(e)
            if matches!(&e[..], [Effect::May { then, .. }]
                if matches!(&then[..], [Effect::Move { .. }, Effect::Modify { .. }, Effect::Delayed { .. }]))));
        let a = read(
            "Some Ray",
            "Sorcery",
            "Tap all creatures target player controls. You may pay any amount of {E}. If you do, choose up to that many creatures tapped this way. Put a stun counter on each of them.",
        );
        let Ability::Spell(e) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(
            matches!(&e[2], Effect::IfYouDo(then)
            if matches!(&then[..], [Effect::Choose(_), Effect::PutCounters { .. }])),
            "{e:?}"
        );
        // "The rest" were looked at either way, so they stay outside.
        let a = read(
            "Some Symbiosis",
            "Sorcery",
            "Look at the top seven cards of your library. You may put a creature card from among them onto the battlefield. If that card has mana value 3 or less, it enters with three additional +1/+1 counters on it. Put the rest on the bottom of your library in a random order.",
        );
        assert!(matches!(&a[0], Ability::Spell(e)
            if matches!(&e[..], [Effect::LookAtTop { .. }, Effect::May { then, .. }, Effect::Move { .. }] if then.len() == 2)));
    }

    #[test]
    fn those_players_after_a_loop_are_the_loops_controllers() {
        let a = read(
            "Some Creativity",
            "Sorcery",
            "Destroy X target artifacts and/or creatures. For each permanent destroyed this way, its controller reveals cards from the top of their library until an artifact or creature card is revealed and exiles that card. Those players put the exiled cards onto the battlefield, then shuffle.",
        );
        let Ability::Spell(e) = &a[0] else {
            panic!("{a:?}")
        };
        let Effect::ForEach { each, .. } = &e[1] else {
            panic!("{e:?}")
        };
        let those = Player::Those(Box::new(Player::ControllerOf(Box::new(each.clone()))));
        assert!(matches!(&e[2], Effect::Performed { by, .. } if *by == those));
        assert_eq!(e[3], Effect::Shuffle(those));
    }

    #[test]
    fn a_pronoun_for_the_card_is_the_card() {
        let me = || Objects {
            quant: Quant::SelfRef,
            filter: Filter::default(),
        };
        // A gendered pronoun names only a character card itself.
        let a = read(
            "Ajani, Nacatl Avenger",
            "Legendary Planeswalker — Ajani",
            "0: Create a 2/1 white Cat Warrior creature token. When you do, if you control a red permanent other than Ajani, he deals damage equal to the number of creatures you control to any target.",
        );
        assert!(
            format!("{a:?}").contains(&format!("Damage {{ source: Some({:?})", me())),
            "{a:?}"
        );
        // The sentence before names only the card, so "it" is the card.
        let a = read(
            "Tyvar, the Pummeler",
            "Legendary Creature — Elf Warrior",
            "Tap another untapped creature you control: Tyvar gains indestructible until end of turn. Tap it.",
        );
        assert!(
            matches!(&a[0], Ability::Activated { effect, .. } if effect[1] == Effect::Tap(me()))
        );
        // Damage to a bare "them" is a player's.
        let a = read(
            "Some Vortex",
            "Enchantment",
            "At the beginning of each player's upkeep, this enchantment deals 1 damage to them.",
        );
        assert!(matches!(&a[0], Ability::Triggered { effect, .. }
            if matches!(&effect[0], Effect::Damage { to, .. } if to == &vec![Recipient::Player(Player::That)])));
        // "The exiled card" no earlier verb exiled is the linked ability's
        // (CR 607.2a), and so is "exiled with it".
        let linked = Post::ExiledWith(Box::new(me()));
        for text in [
            "{2}, {T}: You may copy the exiled card. If you do, you may cast the copy without paying its mana cost.",
            "This creature enters with a +1/+1 counter on it for each instant and sorcery card exiled with it.",
        ] {
            let a = read("Some Scepter", "Artifact", text);
            assert!(format!("{a:?}").contains(&format!("{linked:?}")), "{text}: {a:?}");
        }
        // "from among the nonland permanents they control" is all of them.
        let a = read(
            "Some Gearhulk",
            "Artifact Creature — Construct",
            "When this creature enters, each player chooses an artifact, a creature, an enchantment, and a planeswalker from among the nonland permanents they control, then sacrifices the rest.",
        );
        assert!(
            format!("{a:?}").contains("among: Objects { quant: All"),
            "{a:?}"
        );
    }

    #[test]
    fn other_and_lesser_on_a_trigger_are_said_of_its_object() {
        let triggering = Objects {
            quant: Quant::Triggering,
            filter: Filter::default(),
        };
        let a = read(
            "Some Pinnacle",
            "Land",
            "Whenever a Mountain you control enters, if you control at least five other Mountains, you may have this land deal 3 damage to any target.",
        );
        let Ability::Triggered {
            trigger: Trigger::InterveningIf(_, c),
            ..
        } = &a[0]
        else {
            panic!("{a:?}")
        };
        // Other than the Mountain that entered: "five or more Mountains
        // other than that new one" (Valakut's ruling).
        assert!(matches!(&**c, Condition::YouControl(o)
            if o.filter.alts == vec![vec![Adj::OtherThan(Box::new(triggering.clone())), Adj::Subtype("mountain".into())]]));
        let a = read(
            "Some Hierophant",
            "Creature — Kor Cleric",
            "Whenever this creature or another Cleric you control dies, return target Cleric card with lesser mana value from your graveyard to the battlefield.",
        );
        assert!(
            format!("{a:?}").contains(&format!("ManaValueOf({triggering:?})")),
            "{a:?}"
        );
        // With no event object, "lesser" is than what the effect named.
        let a = read(
            "Some Goliath",
            "Creature — Dragon",
            "At the beginning of your end step, you may sacrifice another creature. If you do, reveal cards from the top of your library until you reveal a nonlegendary creature card with lesser mana value, put it onto the battlefield, then put the rest on the bottom of your library in a random order.",
        );
        assert!(!format!("{a:?}").contains("Triggering"), "{a:?}");
    }

    #[test]
    fn what_mana_does_to_the_spell_it_pays_for_happens_as_it_is_spent() {
        let a = read(
            "Some Arena",
            "Land",
            "{R}, {T}, Exert this land: Add {R}{R}. If any of that mana is spent on a creature spell, it gains haste until end of turn.",
        );
        // CR 106.6: as the mana is spent, not as the ability resolves, when
        // none of it has been spent yet.
        assert!(matches!(&a[0], Ability::Activated { effect, .. }
            if matches!(&effect[1], Effect::OnSpend { of: SpentOf::Any, then, .. } if matches!(then[0], Effect::Modify { .. }))));
        let a = read(
            "Some Palace",
            "Land",
            "{1}, {T}: Add one mana of any color in your commander's color identity. If you spend this mana to cast your commander, it enters with a number of additional +1/+1 counters on it equal to the number of times it's been cast from the command zone this game.",
        );
        assert!(matches!(&a[0], Ability::Activated { effect, .. }
            if matches!(&effect[1], Effect::OnSpend { of: SpentOf::All, .. })));
    }

    #[test]
    fn a_qualifier_after_a_list_said_with_one_article_holds_of_each_kind() {
        let posts = |text: &str| {
            let a = read("Some Stupor", "Instant", text);
            let Ability::Spell(e) = &a[0] else {
                panic!("{a:?}")
            };
            let Effect::Move { what, .. } = &e[0] else {
                panic!("{e:?}")
            };
            (
                what.filter.posts.clone(),
                what.filter.or.as_ref().unwrap().posts.clone(),
            )
        };
        // A spell has a controller (CR 601.2a), so an opponent's spell only.
        let (spell, permanent) = posts(
            "Return target spell or nonland permanent an opponent controls to its owner's hand.",
        );
        assert_eq!(spell, vec![Post::Controller(Player::AnOpponent)]);
        assert_eq!(spell, permanent);
        let (spell, permanent) = posts(
            "Return target spell or permanent with mana value 1 or greater to its owner's hand.",
        );
        assert!(!spell.is_empty() && spell == permanent);
        // A card in a graveyard has no controller and is in no other zone.
        let (spell, _) = posts(
            "Return target spell, nonland permanent, or card in a graveyard to its owner's hand.",
        );
        assert!(spell.is_empty());
        // With the article said again, what trails is the last kind's.
        let a = read(
            "Some Teachings",
            "Instant",
            "Search your library for an instant card or a card with flash, reveal it, put it into your hand, then shuffle.",
        );
        assert!(
            format!("{a:?}").contains("kind: Some(Card), posts: [], or: Some"),
            "{a:?}"
        );
    }

    #[test]
    fn the_settled_conventions_hold() {
        // CR 603.12 + 603.4: a reflexive trigger's "if" is an intervening if.
        let a = read(
            "Some Sigil",
            "Enchantment",
            "At the beginning of combat on your turn, mill a card. When you do, if there are four or more creature cards in your graveyard, put a +1/+1 counter on target creature you control.",
        );
        assert!(matches!(&a[0], Ability::Triggered { effect, .. }
            if matches!(&effect[1], Effect::WhenYouDoIf { then, .. } if matches!(then[0], Effect::PutCounters { .. }))));
        // "This process" is the whole ability: the land as well as the draw.
        let a = read(
            "Some Colossus",
            "Creature — Plant Beast",
            "When this creature enters, you may put a land card from your hand onto the battlefield tapped. If you do, draw a card and repeat this process.",
        );
        assert!(matches!(&a[0], Ability::Triggered { effect, .. }
            if matches!(&effect[..], [Effect::Process(v)] if matches!(&v[..], [Effect::May { .. }, Effect::IfYouDo(_)]))));
        // One delayed trigger per opponent (Chancellor of the Annex's ruling).
        let a = read(
            "Some Chancellor",
            "Creature — Phyrexian Advisor",
            "You may reveal this card from your opening hand. If you do, when each opponent casts their first spell of the game, counter that spell unless that player pays {1}.",
        );
        assert!(format!("{a:?}").contains("ForEach { each: Objects { quant: Plural, filter: Filter { alts: [], kind: Some(Opponent)"), "{a:?}");
        assert!(
            format!("{a:?}").contains("DelayedWhen { event: Casts { who: That"),
            "{a:?}"
        );
        // A land subtype replaces land types only (CR 205.1a, 305.7).
        let a = read("Some Moon", "Enchantment", "Nonbasic lands are Mountains.");
        assert!(matches!(&a[0], Ability::Static(s)
            if matches!(&s[0], Static::Is { types: TypeSet::Subtypes(_), .. })));
        let a = read(
            "Some Shaper",
            "Creature — Merfolk Wizard",
            "When this creature enters, target land becomes an Island.",
        );
        assert!(
            format!("{a:?}").contains("keeps: AllButSubtypesOfSet"),
            "{a:?}"
        );
        // "becomes an artifact creature" keeps its other types (CR 205.1b).
        let a = read(
            "Some Karn",
            "Legendary Planeswalker — Karn",
            "+1: Until your next turn, up to one target noncreature artifact becomes an artifact creature with power and toughness each equal to its mana value.",
        );
        assert!(format!("{a:?}").contains("keeps: OtherTypes"), "{a:?}");
    }

    #[test]
    fn a_keyword_after_a_list_of_kinds_is_the_last_kinds() {
        let what = |text: &str| {
            let a = read("Some Fall", "Instant", text);
            let Ability::Spell(e) = &a[0] else {
                panic!("{a:?}")
            };
            let Effect::Destroy { what, .. } = &e[0] else {
                panic!("{e:?}")
            };
            what.filter.clone()
        };
        let f = what("Destroy target artifact, battle, enchantment, or creature with flying.");
        assert_eq!(f.alts.len(), 3);
        assert!(f.posts.is_empty(), "{f:?}");
        let last = f.or.unwrap();
        assert_eq!(last.alts, vec![vec![Adj::Type("creature".into())]]);
        assert_eq!(last.posts, vec![Post::WithKeyword("flying".into())]);
        // A list of colors shares one kind, and the keyword holds of it.
        let f = what("Destroy target blue or black creature with flying.");
        assert!(f.or.is_none() && f.posts == vec![Post::WithKeyword("flying".into())]);
    }

    #[test]
    fn this_creature_or_a_commander_is_two_objects() {
        let a = read(
            "Some Enforcer",
            "Artifact Creature — Soldier",
            "Whenever this creature or a commander you control attacks alone, draw a card.",
        );
        let Ability::Triggered {
            trigger: Trigger::When(e),
            ..
        } = &a[0]
        else {
            panic!("{a:?}")
        };
        let Event::AttacksAlone(o) = &**e else {
            panic!("{e:?}")
        };
        assert!(matches!(&o.quant, Quant::SelfOrA(me) if me.quant == Quant::SelfRef));
        assert_eq!(o.filter.kind, Some(Noun::Commander));
        let a = read(
            "Some Advocate",
            "Creature — Elf",
            "This creature and land creatures you control get +2/+2.",
        );
        assert!(format!("{a:?}").contains("quant: SelfAnd("), "{a:?}");
    }

    #[test]
    fn a_sentence_about_what_an_if_brought_in_is_the_ifs() {
        let a = read(
            "Some Triumph",
            "Sorcery",
            "Exile target creature or planeswalker. If that permanent's mana value was 3 or less, return it to the battlefield tapped under your control. Exile it at the beginning of the next end step.",
        );
        let Ability::Spell(e) = &a[0] else {
            panic!("{a:?}")
        };
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(matches!(&e[1], Effect::If { then, .. } if then.len() == 2));
        // "Otherwise, put that card onto the battlefield. That creature
        // gains haste.": the creature is the other branch's.
        let a = read(
            "Some Raid",
            "Enchantment",
            "{2}{R}: Reveal the top card of your library. If it isn't a creature card, put it into your graveyard. Otherwise, put that card onto the battlefield. That creature gains haste. Sacrifice it at the beginning of the next end step.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(effect.last(), Some(Effect::Otherwise(then)) if then.len() == 3));
        // Two cases of one revealed card stay side by side.
        let a = read(
            "Some Satchel",
            "Artifact",
            "{2}, {T}: Reveal the top card of your library. If it's a creature card, create a 1/1 green Saproling creature token. If it's a land card, put that card onto the battlefield under your control.",
        );
        let Ability::Activated { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert_eq!(effect.len(), 3, "{effect:?}");
    }

    #[test]
    fn a_reference_back_to_the_triggers_object_is_triggering_or_the_card() {
        let quants = |name: &str, ty: &str, text: &str| -> Vec<String> {
            let a = format!("{:?}", read(name, ty, text));
            a.match_indices("quant: ")
                .map(|(i, _)| {
                    a[i + 7..]
                        .split([' ', ',', '('])
                        .next()
                        .unwrap()
                        .to_string()
                })
                .filter(|q| matches!(q.as_str(), "Antecedent" | "Triggering" | "SelfRef"))
                .collect()
        };
        // The event's object, in the intervening "if" and in the effect.
        assert_eq!(
            quants(
                "Some Magistrate",
                "Creature — Human",
                "Whenever another nontoken creature you control dies, if its mana value was 1 or greater, return that card to the battlefield under its owner's control.",
            ),
            ["Triggering", "Triggering", "Triggering"]
        );
        // The card named in a finished clause is what a bare "it" is.
        assert_eq!(
            quants(
                "Some Sahagin",
                "Creature — Merfolk",
                "Whenever you cast a noncreature spell, if at least four mana was spent to cast it, put a +1/+1 counter on this creature and it can't be blocked this turn.",
            ),
            ["Triggering", "SelfRef", "SelfRef"]
        );
        // In its own clause the card would be "itself".
        assert_eq!(
            quants(
                "Some Minefield",
                "Enchantment",
                "Whenever a creature attacks or blocks, this enchantment deals 2 damage to it.",
            ),
            ["SelfRef", "Triggering"]
        );
        // A revealed card is another object.
        assert_eq!(
            quants(
                "Some Predators",
                "Enchantment",
                "Whenever an opponent casts a spell, reveal the top card of your library. If it's a creature card, put it onto the battlefield.",
            ),
            ["Antecedent", "Antecedent"]
        );
        // "a spell that targets this creature" names two objects.
        assert_eq!(
            quants(
                "Some Trailblazer",
                "Creature — Cat",
                "Whenever you cast a spell that targets this creature, put a +1/+1 counter on it.",
            ),
            ["SelfRef", "Antecedent"]
        );
    }

    #[test]
    fn an_activated_abilitys_it_after_the_card_is_the_card() {
        let a = format!(
            "{:?}",
            read(
                "Some Figure",
                "Creature — Kithkin",
                "{R/W}{R/W}{R/W}: If this creature is a Spirit, it becomes a Kithkin Spirit Warrior with base power and toughness 4/4.",
            )
        );
        assert!(!a.contains("Antecedent"), "{a}");
        // "them" after the card and another object is both.
        let a = format!(
            "{:?}",
            read(
                "Some Battlements",
                "Land",
                "{3}{R}{R}, {T}: If you both own and control this land and a creature named Hanweir Garrison, exile them.",
            )
        );
        assert!(a.contains("Antecedent"), "{a}");
    }

    #[test]
    fn a_static_about_the_card_says_it_for_the_card() {
        for text in [
            "Some Thrasta has hexproof as long as it entered this turn.",
            "As long as this creature is attacking, it gets +2/+0.",
        ] {
            let a = format!("{:?}", read("Some Thrasta", "Creature — Dinosaur", text));
            assert!(!a.contains("Antecedent"), "{a}");
        }
        // Each land in turn, not one object ([trailing-if]).
        let a = format!(
            "{:?}",
            read(
                "Some Surge",
                "Enchantment",
                "Each land gets +2/+2 as long as it's a creature."
            )
        );
        assert!(
            a.contains("cond: Is { what: Objects { quant: Itself"),
            "{a}"
        );
    }

    #[test]
    fn one_object_with_the_same_name_shares_it_with_the_one_before() {
        let a = format!(
            "{:?}",
            read(
                "Some Bazaar",
                "Enchantment",
                "Whenever a player casts a spell, counter it if a card with the same name is in a graveyard or a nontoken permanent with the same name is on the battlefield.",
            )
        );
        // Both comparands and the countered spell are the spell cast.
        assert_eq!(
            a.matches("SameNameAs(Objects { quant: Triggering").count(),
            2,
            "{a}"
        );
        assert!(
            a.contains("Counter { what: Objects { quant: Triggering"),
            "{a}"
        );
        // Several objects share it with one another.
        let a = format!(
            "{:?}",
            read(
                "Some Atlas",
                "Artifact",
                "{2}, {T}: Draw a card. Activate only if you control three or more lands with the same name.",
            )
        );
        assert!(a.contains("SameName]") || a.contains("SameName,"), "{a}");
    }

    #[test]
    fn a_size_alone_keeps_every_type() {
        let a = read(
            "Some Square",
            "Instant",
            "Target creature has base power and toughness 4/4 until end of turn.",
        );
        assert!(format!("{a:?}").contains("keeps: OtherTypes"), "{a:?}");
        assert_eq!(Keeps::implied_by(&[]), Keeps::OtherTypes);
        // A color is no type: "becomes black", "becomes a blue Serpent".
        assert_eq!(
            Keeps::implied_by(&[Adj::Color(Color::Black)]),
            Keeps::OtherTypes
        );
        assert_eq!(
            Keeps::implied_by(&[Adj::Color(Color::Blue), Adj::Subtype("serpent".into())]),
            Keeps::AllButSubtypesOfSet
        );
    }

    #[test]
    fn mana_of_any_type_is_not_mana_of_any_color() {
        let text = |kind: &str| {
            format!(
                "Exile the top card of your library. You may cast that card this turn, and you may spend mana as though it were mana of any {kind} to cast that spell."
            )
        };
        let ty = format!("{:?}", read("Some Robbery", "Sorcery", &text("type")));
        let color = format!("{:?}", read("Some Robbery", "Sorcery", &text("color")));
        assert!(ty.contains("SpendAsAnyType"), "{ty}");
        assert!(color.contains("SpendAsAnyColor"), "{color}");
    }

    #[test]
    fn round_ten_templates() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        // A loyalty cost in brackets inside quotes.
        let a = one(
            "Enchantment",
            "Planeswalkers you control have \"[+1]: Add {R}.\"",
        );
        assert!(a.contains("Loyalty { change: N(1)"), "{a}");
        // Crewing and saddling as though bigger.
        let a = one(
            "Creature — Human Pilot",
            "This creature saddles Mounts and crews Vehicles as though its power were 2 greater.",
        );
        assert!(
            a.contains("PowerGreaterFor") && a.contains("\"saddle\"") && a.contains("\"crew\""),
            "{a}"
        );
        // The size after the types, and what the same effect adds.
        let a = one(
            "Instant",
            "Until end of turn, target artifact or creature becomes an artifact creature with base power and toughness 4/4 and gains flying.",
        );
        assert!(
            a.contains("pt: Some(Pt { power: N(4), toughness: N(4)")
                && a.contains("gains: [Keyword { name: \"flying\""),
            "{a}"
        );
    }

    #[test]
    fn round_eleven_templates() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        // Keywords and quoted abilities after one "gains", each ability its
        // own grant to the same object for the same duration.
        let a = one(
            "Instant",
            "Until end of turn, target creature gains reach, hexproof, and \"Whenever this creature becomes tapped, draw a card.\"",
        );
        assert!(
            a.contains("gains: [Keyword { name: \"reach\"")
                && a.matches("GrantAbility").count() == 1,
            "{a}"
        );
        // A copy's exception with a keyword and an ability.
        let a = one(
            "Instant",
            "Create a token that's a copy of target creature you control, except it has haste and \"At the beginning of the end step, sacrifice this token.\"",
        );
        assert!(
            a.contains("Has { keywords: [Keyword { name: \"haste\""),
            "{a}"
        );
        // A quoted ability in a list ends at the list's comma.
        let a = one(
            "Artifact — Equipment",
            "Equipped creature gets +1/+1, has \"Whenever this creature attacks, you gain 1 life,\" and is a Cleric in addition to its other types.",
        );
        assert!(a.contains("HasAbility") && a.contains("AlsoIs"), "{a}");
        // No "you may": the caster chooses both.
        let a = one(
            "Instant",
            "Choose one. If this spell was cast using teamwork, choose both instead.\n• Draw a card.\n• Scry 2.",
        );
        assert!(a.contains("may: false"), "{a}");
        let a = one(
            "Creature — Rogue",
            "When this creature enters, you may cast target instant card from a graveyard, and mana of any type can be spent to cast that spell.",
        );
        assert!(a.contains("SpendAsAnyType"), "{a}");
        // "activate only" said once for two limits.
        let a = one(
            "Artifact",
            "{T}: Copy target activated or triggered ability you control from an artifact source. Activate only during your turn and only once each turn.",
        );
        assert!(
            a.contains("[[Ability(Activated)], [Ability(Triggered)]]")
                && a.contains("FromSource([Type(\"artifact\")])")
                && a.contains("limits: [OnlyDuringYourTurn, OncePerTurn]"),
            "{a}"
        );
    }

    #[test]
    fn round_eleven_delayed_and_flash_templates() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        let a = one(
            "Artifact",
            "{T}: When you next cast an instant or sorcery spell this turn, copy that spell. You may choose new targets for the copy.",
        );
        assert!(
            a.contains("DelayedNext { event: Casts") && a.contains("dur: ThisTurn"),
            "{a}"
        );
        let a = one(
            "Creature — Bird",
            "Whenever you cast a spell during an opponent's turn, surveil 1.\nWhenever you gain life for the first time each turn, draw a card.",
        );
        assert!(
            a.contains("OpponentsTurn") && a.contains("FirstTimeEachTurn(GainsLife"),
            "{a}"
        );
        let a = one(
            "Creature — Human",
            "You may cast this spell as though it had flash if you pay {2} more to cast it.\nYou may cast this spell as though it had flash if you control a Human.",
        );
        assert!(
            a.contains("CastAsThoughFlashFor") && a.contains("If { cond: YouControl"),
            "{a}"
        );
        let a = one(
            "Sorcery",
            "Return up to three target creature cards with total mana value 3 or less from your graveyard to the battlefield.",
        );
        assert!(a.contains("Total(Compare { stat: ManaValue"), "{a}");
        let a = one(
            "Instant",
            "Target creature you control gets +1/+0 until end of turn and can't be blocked this turn.",
        );
        assert!(a.contains("Modify") && a.contains("Restrict"), "{a}");
    }

    #[test]
    fn round_eleven_measures_and_the_card_by_its_noun() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        let a = one(
            "Creature — Human Monk",
            "Firebending X, where X is this creature's power.\nAt the beginning of your end step, if an opponent lost life this turn, target opponent loses X life, where X is the amount of life you gained this turn.",
        );
        assert!(
            a.contains("param: Some(N(PowerOf")
                && a.contains("DidThisTurn(LosesLife(AnOpponent))")
                && a.contains("LifeGainedThisTurn(You)"),
            "{a}"
        );
        let a = one(
            "Creature — Elemental",
            "Converge — This creature enters with a +1/+1 counter on it for each color of mana spent to cast it.",
        );
        assert!(a.contains("n: ColorsOfManaSpentToCast"), "{a}");
        let a = one(
            "Creature — Dinosaur",
            "Whenever this creature attacks, Dinosaurs you control other than this creature get +1/+1 until end of turn.",
        );
        assert!(a.contains("OtherThanSelf"), "{a}");
        let a = one(
            "Creature — Spirit",
            "This creature can't block or be blocked by creatures with power 2 or greater.",
        );
        assert!(
            a.contains("CantBlockAny") && a.contains("CantBeBlockedBy"),
            "{a}"
        );
    }

    #[test]
    fn round_eleven_anchor_words_and_turn_counts() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        let a = one(
            "Enchantment",
            "As this enchantment enters, choose Khans or Dragons.\n• Khans — At the beginning of combat on your turn, put two +1/+1 counters on target creature you control.\n• Dragons — At the beginning of combat on each opponent's turn, tap target creature that player controls.",
        );
        assert!(
            a.starts_with("[Anchored")
                && a.contains("word: \"dragons\"")
                && a.contains("Combat(EachOpponent)"),
            "{a}"
        );
        let a = one(
            "Creature — Bird",
            "This creature gets +2/+0 as long as you've drawn two or more cards this turn.\nAt the beginning of your end step, if a creature died under your control this turn, draw a card.",
        );
        assert!(
            a.contains("CardsDrawnThisTurn(You)")
                && a.contains("posts: [Controller(You)], or: None } }))"),
            "{a}"
        );
        let a = one(
            "Artifact",
            "Activated abilities of artifacts you control cost {1} less to activate.",
        );
        assert!(a.contains("AbilityCostChange { kind: Activated"), "{a}");
        let a = one(
            "Sorcery",
            "Exile cards from the top of your library until you exile a nonland card. You may cast that card this turn.",
        );
        assert!(a.contains("UntilFound { who: You"), "{a}");
    }

    #[test]
    fn round_eleven_entering_conditions_and_halves() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        let a = one(
            "Creature — Human",
            "Raid — This creature enters with a +1/+1 counter on it if you attacked this turn.\nThis creature enters with a divinity counter on it if you cast it from your hand.",
        );
        assert!(a.contains("If { cond: YouAttackedThisTurn"), "{a}");
        assert!(
            a.contains("YouCastItFrom { what: Objects { quant: SelfRef"),
            "{a}"
        );
        let a = one(
            "Creature — Wall",
            "{W}: This creature can attack this turn as though it didn't have defender.\nWhen this creature dies, if it had a -1/-1 counter on it, draw a card.",
        );
        assert!(
            a.contains("AttacksDespiteDefender, dur: ThisTurn")
                && a.contains("Past(HasCounter { what: Objects { quant: SelfRef"),
            "{a}"
        );
        let a = one(
            "Sorcery",
            "Any number of target players each mill two cards. Target opponent loses half their life, rounded up.",
        );
        assert!(
            a.contains("Mill { who: AnyNumberOfTarget")
                && a.contains("Half { of: LifeTotal(That), round_up: true }"),
            "{a}"
        );
    }

    #[test]
    fn round_eleven_ranges_and_past_events() {
        let one = |ty: &str, text: &str| format!("{:?}", read("Some Card", ty, text));
        let a = one(
            "Sorcery",
            "Distribute three +1/+1 counters among one, two, or three target creatures you control.",
        );
        assert!(a.contains("Between(1, 3)"), "{a}");
        let a = one(
            "Creature — Human",
            "When this creature enters, exile up to two target cards from a single graveyard.\nAt the beginning of your end step, if a card left your graveyard this turn, draw a card.",
        );
        assert!(
            a.contains("SingleGraveyard") && a.contains("Leaves { what: Objects { quant: One"),
            "{a}"
        );
        let a = one(
            "Artifact",
            "{2}, {T}: Tap target creature. This ability costs {1} less to activate if you control a creature with a +1/+1 counter on it.",
        );
        assert!(a.contains("cond: Some(YouControl"), "{a}");
        let a = one(
            "Sorcery",
            "Each player sacrifices a creature of their choice. If you sacrificed a creature this way, draw a card.",
        );
        assert!(a.contains("ThisWay(Sacrifices { who: You"), "{a}");
    }

    #[test]
    fn hero_is_a_creature_type() {
        let a = read(
            "Some Assembly",
            "Enchantment",
            "Heroes you control get +2/+2.",
        );
        let s = format!("{a:?}");
        assert!(
            s.contains("Subtype(\"hero\")") && !s.contains("Type(\"hero\")"),
            "{s}"
        );
    }

    #[test]
    fn protection_from_two_qualities_is_two_protection_abilities() {
        let a = read(
            "Some Riftwalker",
            "Creature — Fox Wizard",
            "Protection from Spirits and from Arcane",
        );
        // CR 702.16g: shorthand for two separate abilities.
        assert!(matches!(&a[0], Ability::Keywords(ks)
            if ks.len() == 2 && ks.iter().all(|k| k.name == "protection")));
    }

    #[test]
    fn colors_before_a_list_of_kinds_hold_of_every_kind() {
        let a = read(
            "Some Sanctifier",
            "Creature — Human Cleric",
            "If a black or red permanent, spell, or card not on the battlefield would be put into a graveyard, exile it instead.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        let Static::Replacement { event, .. } = &s[0] else {
            panic!("{s:?}")
        };
        let Event::PutInto { what, .. } = &**event else {
            panic!("{event:?}")
        };
        let colors = vec![vec![Adj::Color(Color::Black)], vec![Adj::Color(Color::Red)]];
        let spell = what.filter.or.as_deref().unwrap();
        let card = spell.or.as_deref().unwrap();
        assert_eq!(what.filter.alts, colors);
        assert_eq!(spell.alts, colors);
        assert_eq!(card.alts, colors);
        assert_eq!(card.posts, vec![Post::NotIn(Zone::Battlefield)]);
    }

    #[test]
    fn a_zone_after_a_list_holds_of_each_card_but_not_of_other_kinds() {
        let zone_of = |text: &str| {
            let a = read("Some Ending", "Sorcery", text);
            let Ability::Spell(e) = &a[0] else {
                panic!("{a:?}")
            };
            match &e[0] {
                Effect::Move { what, .. } => what.filter.clone(),
                e => panic!("{e:?}"),
            }
        };
        let both = zone_of("Return target Dragon creature card or Ugin planeswalker card from your graveyard to your hand.");
        assert_eq!(both.posts, vec![Post::In(Zone::Graveyard(Player::You))]);
        // A spell is on the stack and a permanent on the battlefield, so "in
        // a graveyard" is the card's alone.
        let last = zone_of(
            "Return target spell, nonland permanent, or card in a graveyard to its owner's hand.",
        );
        assert!(last.posts.is_empty(), "{last:?}");
    }

    #[test]
    fn cards_you_dont_own_are_not_cards_your_opponents_own() {
        let posts = |text: &str| {
            let a = read("Some Thief", "Creature — Human Rogue", text);
            format!("{a:?}")
        };
        // In Two-Headed Giant a teammate owns cards that are neither.
        assert!(
            posts("Whenever you cast a spell you don't own, draw a card.")
                .contains("NotOwnedBy(You)")
        );
        assert!(
            posts("Whenever you cast a spell your opponents own, draw a card.")
                .contains("Owner(EachOpponent)")
        );
    }

    #[test]
    fn a_legendary_x_the_y_calls_itself_x() {
        let a = read(
            "Gideon the Oathless",
            "Legendary Creature — Human Mercenary",
            "Whenever a creature an opponent controls enters, Gideon deals 1 damage to that player.",
        );
        assert!(matches!(&a[0], Ability::Triggered { effect, .. }
            if matches!(&effect[0], Effect::Damage { source: Some(s), .. } if s.quant == Quant::SelfRef)));
    }

    #[test]
    fn a_passive_token_doubler_names_no_creator() {
        let a = read(
            "Some Dominus",
            "Creature — Phyrexian Dragon",
            "If one or more tokens would be created under your control, twice that many of those tokens are created instead.",
        );
        let Ability::Static(s) = &a[0] else {
            panic!("{a:?}")
        };
        assert!(matches!(
            &s[0],
            Static::Replacement { event, instead }
                if matches!(**event, Event::Creates { by: Actor::Passive, under: Some(Player::You), .. })
                && matches!(&instead[0], Effect::CreateThose { by: Actor::Passive, n: Amount::Twice(_), .. })
        ));
    }
}

#[cfg(test)]
mod round_nine {
    use super::*;
    use ast::*;

    fn read(type_line: &str, text: &str) -> Vec<Ability> {
        let face = Face {
            name: "Some Card",
            type_line,
            text,
        };
        read_face(&face, "Some Card")
            .into_iter()
            .map(|l| match l.parsed {
                Ok(a) => a,
                Err(b) => panic!("{}: broke at {:?} after {:?}", l.text, b.at, b.read),
            })
            .collect()
    }

    #[test]
    fn max_speed_is_a_keyword_that_grants_an_ability_not_an_ability_word() {
        let a = read(
            "Artifact",
            "Start your engines!\nMax speed — {T}: Add {C}{C}.",
        );
        assert_eq!(
            a[0],
            Ability::Keywords(vec![Keyword {
                name: "start your engines!".into(),
                param: None
            }])
        );
        assert!(
            matches!(&a[1], Ability::KeywordOn { keyword: Prefix::MaxSpeed, ability } if matches!(**ability, Ability::Activated { .. }))
        );
    }

    #[test]
    fn a_saga_chapter_may_carry_a_flavour_word_but_choose_one_is_a_mode() {
        let a = read(
            "Enchantment Creature — Saga Bird",
            "I — Crescent Fang — Draw a card.\nII — Choose one —\n• Draw a card.\n• Discard a card.",
        );
        assert!(matches!(&a[0], Ability::Chapter { effect, .. }
            if matches!(&effect[..], [Effect::Labelled { label, .. }] if label == "crescent fang")));
        assert!(matches!(&a[1], Ability::Chapter { effect, .. }
            if matches!(&effect[..], [Effect::Modal { .. }])));
    }

    #[test]
    fn a_lander_is_a_predefined_artifact_token() {
        let a = read("Sorcery", "Create a Lander token.");
        assert!(matches!(&a[0], Ability::Spell(e)
            if matches!(&e[0], Effect::Create { token, .. } if token.named.as_deref() == Some("lander"))));
    }

    #[test]
    fn speed_is_an_amount_and_max_speed_a_condition() {
        let a = read(
            "Creature — God",
            "This creature can't attack or block unless you have max speed.\nOther creatures you control get +X/+0, where X is your speed.",
        );
        assert!(format!("{:?}", a[0]).contains("MaxSpeed(You)"));
        assert!(format!("{:?}", a[1]).contains("Speed(You)"));
    }
}

#[cfg(test)]
mod round_nine_b {
    use super::*;
    use ast::*;

    fn read(type_line: &str, text: &str) -> Vec<Ability> {
        let face = Face {
            name: "Some Card",
            type_line,
            text,
        };
        read_face(&face, "Some Card")
            .into_iter()
            .map(|l| match l.parsed {
                Ok(a) => a,
                Err(b) => panic!("{}: broke at {:?} after {:?}", l.text, b.at, b.read),
            })
            .collect()
    }

    #[test]
    fn exhaust_and_power_up_are_keywords_on_their_activated_ability() {
        let a = read(
            "Creature — Robot",
            "Exhaust — {2}{G}{G}: Put two +1/+1 counters on this creature.\nPower-up — {4}{W}: Draw a card.",
        );
        assert!(
            matches!(&a[0], Ability::KeywordOn { keyword: Prefix::Exhaust, ability } if matches!(**ability, Ability::Activated { .. }))
        );
        assert!(matches!(
            &a[1],
            Ability::KeywordOn {
                keyword: Prefix::PowerUp,
                ..
            }
        ));
    }

    #[test]
    fn a_prepared_creature_and_the_void_condition() {
        let a = read(
            "Creature — Human Wizard",
            "At the beginning of your upkeep, if this creature isn't prepared, it becomes prepared.\nVoid — At the beginning of your end step, if a nonland permanent left the battlefield this turn or a spell was warped this turn, draw a card.",
        );
        let Ability::Triggered {
            trigger: Trigger::InterveningIf(_, c),
            effect,
            ..
        } = &a[0]
        else {
            panic!("{a:?}")
        };
        assert!(matches!(&**c, Condition::Not(_)));
        assert!(matches!(&effect[0], Effect::BecomesPrepared(_)));
        let s = format!("{:?}", a[1]);
        assert!(s.contains("Or([DidThisTurn(Leaves"), "{s}");
        assert!(s.contains("CastUsing { what"), "{s}");
    }

    #[test]
    fn mana_spent_at_least_and_cast_using_teamwork() {
        let a = read(
            "Instant",
            "Draw a card. If this spell was cast using teamwork, draw two cards instead.",
        );
        assert!(format!("{a:?}").contains("CastUsing { what: Objects { quant: SelfRef"));
        let b = read(
            "Creature — Human",
            "Whenever you cast a noncreature spell, if at least four mana was spent to cast it, draw a card.",
        );
        assert!(format!("{b:?}").contains("AmountIs { n: ManaSpentToCast("));
    }
}

#[cfg(test)]
mod round_nine_c {
    use super::*;
    use ast::*;

    fn read(type_line: &str, text: &str) -> Vec<Ability> {
        let face = Face {
            name: "Some Card",
            type_line,
            text,
        };
        read_face(&face, "Some Card")
            .into_iter()
            .map(|l| match l.parsed {
                Ok(a) => a,
                Err(b) => panic!("{}: broke at {:?} after {:?}", l.text, b.at, b.read),
            })
            .collect()
    }

    fn event(a: &Ability) -> &Event {
        match a {
            Ability::Triggered {
                trigger: Trigger::When(e),
                ..
            } => e,
            a => panic!("{a:?}"),
        }
    }

    #[test]
    fn attacking_alone_and_while_saddled_are_said_of_the_attacker() {
        let a = read(
            "Creature — Horse Mount",
            "Whenever a creature you control attacks alone, draw a card.\nWhenever this creature attacks while saddled, draw a card.",
        );
        assert!(matches!(event(&a[0]), Event::AttacksAlone(_)));
        let Event::While { event: e, cond } = event(&a[1]) else {
            panic!("{a:?}")
        };
        let Event::Attacks(x) = &**e else {
            panic!("{e:?}")
        };
        assert!(
            matches!(&**cond, Condition::Is { what, filter } if what == x
            && filter.alts == [vec![Adj::State(State::Saddled)]])
        );
    }

    #[test]
    fn scry_or_surveil_is_two_actions_of_one_player() {
        let a = read(
            "Creature — Human",
            "Whenever you scry or surveil, draw a card.",
        );
        assert_eq!(
            *event(&a[0]),
            Event::Or(vec![
                Event::Performs {
                    who: Player::You,
                    action: "scry".into()
                },
                Event::Performs {
                    who: Player::You,
                    action: "surveil".into()
                },
            ])
        );
    }

    #[test]
    fn youve_cast_and_entered_this_turn_are_past_events_of_the_turn() {
        let a = read(
            "Land",
            "{T}: Add {B} or {R}. Activate only if this land entered this turn or if you control a basic land.",
        );
        assert!(format!("{a:?}")
            .contains("OnlyIf(Or([DidThisTurn(Enters { what: Objects { quant: SelfRef"));
        let b = read(
            "Sorcery",
            "This spell costs {2} less to cast if you've cast another spell this turn.",
        );
        assert!(format!("{b:?}")
            .contains("DidThisTurn(Casts { who: You, what: Objects { quant: Another"));
    }
}

#[cfg(test)]
mod round_nine_d {
    use super::*;
    use ast::*;

    fn read(type_line: &str, text: &str) -> Vec<Ability> {
        let face = Face {
            name: "Some Card",
            type_line,
            text,
        };
        read_face(&face, "Some Card")
            .into_iter()
            .map(|l| match l.parsed {
                Ok(a) => a,
                Err(b) => panic!("{}: broke at {:?} after {:?}", l.text, b.at, b.read),
            })
            .collect()
    }

    #[test]
    fn behold_is_a_cost_and_beheld_a_condition_on_it() {
        let a = read(
            "Instant",
            "As an additional cost to cast this spell, behold a Goblin or pay {2}.\nDraw a card. If a Goblin was beheld, draw two cards instead.",
        );
        assert!(matches!(&a[0], Ability::Static(s) if matches!(&s[0],
            Static::AdditionalCost(c) if matches!(&c[..], [Cost::OneOf(v)] if matches!(&v[0], Cost::ActionOn { name, .. } if name == "behold")))));
        assert!(format!("{:?}", a[1]).contains("Beheld("));
    }

    #[test]
    fn a_mana_value_said_in_the_past_is_the_one_it_had() {
        let a = read(
            "Instant",
            "Destroy target artifact. If its mana value was 2 or less, draw a card.",
        );
        assert!(format!("{a:?}").contains("If { cond: Past(AmountIs { n: ManaValueOf("));
    }

    #[test]
    fn its_counters_are_every_counter_on_it_and_a_counter_is_of_any_kind() {
        let a = read(
            "Creature — Insect",
            "When this creature dies, put its counters on target creature you control.\n{1}{U}, Remove a counter from this creature: Draw a card.",
        );
        let s = format!("{a:?}");
        assert!(s.contains("PutThoseCounters { those: Objects { quant: All, filter: Filter { alts: [], kind: Some(Counter(None))"), "{s}");
        assert!(s.contains("RemoveAnyCounters { n: N(1)"), "{s}");
    }

    #[test]
    fn it_said_before_a_trailing_for_each_is_not_what_the_loop_counts() {
        let a = read(
            "Enchantment",
            "Whenever a creature you control attacks, it gets +1/+0 until end of turn for each other attacking creature that shares a creature type with it.",
        );
        let s = format!("{a:?}");
        assert!(
            s.contains("then: [Modify { what: Objects { quant: Triggering"),
            "{s}"
        );
        let a = read(
            "Instant",
            "Create a 1/1 white Ally creature token. Put a +1/+1 counter on it for each creature your opponents control.",
        );
        let s = format!("{a:?}");
        assert!(s.contains("on: Objects { quant: BeforeEach"), "{s}");
        // Said first, "it" is the object of each pass.
        let a = read(
            "Sorcery",
            "For each creature you control, put a +1/+1 counter on it.",
        );
        let s = format!("{a:?}");
        assert!(s.contains("on: Objects { quant: Antecedent"), "{s}");
    }

    /// Every bare "it" here is the card; none is `Antecedent`.
    #[test]
    fn it_after_the_card_is_named_anywhere_before_is_the_card() {
        for (type_line, text) in [
            ("Artifact", "At the beginning of your draw step, if this artifact is tapped, it deals 1 damage to you."),
            ("Artifact Creature — Construct", "Remove a +1/+1 counter from this creature: It deals 1 damage to any target."),
            ("Land", "Channel — {2}{W}, Discard this card: It deals 4 damage to target attacking or blocking creature."),
            ("Land — Swamp Forest", "As this land enters, you may pay 2 life. If you don't, it enters tapped."),
            ("Artifact", "If this artifact would enter, you may discard a land card instead. If you do, put this artifact onto the battlefield. If you don't, put it into its owner's graveyard."),
            ("Artifact Creature — Turtle", "Whenever this creature or another artifact you control enters, put a +1/+1 counter on this creature. It can't be blocked this turn."),
            ("Creature — Mouse", "Valiant — Whenever this creature becomes the target of a spell or ability you control for the first time each turn, put a +1/+1 counter on it."),
            ("Creature — Elf", "This creature enters with a +1/+1 counter on it for each color of mana spent to cast it."),
            ("Creature — Spirit", "As long as this creature has three or more +1/+1 counters on it, it can attack as though it didn't have defender."),
        ] {
            let s = format!("{:?}", read(type_line, text));
            assert!(!s.contains("Antecedent"), "{text}\n{s}");
        }
        // The tokens created are what "them" is, not the card.
        let s = format!(
            "{:?}",
            read(
                "Enchantment — Aura",
                "Enchanted creature has \"{T}: Create X 1/1 red Elemental creature tokens with haste, where X is this creature's power. Exile them at the beginning of the next end step.\""
            )
        );
        assert!(s.contains("Exile(Objects { quant: Created"), "{s}");
        // "that spell" is the spell that targets, not the creature.
        let s = format!(
            "{:?}",
            read(
                "Creature — Spirit",
                "Whenever this creature becomes the target of a spell, counter that spell."
            )
        );
        assert!(
            !s.contains("Counter { what: Objects { quant: SelfRef"),
            "{s}"
        );
    }

    /// "other" said against an object the ability names, not the card.
    #[test]
    fn other_is_other_than_the_object_it_is_said_against() {
        for (type_line, text, want) in [
            ("Sorcery", "Target creature you control deals damage equal to its power to each other creature and each opponent.", "OtherThan(Objects { quant: Antecedent"),
            ("Artifact", "Each creature gets +1/+1 for each other creature on the battlefield that shares at least one creature type with it.", "OtherThan(Objects { quant: Itself"),
            ("Enchantment", "Whenever a nontoken creature you control enters, if it doesn't have the same name as another creature you control or a creature card in your graveyard, draw a card.", "quant: One, filter: Filter { alts: [[OtherThan(Objects { quant: Triggering"),
            ("Legendary Creature — Elf", "Whenever another creature enters, its controller may draw a card if its power is greater than each other creature's power.", "OtherThan(Objects { quant: Triggering"),
            ("Sorcery", "Each player chooses any number of creatures they control with total power 4 or less, then sacrifices all other creatures they control.", "OtherThan(Objects { quant: Antecedent"),
            ("Sorcery", "Each player sacrifices a creature or planeswalker of their choice. If you sacrificed a permanent this way, you may return another permanent card from your graveyard to your hand.", "OtherThan(Objects { quant: Antecedent"),
        ] {
            let s = format!("{:?}", read(type_line, text));
            assert!(s.contains(want) && !s.contains("State(Other)"), "{text}\n{s}");
        }
        // The card's own damage, and another spell: the card.
        let s = format!(
            "{:?}",
            read(
                "Sorcery",
                "Some Card deals 2 damage to each other creature."
            )
        );
        assert!(s.contains("State(Other)"), "{s}");
        let s = format!(
            "{:?}",
            read(
                "Instant",
                "Destroy target creature. If you've cast another spell this turn, draw a card."
            )
        );
        assert!(s.contains("quant: Another"), "{s}");
    }

    #[test]
    fn the_payer_of_an_unless_decides_before_the_player_who_may() {
        let s = format!(
            "{:?}",
            read(
                "Enchantment",
                "Whenever an opponent casts a spell, you may draw a card unless that player pays {1}."
            )
        );
        assert!(
            s.contains("effect: [Unless { effect: May { who: You"),
            "{s}"
        );
    }

    #[test]
    fn a_sentence_about_a_branchs_choice_is_the_branchs() {
        let s = format!(
            "{:?}",
            read(
                "Legendary Enchantment Creature — Nightmare",
                "At the beginning of your end step, you may sacrifice an artifact, creature, enchantment, land, or planeswalker. If you do, each opponent may sacrifice a permanent of their choice that shares a card type with it. For each opponent who doesn't, that player loses 2 life and you draw a card."
            )
        );
        assert!(
            s.contains("IfYouDo([May { who: EachOpponent") && s.ends_with("}] }])], limits: [] }]"),
            "{s}"
        );
        let s = format!(
            "{:?}",
            read(
                "Enchantment",
                "At the beginning of your draw step, you may draw two additional cards. If you do, choose two cards in your hand drawn this turn. For each of those cards, pay 4 life or put the card on top of your library."
            )
        );
        assert!(s.contains("})), ForEach {"), "{s}");
    }

    #[test]
    fn a_choice_said_with_a_delayed_step_is_made_as_the_delayed_trigger_resolves() {
        let s = format!(
            "{:?}",
            read(
                "Instant",
                "Counter target spell. Its controller may draw up to two cards at the beginning of the next turn's upkeep."
            )
        );
        assert!(
            s.contains("Delayed { step: Upkeep(NextTurns), then: [May {"),
            "{s}"
        );
    }

    #[test]
    fn x_defined_once_binds_every_later_sentence_that_says_it() {
        let a = read(
            "Creature — Merfolk Wizard",
            "When this creature enters, look at the top X cards of your library, where X is your devotion to blue. Put up to one of them on top of your library and the rest on the bottom of your library in a random order. If X is greater than or equal to the number of cards in your library, you win the game.",
        );
        let Ability::Triggered { effect, .. } = &a[0] else {
            panic!("{a:?}")
        };
        assert!(
            matches!(&effect[..], [Effect::Where { then, .. }] if matches!(then.last(), Some(Effect::If { .. }))),
            "{effect:?}"
        );
    }

    #[test]
    fn the_top_card_of_each_players_library_is_one_per_library() {
        let s = format!(
            "{:?}",
            read("Sorcery", "Exile the top card of each player's library.")
        );
        assert!(s.contains("quant: NFromEach(N(1))"), "{s}");
    }
}
