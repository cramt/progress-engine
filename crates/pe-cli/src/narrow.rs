//! One enumeration per class of question, rather than one for the whole file.
//!
//! The engine groups cards by which queries they match and walks compositions
//! over those groups. Both halves of that were sized **once, for the file**, as
//! the join of what every question in it needs — so a `can_cast` clause, which
//! has to tell a Plains from an Island, shattered a real Commander manabase
//! into seventeen groups and then charged all seventeen to the criterion next
//! to it that only ever asked `count('cat:"Ramp"')`. Likewise a criterion
//! correlating turns 1 and 2 needs a path through the checkpoints, and a
//! criterion about turn 5 alone needs one hypergeometric — and the second paid
//! for the first.
//!
//! This is the partition that stops that:
//! [#31](https://github.com/cramt/progress-engine/issues/31). Questions that
//! read the same things are one class, each class gets the cheapest
//! enumeration that can answer it, and the answers are stitched back together
//! by position. Nothing about the answers changes — a coarser grouping is a
//! marginal of the finer one, and the checkpoints a class does not look at
//! collapse into the next one it does — which is the only reason this is
//! allowed to exist at all.
//!
//! What a class has to keep is not only what its clauses name. The **walk**
//! reads grouping bits too: which cards a live effect applies to, where it
//! routes them, and which land the declared priority plays. Two cards that
//! disagree about any of those end up in different zones, so they are not
//! interchangeable however little the criterion cares about the difference.

use pe_criteria::{Answering, Grouping, Plan, Reading, Schedule};
use pe_toml::{QuestionReads, Reads};

/// Grouping bits the walk itself reads, whatever any criterion asks about.
///
/// Both are `Option` rather than a mask that might be zero, because *no live
/// effect* and *a live effect that happens to own bit 0* are different facts
/// and a zero mask cannot tell them apart. The distinction decides whether
/// checkpoints may collapse at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shared {
    /// Which cards each live effect applies to, and where it routes them.
    /// `None` when nothing in this run routes a card anywhere.
    pub effects: Option<u64>,
    /// The declared land-drop priority, and the query saying what a land is.
    /// `None` when the file declared none.
    pub land_drop: Option<u64>,
}

/// One class of questions and the enumeration that answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The grouping bits this class can tell apart. Everything else merges.
    keep: u64,
    /// Whether the lands have to keep what they produce and whether they
    /// arrive tapped. True exactly where something asks whether a cost could
    /// have been paid.
    mana: bool,
    reading: Reading,
    /// The turns whose counts this class reads. For [`Reading::PerTurn`] it is
    /// the last one, because that reading keeps every turn up to it anyway.
    turns: Vec<usize>,
    criteria: Vec<usize>,
    expectations: Vec<usize>,
}

impl Class {
    /// The coarsest grouping that still answers this class.
    pub fn grouping(&self, full: &Grouping) -> Grouping {
        full.coarsened(self.keep, self.mana)
    }

    /// The shortest schedule that still answers it.
    pub fn schedule(&self, full: &Schedule) -> Schedule {
        full.narrowed(&self.turns, self.reading)
    }

    /// Which of the file's questions this enumeration is for.
    pub fn answering(&self, plan: Plan) -> Option<Answering> {
        Answering::some(plan, self.criteria.clone(), self.expectations.clone())
    }
}

/// What one question needs of an enumeration, once the walk's own bits are in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Need {
    keep: u64,
    mana: bool,
    reading: Reading,
    turns: Vec<usize>,
}

impl Need {
    fn of(reads: &Reads, shared: &Shared) -> Need {
        // One land drop a turn is use-it-or-lose-it, so what is in play and
        // what could be paid are facts about the whole history rather than
        // about a total. Everything else is a count of cards seen.
        let history = reads.casts() || reads.battlefield();
        let mut keep = reads.queries();
        // A live effect moves cards between zones, so every count in the run
        // depends on which cards it applies to and where it sends them.
        keep |= shared.effects.unwrap_or(0);
        // The priority only moves a number where something reads the drops it
        // made, or where an effect fires off the land it chose.
        if history || shared.effects.is_some() {
            keep |= shared.land_drop.unwrap_or(0);
        }
        Need {
            keep,
            mana: reads.casts(),
            reading: if history {
                Reading::PerTurn
            } else {
                Reading::Cumulative
            },
            // A per-turn reading keeps every turn up to the last one named, so
            // two questions differing only in which earlier turns they mention
            // are one class rather than two identical enumerations.
            turns: match history {
                true => reads.turns().iter().copied().max().into_iter().collect(),
                false => reads.turns().to_vec(),
            },
        }
    }
}

