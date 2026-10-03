//! A class, lowered to the flat arrays the kernel reads, or the reason it is
//! not a count-only walk.

use gauntlet_cli::spike::Class;
use gauntlet_criteria::{Counted, Zone};
use gauntlet_toml::Criteria;

/// Groups are a `u32` bitmask per clause in the kernel.
pub const MAX_GROUPS: usize = 32;

/// Where a clause reads its count. Numbered as the kernel matches on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Reads {
    Hand = 0,
    /// Everything the query matches, less the hand: nothing else has left.
    Library = 1,
    /// The undeclared land drop's recurrence, `Board::played_by`.
    Battlefield = 2,
    /// A zone no count-only walk can put a card in, or a turn past the run.
    Nothing = 3,
}

pub struct Lowered {
    pub sizes: Vec<u32>,
    pub gaps: Vec<u32>,
    /// Per clause: checkpoint, [`Reads`], group mask, min, max, query total.
    pub clauses: Vec<[u32; 6]>,
    /// Where each conjunction's clauses end, into `clauses`.
    pub conj_end: Vec<u32>,
    /// Where each criterion's conjunctions end, into `conj_end`.
    pub crit_end: Vec<u32>,
    /// The file's index of each criterion, parallel to `crit_end`.
    pub criteria: Vec<usize>,
    /// Whether any clause reads the battlefield, the one branch this port has.
    pub land_drops: bool,
}

pub fn lower(class: &Class, criteria: &Criteria) -> Result<Lowered, String> {
    let s = &class.schedule;
    let why = [
        (!s.effects().is_empty(), "a live effect"),
        (s.casting().is_some(), "a casting line"),
        (s.land_drop().is_some(), "a declared land drop"),
        (s.mulligan().is_some(), "a mulligan"),
        (s.chosen().is_some(), "a chosen strategy"),
        (s.discard().is_some(), "a discard priority"),
        (class.priced, "a priced cost"),
        (class.answering.criteria().is_empty(), "no criterion"),
    ];
    if let Some((_, what)) = why.iter().find(|(yes, _)| *yes) {
        return Err(format!("not count-only: {what}"));
    }
    let gaps = s.gaps().to_vec();
    // One checkpoint a turn is what lets the kernel index its history by turn.
    if s.turns() != gaps.len() || (0..s.turns()).any(|t| s.checkpoints_of(t) != (t, t)) {
        return Err("not count-only: a turn spans several checkpoints".into());
    }
    let g = &class.grouping;
    let sizes = g.group_sizes().to_vec();
    if sizes.len() > MAX_GROUPS {
        return Err(format!(
            "{} groups, the kernel holds {MAX_GROUPS}",
            sizes.len()
        ));
    }

    let mut lowered = Lowered {
        sizes,
        gaps,
        clauses: Vec::new(),
        conj_end: Vec::new(),
        crit_end: Vec::new(),
        criteria: class.answering.criteria().to_vec(),
        land_drops: false,
    };
    for &i in class.answering.criteria() {
        let dnf = criteria
            .counts_of(i)
            .ok_or("not count-only: a criterion asks whether a cost is payable")?;
        for conj in dnf {
            for c in conj {
                let reads = match c.counted {
                    _ if c.turn >= lowered.gaps.len() => Reads::Nothing,
                    Counted::In(Zone::Hand) => Reads::Hand,
                    Counted::In(Zone::Library) => Reads::Library,
                    Counted::In(Zone::Battlefield) => Reads::Battlefield,
                    Counted::In(Zone::Graveyard) | Counted::Cast => Reads::Nothing,
                };
                lowered.land_drops |= reads == Reads::Battlefield;
                let mask = g.members(c.query).iter().fold(0u32, |m, &x| m | (1 << x));
                lowered.clauses.push([
                    c.turn as u32,
                    reads as u32,
                    mask,
                    c.min,
                    c.max,
                    g.matching_total(c.query),
                ]);
            }
            lowered.conj_end.push(lowered.clauses.len() as u32);
        }
        lowered.crit_end.push(lowered.conj_end.len() as u32);
    }
    Ok(lowered)
}
