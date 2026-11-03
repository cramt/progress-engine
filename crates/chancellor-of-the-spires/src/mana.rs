//! Mana: its types (CR 106.1b), a pool of it (CR 106.4), the costs it pays
//! (CR 107.4, 202.1a), and the mana abilities that make it (CR 605), read
//! from Insight's trees.

use std::fmt;

use vivisurgeons_insight::ast as i;

use crate::lower;
use crate::model::{BasicLandType, Color, Filter};

/// One mana's type (CR 106.1b): a colour or colourless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mana {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl Mana {
    pub const ALL: [Mana; 6] = [
        Mana::White,
        Mana::Blue,
        Mana::Black,
        Mana::Red,
        Mana::Green,
        Mana::Colorless,
    ];

    /// What a basic land type's intrinsic ability adds (CR 305.6).
    pub fn of(land: BasicLandType) -> Mana {
        match land {
            BasicLandType::Plains => Mana::White,
            BasicLandType::Island => Mana::Blue,
            BasicLandType::Swamp => Mana::Black,
            BasicLandType::Mountain => Mana::Red,
            BasicLandType::Forest => Mana::Green,
        }
    }

    pub fn of_color(c: Color) -> Mana {
        match c {
            Color::White => Mana::White,
            Color::Blue => Mana::Blue,
            Color::Black => Mana::Black,
            Color::Red => Mana::Red,
            Color::Green => Mana::Green,
        }
    }

    /// `W`, `U`, `B`, `R`, `G`, `C` (CR 107.4a, 107.4c).
    fn symbol(s: &str) -> Option<Mana> {
        Some(match s {
            "W" => Mana::White,
            "U" => Mana::Blue,
            "B" => Mana::Black,
            "R" => Mana::Red,
            "G" => Mana::Green,
            "C" => Mana::Colorless,
            _ => return None,
        })
    }

    fn letter(self) -> char {
        match self {
            Mana::White => 'W',
            Mana::Blue => 'U',
            Mana::Black => 'B',
            Mana::Red => 'R',
            Mana::Green => 'G',
            Mana::Colorless => 'C',
        }
    }
}

/// An amount of each type of mana: a player's pool (CR 106.4), what an
/// ability adds, or what a payment spends from a pool.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManaPool([u32; 6]);

impl ManaPool {
    pub fn get(&self, m: Mana) -> u32 {
        self.0[m as usize]
    }

    pub fn total(&self) -> u32 {
        self.0.iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn add(&mut self, m: Mana) {
        self.0[m as usize] += 1;
    }

    pub fn add_all(&mut self, other: &ManaPool) {
        for m in Mana::ALL {
            self.0[m as usize] += other.get(m);
        }
    }

    pub fn of(mana: impl IntoIterator<Item = Mana>) -> ManaPool {
        let mut pool = ManaPool::default();
        for m in mana {
            pool.add(m);
        }
        pool
    }

    pub fn contains(&self, other: &ManaPool) -> bool {
        Mana::ALL.iter().all(|m| self.get(*m) >= other.get(*m))
    }

    /// `other` taken out; the caller has checked `contains`.
    pub fn minus(&self, other: &ManaPool) -> ManaPool {
        let mut left = *self;
        for m in Mana::ALL {
            left.0[m as usize] -= other.get(m);
        }
        left
    }
}

impl fmt::Display for ManaPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for m in Mana::ALL {
            for _ in 0..self.get(m) {
                write!(f, "{{{}}}", m.letter())?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for ManaPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ManaPool({self})")
    }
}

/// One mana symbol of a cost other than generic mana.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shard {
    /// `{G}`: that colour; `{C}`: colourless (CR 107.4a, 107.4c).
    One(Mana),
    /// `{G/U}`, and `{C/G}` with colourless as one half (CR 107.4e).
    Hybrid(Mana, Mana),
    /// `{2/G}`: one mana of that colour or two of any type (CR 107.4e).
    TwoOr(Mana),
}

/// A mana cost (CR 202.1a), or the mana part of one. `{X}`, Phyrexian and
/// snow mana aren't modelled, so a cost with them is refused when read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaCost {
    pub generic: u32,
    pub shards: Vec<Shard>,
}