/// Partition a file's questions into classes that can each be answered on its
/// own.
///
/// Questions land together exactly when they need the same enumeration. There
/// is no attempt to merge two classes whose needs merely overlap: a joint
/// enumeration is at least as wide as either, so merging can only cost width,
/// and width is the thing this exists to spend less of.
pub fn partition(reads: &QuestionReads, shared: &Shared) -> Vec<Class> {
    let mut classes: Vec<Class> = Vec::new();
    let mut place = |need: Need, criterion: Option<usize>, expectation: Option<usize>| {
        let slot = classes.iter().position(|c| {
            c.keep == need.keep
                && c.mana == need.mana
                && c.reading == need.reading
                && c.turns == need.turns
        });
        let class = match slot {
            Some(i) => &mut classes[i],
            None => {
                classes.push(Class {
                    keep: need.keep,
                    mana: need.mana,
                    reading: need.reading,
                    turns: need.turns,
                    criteria: Vec::new(),
                    expectations: Vec::new(),
                });
                classes.last_mut().expect("just pushed")
            }
        };
        class.criteria.extend(criterion);
        class.expectations.extend(expectation);
    };
    for (i, question) in reads.criteria.iter().enumerate() {
        place(Need::of(question, shared), Some(i), None);
    }
    for (i, question) in reads.expectations.iter().enumerate() {
        place(Need::of(question, shared), None, Some(i));
    }
    classes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reads_of(source: &str) -> QuestionReads {
        pe_toml::Criteria::parse(source, "test")
            .expect("parses")
            .reads()
    }

    /// The bits are positions in the file's query list, in first-mention
    /// order, which is what a clause holds.
    fn bit(i: usize) -> u64 {
        1u64 << i
    }

    #[test]
    fn a_criterion_keeps_only_the_queries_it_names() {
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "ramp"
                require = [{ turn = 3, query = 'cat:"Ramp"', min = 1 }]

                [[criterion]]
                name = "lands"
                require = [{ turn = 3, query = "t:land", min = 3 }]
                "#,
            ),
            &Shared::default(),
        );
        assert_eq!(classes.len(), 2);
        assert_eq!(classes[0].keep, bit(0));
        assert_eq!(classes[1].keep, bit(1));
        // Neither can tell a Plains from an Island, so neither pays for it.
        assert!(!classes[0].mana && !classes[1].mana);
    }

    #[test]
    fn a_cross_turn_criterion_keeps_both_its_turns() {
        // The case the narrowing must not touch: this correlates two turns, so
        // collapsing them into one total would answer a different question.
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "a land, then two"
                require = [
                  { turn = 1, query = "t:land", min = 1 },
                  { turn = 2, query = "t:land", min = 2 },
                ]
                "#,
            ),
            &Shared::default(),
        );
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].turns, vec![1, 2]);
        assert_eq!(classes[0].reading, Reading::Cumulative);
    }

    #[test]
    fn one_turn_is_one_checkpoint() {
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "loam by five"
                require = [{ turn = 5, query = 'name:"Life from the Loam"', min = 1 }]
                "#,
            ),
            &Shared::default(),
        );
        assert_eq!(classes[0].turns, vec![5]);
        let full = Schedule::build(5, false, Vec::new(), None);
        assert_eq!(full.gaps(), &[7, 0, 1, 1, 1, 1]);
        // Eleven cards seen by turn five, and nothing about the order.
        assert_eq!(classes[0].schedule(&full).gaps(), &[0, 0, 0, 0, 0, 11]);
    }

    #[test]
    fn a_cast_clause_pays_for_the_manabase_and_nothing_else_does() {
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "castable"
                require = [{ turn = 4, can_cast = "{1}{U}" }]

                [[criterion]]
                name = "ramp"
                require = [{ turn = 4, query = 'cat:"Ramp"', min = 1 }]
                "#,
            ),
            &Shared::default(),
        );
        assert_eq!(classes.len(), 2);
        assert!(classes[0].mana, "the cost is what can see a land's colour");
        assert_eq!(classes[0].reading, Reading::PerTurn);
        assert!(!classes[1].mana, "counting Ramp cannot");
        assert_eq!(classes[1].reading, Reading::Cumulative);
    }

    #[test]
    fn a_battlefield_count_reads_the_whole_history() {
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "three lands in play"
                require = [{ turn = 4, query = "t:land", zone = "battlefield", min = 3 }]
                "#,
            ),
            &Shared::default(),
        );
        // One drop a turn, so five lands drawn is not five lands played, and
        // the totals at turn four cannot say which.
        assert_eq!(classes[0].reading, Reading::PerTurn);
        // But nothing here asks what a land makes.
        assert!(!classes[0].mana);
        let full = Schedule::build(5, false, Vec::new(), None);
        assert_eq!(classes[0].schedule(&full).gaps(), &[7, 0, 1, 1, 1, 0]);
    }

    #[test]
    fn a_live_effect_is_kept_by_every_class() {
        let shared = Shared {
            effects: Some(bit(9) | bit(10)),
            land_drop: Some(bit(11)),
        };
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "ramp"
                require = [{ turn = 3, query = 'cat:"Ramp"', min = 1 }]
                "#,
            ),
            &shared,
        );
        // Where a card ends up depends on the effect that looked at it and on
        // the land the priority played, so a count in any zone depends on both.
        assert_eq!(classes[0].keep, bit(0) | bit(9) | bit(10) | bit(11));
    }

    #[test]
    fn a_declared_priority_costs_nothing_where_nothing_reads_the_drops() {
        let shared = Shared {
            effects: None,
            land_drop: Some(bit(11)),
        };
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "ramp"
                require = [{ turn = 3, query = 'cat:"Ramp"', min = 1 }]
                "#,
            ),
            &shared,
        );
        assert_eq!(classes[0].keep, bit(0));
    }

    #[test]
    fn questions_reading_the_same_things_share_one_enumeration() {
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "a"
                require = [{ turn = 5, query = "t:land", min = 3 }]

                [[criterion]]
                name = "b"
                require = [{ turn = 5, query = "t:land", min = 4 }]

                [[expect]]
                name = "c"
                turn = 5
                query = "t:land"
                "#,
            ),
            &Shared::default(),
        );
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0].criteria, vec![0, 1]);
        assert_eq!(classes[0].expectations, vec![0]);
    }
}
