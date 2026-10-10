//! The words Magic defines, from Scryfall's catalogs (`vocab/*.txt`, fetched
//! 2026-10-11 from `api.scryfall.com/catalog/<name>`). The grammar knows the
//! shape of a sentence; these lists know which words fill a slot.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub struct Vocab {
    /// Keyword abilities by first word, each list longest first.
    pub keyword_abilities: HashMap<String, Vec<String>>,
    pub keyword_actions: HashSet<String>,
    pub ability_words: Vec<Vec<String>>,
    pub card_types: HashSet<String>,
    pub supertypes: HashSet<String>,
    /// Every subtype of every card type, lowercased, one word or two.
    pub subtypes: HashSet<String>,
}

fn lines(s: &str) -> impl Iterator<Item = String> + '_ {
    s.lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
}

/// Longest first, so "first strike" is tried before "first" could be.
fn phrases(s: &str) -> Vec<Vec<String>> {
    let mut v: Vec<Vec<String>> = lines(s)
        .map(|l| l.split_whitespace().map(str::to_string).collect())
        .collect();
    v.sort_by_key(|p| std::cmp::Reverse(p.len()));
    v
}

fn by_first_word(s: &str) -> HashMap<String, Vec<String>> {
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for p in phrases(s) {
        m.entry(p[0].clone()).or_default().push(p.join(" "));
    }
    m
}

pub fn vocab() -> &'static Vocab {
    static V: OnceLock<Vocab> = OnceLock::new();
    V.get_or_init(|| {
        let subtypes = [
            include_str!("../vocab/creature-types.txt"),
            include_str!("../vocab/land-types.txt"),
            include_str!("../vocab/artifact-types.txt"),
            include_str!("../vocab/enchantment-types.txt"),
            include_str!("../vocab/spell-types.txt"),
            include_str!("../vocab/planeswalker-types.txt"),
            include_str!("../vocab/battle-types.txt"),
        ]
        .into_iter()
        .flat_map(lines)
        .collect();
        Vocab {
            keyword_abilities: by_first_word(include_str!("../vocab/keyword-abilities.txt")),
            keyword_actions: lines(include_str!("../vocab/keyword-actions.txt")).collect(),
            ability_words: phrases(include_str!("../vocab/ability-words.txt")),
            card_types: lines(include_str!("../vocab/card-types.txt")).collect(),
            supertypes: lines(include_str!("../vocab/supertypes.txt")).collect(),
            subtypes,
        }
    })
}

/// Singular forms a printed plural might come from. Card text pluralises
/// subtypes ("Elves", "Wolves", "Zombies"), and the catalogs are singular.
pub fn singulars(w: &str) -> Vec<String> {
    let mut v = vec![w.to_string()];
    let irregular = [
        ("mice", "mouse"),
        ("fungi", "fungus"),
        ("men", "man"),
        ("teeth", "tooth"),
        ("octopuses", "octopus"),
    ];
    for (pl, sg) in irregular {
        if let Some(stem) = w.strip_suffix(pl) {
            v.push(format!("{stem}{sg}"));
        }
    }
    if let Some(s) = w.strip_suffix("ves") {
        v.push(format!("{s}f"));
        v.push(format!("{s}fe"));
    }
    if let Some(s) = w.strip_suffix("ies") {
        v.push(format!("{s}y"));
    }
    if let Some(s) = w.strip_suffix("es") {
        v.push(s.to_string());
    }
    if let Some(s) = w.strip_suffix('s') {
        v.push(s.to_string());
    }
    v
}
