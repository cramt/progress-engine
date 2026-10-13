//! "This creature", "this Aura", "this spell", "~": every name the card
//! gives itself reads as one node, a `SelfRef` with no alts and no kind
//! (CR 700.7, SEMANTICS [self-reference]). Checked over every face in
//! `snapshot/` and `golden/`, read again from its Oracle text, so it needs
//! no bulk file and holds the grammar as it is now, not as it was blessed.

use std::fs;
use std::path::PathBuf;
use std::thread;

use vivisurgeons_insight::ast::*;
use vivisurgeons_insight::golden;
use vivisurgeons_insight::walk::{Visitor, Walk};

const SELF: &str = "quant: SelfRef, filter: Filter { ";

/// Every `SelfRef` the walk reaches, nested abilities included, and those
/// that name the card by a noun.
#[derive(Default)]
struct Selves {
    seen: usize,
    described: Vec<String>,
}

impl Visitor for Selves {
    fn enter(&mut self, o: &mut Objects) -> bool {
        if o.quant == Quant::SelfRef {
            self.seen += 1;
            if !o.filter.alts.is_empty() || o.filter.kind.is_some() || o.filter.or.is_some() {
                self.described.push(format!("{o:?}"));
            }
        }
        true
    }
    fn ability(&mut self, _a: &mut Ability) -> bool {
        true
    }
}

/// The same from the `Debug` text, which prints every field whatever the
/// walk visits: how many there are, and how many carry more than a zone.
fn in_text(tree: &str) -> (usize, usize) {
    let seen = tree.matches(SELF).count();
    let described = tree
        .match_indices(SELF)
        .filter(|(i, _)| !tree[i + SELF.len()..].starts_with("alts: [], kind: None, "))
        .count();
    (seen, described)
}

/// Faults in one ability: a described `SelfRef`, or a walk that missed one.
fn faults(a: &Ability) -> Vec<String> {
    let mut v = Selves::default();
    a.clone().walk(&mut v);
    let tree = format!("{a:?}");
    let (seen, described) = in_text(&tree);
    let mut out: Vec<String> = v
        .described
        .into_iter()
        .map(|o| format!("described {o}"))
        .collect();
    if seen != v.seen {
        out.push(format!(
            "the walk reached {} of {seen} SelfRefs in {tree}",
            v.seen
        ));
    }
    if described != 0 && out.is_empty() {
        out.push(format!(
            "{described} described SelfRefs only the text shows in {tree}"
        ));
    }
    out
}

fn files() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for dir in ["snapshot", "golden"] {
        let dir = format!("{}/{dir}", env!("CARGO_MANIFEST_DIR"));
        for e in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{dir}: {e}")) {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "txt") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

#[test]
fn every_self_reference_is_the_bare_card() {
    let files = files();
    let chunks: Vec<Vec<PathBuf>> = files
        .chunks(files.len().div_ceil(8))
        .map(<[_]>::to_vec)
        .collect();
    let results: Vec<(usize, Vec<String>)> = thread::scope(|s| {
        let hs: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                s.spawn(move || {
                    let mut seen = 0;
                    let mut bad = Vec::new();
                    for path in chunk {
                        let text = fs::read_to_string(path).unwrap();
                        let entries = golden::read(&text)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        for (entry, _) in entries {
                            for line in entry.lines() {
                                let Ok(a) = line.parsed else { continue };
                                seen += format!("{a:?}").matches(SELF).count();
                                bad.extend(
                                    faults(&a)
                                        .into_iter()
                                        .map(|f| format!("{}: {f}", entry.card)),
                                );
                            }
                        }
                    }
                    (seen, bad)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let seen: usize = results.iter().map(|r| r.0).sum();
    let bad: Vec<&String> = results.iter().flat_map(|r| &r.1).collect();
    // The corpus names itself about 21,000 times; far fewer means the
    // files were not read.
    assert!(seen > 20_000, "only {seen} SelfRefs read");
    assert!(
        bad.is_empty(),
        "{} faults, the first:\n{}",
        bad.len(),
        bad.iter()
            .take(20)
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The check sees a described `SelfRef` in a cost and inside a quoted
/// ability, where Ghost Vacuum's "Sacrifice this artifact" and a granted
/// "this creature" sit.
#[test]
fn the_check_finds_a_described_self_reference_anywhere() {
    let described = Objects {
        quant: Quant::SelfRef,
        filter: Filter {
            alts: vec![vec![Adj::Type("artifact".into())]],
            ..Filter::default()
        },
    };
    let in_cost = Ability::Activated {
        cost: vec![Cost::Tap, Cost::Sacrifice(described.clone())],
        effect: vec![],
        limits: vec![],
    };
    assert_eq!(faults(&in_cost).len(), 1, "{:?}", faults(&in_cost));
    let quoted = Ability::Spell(vec![Effect::GrantAbility {
        what: self_ref(),
        ability: Box::new(Ability::Activated {
            cost: vec![Cost::Sacrifice(described)],
            effect: vec![],
            limits: vec![],
        }),
        dur: Duration::UntilEndOfTurn,
    }]);
    assert_eq!(faults(&quoted).len(), 1, "{:?}", faults(&quoted));
    let zone = Ability::Activated {
        cost: vec![Cost::Sacrifice(self_ref_in(vec![Post::In(
            Zone::Graveyard(Player::You),
        )]))],
        effect: vec![],
        limits: vec![],
    };
    assert!(faults(&zone).is_empty(), "a zone is no descriptor");
}
