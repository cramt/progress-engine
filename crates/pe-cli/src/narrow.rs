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
//!
//! The third narrowing, [#55](https://github.com/cramt/progress-engine/issues/55),
//! is the same idea aimed at the manabase: a grouping keyed on the whole
//! palette is finer than any one cost can see, so it is restricted to the pips
//! the class's costs actually demand. That one is only applied where the
//! **walk** reads nothing about a land that the cost cannot — see
//! [`Shared::picks_a_land`], which is where it is refused.

use pe_criteria::{Answering, Grouping, LandDetail, Palette, Plan, Reading, Schedule};
use pe_toml::{QuestionReads, Reads};

/// Grouping bits the walk itself reads, whatever any criterion asks about.
///
/// Both are `Option` rather than a mask that might be zero, because *no live
/// effect* and *a live effect that happens to own bit 0* are different facts
/// and a zero mask cannot tell them apart. The distinction decides whether
/// checkpoints may collapse at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct Shared {
    /// Which cards each live effect applies to, where it routes them, and
    /// whether any of them fires on the land drop. `None` when nothing in this
    /// run moves a card at all.
    pub effects: Option<Effects>,
    /// The declared land-drop priority, and the query saying what a land is.
    /// `None` when the file declared none.
    pub land_drop: Option<u64>,
    /// The declared casting priority. `None` when the file declared none, and
    /// then this run casts nothing at all.
    ///
    /// Not another `Option<u64>` beside the two above, and the difference is
    /// the point: a budget makes a class read *what a land makes* as well as
    /// which cards the policy names, and that is a second fact. Bundling it
    /// here rather than adding a loose palette field keeps "a class that
    /// prices spells prices lands too" a thing the type says.
    pub casting: Option<Casting>,
}

/// What the live effects make every class read.
///
/// Two fields rather than a bare mask, because *which* cards an effect moves
/// and *when it fires* are different facts and only the second one decides
/// whether the manabase may be narrowed. A surveil land fires on the drop, so
/// which land was played is a live question and no cost may merge two lands a
/// ranking could tell apart. A tutor on a cast spends mana rather than the
/// drop, and the budget has already said which spells those are — so it costs
/// its own query bits and nothing else.
#[derive(Debug, Clone, Copy)]
pub struct Effects {
    /// Which cards each live effect applies to, where it routes them, and what
    /// it would go and fetch.
    pub queries: u64,
    /// Whether any of them fires on the land drop.
    pub on_the_drop: bool,
}

/// What a declared casting priority makes every class read.
///
/// **Every** class, which is the expensive half of the budget and is not
/// optional. A spell that is cast leaves the hand, so *how many cards
/// matching anything are in my hand on turn 4* depends on which spells the
/// pool paid for on turns 1 to 3 — and that depends on the manabase, on what
/// each named spell costs, and on the order the turns went in. A class
/// counting `cat:"Ramp"` beside a budget cannot be answered on a cumulative
/// total of a merged manabase, because the merge changes what was cast and the
/// total cannot say which turn had the mana.
///
/// That is the cost stated up front rather than discovered: a file that
/// declares `[casting]` pays the mana grouping on every question in it. A file
/// that declares none pays nothing, and none of its numbers move.
#[derive(Debug, Clone, Copy)]
pub struct Casting {
    /// The priority's own queries, which decide which spells are cast and in
    /// what order.
    pub queries: u64,
    /// The pip kinds every cost in the declared line demands, joined.
    ///
    /// The half of the palette narrowing that only the deck can state. A
    /// `can_cast = "{1}{U}"` clause names its own colour; *how many Opts did I
    /// cast* names none, and the blue is in the card data.
    pub demands: Palette,
}

