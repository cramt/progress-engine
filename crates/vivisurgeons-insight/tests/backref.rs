//! Every pointer in the corpus lands where it says ([backref] in
//! SEMANTICS.md): read from `snapshot/` again, each `BackRef`, `Rest`,
//! `BackRefs` and `LinkedBackRef` the resolver writes names an introducer
//! in its scope that its words fit, and no reference in a tree
//! `read_face` returns is left in an old form. Then the binders made of
//! them ([binders]): each `Bound` names a binder in scope its words fit,
//! and saying each binder where it was first said gives back the tree
//! with pointers. A reference no
//! introducer fits is `Quant::Unresolved`; their number per hint may fall
//! but not rise.

use std::collections::BTreeMap;
use std::fs;

use vivisurgeons_insight::ast::{Ability, Hint, Noun, Objects, Post, Quant};
use vivisurgeons_insight::backref::{analyse, Kind};
use vivisurgeons_insight::golden;
use vivisurgeons_insight::walk::{Visitor, Walk};

/// The references no introducer fits, by what their words say. Most are
/// in a face whose introducing line does not parse (a die roll's line, a
/// linked ability), a "that spell" said of a card, a damage amount said
/// after its target, or "the cards from your hand" read as a reference.
const UNRESOLVED: [(Hint, usize); 5] = [
    (Hint::Antecedent, 42),
    (Hint::Linked, 6),
    (Hint::Listed, 7),
    (Hint::Pool, 2),
    (Hint::Rest, 1),
];

#[test]
fn every_pointer_lands_on_an_introducer_its_words_fit() {
    let dir = format!("{}/snapshot", env!("CARGO_MANIFEST_DIR"));
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    let mut misses = Vec::new();
    let mut old = Vec::new();
    let mut unresolved: BTreeMap<Hint, usize> = BTreeMap::new();
    let mut pointers = 0;
    for p in paths {
        let text = fs::read_to_string(&p).unwrap();
        for (e, _) in golden::read(&text).unwrap() {
            let lines = e.lines_pointers();
            let abs: Vec<(Option<&Ability>, bool)> = lines
                .iter()
                .map(|l| (l.parsed.as_ref().ok(), l.continues))
                .collect();
            let an = analyse(&abs, &e.type_line);
            pointers += an
                .resolved
                .iter()
                .filter(|r| !matches!(r.kind, Kind::Text(_) | Kind::Itself))
                .count();
            for (k, what) in an.problems() {
                match k {
                    Kind::Text(h) => *unresolved.entry(h).or_default() += 1,
                    _ => misses.push(format!("{}: {what}", e.card)),
                }
            }
            for l in &e.lines() {
                if let Ok(a) = &l.parsed {
                    if let Some(q) = old_form(a) {
                        old.push(format!("{}: {q}", e.card));
                    }
                }
            }
        }
    }
    assert!(pointers > 8000, "only {pointers} pointers read");
    assert!(
        misses.is_empty(),
        "{} pointers miss:\n{}",
        misses.len(),
        misses.join("\n")
    );
    assert!(
        old.is_empty(),
        "{} references in an old form:\n{}",
        old.len(),
        old.join("\n")
    );
    let rose: Vec<String> = UNRESOLVED
        .iter()
        .filter_map(|(h, n)| {
            let now = unresolved.get(h).copied().unwrap_or(0);
            (now > *n).then(|| format!("{h:?}: {n} -> {now}"))
        })
        .chain(
            unresolved
                .iter()
                .filter(|(h, _)| !UNRESOLVED.iter().any(|(k, _)| k == *h))
                .map(|(h, n)| format!("{h:?}: 0 -> {n}")),
        )
        .collect();
    assert!(
        rose.is_empty(),
        "unresolved references rose (`cargo run -p vivisurgeons-insight --bin backref -- --problems` lists them):\n{}",
        rose.join("\n")
    );
}

