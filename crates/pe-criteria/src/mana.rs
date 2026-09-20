//! Mana as a gate: could this have been paid for, by this turn.
//!
//! This is half of the mana model and the cheap half. It answers *could I have
//! cast a `{1}{W}{U}` three-drop on turn three* and nothing about what casting
//! it would then have cost you, because a gate is a predicate over the lands
//! you were able to play and a budget is a resource that spending depletes. The
//! budget needs sequencing and an effect that draws makes *cards seen by turn
//! T* path-dependent; the gate is a function of the path the enumeration
//! already walks. See
//! [issue #10](https://github.com/cramt/progress-engine/issues/10).
//!
//! The reason castability is a primitive rather than something a criteria file
//! assembles for itself is [`Cost::payable`]. One Hallowed Fountain counts
//! toward `produces:w` and toward `produces:u`, so two clauses asking for one
//! of each are both satisfied by a hand that cannot pay `{W}{U}` — HANDS.md
//! hand 6. Whether a set of sources can cover a set of pips is a bipartite
//! matching, no arithmetic over independent counts answers it, and the engine
//! can solve it exactly per composition because a composition knows the sources
//! jointly.

use thiserror::Error;

/// One kind of mana symbol a cost can demand.
///
/// Colourless is a demand like any colour — `{C}` needs a source that makes
/// `{C}` — and generic is not here, because generic is not a kind of mana. It
/// is a number of sources of no kind at all, which is why [`Cost`] keeps it as
/// a count rather than as a sixth pip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pip {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl Pip {
    /// Every pip, in the order [`Cost`] indexes them.
    pub const ALL: [Pip; 6] = [
        Pip::White,
        Pip::Blue,
        Pip::Black,
        Pip::Red,
        Pip::Green,
        Pip::Colorless,
    ];

    fn bit(self) -> u8 {
        1 << (self as u8)
    }

    fn from_letter(c: char) -> Option<Pip> {
        match c.to_ascii_uppercase() {
            'W' => Some(Pip::White),
            'U' => Some(Pip::Blue),
            'B' => Some(Pip::Black),
            'R' => Some(Pip::Red),
            'G' => Some(Pip::Green),
            'C' => Some(Pip::Colorless),
            _ => None,
        }
    }
}

/// The kinds of mana one source can make.
///
/// A set rather than a colour, because a dual land makes either and the whole
/// point of the matching is that it makes only one of them at a time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Palette(u8);

impl Palette {
    pub const EMPTY: Palette = Palette(0);

    /// Read a palette from Scryfall's `produced_mana` letters.
    ///
    /// Anything that is not a WUBRG or C letter is dropped rather than
    /// refused. Scryfall lists `{S}` snow and `{E}` energy there too, and
    /// neither pays a mana cost — a source that makes only those makes no mana
    /// for this model, which is the honest reading rather than a silent
    /// upgrade.
    pub fn from_letters<S: AsRef<str>>(letters: impl IntoIterator<Item = S>) -> Palette {
        let mut bits = 0;
        for letter in letters {
            for c in letter.as_ref().chars() {
                if let Some(pip) = Pip::from_letter(c) {
                    bits |= pip.bit();
                }
            }
        }
        Palette(bits)
    }

    pub fn makes(self, pip: Pip) -> bool {
        self.0 & pip.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether this source can pay any pip in `demand`.
    fn serves(self, demand: PipSet) -> bool {
        self.0 & demand.0 != 0
    }
}

/// A set of pip kinds, used to walk the subsets Hall's condition asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PipSet(u8);

/// What one card in the library does for mana before anything is cast.
///
/// Two variants, and the missing third is the point. A Sol Ring makes mana and
/// is not here, because getting it onto the battlefield costs mana — that is
/// the budget half, and a variant for it would be this type promising an answer
/// the engine does not have. A land arrives on a land drop, which is free and
/// capped at one a turn, and that cap is the whole reason the gate is cheap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ManaSource {
    /// Anything you would have to cast. Contributes nothing to the gate.
    #[default]
    Spell,
    /// A land: one drop a turn, free.
    Land {
        /// Whether it makes no mana on the turn it arrives.
        ///
        /// A decision rather than a property for a shockland, which is
        /// HANDS.md hand 8 — and the decision is made where the card data is,
        /// not here, so that whatever is assumed is assumed once and said out
        /// loud in the run that depends on it.
        enters_tapped: bool,
        produces: Palette,
    },
}

