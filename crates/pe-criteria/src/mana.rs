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

    fn letter(self) -> char {
        match self {
            Pip::White => 'W',
            Pip::Blue => 'U',
            Pip::Black => 'B',
            Pip::Red => 'R',
            Pip::Green => 'G',
            Pip::Colorless => 'C',
        }
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

    /// Every kind of mana a source can make, which is what a grouping keeps
    /// when nothing has proved it may keep less.
    ///
    /// Spelled out rather than folded from [`Pip::ALL`], which is not
    /// something a `const` can do — so a test asserts the two agree, and a
    /// seventh pip cannot leave this one behind.
    pub const ALL: Palette = Palette(0b0011_1111);

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

    /// A palette holding exactly `pips`.
    pub fn of(pips: impl IntoIterator<Item = Pip>) -> Palette {
        Palette(pips.into_iter().fold(0, |bits, pip| bits | pip.bit()))
    }

    pub fn makes(self, pip: Pip) -> bool {
        self.0 & pip.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Both palettes at once: what this source makes, as far as `other` can
    /// see it.
    pub fn intersect(self, other: Palette) -> Palette {
        Palette(self.0 & other.0)
    }

    /// The union, which is how a class joins what several costs demand.
    pub fn union(self, other: Palette) -> Palette {
        Palette(self.0 | other.0)
    }

    /// One symbol per kind, as a cost writes them: `["{W}", "{U}"]`, and
    /// empty for a palette that makes nothing. For the report, which has to
    /// name what an enumeration could tell apart.
    pub fn symbols(self) -> Vec<String> {
        Pip::ALL
            .into_iter()
            .filter(|p| self.makes(*p))
            .map(|p| format!("{{{}}}", p.letter()))
            .collect()
    }

    /// Whether this source can pay any pip in `demand`.
    fn serves(self, demand: PipSet) -> bool {
        self.0 & demand.0 != 0
    }
}

/// A set of pip kinds, used to walk the subsets Hall's condition asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PipSet(u8);

/// What one card in the library does about mana: what it makes, or what it
/// costs.
///
/// A Sol Ring is still not here as a *source*, and the reason is unchanged:
/// what it adds to a pool once it resolves is not modelled. What is new is
/// that a card can be on the paying end. A spell the run's declared casting
/// priority names carries the bill it puts on the pool, because two spells are
/// interchangeable to the budget exactly when they cost the same — so the cost
/// is part of a group's identity, the way a land's palette is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ManaSource {
    /// Anything the run never casts: it neither makes mana nor spends any.
    ///
    /// The default, and what every card is in a run that declared no casting
    /// priority — because *which* spells you cast is a decision, and a run
    /// that was not told cannot spend the pool on anyone's behalf.
    #[default]
    Spell,
    /// A spell the declared casting priority names, with the bill it presents.
    Castable { cost: Demand },
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

/// How much of what a land makes one enumeration is allowed to tell apart.
///
/// The whole palette is finer than any single cost can see. [`Cost::payable`]
/// runs Hall's condition over the pip kinds the cost demands and nothing else,
/// so for `{1}{U}` a Plains, a Swamp and a Forest are the same source: each
/// pays one generic and no `{U}`. Restricting every palette to the pips the
/// question actually demands is therefore a coarsening the question cannot
/// observe, and on a real Commander manabase it is the difference between
/// sixteen land profiles and four —
/// [#55](https://github.com/cramt/progress-engine/issues/55).
///
/// Which pips those are is the **caller's** claim, not this type's: a walk
/// that picks the land drop by a declared priority reads land groups for a
/// reason the cost knows nothing about, and merging two lands it ranks
/// differently would change which one it played. So a narrower palette is
/// something a caller proves and passes in, and [`Palette::ALL`] is what a
/// caller that cannot prove one is entitled to.
/// **It is also what makes a spell castable at all.** A budget spends the pool
/// on the cards its priority names, so [`LandDetail::Ignored`] — which is a
/// class saying "nothing here is about mana" — erases a castable card's bill
/// along with every land's palette. That is sound only where nothing in the
/// run casts anything: once a card leaves the hand to be cast, *every* count
/// in the run depends on which cards were paid for, so a casting run prices
/// every one of its classes. The caller states that, the same way it states
/// the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandDetail {
    /// None of it. Every card becomes a [`ManaSource::Spell`] and the whole
    /// manabase collapses into the groups its queries already made, which is
    /// what a class with no casting clause gets.
    Ignored,
    /// Whether a land enters tapped, which of *these* pip kinds it makes, and
    /// what a castable spell costs.
    Pips(Palette),
}

