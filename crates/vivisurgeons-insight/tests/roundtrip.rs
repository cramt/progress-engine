//! Properties of the pinned trees that need no bulk file: each renders back
//! into its own Oracle text, and none depends on what its card is called.

use std::fs;

use vivisurgeons_insight::golden::{self, Entry};
use vivisurgeons_insight::{read_face, unparse, Face};

fn pinned() -> Vec<Entry> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/golden");
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("golden/ exists") {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        let entries = golden::read(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        out.extend(entries.into_iter().map(|(e, _)| e));
    }
    out
}

fn trees(card: &str, face: &str, type_line: &str, text: &str) -> Vec<String> {
    let face = Face {
        name: face,
        type_line,
        text,
    };
    read_face(&face, card)
        .into_iter()
        .map(|l| match l.parsed {
            Ok(a) => format!("{a:?}"),
            Err(b) => format!("BROKE at {:?} after: {}", b.at, b.read),
        })
        .collect()
}

/// A tree that lost a word cannot say it again, so a pinned ability whose
/// tree does not render back into its text has lost or misread something.
#[test]
fn every_pinned_ability_renders_back_into_its_own_words() {
    let mut failed = Vec::new();
    let mut checked = 0;
    for e in pinned() {
        let face = Face {
            name: &e.face,
            type_line: &e.type_line,
            text: &e.text,
        };
        for l in read_face(&face, &e.card) {
            let Some(r) = l.round_trip() else { continue };
            checked += 1;
            if let Err(d) = r {
                let Ok(a) = &l.parsed else { continue };
                let canon = unparse::ability(a).text();
                failed.push(format!(
                    "{} ({}): {}\n  renders {canon}\n  parts at token {}: wanted {}, found {:?}",
                    e.card,
                    e.face,
                    l.text,
                    d.at,
                    d.want.join(" | "),
                    d.found
                ));
            }
        }
    }
    assert!(checked > 0, "no pinned abilities");
    assert!(
        failed.is_empty(),
        "{} of {checked} pinned abilities do not round-trip; `insight roundtrip --golden` groups them:\n\n{}",
        failed.len(),
        failed.join("\n\n")
    );
}

/// A word that is not a grammar word, kept per name word so a card renamed
/// twice reads the same.
fn nonsense(i: usize) -> String {
    let letters = ["b", "d", "f", "g", "k", "m", "p", "t", "v", "z"];
    format!("Qu{}{}x", letters[i % 10], letters[i / 10 % 10])
}

/// Function words keep their place so a renamed card still has the shape
/// a short name is cut from: "Loran of the Third Path" is "Loran".
const KEPT: &[&str] = &[
    "of", "the", "a", "an", "and", "to", "in", "on", "with", "for", "from",
];

/// A name with each word but the function words made nonsense, its
/// punctuation kept.
fn renamed(name: &str, words: &mut Vec<String>) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let mut flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() && !KEPT.contains(&word.to_lowercase().as_str()) {
            let i = words.iter().position(|w| w == word).unwrap_or_else(|| {
                words.push(word.clone());
                words.len() - 1
            });
            out.push_str(&nonsense(i));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in name.chars() {
        if c.is_alphanumeric() || c == '-' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// The ways a card names itself: its whole name and, for a legendary card,
/// a leading part of it (CR 201.5c, "a shortened version of its name"),
/// longest first. Defined from the name alone, not from the lexer, so a
/// self-reference the lexer misses shows up as a moved tree.
fn self_names(names: &[&str], legendary: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for n in names {
        v.push(n.to_string());
        if legendary {
            let head = n.split(',').next().unwrap_or(n);
            let ws: Vec<&str> = head.split(' ').collect();
            for k in 1..ws.len() {
                v.push(ws[..k].join(" "));
            }
            if head != *n {
                v.push(head.to_string());
            }
        }
    }
    v.retain(|s| !s.is_empty() && !KEPT.contains(&s.to_lowercase().as_str()));
    v.sort_by_key(|s| std::cmp::Reverse(s.len()));
    v.dedup();
    v
}

/// Every whole-word occurrence of a self-name in the text, renamed.
fn rename_text(text: &str, names: &[String], words: &mut Vec<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    'scan: while !rest.is_empty() {
        let at_word = out
            .chars()
            .last()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '-'));
        if at_word {
            for n in names {
                if let Some(after) = rest.strip_prefix(n.as_str()) {
                    if after
                        .chars()
                        .next()
                        .is_none_or(|c| !(c.is_alphanumeric() || c == '-'))
                    {
                        out.push_str(&renamed(n, words));
                        rest = after;
                        continue 'scan;
                    }
                }
            }
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Renaming a card, in its name and its text alike, must not change a
/// single tree: what a card does cannot depend on what it is called. A
/// tree that holds the printed name ("another spell named ~",
/// [named-self]) holds the new one.
#[test]
fn renaming_a_card_does_not_change_its_trees() {
    let mut moved = Vec::new();
    for e in pinned() {
        let mut words = Vec::new();
        let names = self_names(&[&e.face, &e.card], e.type_line.contains("Legendary"));
        let card = renamed(&e.card, &mut words);
        let face = renamed(&e.face, &mut words);
        let text = rename_text(&e.text, &names, &mut words);
        let printed = |name: &str| format!("PrintedName({name:?})");
        let before: Vec<String> = trees(&e.card, &e.face, &e.type_line, &e.text)
            .into_iter()
            .map(|t| t.replace(&printed(&e.face), &printed(&face)))
            .collect();
        let after = trees(&card, &face, &e.type_line, &text);
        if before != after {
            moved.push(format!(
                "{} -> {card}:\n  {}\n  before {before:?}\n  after  {after:?}",
                e.card,
                text.replace('\n', " / ")
            ));
        }
    }
    assert!(
        moved.is_empty(),
        "{} cards read differently once renamed:\n\n{}",
        moved.len(),
        moved.join("\n\n")
    );
}
