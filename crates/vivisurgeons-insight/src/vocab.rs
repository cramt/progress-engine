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
    /// Land subtypes alone: "a Swamp in addition to its other land types".
    pub land_types: HashSet<String>,
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
        // The artifact catalog misses four artifact types CR 205.3g lists,
        // each a predefined token's (CR 111.10u–x): "Create a Lander token".
        // Remove them once `artifact-types.txt` carries them.
        .chain(["heartwood", "lander", "mutagen", "vibranium"].map(String::from))
        .collect();
        // Scryfall's catalogs miss a few the Comprehensive Rules define: Plot
        // is a keyword ability (CR 702.170), filed under actions there, and
        // "Empower Jace" a keyword action (CR 701.71) not in either list.
        // Remove an entry once the catalog it belongs in carries it.
        let abilities = format!("{}\nplot\n", include_str!("../vocab/keyword-abilities.txt"));
        let actions = format!(
            "{}\nempower jace\n",
            include_str!("../vocab/keyword-actions.txt")
        );
        Vocab {
            keyword_abilities: by_first_word(&abilities),
            keyword_actions: lines(&actions).collect(),
            ability_words: phrases(include_str!("../vocab/ability-words.txt")),
            // Scryfall's card-types catalog lists "Hero" for the Theros
            // challenge decks' Hero cards (memorabilia, whose text never
            // names it). CR 205.2a has no such card type and CR 205.3m lists
            // Hero among the creature types, which every corpus line means:
            // "Heroes you control get +2/+2". The grammar tries card types
            // first, so it is removed here. "Dungeon", also filed among the
            // planeswalker types, is the card type CR 205.2a lists, and stays.
            card_types: lines(include_str!("../vocab/card-types.txt"))
                .filter(|t| t != "hero")
                .collect(),
            land_types: lines(include_str!("../vocab/land-types.txt")).collect(),
            supertypes: lines(include_str!("../vocab/supertypes.txt")).collect(),
            subtypes,
        }
    })
}

/// A noun after "this" that names the card itself: "this creature", "this
/// Aura", "this Saga", "this emblem", "this spell". Every card type,
/// supertype and subtype, and the object nouns "card", "spell", "token"
/// and "permanent" (CR 700.7). Each is one word: no two-word subtype
/// follows "this" in the corpus.
pub fn is_self_descriptor(w: &str) -> bool {
    let v = vocab();
    matches!(w, "card" | "spell" | "token" | "permanent")
        || v.card_types.contains(w)
        || v.supertypes.contains(w)
        || v.subtypes.contains(w)
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
