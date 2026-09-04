//! Mana costs, as the multisets of symbols they are.
//!
//! `{2}{W}{W}` is not a string and not a number: it is two generic and two
//! white, and every question worth asking about it — does this cost contain a
//! green symbol, does it cost more than three generic and a white, how much
//! devotion does it give — is a question about that multiset. Comparing the
//! printed text instead is how `{W/U}` comes to differ from `{U/W}`.

use std::collections::BTreeMap;

use crate::Colors;

/// The order Magic prints colours in, and so the order a hybrid symbol is
/// normalised into: a query may type `{U/W}` for a card that prints `{W/U}`.
const WUBRG: [char; 5] = ['W', 'U', 'B', 'R', 'G'];

/// A mana cost as a multiset of symbols.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManaCost {
    /// Generic mana, which is a number rather than a repeated symbol: `{7}` is
    /// one symbol worth seven, and `m>{6}` has to know that.
    generic: u32,
    /// Everything else, by canonical spelling, with the number of times it
    /// appears. `{W}{W}` is one entry counted twice.
    symbols: BTreeMap<String, u32>,
}

impl ManaCost {
    /// Read a cost, accepting both the printed form and Scryfall's shorthand.
    ///
    /// A card always prints every symbol in braces; a query may write `2WW` for
    /// `{2}{W}{W}`. Scryfall allows the shorthand only for symbols that are not
    /// split, because `2/G` unbraced would be unreadable, and one parser can
    /// take both: inside braces is one symbol, outside braces a run of digits
    /// is generic and a letter is a symbol of its own.
    pub fn parse(text: &str) -> Self {
        let mut out = ManaCost::default();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '{' => {
                    let start = i + 1;
                    let end = chars[start..]
                        .iter()
                        .position(|c| *c == '}')
                        .map_or(chars.len(), |p| start + p);
                    out.add(&chars[start..end].iter().collect::<String>());
                    i = end + 1;
                }
                c if c.is_ascii_digit() => {
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                    let n: String = chars[start..i].iter().collect();
                    out.generic += n.parse::<u32>().unwrap_or(0);
                }
                c if c.is_ascii_alphabetic() => {
                    out.add(&c.to_string());
                    i += 1;
                }
                // Whitespace, and the `//` that joins the halves of a split
                // card when a caller hands the whole line over.
                _ => i += 1,
            }
        }
        out
    }

    fn add(&mut self, symbol: &str) {
        if symbol.is_empty() {
            return;
        }
        if let Ok(n) = symbol.parse::<u32>() {
            self.generic += n;
            return;
        }
        *self.symbols.entry(normalize(symbol)).or_insert(0) += 1;
    }

    /// Whether every symbol of `self` appears in `other` at least as often.
    ///
    /// Scryfall's rule exactly: "a mana cost is greater than another if it
    /// includes all the same symbols and more".
    pub fn is_subset_of(&self, other: &ManaCost) -> bool {
        self.generic <= other.generic
            && self
                .symbols
                .iter()
                .all(|(s, n)| other.symbols.get(s).copied().unwrap_or(0) >= *n)
    }

    /// How much this cost contributes to devotion to a set of colours.
    ///
    /// Every symbol that includes any of those colours counts once, which is
    /// what the rules say and what makes `{U/B}` worth one to a blue-black
    /// devotion rather than two. Generic and `{X}` contribute nothing.
    pub fn devotion_to(&self, colors: Colors) -> u32 {
        self.symbols
            .iter()
            .filter(|(symbol, _)| {
                symbol_colors(symbol).is_some_and(|c| !c.intersect(colors).is_empty())
            })
            .map(|(_, n)| n)
            .sum()
    }

    /// The colours named anywhere in this cost, for reading a devotion term.
    pub fn colors(&self) -> Colors {
        self.symbols
            .keys()
            .filter_map(|s| symbol_colors(s))
            .fold(Colors::default(), |acc, c| acc.union(c))
    }

    /// How many symbols there are, which is the level a devotion term asks for.
    pub fn symbol_count(&self) -> u32 {
        self.symbols.values().sum()
    }

    /// Whether every symbol names the same colours, which a devotion term
    /// requires: `devotion:{u}{b}` is two different questions in one term.
    pub fn symbols_agree_on_colors(&self) -> bool {
        let mut seen: Option<Colors> = None;
        self.symbols.keys().all(|s| match (symbol_colors(s), seen) {
            (Some(c), None) => {
                seen = Some(c);
                true
            }
            (Some(c), Some(first)) => c == first,
            (None, _) => false,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.generic == 0 && self.symbols.is_empty()
    }
}

/// The colours a symbol names, or `None` when it names none.
///
/// `{2/W}` and `{W/P}` are both white — the `2` and the `P` are how you pay,
/// not what colour it is — so a symbol's colours are its WUBRG letters and
/// nothing else.
fn symbol_colors(symbol: &str) -> Option<Colors> {
    let letters: String = symbol
        .chars()
        .filter(|c| WUBRG.contains(&c.to_ascii_uppercase()))
        .collect();
    (!letters.is_empty()).then(|| Colors::from_letters(&letters).unwrap_or_default())
}

/// A symbol's canonical spelling: upper case, and hybrid halves in WUBRG order.
///
/// Only pure colour hybrids are reordered. `{W/P}` and `{2/G}` have a fixed
/// order — the marker goes last — and sorting them would invent a symbol.
fn normalize(symbol: &str) -> String {
    let upper = symbol.to_ascii_uppercase();
    let halves: Vec<&str> = upper.split('/').collect();
    let both_colors = halves.len() == 2
        && halves
            .iter()
            .all(|h| h.len() == 1 && WUBRG.contains(&h.chars().next().unwrap_or(' ')));
    if !both_colors {
        return upper;
    }
    let mut cs: Vec<char> = halves.iter().filter_map(|h| h.chars().next()).collect();
    cs.sort_by_key(|c| WUBRG.iter().position(|o| o == c).unwrap_or(usize::MAX));
    format!("{}/{}", cs[0], cs[1])
}