impl ManaCost {
    /// A printed mana cost such as `{1}{G}{G}`, or `None` for a card with
    /// none (CR 202.1b).
    pub fn parse(printed: &str) -> Result<Option<ManaCost>, String> {
        if printed.trim().is_empty() {
            return Ok(None);
        }
        let symbols: Vec<String> = printed
            .split('}')
            .filter_map(|s| s.trim().strip_prefix('{'))
            .map(str::to_string)
            .collect();
        ManaCost::from_symbols(&i::Mana(symbols)).map(Some)
    }

    /// Insight's symbols, as printed inside each `{…}`.
    pub fn from_symbols(mana: &i::Mana) -> Result<ManaCost, String> {
        let mut cost = ManaCost::default();
        for s in &mana.0 {
            if let Ok(n) = s.parse::<u32>() {
                cost.generic += n;
            } else if let Some(m) = Mana::symbol(s) {
                cost.shards.push(Shard::One(m));
            } else if s == "X" || s == "Y" || s == "Z" {
                return Err(format!("{{{s}}} in a cost (CR 107.3)"));
            } else if s == "S" {
                return Err("snow mana in a cost (CR 107.4h)".into());
            } else if s.ends_with("/P") || s == "H" {
                return Err("Phyrexian mana in a cost (CR 107.4f)".into());
            } else if let Some((a, b)) = s.split_once('/') {
                cost.shards
                    .push(match (a, Mana::symbol(a), Mana::symbol(b)) {
                        ("2", _, Some(m)) if m != Mana::Colorless => Shard::TwoOr(m),
                        (_, Some(x), Some(y)) if x != y && y != Mana::Colorless => {
                            Shard::Hybrid(x, y)
                        }
                        _ => return Err(format!("the mana symbol {{{s}}}")),
                    });
            } else {
                return Err(format!("the mana symbol {{{s}}}"));
            }
        }
        Ok(cost)
    }

    /// This cost and `n` more generic mana, as commander tax adds (CR 903.8).
    pub fn plus_generic(&self, n: u32) -> ManaCost {
        ManaCost {
            generic: self.generic + n,
            shards: self.shards.clone(),
        }
    }

    /// Whether spending exactly this mana pays the cost: coloured and
    /// colourless symbols matched by type, generic by any (CR 202.1a,
    /// 107.4b), and nothing left over.
    pub fn paid_by(&self, spent: &ManaPool) -> bool {
        fn fits(shards: &[Shard], left: ManaPool, generic: u32) -> bool {
            let Some((s, rest)) = shards.split_first() else {
                return left.total() == generic;
            };
            let take = |m: Mana| {
                let one = ManaPool::of([m]);
                left.contains(&one) && fits(rest, left.minus(&one), generic)
            };
            match *s {
                Shard::One(m) => take(m),
                Shard::Hybrid(a, b) => take(a) || take(b),
                Shard::TwoOr(m) => take(m) || fits(rest, left, generic + 2),
            }
        }
        fits(&self.shards, *spent, self.generic)
    }

    /// Every amount of the pool's mana that pays the cost exactly. Which
    /// mana paid which symbol changes nothing the engine models, so a
    /// payment is the mana spent.
    pub fn payments(&self, pool: &ManaPool) -> Vec<ManaPool> {
        let mut out = Vec::new();
        let mut spent = ManaPool::default();
        fn each(
            cost: &ManaCost,
            pool: &ManaPool,
            m: usize,
            spent: &mut ManaPool,
            out: &mut Vec<ManaPool>,
        ) {
            if m == Mana::ALL.len() {
                if cost.paid_by(spent) {
                    out.push(*spent);
                }
                return;
            }
            for n in 0..=pool.0[m] {
                spent.0[m] = n;
                each(cost, pool, m + 1, spent, out);
            }
            spent.0[m] = 0;
        }
        each(self, pool, 0, &mut spent, &mut out);
        out
    }
}

