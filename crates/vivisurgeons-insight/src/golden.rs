//! Reviewed trees, pinned. A golden file holds each face's Oracle text beside
//! the trees it reads into, so the test that checks them needs no bulk file
//! and a changed tree is a line in a git diff, read before it is blessed.
//!
//! ```text
//! ## Sol Ring
//! face: Sol Ring
//! type: Artifact
//! cost: {1}
//! > {T}: Add {C}{C}.
//! = Activated { cost: [Tap], ... }
//! ```
//!
//! `cost:` is the face's mana cost, absent where it has none. `>` lines are
//! the face's text as Scryfall prints it; `=` lines are its abilities in
//! order, as `Debug` prints them.

use std::fmt::Write;

use crate::{read_face, Face, Line};

#[derive(Clone)]
pub struct Entry {
    pub card: String,
    pub face: String,
    pub type_line: String,
    pub text: String,
    /// As Scryfall prints it: "{X}{R}". An {X} in it defines X for the
    /// face's text (CR 107.3a), which `insight lint` checks.
    pub mana_cost: String,
}

impl Entry {
    pub fn lines(&self) -> Vec<Line> {
        let face = Face {
            name: &self.face,
            type_line: &self.type_line,
            text: &self.text,
        };
        read_face(&face, &self.card)
    }

    /// The face's abilities as one line each, as the grammar reads them now.
    pub fn trees(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .map(|l| match l.parsed {
                Ok(a) => format!("{a:?}"),
                Err(b) => format!("BROKE at {:?} after: {}", b.at, b.read),
            })
            .collect()
    }
}

/// The `snapshot/` file a card's faces live in: one of 256, by an FNV-1a
/// hash of its name. A hash std does not promise to keep would move every
/// card between files on a toolchain bump. Splitting at all is for git: a
/// commit that moves a few trees rewrites a few small files, not one the
/// size of the corpus.
pub fn shard(card: &str) -> String {
    let h = card.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{:02x}.txt", h & 0xff)
}

pub fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    for e in entries {
        let _ = writeln!(
            out,
            "## {}\nface: {}\ntype: {}",
            e.card, e.face, e.type_line
        );
        if !e.mana_cost.is_empty() {
            let _ = writeln!(out, "cost: {}", e.mana_cost);
        }
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
                    mana_cost: String::new(),
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
        } else if let Some(c) = line.strip_prefix("cost: ") {
            e.mana_cost = c.into();
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