impl ManaSource {
    pub fn is_land(self) -> bool {
        matches!(self, ManaSource::Land { .. })
    }

    /// What this card costs, where the run's priority casts it at all.
    pub fn castable(self) -> Option<Demand> {
        match self {
            ManaSource::Castable { cost } => Some(cost),
            ManaSource::Spell | ManaSource::Land { .. } => None,
        }
    }

    /// This source as an enumeration keeping only `detail` sees it.
    ///
    /// A land is still a land at every detail but [`LandDetail::Ignored`]:
    /// only a land arrives without being cast, so merging one into a spell
    /// would take a payer out of the pool rather than merge two equal ones.
    /// A castable spell keeps its bill on the same terms, and for the mirror
    /// reason: two spells that cost differently are not interchangeable to a
    /// pool that has to pay for them.
    pub fn seen_as(self, detail: LandDetail) -> ManaSource {
        match (detail, self) {
            (LandDetail::Ignored, _) | (_, ManaSource::Spell) => ManaSource::Spell,
            (LandDetail::Pips(_), ManaSource::Castable { cost }) => ManaSource::Castable { cost },
            (
                LandDetail::Pips(kept),
                ManaSource::Land {
                    enters_tapped,
                    produces,
                },
            ) => ManaSource::Land {
                enters_tapped,
                produces: produces.intersect(kept),
            },
        }
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
            ManaSource::Spell | ManaSource::Castable { .. } => Palette::EMPTY,
            ManaSource::Land { produces, .. } => produces,
        }
    }
}

/// An amount of mana owed: so much generic, so many of each pip kind.
///
/// Split out of [`Cost`] because **the budget adds bills together**. Casting
/// Opt and then Lantern of Insight out of one turn's lands is not two
/// independent questions — it is one payment of `{U}` plus `{1}`, made from
/// one pool, and whether it can be made is Hall's condition on the sum. Asking
/// the two separately is the same mistake `produces:w` and `produces:u` make
/// about one Hallowed Fountain, one turn later: each is satisfiable and the
/// pair is not.
///
/// `Copy`, `Eq` and `Hash` because a castable card carries its demand into the
/// grouping. Two spells are interchangeable to the budget exactly when they
/// cost the same, so this is part of a group's identity the way a land's
/// palette is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Demand {
    /// How many sources of no particular kind this needs.
    generic: u32,
    /// Demand per pip, indexed by `Pip as usize`.
    pips: [u32; 6],
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
    /// What is owed. Everything about paying it lives on [`Demand`], because
    /// the budget pays several of these at once and the text is the only part
    /// of a cost that belongs to one card.
    demand: Demand,
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
            demand: Demand::default(),
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
        let total = cost.demand.total();
        if total > MAX_COST {
            return Err(CostError::TooLarge {
                text: text.to_string(),
                total,
            });
        }
        Ok(cost)
    }

    fn add(&mut self, symbol: &str, text: &str) -> Result<(), CostError> {
        if let Ok(n) = symbol.parse::<u32>() {
            self.demand.generic = self.demand.generic.saturating_add(n);
            return Ok(());
        }
        let mut letters = symbol.chars();
        match (letters.next().and_then(Pip::from_letter), letters.next()) {
            (Some(pip), None) => {
                self.demand.pips[pip as usize] += 1;
                Ok(())
            }
            _ => Err(CostError::Unpayable {
                text: text.to_string(),
                symbol: symbol.to_string(),
            }),
        }
    }

    /// What this cost owes, which is the part of it the budget adds up.
    pub fn demand(&self) -> Demand {
        self.demand
    }

    /// How many sources paying this needs. A payment uses exactly this many.
    pub fn total(&self) -> u32 {
        self.demand.total()
    }

    pub fn is_free(&self) -> bool {
        self.demand.is_free()
    }

    /// The pip kinds this cost demands, and therefore the only ones it can
    /// tell apart.
    ///
    /// Empty for a cost that is all generic: `{2}` is paid by any two sources
    /// whatever they make, so every land in the deck is the same land to it.
    /// That is the narrowing [`LandDetail`] is for, and this is the half of it
    /// only a cost can state.
    pub fn demands(&self) -> Palette {
        self.demand.demands()
    }

    /// The cost as it was written, for a report that has to name it.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether `sources` can cover this cost. See [`Demand::payable`].
    pub fn payable(
        &self,
        sources: &[Source],
        count: impl Fn(usize) -> u32,
        constraint: Constraint,
    ) -> bool {
        self.demand.payable(sources, count, constraint)
    }
}