impl Shared {
    /// Whether anything but the cost gets to choose **which** land was played.
    ///
    /// This is the gate on the palette narrowing and the reason it is not
    /// applied everywhere. Two lands with the same restricted palette are
    /// interchangeable to [`pe_criteria::Cost`], and they are interchangeable
    /// to the rest of the walk — the drop count, what is in play, whether a
    /// land arrived this turn — because all of those read sums over land
    /// groups and a sum does not care how its terms were labelled. They are
    /// **not** interchangeable to a declared priority, which plays the first
    /// land it is holding in a ranking that ends *"then the card this decklist
    /// names first"*. Merging renumbers that ranking: ranked `Plains, Island,
    /// Swamp` and asked for `{U}`, the Plains and the Swamp merge into a group
    /// that now sits where the Plains did, so a hand holding an Island and a
    /// Swamp plays the Swamp where the file said Island — and the answer
    /// moves, by 29% to 16% on the hand
    /// `a_declared_priority_is_not_allowed_to_merge_two_lands_it_ranks_apart`
    /// deals. The tie rule is a promise the run prints; the narrowing is an
    /// optimisation. The optimisation loses.
    ///
    /// Recovering it by merging only lands the ranking already places side by
    /// side is [#56](https://github.com/cramt/progress-engine/issues/56).
    ///
    /// An effect that fires **on the drop** is refused for the same kind of
    /// reason and costs nothing to refuse: a run with one and no declared
    /// priority cannot ask a mana question at all — it is refused by name,
    /// because the effect and the gate would be two policies over one land
    /// drop — so the only castable class this can cost is one that already has
    /// a priority.
    ///
    /// An effect that fires on a **cast** is not that. It spends mana rather
    /// than the land drop, and the budget beside it has already said which
    /// spells the pool paid for, so it chooses no land and the manabase may
    /// still be narrowed to the pips the costs demand. Reading this as *any*
    /// live effect cost `decks/lantern.txt` 19 groups and 62 billion
    /// compositions for a tutor that never looked at a land.
    fn picks_a_land(&self) -> bool {
        self.land_drop.is_some() || self.effects.is_some_and(|e| e.on_the_drop)
    }
}

/// One class of questions and the enumeration that answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The grouping bits this class can tell apart. Everything else merges.
    keep: u64,
    /// How much of what a land makes this class can tell apart:
    /// [`LandDetail::Ignored`] unless something here asks whether a cost could
    /// have been paid, and then only the pips those costs demand — where that
    /// is provable.
    mana: LandDetail,
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

    /// The queries this class can tell apart, by name, in grouping order.
    ///
    /// For the report: a class is identified by what it reads, and a bit
    /// number is not something a reader can check against their own file.
    pub fn queries<'a>(&self, full: &'a Grouping) -> Vec<&'a str> {
        full.queries()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.keep & (1u64 << i) != 0)
            .map(|(_, q)| q.as_str())
            .collect()
    }

    /// The pip kinds this class can tell lands apart by: `None` where nothing
    /// here prices mana, and the palette it kept where something does — which
    /// is empty for a cost that is all generic.
    pub fn pips(&self) -> Option<Palette> {
        match self.mana {
            LandDetail::Ignored => None,
            LandDetail::Pips(palette) => Some(palette),
        }
    }

    /// The turns whose counts this class reads, and how it reads them.
    pub fn turns(&self) -> &[usize] {
        &self.turns
    }

    pub fn reading(&self) -> Reading {
        self.reading
    }
}

/// What one question needs of an enumeration, once the walk's own bits are in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Need {
    keep: u64,
    mana: LandDetail,
    reading: Reading,
    turns: Vec<usize>,
}

