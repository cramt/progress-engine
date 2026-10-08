//! Which line of a file holds a card: the one rule a deck and the collection
//! both add by, so that "one more Sol Ring" finds the same line in either.
//!
//! A file names a card by name or by printing, and a printing's name is not in
//! the file. [`Names`] carries it in from whoever has the printing data, as
//! the exports take it, so this crate decides identity without holding any
//! card data of its own.

use std::collections::HashMap;

use crate::deck::{CardRef, Printing};

/// The card name of each printing, keyed `set/num` as the file writes it.
pub type Names = HashMap<String, String>;

/// A name's front face: `Delver of Secrets` for `Delver of Secrets //
/// Insectile Aberration`, and the name itself for a card with one face.
fn front(name: &str) -> &str {
    name.split_once(" // ").map_or(name, |(front, _)| front)
}

fn same_text(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

/// `a` and `b` name one card: alike whatever the case, or one being the
/// other's front face, the way Archidekt and quick add name a double-faced
/// card. A back face alone names nothing, and two cards that share a front
/// face but differ behind it are two cards.
pub fn same_name(a: &str, b: &str) -> bool {
    same_text(a, b) || same_text(front(a), b) || same_text(a, front(b))
}

/// The name a line's card goes by: its own, or its printing's from `names`.
pub fn name_of<'a>(card: &'a CardRef, names: &'a Names) -> Option<&'a str> {
    match card {
        CardRef::Name(name) => Some(name),
        CardRef::Printing(p) => names.get(&p.to_string()).map(String::as_str),
    }
}

/// The line naming `line` holds the card `wanted` asks for. A printing is
/// held only by a line of that printing, since it says which copy it is; a
/// name by any line whose card goes by that name, named or printed.
pub fn holds(line: &CardRef, wanted: &CardRef, names: &Names) -> bool {
    match wanted {
        CardRef::Printing(p) => matches!(line, CardRef::Printing(q) if same_printing(p, q)),
        CardRef::Name(n) => name_of(line, names).is_some_and(|m| same_name(m, n)),
    }
}

fn same_printing(a: &Printing, b: &Printing) -> bool {
    a.set.eq_ignore_ascii_case(&b.set) && a.num == b.num
}
