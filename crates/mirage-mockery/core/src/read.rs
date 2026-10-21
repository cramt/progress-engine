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

/// Every face's rules text, each read as its own face: a double-faced card's
/// faces each have their own characteristics (CR 712.8), and so do a split
/// card's halves on the stack (CR 709.3b).
pub fn read(card: &Card) -> Reading {
    let mut reading = Reading {
        lines: 0,
        unread: Vec::new(),
    };
    for face in card.faces() {
        let text = face.rules.lines().join("\n");
        let lines = read_face(
            &Face {
                name: &face.name,
                type_line: &face.type_line,
                text: &text,
            },
            &face.name,
        );
        reading.lines += lines.len();
        reading.unread.extend(lines.into_iter().filter_map(|l| {
            let broke = l.parsed.err()?;
            Some(Unread {
                line: l.text,
                read: broke.read,
                at: broke.at,
            })
        }));
    }
    reading
}