/// A reference the resolver should have made a pointer, if the tree holds
/// one.
fn old_form(a: &Ability) -> Option<String> {
    struct Old(Option<String>);
    impl Visitor for Old {
        fn enter(&mut self, o: &mut Objects) -> bool {
            // "those players", "those counters": players and counters
            // introduce nothing, so a reference to them is no pointer yet.
            let not_object = matches!(
                o.filter.kind,
                Some(Noun::Player | Noun::Opponent | Noun::Counter(_))
            );
            let old = matches!(o.quant, Quant::Antecedent) && !not_object
                || matches!(
                    o.quant,
                    Quant::BeforeEach
                        | Quant::ClauseObject
                        | Quant::Created
                        | Quant::Listed
                        | Quant::Both
                        | Quant::Triggering
                        | Quant::OriginalAndCopies
                        | Quant::Linked
                )
                || o.filter.posts.contains(&Post::FromAmongThem);
            if old && self.0.is_none() {
                self.0 = Some(format!("{:?}", o.quant));
            }
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut v = Old(None);
    a.clone().walk(&mut v);
    v.0
}

/// Every reference named by its binder ([binders]): in scope, fitting its
/// words, and the tree with each binder said in place is the pointer one.
#[test]
fn every_bound_names_a_binder_in_scope_and_binding_loses_nothing() {
    use vivisurgeons_insight::backref::fits;
    use vivisurgeons_insight::binders::{bindings, inline};
    let dir = format!("{}/snapshot", env!("CARGO_MANIFEST_DIR"));
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    let (mut bound, mut binders, mut lets, mut withs) = (0, 0, 0, 0);
    let mut bad = Vec::new();
    for p in paths {
        let text = fs::read_to_string(&p).unwrap();
        for (e, _) in golden::read(&text).unwrap() {
            let done = e.lines();
            let pointers = e.lines_pointers();
            let counts: Vec<usize> = done
                .iter()
                .map(|l| l.parsed.as_ref().map_or(0, |a| bindings(a).binders.len()))
                .collect();
            for (l, (d, p)) in done.iter().zip(&pointers).enumerate() {
                let (Ok(a), Ok(pa)) = (&d.parsed, &p.parsed) else {
                    continue;
                };
                let b = bindings(a);
                binders += b.binders.len();
                let dbg = format!("{a:?}");
                lets += dbg.matches("Let { bind").count();
                withs += dbg.matches("With { bind").count();
                for (decl, v) in &b.refs {
                    for x in v {
                        bound += 1;
                        match x {
                            None => bad.push(format!(
                                "{} line {l}: object {decl} names no binder in scope",
                                e.card
                            )),
                            Some(i) => {
                                let words = &b.words[decl];
                                if !matches!(words.quant, Quant::Rest { .. })
                                    && !fits(words, &b.binders[*i].what)
                                {
                                    bad.push(format!(
                                        "{} line {l}: object {decl} does not fit its binder",
                                        e.card
                                    ));
                                }
                            }
                        }
                    }
                }
                for (line, binder) in linked(a) {
                    if counts
                        .get(usize::from(line))
                        .is_none_or(|&n| usize::from(binder) >= n)
                    {
                        bad.push(format!(
                            "{} line {l}: line {line} has no binder {binder}",
                            e.card
                        ));
                    }
                }
                if said(&inline(a)) != said(pa) {
                    bad.push(format!(
                        "{} line {l}: said in place, not the pointer tree",
                        e.card
                    ));
                }
            }
        }
    }
    eprintln!("{bound} references to {binders} binders ({lets} Let, {withs} With)");
    assert!(bound > 8000, "only {bound} references read");
    assert!(
        bad.is_empty(),
        "{} problems:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

fn linked(a: &Ability) -> Vec<(u8, u8)> {
    struct L(Vec<(u8, u8)>);
    impl Visitor for L {
        fn enter(&mut self, o: &mut Objects) -> bool {
            match &o.quant {
                Quant::LinkedBound { line, binder } => self.0.push((*line, *binder)),
                Quant::Bounds(ps) => {
                    for p in ps {
                        if let vivisurgeons_insight::ast::Pointer::LinkedBound { line, binder } = p
                        {
                            self.0.push((*line, *binder));
                        }
                    }
                }
                _ => {}
            }
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut v = L(Vec::new());
    a.clone().walk(&mut v);
    v.0
}

/// The tree with every reference's pointer written the same way: what
/// binding and pointing both keep.
fn said(a: &Ability) -> String {
    struct S;
    impl Visitor for S {
        fn enter(&mut self, o: &mut Objects) -> bool {
            match o.quant {
                Quant::BackRef { .. }
                | Quant::LinkedBackRef { .. }
                | Quant::Bound(_)
                | Quant::Introduced(_)
                | Quant::Unresolved(_)
                | Quant::LinkedBound { .. } => o.quant = Quant::BackRef { back: 0 },
                Quant::BackRefs(_) | Quant::Bounds(_) => o.quant = Quant::BackRefs(Vec::new()),
                Quant::Rest { .. } => o.quant = Quant::Rest { of: 0 },
                _ => {}
            }
            true
        }
        fn ability(&mut self, _: &mut Ability) -> bool {
            true
        }
    }
    let mut a = a.clone();
    a.walk(&mut S);
    format!("{a:?}")
}