impl ManaSource {
    pub fn is_land(self) -> bool {
        matches!(self, ManaSource::Land { .. })
    }

    fn enters_tapped(self) -> bool {
        matches!(
            self,
            ManaSource::Land {
                enters_tapped: true,
                ..
            }
        )
    }

    fn palette(self) -> Palette {
        match self {
            ManaSource::Spell => Palette::EMPTY,
            ManaSource::Land { produces, .. } => produces,
        }
    }
}

/// A mana cost, as a demand waiting to be paid.
///
/// Not [`pe_scryfall::mana::ManaCost`]'s job and deliberately not its type. That
/// one is a multiset for comparing costs — is `{U/W}` the same symbol as
/// `{W/U}`, is this cost greater than that one — and it accepts every symbol
/// that has ever been printed because a query has to be able to ask about them.
/// This one is a bill, and a symbol it cannot work out how to pay is refused by
/// name rather than accepted and quietly ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cost {
    /// How many sources of no particular kind this needs.
    generic: u32,
    /// Demand per pip, indexed by `Pip as usize`.
    pips: [u32; 6],
    /// Sources needed in total: generic plus every pip. A payment is exactly
    /// this many lands, no more and no fewer.
    total: u32,
    /// The pip kinds actually demanded.
    ///
    /// Precomputed because [`Cost::payable`] walks the subsets of this and
    /// nothing else: a subset holding a colour this cost never asks for adds
    /// supply without adding demand, so it can only be satisfied when the
    /// subset without it already was.
    demanded: Vec<Pip>,
    /// How it was written, for the report and the errors.
    text: String,
}

/// A mana cost this engine will not try to pay.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CostError {
    #[error(
        "`{text}` is not a mana cost: it has no symbols at all.\n\
         Write it as it is printed on the card, e.g. `{{1}}{{W}}{{U}}`."
    )]
    Empty { text: String },
    /// Refused by name rather than approximated, which is the whole discipline
    /// here: a hybrid pip read as generic makes a cost cheaper than it is, and
    /// an `{X}` read as zero makes an X-spell castable on turn one.
    #[error(
        "`{{{symbol}}}` in `{text}` is not a symbol this engine can pay.\n\
         The gate pays generic, {{W}} {{U}} {{B}} {{R}} {{G}} and {{C}}. Hybrid, Phyrexian and \
         {{X}} are each a decision about how much to pay rather than an amount, and guessing \
         one would report a cost the deck was never asked for."
    )]
    Unpayable { text: String, symbol: String },
    #[error(
        "`{text}` costs {total} mana, which is more than the {MAX_COST} this engine will look \
         for. Nothing is castable off that many land drops inside a run this tool can enumerate."
    )]
    TooLarge { text: String, total: u32 },
}

/// The largest cost the gate will accept.
///
/// A bound on the work rather than a rule of the game, in the same spirit as
/// `MAX_TURN`: the gate needs one land drop per mana, so a cost above this
/// cannot be paid before a turn any criteria file may name.
pub const MAX_COST: u32 = 100;

