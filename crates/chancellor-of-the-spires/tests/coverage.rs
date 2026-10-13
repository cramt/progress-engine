//! How much of the Modern metagame lowers: every face of every card in
//! Insight's `milestones/modern-meta.txt`, read from the Scryfall bulk file
//! `scryfall sync` keeps, and the refusals counted by node. Needs that file,
//! so it is ignored by default:
//! `cargo test --release -p chancellor-of-the-spires --test coverage -- --ignored --nocapture`.

use std::collections::{BTreeMap, HashSet};

use chancellor_of_the_spires::lower::{self, Card, Refusal};
use vivisurgeons_insight::corpus;

#[test]
#[ignore]
fn modern_metagame_lowering() {
    let names: HashSet<String> =
        include_str!("../../vivisurgeons-insight/milestones/modern-meta.txt")
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_lowercase)
            .collect();
    let (cards, _) = corpus::read(&corpus::cache("oracle-cards.jsonl"), &HashSet::new(), None)
        .expect("the Scryfall bulk file `scryfall sync` keeps");
    let mut faces = 0;
    let mut lowered = 0;
    let mut cards_whole = 0;
    let mut cards_seen = 0;
    let mut refused: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for card in &cards {
        let front = card.name.split(" // ").next().unwrap_or("").to_lowercase();
        if !names.contains(&card.name.to_lowercase()) && !names.contains(&front) {
            continue;
        }
        cards_seen += 1;
        let mut whole = true;
        for f in &card.faces {
            faces += 1;
            let c = Card {
                name: &f.face,
                type_line: &f.type_line,
                oracle_text: &f.text,
                mana_value: 0,
                colors: &[],
                pt: None,
            };
            match lower::card(&c) {
                Ok(_) => lowered += 1,
                Err(r) => {
                    whole = false;
                    let key = match &r {
                        Refusal::TypeLine(_) => "type line".to_string(),
                        Refusal::Unparsed { .. } => "Insight breaks".to_string(),
                        Refusal::Unlowered { node, .. } => node.clone(),
                    };
                    let e = refused.entry(key).or_insert((0, f.face.clone()));
                    e.0 += 1;
                }
            }
        }
        cards_whole += whole as usize;
    }
    eprintln!("{cards_whole}/{cards_seen} cards and {lowered}/{faces} faces lower whole");
    let mut top: Vec<_> = refused.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1 .0));
    for (node, (n, example)) in top.iter().take(25) {
        eprintln!("{n:>5}  {node:<28} e.g. {example}");
    }
}