impl fmt::Display for ManaCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.generic > 0 || self.shards.is_empty() {
            write!(f, "{{{}}}", self.generic)?;
        }
        for s in &self.shards {
            match s {
                Shard::One(m) => write!(f, "{{{}}}", m.letter())?,
                Shard::Hybrid(a, b) => write!(f, "{{{}/{}}}", a.letter(), b.letter())?,
                Shard::TwoOr(m) => write!(f, "{{2/{}}}", m.letter())?,
            }
        }
        Ok(())
    }
}

/// What a mana ability adds, as written; which of its options the
/// activating player takes is chosen as it resolves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Adds {
    /// "Add {C}", "Add {R} or {G}", "Add {U}{R}".
    Options(Vec<ManaPool>),
    /// "Add one mana of any color" (CR 105.1).
    AnyColor,
    /// "Add one mana of any color in your commander's color identity" (CR
    /// 903.4): none if you have no commander (CR 903.4f).
    CommanderIdentity,
    /// "Add {G} for each creature you control": `each` once per permanent
    /// the filter matches, counted as the ability resolves (CR 608.2h),
    /// which for a mana ability is as it is activated (CR 605.3b). The
    /// filter is read from the source, so "you" is its controller.
    ForEach { each: ManaPool, filter: Filter },
}

/// An activated mana ability the engine can run (CR 605.1a): its cost is
/// tapping its source, paying life and paying mana, and it adds mana,
/// maybe dealing its controller damage too, as a painland does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManaAbility {
    pub tap: bool,
    /// CR 119.4.
    pub life: u32,
    /// "{1}, {T}: Add {R}{G}", a filter land's "{U/R}, {T}": mana paid
    /// from its controller's pool as it is activated (CR 605.3a, 601.2h).
    pub mana: Option<ManaCost>,
    pub adds: Adds,
    /// "This land deals 1 damage to you."
    pub damage_to_you: u32,
}

impl ManaAbility {
    /// The mana each way of activating it can add, for a player with this
    /// commander colour identity (`None`: no commander), with `count`
    /// saying how many permanents a filter matches now.
    pub fn options(
        &self,
        identity: Option<&[Color]>,
        count: impl Fn(&Filter) -> u32,
    ) -> Vec<ManaPool> {
        match &self.adds {
            Adds::Options(o) => o.clone(),
            Adds::ForEach { each, filter } => {
                let n = count(filter);
                let mut pool = ManaPool::default();
                for _ in 0..n {
                    pool.add_all(each);
                }
                vec![pool]
            }
            Adds::AnyColor => [
                Color::White,
                Color::Blue,
                Color::Black,
                Color::Red,
                Color::Green,
            ]
            .into_iter()
            .map(|c| ManaPool::of([Mana::of_color(c)]))
            .collect(),
            Adds::CommanderIdentity => identity
                .unwrap_or(&[])
                .iter()
                .map(|c| ManaPool::of([Mana::of_color(*c)]))
                .collect(),
        }
    }
}

/// An activated ability read as a mana ability: `None` if it isn't one
/// (it adds no mana, CR 605.1a), or the node the engine can't run. One with
/// a target is under a `Let` and so isn't an `Activated` here, which is
/// right: it isn't a mana ability (CR 605.5a).
pub fn mana_ability(a: &i::Ability) -> Option<Result<ManaAbility, String>> {
    let i::Ability::Activated {
        cost,
        effect,
        limits,
    } = a
    else {
        return None;
    };
    let adds_mana = |e: &i::Effect| match e {
        i::Effect::AddMana(_) => true,
        i::Effect::ForEach { then, .. } => matches!(then.as_slice(), [i::Effect::AddMana(_)]),
        _ => false,
    };
    if !effect.iter().any(adds_mana) {
        return None;
    }
    Some(lower_mana_ability(cost, effect, limits))
}