impl Demand {
    /// Owing nothing, which is what a budget starts every turn holding.
    pub const FREE: Demand = Demand {
        generic: 0,
        pips: [0; 6],
    };

    /// Both bills at once: the payment a pilot casting both spells has to make.
    ///
    /// Addition rather than two answers, because the two spells come out of one
    /// pool. Saturating, so a criteria file naming a hundred one-drops asks an
    /// unpayable question rather than an overflowing one.
    pub fn plus(self, other: Demand) -> Demand {
        let mut sum = self;
        sum.generic = sum.generic.saturating_add(other.generic);
        for (pip, add) in sum.pips.iter_mut().zip(other.pips) {
            *pip = pip.saturating_add(add);
        }
        sum
    }

    /// How many sources paying this needs. A payment uses exactly this many.
    pub fn total(self) -> u32 {
        self.generic
            .saturating_add(self.pips.iter().copied().fold(0u32, u32::saturating_add))
    }

    pub fn is_free(self) -> bool {
        self.total() == 0
    }

    /// The pip kinds this demand names, and therefore the only ones it can
    /// tell apart.
    pub fn demands(self) -> Palette {
        let (demanded, kinds) = self.kinds();
        Palette::of(demanded[..kinds].iter().copied())
    }

    /// The demanded pip kinds, as a stack array and a length.
    ///
    /// Not a `Vec` and not cached on the type: [`Demand`] is `Copy` and lives
    /// in a grouping, and the subsets below are walked once per question per
    /// path, where an allocation costs more than six comparisons.
    fn kinds(self) -> ([Pip; 6], usize) {
        let mut demanded = [Pip::White; 6];
        let mut kinds = 0;
        for pip in Pip::ALL {
            if self.pips[pip as usize] > 0 {
                demanded[kinds] = pip;
                kinds += 1;
            }
        }
        (demanded, kinds)
    }