impl Cost {
    /// Read a cost as a card prints it: `{1}{W}{U}`.
    ///
    /// The unbraced shorthand a Scryfall query may write — `1WU` — is accepted
    /// too, because the same string is going to be pasted from both places and
    /// one of them refusing would be a distinction nobody asked for.
    pub fn parse(text: &str) -> Result<Cost, CostError> {
        let mut cost = Cost {
            generic: 0,
            pips: [0; 6],
            total: 0,
            demanded: Vec::new(),
            text: text.to_string(),
        };
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let mut saw_symbol = false;
        while i < chars.len() {
            match chars[i] {
                c if c.is_whitespace() => i += 1,
                '{' => {
                    let start = i + 1;
                    let end = chars[start..]
                        .iter()
                        .position(|c| *c == '}')
                        .map_or(chars.len(), |p| start + p);
                    let symbol: String = chars[start..end].iter().collect();
                    cost.add(&symbol, text)?;
                    saw_symbol = true;
                    i = end + 1;
                }
                c if c.is_ascii_digit() => {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                    let digits: String = chars[start..i].iter().collect();
                    cost.add(&digits, text)?;
                    saw_symbol = true;
                }
                c => {
                    cost.add(&c.to_string(), text)?;
                    saw_symbol = true;
                    i += 1;
                }
            }
        }
        if !saw_symbol {
            return Err(CostError::Empty {
                text: text.to_string(),
            });
        }
        cost.total = cost.generic + cost.pips.iter().sum::<u32>();
        if cost.total > MAX_COST {
            return Err(CostError::TooLarge {
                text: text.to_string(),
                total: cost.total,
            });
        }
        cost.demanded = Pip::ALL
            .into_iter()
            .filter(|p| cost.pips[*p as usize] > 0)
            .collect();
        Ok(cost)
    }

    fn add(&mut self, symbol: &str, text: &str) -> Result<(), CostError> {
        if let Ok(n) = symbol.parse::<u32>() {
            self.generic = self.generic.saturating_add(n);
            return Ok(());
        }
        let mut letters = symbol.chars();
        match (letters.next().and_then(Pip::from_letter), letters.next()) {
            (Some(pip), None) => {
                self.pips[pip as usize] += 1;
                Ok(())
            }
            _ => Err(CostError::Unpayable {
                text: text.to_string(),
                symbol: symbol.to_string(),
            }),
        }
    }

    /// How many sources paying this needs. A payment uses exactly this many.
    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn is_free(&self) -> bool {
        self.total == 0
    }

    /// The cost as it was written, for a report that has to name it.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether `sources` can cover this cost, with `constraint` on which of
    /// them the payment has to use.
    ///
    /// Hall's condition, over the pip kinds this cost demands and nothing else.
    /// A payment exists exactly when every subset of the demanded kinds has at
    /// least as many sources serving it as it has pips, and there are enough
    /// sources left over for the generic — the first half is the matching, the
    /// second is that generic takes any source at all, so it is a count.
    ///
    /// `sources` is one entry per group of interchangeable cards rather than
    /// one per card, which is what keeps this cheap enough to run on every path
    /// of an enumeration: the subsets are at most 63 and the groups are however
    /// many distinct kinds of land the deck plays. `count` says how many of
    /// each are available, as a function rather than a second slice so that the
    /// caller answering it from the path it just walked needs no buffer of its
    /// own — this runs once per criterion per path, and a scratch allocation
    /// there costs more than the matching does.
    pub fn payable(
        &self,
        sources: &[Source],
        count: impl Fn(usize) -> u32,
        constraint: Constraint,
    ) -> bool {
        match constraint {
            Constraint::Anything => self.covers(sources, &count, None),
            Constraint::Includes(group) => {
                self.forced(sources, &count, |i, _| i == group && count(i) > 0)
            }
            Constraint::IncludesUntapped => {
                self.forced(sources, &count, |i, s: &Source| count(i) > 0 && !s.tapped)
            }
        }
    }

