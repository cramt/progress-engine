//! Vivisurgeon's Insight: Oracle text in, a typed ability tree out.
//!
//! A spike. Its question is how much of Magic a grammar can read into a tree
//! with no hole in it, and where the rest breaks; see README.md for the number.

pub mod ast;
pub mod lex;
pub mod parse;
pub mod vocab;

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
    lines
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let toks = lex::lex(&l);
            let text = l.replace('\u{E000}', "~");
            let parsed = parse::parse_line(&toks, spell).map_err(|at| Broke {
                read: toks[..at.min(toks.len())]
                    .iter()
                    .map(show)
                    .collect::<Vec<_>>()
                    .join(" "),
                at: toks.get(at).map(show).unwrap_or_else(|| "<end>".into()),
            });
            Line { text, parsed }
        })
        // A line of nothing but reminder text says nothing to read.
        .filter(|l| !lex::lex(&l.text).is_empty())
        .collect()
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
                if **c == Condition::YouCastIt
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
        assert!(matches!(
            &instead[0],
            Effect::PutCounters { n: Amount::Plus(n, 1), .. } if **n == Amount::ThatMany
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
                within: Some(Palette::Chosen),
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
