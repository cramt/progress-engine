//! Reviewed trees, pinned. A golden file holds each face's Oracle text beside
//! the trees it reads into, so the test that checks them needs no bulk file
//! and a changed tree is a line in a git diff, read before it is blessed.
//!
//! ```text
//! ## Sol Ring
//! face: Sol Ring
//! type: Artifact
//! > {T}: Add {C}{C}.
//! = Activated { cost: [Tap], ... }
//! ```
//!
//! `>` lines are the face's text as Scryfall prints it; `=` lines are its
//! abilities in order, as `Debug` prints them.

use std::fmt::Write;

use crate::{read_face, Face};

pub struct Entry {
    pub card: String,
    pub face: String,
    pub type_line: String,
    pub text: String,
}

impl Entry {
    /// The face's abilities as one line each, as the grammar reads them now.
    pub fn trees(&self) -> Vec<String> {
        let face = Face {
            name: &self.face,
            type_line: &self.type_line,
            text: &self.text,
        };
        read_face(&face, &self.card)
            .into_iter()
            .map(|l| match l.parsed {
                Ok(a) => format!("{a:?}"),
                Err(b) => format!("BROKE at {:?} after: {}", b.at, b.read),
            })
            .collect()
    }
}

pub fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    for e in entries {
        let _ = writeln!(
            out,
            "## {}\nface: {}\ntype: {}",
            e.card, e.face, e.type_line
        );
        for l in e.text.lines() {
            let _ = writeln!(out, "> {l}");
        }
        for t in e.trees() {
            let _ = writeln!(out, "= {t}");
        }
        out.push('\n');
    }
    out
}

/// The entries of a golden file with the trees it pinned, per entry.
pub fn read(text: &str) -> Result<Vec<(Entry, Vec<String>)>, String> {
    let mut out: Vec<(Entry, Vec<String>)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let bad = || format!("line {}: {line:?} outside an entry", n + 1);
        if let Some(card) = line.strip_prefix("## ") {
            out.push((
                Entry {
                    card: card.into(),
                    face: String::new(),
                    type_line: String::new(),
                    text: String::new(),
                },
                Vec::new(),
            ));
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let (e, trees) = out.last_mut().ok_or_else(bad)?;
        if let Some(f) = line.strip_prefix("face: ") {
            e.face = f.into();
        } else if let Some(t) = line.strip_prefix("type: ") {
            e.type_line = t.into();
        } else if let Some(t) = line
            .strip_prefix("> ")
            .or(line.strip_prefix(">").filter(|r| r.is_empty()))
        {
            if !e.text.is_empty() {
                e.text.push('\n');
            }
            e.text.push_str(t);
        } else if let Some(t) = line.strip_prefix("= ") {
            trees.push(t.into());
        } else {
            return Err(bad());
        }
    }
    Ok(out)
}