    /// The matching, with one source optionally already spoken for.
    ///
    /// `spent` is a source that has been assigned away: its land is out of the
    /// pool and, where it was assigned to a pip rather than to generic, that
    /// pip's demand is one lower.
    fn covers(
        &self,
        sources: &[Source],
        count: &impl Fn(usize) -> u32,
        spent: Option<(usize, Option<Pip>)>,
    ) -> bool {
        let count_of = |i: usize| {
            let n = count(i);
            match spent {
                Some((g, _)) if g == i => n - 1,
                _ => n,
            }
        };
        let demand_of = |pip: Pip| {
            let d = self.pips[pip as usize];
            match spent {
                Some((_, Some(p))) if p == pip => d - 1,
                _ => d,
            }
        };
        let mut available = 0u32;
        for i in 0..sources.len() {
            available += count_of(i);
        }
        let paid = u32::from(spent.is_some());
        if available + paid < self.total {
            return false;
        }
        // Every non-empty subset of the kinds this cost demands. Hall's
        // condition on any other subset is implied: adding an undemanded kind
        // adds sources that serve it without adding a pip to cover.
        let kinds = self.demanded.len();
        for mask in 1u32..(1 << kinds) {
            let mut demand = 0u32;
            let mut set = PipSet(0);
            for (bit, pip) in self.demanded.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    demand += demand_of(*pip);
                    set.0 |= pip.bit();
                }
            }
            let mut supply = 0u32;
            for (i, source) in sources.iter().enumerate() {
                if source.produces.serves(set) {
                    supply += count_of(i);
                }
            }
            if supply < demand {
                return false;
            }
        }
        true
    }

    /// Whether a payment exists that uses at least one source `wanted` accepts.
    ///
    /// Where the cost has generic to pay, this is just the matching plus the
    /// existence of such a source: generic takes any land, so a payment that
    /// did not use it can swap one of its generic payers for it. Where the cost
    /// is all pips, the wanted source has to be paying one of them, so each way
    /// it could is tried — at most one per pip kind it produces.
    fn forced(
        &self,
        sources: &[Source],
        count: &impl Fn(usize) -> u32,
        wanted: impl Fn(usize, &Source) -> bool,
    ) -> bool {
        let mut any_eligible = false;
        for (i, source) in sources.iter().enumerate() {
            if !wanted(i, source) {
                continue;
            }
            any_eligible = true;
            if self.generic > 0 {
                // Nothing more to check. A payment that did not use this source
                // can be made to: something in it is paying generic, generic
                // takes any land, so swapping that land for this one pays the
                // same cost. The obligation costs nothing beyond one of these
                // existing at all.
                break;
            }
            // All pips, so the obliged source has to be paying one of them.
            for pip in &self.demanded {
                if source.produces.makes(*pip) && self.covers(sources, count, Some((i, Some(*pip))))
                {
                    return true;
                }
            }
        }
        any_eligible && self.generic > 0 && self.covers(sources, count, None)
    }
}

/// One group of interchangeable mana sources, as the matching sees them.
///
/// No count: how many of these are available changes on every path of the
/// enumeration, and what they produce does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub produces: Palette,
    /// Whether a land of this group makes no mana on the turn it arrives.
    pub tapped: bool,
}

/// Which sources a payment is obliged to use.
///
/// The obligations are about the land drop made on the turn being asked about,
/// which is the only land that can still be tapped: a land played earlier has
/// untapped by now, whatever it did on the way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    /// No obligation: the payment can come from anywhere in the pool.
    Anything,
    /// The payment must use a land of this group, because that land is the one
    /// played this turn and nothing else could have been.
    Includes(usize),
    /// The payment must use some land that does not enter tapped, because one
    /// of the lands paying it is being played this turn.
    IncludesUntapped,
}

