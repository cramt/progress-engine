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
}
