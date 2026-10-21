//! What Vivisurgeon's Insight makes of the rules text: whether each line is
//! Magic it can read. A warning, never a refusal; homebrew may say things
//! Insight cannot read yet, and Insight is a spike that does not cover all of
//! printed Magic either.

use vivisurgeons_insight::{read_face, Face};

use crate::card::Card;

/// One line Insight could not read, and where it stopped.
pub struct Unread {
    pub line: String,
    /// The words it got through.
    pub read: String,
    /// The word it stopped at.
    pub at: String,
}

pub struct Reading {
    pub lines: usize,
    pub unread: Vec<Unread>,
}

pub fn read(card: &Card) -> Reading {
    let text = card.rules.lines().join("\n");
    let lines = read_face(
        &Face {
            name: &card.name,
            type_line: &card.type_line,
            text: &text,
        },
        &card.name,
    );
    Reading {
        lines: lines.len(),
        unread: lines
            .into_iter()
            .filter_map(|l| {
                let broke = l.parsed.err()?;
                Some(Unread {
                    line: l.text,
                    read: broke.read,
                    at: broke.at,
                })
            })
            .collect(),
    }
}