impl Need {
    fn of(reads: &Reads, shared: &Shared) -> Need {
        // One land drop a turn is use-it-or-lose-it, so what is in play and
        // what could be paid are facts about the whole history rather than
        // about a total. A budget is the same fact twice over: the pool
        // refreshes every turn and a spell it paid for is gone from the hand,
        // so no total says what was cast. Everything else is a count of cards
        // seen.
        let history = reads.demands().is_some() || reads.battlefield() || shared.casting.is_some();
        let mut keep = reads.queries();
        // A live effect moves cards between zones, so every count in the run
        // depends on which cards it applies to and where it sends them. A
        // casting priority takes cards out of the hand, which is the same
        // argument with a different destination — so every class keeps it,
        // however little its own clauses care.
        keep |= shared.effects.map_or(0, |e| e.queries);
        keep |= shared.casting.map_or(0, |c| c.queries);
        // The land-drop priority only moves a number where something reads the
        // drops it made, where an effect fires off the land it chose, or where
        // a budget spends what it taps for.
        if history || shared.effects.is_some_and(|e| e.on_the_drop) {
            keep |= shared.land_drop.unwrap_or(0);
        }
        // What the class's own costs demand, joined with what the declared
        // line demands — because a budget reads the manabase for a reason the
        // clause never states, and a palette narrowed to the clause alone
        // would merge a blue source into the pile on a turn the line needed it.
        let demanded = match (reads.demands(), shared.casting) {
            (None, None) => None,
            (clause, line) => Some(
                clause
                    .unwrap_or(Palette::EMPTY)
                    .union(line.map_or(Palette::EMPTY, |c| c.demands)),
            ),
        };
        Need {
            keep,
            // A cost can only tell apart the colours it demands, so that is
            // all its enumeration keeps — unless something else in this run
            // gets to choose which land was played, and then the manabase is
            // being read for a reason no cost can state.
            mana: match demanded {
                None => LandDetail::Ignored,
                Some(_) if shared.picks_a_land() => LandDetail::Pips(Palette::ALL),
                Some(demanded) => LandDetail::Pips(demanded),
            },
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
    use pe_criteria::Policies;

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
        assert_eq!(classes[0].mana, LandDetail::Ignored);
        assert_eq!(classes[1].mana, LandDetail::Ignored);
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
        let full = Schedule::build(5, false, Vec::new(), Policies::default());
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
        // The cost is what can see a land's colour — and it sees exactly the
        // one it demands. A Plains, a Swamp and a Forest are one source to
        // `{1}{U}`: each pays a generic and none pays the pip.
        assert_eq!(
            classes[0].mana,
            LandDetail::Pips(Palette::from_letters(["U"]))
        );
        assert_eq!(classes[0].reading, Reading::PerTurn);
        assert_eq!(classes[1].mana, LandDetail::Ignored, "counting Ramp cannot");
        assert_eq!(classes[1].reading, Reading::Cumulative);
    }

    #[test]
    fn two_costs_in_one_question_join_their_demands() {
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "either half"
                require = [
                  { turn = 3, can_cast = "{1}{U}" },
                  { turn = 4, can_cast = "{B}{B}" },
                ]

                [[criterion]]
                name = "the other one"
                require = [{ turn = 4, can_cast = "{2}" }]
                "#,
            ),
            &Shared::default(),
        );
        // One enumeration answers both clauses of the first criterion, so it
        // has to tell blue from black from everything else.
        assert_eq!(
            classes[0].mana,
            LandDetail::Pips(Palette::from_letters(["UB"]))
        );
        // And a cost with no pips at all demands none: any two lands pay {2},
        // so the whole manabase is two groups — tapped and not.
        assert_eq!(classes[1].mana, LandDetail::Pips(Palette::EMPTY));
    }

    #[test]
    fn a_declared_priority_keeps_the_whole_palette() {
        // The negative control, and the crux of #55. A priority plays the
        // first land it is holding, ties going to the card the decklist names
        // first — so merging two lands it ranks differently changes which one
        // it played, and a `{1}{U}` question cannot merge a Plains with a
        // Swamp any more. The class keeps the whole palette instead.
        let shared = Shared {
            effects: None,
            land_drop: Some(bit(3)),
            casting: None,
        };
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "castable"
                require = [{ turn = 4, can_cast = "{1}{U}" }]
                "#,
            ),
            &shared,
        );
        assert_eq!(classes[0].mana, LandDetail::Pips(Palette::ALL));
        // And it keeps the priority's own queries, because they decide which
        // land was played.
        assert_eq!(classes[0].keep, bit(3));
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
        assert_eq!(classes[0].mana, LandDetail::Ignored);
        let full = Schedule::build(5, false, Vec::new(), Policies::default());
        assert_eq!(classes[0].schedule(&full).gaps(), &[7, 0, 1, 1, 1, 0]);
    }

    #[test]
    fn a_live_effect_is_kept_by_every_class() {
        let shared = Shared {
            effects: Some(Effects {
                queries: bit(9) | bit(10),
                on_the_drop: true,
            }),
            land_drop: Some(bit(11)),
            casting: None,
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
            casting: None,
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
    fn a_budget_is_kept_by_every_class_and_prices_the_manabase() {
        // The expensive half of #10, asserted rather than described. A class
        // counting Ramp cannot see a casting priority in its own clauses and
        // has to keep it anyway: the spells the line paid for are spells that
        // left the hand, so its own count depends on them — and on what the
        // lands make, because that is what decided whether they were paid for.
        let shared = Shared {
            effects: None,
            land_drop: None,
            casting: Some(Casting {
                queries: bit(7),
                demands: Palette::from_letters(["U"]),
            }),
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
        assert_eq!(classes[0].keep, bit(0) | bit(7));
        assert_eq!(
            classes[0].mana,
            LandDetail::Pips(Palette::from_letters(["U"])),
            "the colour comes from the card data, not from this criterion"
        );
        assert_eq!(classes[0].reading, Reading::PerTurn);
    }

    #[test]
    fn a_budget_joins_its_colours_with_the_clauses_own() {
        // Two sources of demand, one enumeration. The clause names black and
        // the declared line spends blue, and a grouping that kept either alone
        // would merge a land the other one needed to tell apart.
        let shared = Shared {
            effects: None,
            land_drop: None,
            casting: Some(Casting {
                queries: bit(7),
                demands: Palette::from_letters(["U"]),
            }),
        };
        let classes = partition(
            &reads_of(
                r#"
                [[criterion]]
                name = "castable"
                require = [{ turn = 4, can_cast = "{B}" }]
                "#,
            ),
            &shared,
        );
        assert_eq!(
            classes[0].mana,
            LandDetail::Pips(Palette::from_letters(["UB"]))
        );
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