fn lower_mana_ability(
    cost: &[i::Cost],
    effect: &[i::Effect],
    limits: &[i::Limit],
) -> Result<ManaAbility, String> {
    if let Some(l) = limits.first() {
        return Err(format!("a mana ability limited by {}", short(l)));
    }
    let mut ability = ManaAbility {
        tap: false,
        life: 0,
        mana: None,
        adds: Adds::Options(Vec::new()),
        damage_to_you: 0,
    };
    for c in cost {
        match c {
            i::Cost::Tap => ability.tap = true,
            i::Cost::PayLife(i::Amount::N(n)) => ability.life += n,
            i::Cost::Mana(m) if ability.mana.is_none() => {
                ability.mana = Some(ManaCost::from_symbols(m)?)
            }
            other => return Err(format!("a mana ability costing {}", short(other))),
        }
    }
    let [add, rest @ ..] = effect else {
        return Err("an empty mana ability".into());
    };
    ability.adds = match add {
        i::Effect::AddMana(add) => adds(add)?,
        // "Add {G} for each creature you control" (Gaea's Cradle).
        i::Effect::ForEach { each, then, .. } => match then.as_slice() {
            [i::Effect::AddMana(add)] => match adds(add)? {
                Adds::Options(o) if o.len() == 1 => Adds::ForEach {
                    each: o[0],
                    filter: lower::counted(each)?,
                },
                _ => return Err("adding a choice of mana for each".into()),
            },
            _ => return Err("a mana ability doing more for each".into()),
        },
        other => return Err(format!("a mana ability that does {} first", short(other))),
    };
    for e in rest {
        match e {
            i::Effect::Damage {
                source: Some(source),
                n: i::Amount::N(n),
                to,
            } if source.quant == i::Quant::SelfRef
                && source.filter == i::Filter::default()
                && matches!(to.as_slice(), [i::Recipient::Player(i::Player::You)]) =>
            {
                ability.damage_to_you += n
            }
            other => return Err(format!("a mana ability that also does {}", short(other))),
        }
    }
    Ok(ability)
}

pub(crate) fn adds(add: &i::ManaAdd) -> Result<Adds, String> {
    let pool = |m: &i::Mana| -> Result<ManaPool, String> {
        m.0.iter()
            .map(|s| Mana::symbol(s).ok_or_else(|| format!("adding {{{s}}}")))
            .collect::<Result<Vec<_>, _>>()
            .map(ManaPool::of)
    };
    Ok(match add {
        i::ManaAdd::Symbols(m) => Adds::Options(vec![pool(m)?]),
        i::ManaAdd::Or(ms) => Adds::Options(ms.iter().map(pool).collect::<Result<_, _>>()?),
        i::ManaAdd::Of {
            n: i::Amount::N(1),
            pick: i::Pick::AnyColor | i::Pick::AnyOneColor,
            within,
        } => match within {
            None => Adds::AnyColor,
            Some(i::Palette::CommanderIdentity) => Adds::CommanderIdentity,
            Some(other) => return Err(format!("mana of a color {}", short(other))),
        },
        other => return Err(format!("adding mana by {}", short(other))),
    })
}

pub(crate) fn short(node: &dyn fmt::Debug) -> String {
    let s = format!("{node:?}");
    let end = s.find(['(', '{', ' ']).unwrap_or(s.len());
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(s: &str) -> ManaCost {
        ManaCost::parse(s).unwrap().unwrap()
    }

    /// CR 202.1a, 107.4b, 107.4e.
    #[test]
    fn a_cost_is_paid_by_matching_types_and_any_mana_for_generic() {
        let g = |n| ManaPool::of(std::iter::repeat_n(Mana::Green, n));
        assert!(cost("{1}{G}").paid_by(&g(2)));
        assert!(!cost("{1}{G}").paid_by(&g(3)), "exactly, nothing over");
        assert!(!cost("{G}{U}").paid_by(&g(2)));
        assert!(cost("{G/U}").paid_by(&ManaPool::of([Mana::Blue])));
        assert!(cost("{2/R}").paid_by(&g(2)));
        assert!(cost("{2/R}").paid_by(&ManaPool::of([Mana::Red])));
        assert!(!cost("{C}").paid_by(&g(1)));
        assert_eq!(ManaCost::parse("").unwrap(), None);
        assert!(ManaCost::parse("{X}{G}").is_err());
        assert!(ManaCost::parse("{G/P}").is_err());
        let pool = ManaPool::of([Mana::Green, Mana::Green, Mana::Red]);
        // {1}{G} from {G}{G}{R}: {G}{G} or {R}{G}.
        assert_eq!(cost("{1}{G}").payments(&pool).len(), 2);
    }
}