impl Source {
    pub fn of(mana: ManaSource) -> Source {
        Source {
            produces: mana.palette(),
            tapped: mana.enters_tapped(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool as `(what it makes, does it enter tapped, how many)`.
    fn pool(sources: &[(&str, bool, u32)]) -> (Vec<Source>, Vec<u32>) {
        sources
            .iter()
            .map(|(letters, tapped, count)| {
                (
                    Source {
                        produces: Palette::from_letters([*letters]),
                        tapped: *tapped,
                    },
                    *count,
                )
            })
            .unzip()
    }

    fn pays(cost: &Cost, sources: &[(&str, bool, u32)], constraint: Constraint) -> bool {
        let (kinds, counts) = pool(sources);
        cost.payable(&kinds, |i| counts[i], constraint)
    }

    /// HANDS.md hand 6 and hand 7 as the matching sees them: the same query
    /// counts, the opposite answer.
    #[test]
    fn one_dual_land_does_not_pay_two_pips_and_two_lands_do() {
        let cost = Cost::parse("{W}{U}").unwrap();
        let fountain: &[(&str, bool, u32)] = &[("WU", false, 1)];
        let with_island: &[(&str, bool, u32)] = &[("WU", false, 1), ("U", false, 1)];
        // Both hands hold at least one white source and at least one blue
        // source, so a conjunction of independent counts holds on both.
        let (kinds, _) = pool(fountain);
        assert!(kinds[0].produces.makes(Pip::White));
        assert!(kinds[0].produces.makes(Pip::Blue));
        assert!(!pays(&cost, fountain, Constraint::Anything));
        assert!(pays(&cost, with_island, Constraint::Anything));
    }

    #[test]
    fn generic_takes_any_source_and_pips_do_not() {
        let cost = Cost::parse("{1}{W}").unwrap();
        assert!(pays(
            &cost,
            &[("W", false, 1), ("U", false, 1)],
            Constraint::Anything
        ));
        assert!(!pays(&cost, &[("U", false, 2)], Constraint::Anything));
        // Colourless is a demand of its own: a Forest does not pay {C}.
        let colorless = Cost::parse("{C}").unwrap();
        assert!(!pays(&colorless, &[("G", false, 3)], Constraint::Anything));
        assert!(pays(&colorless, &[("C", false, 1)], Constraint::Anything));
    }

    #[test]
    fn a_payment_can_be_made_to_use_a_named_source() {
        let cost = Cost::parse("{W}{U}").unwrap();
        let sources: &[(&str, bool, u32)] = &[("WU", false, 1), ("U", false, 1)];
        // Forced through the Island, which can only pay the blue pip.
        assert!(pays(&cost, sources, Constraint::Includes(1)));
        // Forced through a group with nothing in it.
        assert!(!pays(
            &cost,
            &[("WU", false, 2), ("U", false, 0)],
            Constraint::Includes(1)
        ));
        // Two Hallowed Fountains do pay it, and forcing the payment through a
        // source that enters untapped does not: both of them enter tapped.
        assert!(pays(&cost, &[("WU", true, 2)], Constraint::Anything));
        assert!(!pays(
            &cost,
            &[("WU", true, 2)],
            Constraint::IncludesUntapped
        ));
        // A cost with generic in it only needs such a source to exist, because
        // generic takes any land at all.
        let one_and_a_white = Cost::parse("{1}{W}").unwrap();
        assert!(pays(
            &one_and_a_white,
            &[("W", true, 1), ("U", false, 1)],
            Constraint::IncludesUntapped
        ));
    }

    #[test]
    fn a_symbol_the_gate_cannot_pay_is_refused_by_name() {
        for text in ["{X}{U}", "{W/U}", "{2/W}", "{U/P}"] {
            let err = Cost::parse(text).unwrap_err();
            assert!(
                matches!(err, CostError::Unpayable { .. }),
                "{text} should be refused: {err}"
            );
        }
        assert!(matches!(
            Cost::parse("").unwrap_err(),
            CostError::Empty { .. }
        ));
        assert_eq!(Cost::parse("{0}").unwrap().total(), 0);
    }

    #[test]
    fn both_spellings_of_a_cost_read_the_same() {
        let braced = Cost::parse("{1}{W}{U}").unwrap();
        let shorthand = Cost::parse("1WU").unwrap();
        assert_eq!(braced.total(), shorthand.total());
        assert_eq!(braced.pips, shorthand.pips);
        assert_eq!(braced.generic, shorthand.generic);
    }
}