    /// Whether `sources` can cover this demand, with `constraint` on which of
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
        if available + paid < self.total() {
            return false;
        }
        // Every non-empty subset of the kinds this cost demands. Hall's
        // condition on any other subset is implied: adding an undemanded kind
        // adds sources that serve it without adding a pip to cover.
        let (demanded, kinds) = self.kinds();
        for mask in 1u32..(1 << kinds) {
            let mut demand = 0u32;
            let mut set = PipSet(0);
            for (bit, pip) in demanded[..kinds].iter().enumerate() {
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
            let (demanded, kinds) = self.kinds();
            for pip in &demanded[..kinds] {
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
    fn every_pip_is_in_the_whole_palette() {
        assert_eq!(Palette::of(Pip::ALL), Palette::ALL);
        assert!(Pip::ALL.into_iter().all(|p| Palette::ALL.makes(p)));
        // And restricting to it is the identity, which is what makes
        // `LandDetail::Pips(Palette::ALL)` the un-narrowed case rather than a
        // fourth state to keep consistent.
        for letters in ["", "U", "WU", "WUBRGC"] {
            let palette = Palette::from_letters([letters]);
            assert_eq!(palette.intersect(Palette::ALL), palette);
        }
    }

    /// The lemma [`LandDetail::Pips`] rests on, checked over every pool of
    /// three sources this engine can describe: what a cost can see of a source
    /// is the palette intersected with the pips it demands, and whether the
    /// source enters tapped. Two sources agreeing on those are the same source
    /// to it, so restricting the pool cannot move an answer.
    #[test]
    fn a_cost_cannot_see_a_colour_it_does_not_demand() {
        for text in [
            "{1}{U}",
            "{W}{U}",
            "{2}",
            "{C}{G}",
            "{3}{B}{B}",
            "{U}{U}{U}",
        ] {
            let cost = Cost::parse(text).unwrap();
            let demanded = cost.demands();
            for bits in 0u32..(1 << 9) {
                // Three sources, each with one of eight palettes, and the
                // counts that make Hall's condition bite.
                let palettes = [
                    Palette(((bits & 0b111) as u8) << 1),
                    Palette((((bits >> 3) & 0b111) as u8) << 2),
                    Palette(((bits >> 6) & 0b111) as u8),
                ];
                for tapped in [false, true] {
                    let sources: Vec<Source> = palettes
                        .iter()
                        .map(|&produces| Source { produces, tapped })
                        .collect();
                    let restricted: Vec<Source> = sources
                        .iter()
                        .map(|s| Source {
                            produces: s.produces.intersect(demanded),
                            tapped: s.tapped,
                        })
                        .collect();
                    for counts in [[0u32, 1, 2], [1, 1, 1], [2, 0, 1], [3, 1, 0]] {
                        for constraint in [
                            Constraint::Anything,
                            Constraint::IncludesUntapped,
                            Constraint::Includes(0),
                            Constraint::Includes(2),
                        ] {
                            assert_eq!(
                                cost.payable(&sources, |i| counts[i], constraint),
                                cost.payable(&restricted, |i| counts[i], constraint),
                                "{text} over {palettes:?} {counts:?} {constraint:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The other half: sources a cost cannot tell apart are additive, so
    /// merging two entries that agree on palette and tapped-ness into one with
    /// their counts summed is the same question.
    #[test]
    fn two_sources_a_cost_cannot_tell_apart_are_one_source() {
        for text in ["{1}{U}", "{W}{U}", "{2}", "{U}{U}"] {
            let cost = Cost::parse(text).unwrap();
            for letters in ["", "U", "W", "WU"] {
                for tapped in [false, true] {
                    let produces = Palette::from_letters([letters]);
                    let same = Source { produces, tapped };
                    let island = Source {
                        produces: Palette::from_letters(["U"]),
                        tapped: false,
                    };
                    for split in [[0u32, 0], [1, 0], [0, 1], [1, 1], [2, 1]] {
                        for others in 0u32..3 {
                            let apart = [same, same, island];
                            let merged = [same, island];
                            let apart_counts = [split[0], split[1], others];
                            let merged_counts = [split[0] + split[1], others];
                            for constraint in [Constraint::Anything, Constraint::IncludesUntapped] {
                                assert_eq!(
                                    cost.payable(&apart, |i| apart_counts[i], constraint),
                                    cost.payable(&merged, |i| merged_counts[i], constraint),
                                    "{text} over {letters:?} {apart_counts:?} {constraint:?}"
                                );
                            }
                            // The obligation too: forcing the payment through
                            // either copy is forcing it through the merged
                            // group, because they are the same source.
                            let either =
                                cost.payable(&apart, |i| apart_counts[i], Constraint::Includes(0))
                                    || cost.payable(
                                        &apart,
                                        |i| apart_counts[i],
                                        Constraint::Includes(1),
                                    );
                            assert_eq!(
                                either,
                                cost.payable(
                                    &merged,
                                    |i| merged_counts[i],
                                    Constraint::Includes(0)
                                ),
                                "{text} forced through {letters:?} {apart_counts:?}"
                            );
                        }
                    }
                }
            }
        }
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
        assert_eq!(braced.demand, shorthand.demand);
    }
}
