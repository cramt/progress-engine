//! Many versions of one deck, read as [`Deck::parse`] reads each, but each
//! card line once.
//!
//! A deck's history is hundreds of versions that differ in a line or two,
//! and reading a hundred-card file is milliseconds, so reading every version
//! whole made blame seconds long. The writer lays a deck out one card per
//! line (`  { printing = "woc/145", in = ["Ramp"] },  # Arcane Signet`), so a
//! version in that layout is read as its card lines and the rest of the file,
//! each looked up among the lines already read. The deck is then made by the
//! same [`Deck`] checks as [`Deck::parse`], card by card in file order, so it
//! is the same deck. A version in any other layout, or one either part of
//! which does not read, is read whole, which is also where its error comes
//! from.

use std::collections::HashMap;

use facet::Facet;

use crate::deck::{card, Card, Deck, DeckError, RawCard, RawDeck};
use crate::edit::card_comments;

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct OneCard {
    c: RawCard,
}

/// A deck and the comment on each card's line, by card index: what
/// [`Deck::parse`] and [`card_comments`] make of one text.
pub type Read = (Deck, Vec<Option<String>>);

/// Reads versions of a deck, remembering every card line and every rest of a
/// file it has read.
#[derive(Default)]
pub struct Versions {
    /// Each rest of a file (all but its card lines) read, by its text: the
    /// deck it makes with no cards, and a number for it.
    rests: HashMap<String, Option<(usize, Deck)>>,
    /// Each card line's table read.
    tables: HashMap<String, Option<RawCard>>,
    /// Each card line made into a card against a rest's categories, by the
    /// rest's number. Only cards made; a refused line is refused again, by
    /// the whole file, so that its error names its index.
    cards: HashMap<usize, HashMap<String, Card>>,
}

impl std::fmt::Debug for Versions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Versions")
            .field("rests", &self.rests.len())
            .field("tables", &self.tables.len())
            .finish()
    }
}

/// A card line split into its inline table and the comment after its comma.
fn split_card(line: &str) -> Option<(&str, Option<String>)> {
    let line = line.trim();
    if !line.starts_with('{') {
        return None;
    }
    // The table ends at the last `}` that only a comma and a comment follow;
    // a `}` inside a string or the comment leaves something else after it.
    let mut end = line.len();
    while let Some(close) = line[..end].rfind('}') {
        let after = line[close + 1..].trim_start();
        let after = after.strip_prefix(',').unwrap_or(after).trim_start();
        if after.is_empty() || after.starts_with('#') {
            let comment = after
                .strip_prefix('#')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_owned);
            return Some((&line[..=close], comment));
        }
        end = close;
    }
    None
}

impl Versions {
    pub fn new() -> Self {
        Self::default()
    }

    /// `text` read as [`Deck::parse`] and [`card_comments`] read it.
    pub fn read(&mut self, text: &str) -> Result<Read, DeckError> {
        match self.by_lines(text) {
            Some(read) => read,
            None => {
                let deck = Deck::parse(text)?;
                let mut comments = card_comments(text);
                comments.resize(deck.cards.len(), None);
                Ok((deck, comments))
            }
        }
    }

    /// The deck read line by line, or `None` when `text` is not in the
    /// writer's layout or a part of it does not read on its own.
    fn by_lines(&mut self, text: &str) -> Option<Result<Read, DeckError>> {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let open = lines.iter().position(|l| l.trim() == "cards = [")?;
        let close = open + lines[open..].iter().position(|l| l.trim_end() == "]")?;
        let mut tables = Vec::new();
        let mut comments = Vec::new();
        let mut commas = Vec::new();
        for line in &lines[open + 1..close] {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let (table, comment) = split_card(line)?;
            let comma = t[table.len()..].trim_start().starts_with(',');
            // A comment is read from after the comma, as `card_comments`
            // reads it; a last card without one has none.
            comments.push(comment.filter(|_| comma));
            commas.push(comma);
            tables.push(table);
        }
        // Every card but the last is followed by its comma.
        if commas.iter().rev().skip(1).any(|c| !c) {
            return None;
        }
        let mut rest = String::with_capacity(text.len());
        rest.extend(lines[..open].iter().copied());
        rest.push_str("cards = []\n");
        rest.extend(lines[close + 1..].iter().copied());

        let next = self.rests.len();
        let (id, head) = match self.rests.get(&rest) {
            Some(r) => r.clone()?,
            None => {
                // A rest that reads but is refused, a bad cover say, is read
                // whole, so its error is the one the whole file gives.
                let r = facet_toml::from_str::<RawDeck>(&rest)
                    .ok()
                    .and_then(|raw| Deck::from_raw(raw).ok())
                    .map(|deck| (next, deck));
                self.rests.insert(rest, r.clone());
                r?
            }
        };
        let mut cards = Vec::with_capacity(tables.len());
        for table in tables {
            if let Some(c) = self.cards.get(&id).and_then(|m| m.get(table)) {
                cards.push(c.clone());
                continue;
            }
            let raw = match self.tables.get(table) {
                Some(c) => c.clone()?,
                None => {
                    let c = facet_toml::from_str::<OneCard>(&format!("c = {table}"))
                        .ok()
                        .map(|o| o.c);
                    self.tables.insert(table.to_owned(), c.clone());
                    c?
                }
            };
            // Categories are checked before cards and the cover after them,
            // so with the rest made, the first refused card is the whole
            // file's error.
            let made = card(cards.len() + 1, raw, &head.categories);
            match made {
                Ok(c) => {
                    self.cards
                        .entry(id)
                        .or_default()
                        .insert(table.to_owned(), c.clone());
                    cards.push(c);
                }
                Err(e) => return Some(Err(e)),
            }
        }
        Some(Ok((Deck { cards, ..head }, comments)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same_as_whole(versions: &mut Versions, text: &str) {
        let whole = Deck::parse(text).map(|d| {
            let mut c = card_comments(text);
            c.resize(d.cards.len(), None);
            (d, c)
        });
        assert_eq!(versions.read(text), whole, "{text}");
    }

    #[test]
    fn every_committed_deck_and_edits_of_it_read_as_the_whole_file_does() {
        let mut versions = Versions::new();
        for path in [
            "../../../decks/lantern.deck.toml",
            "../../../decks/loam.deck.toml",
        ] {
            let text = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
            )
            .unwrap();
            assert!(
                versions.by_lines(&text).is_some(),
                "{path} is in the writer's layout"
            );
            same_as_whole(&mut versions, &text);
            // Each card line dropped, doubled or its quantity changed.
            let lines: Vec<&str> = text.lines().collect();
            // Every fifth card line, to keep the whole reads few.
            for (i, l) in lines.iter().enumerate().step_by(5) {
                if !l.trim_start().starts_with('{') {
                    continue;
                }
                let mut without = lines.clone();
                without.remove(i);
                same_as_whole(&mut versions, &(without.join("\n") + "\n"));
                let mut doubled = lines.clone();
                doubled.insert(i, l);
                same_as_whole(&mut versions, &(doubled.join("\n") + "\n"));
                if !l.contains("qty") {
                    let mut more = lines.clone();
                    let bumped = l.replacen(" in = ", " qty = 3, in = ", 1);
                    more[i] = &bumped;
                    same_as_whole(&mut versions, &(more.join("\n") + "\n"));
                }
            }
        }
    }

    #[test]
    fn what_the_layout_does_not_cover_reads_whole_and_fails_as_the_whole_does() {
        let mut versions = Versions::new();
        let cases = [
            // One card over two lines.
            "cards = [\n  { name = \"Sol Ring\",\n    qty = 2 },\n]\n",
            // A missing comma between cards.
            "cards = [\n  { name = \"Sol Ring\" }\n  { name = \"Opt\" },\n]\n",
            // A brace and a hash inside strings.
            "cards = [\n  { name = \"A } # B\", in = [\"x}\"] },  # it\n]\n\n[categories]\n\"x}\" = {}\n",
            // An undeclared category, a bad printing, an unknown key.
            "cards = [\n  { name = \"Sol Ring\", in = [\"Ramp\"] },\n]\n",
            "cards = [\n  { printing = \"nope\" },\n]\n",
            "cards = [\n  { name = \"Sol Ring\", qtty = 2 },\n]\n",
            // Comments and blanks between cards, and no trailing comma.
            "name = \"x\"\ncards = [\n  # ramp\n\n  { name = \"Sol Ring\" },  # one\n  { printing = \"woc/145\" }  # Arcane Signet\n]\n",
            "variant_of = \"a\"\ncards = [\n]\n",
        ];
        for text in cases {
            same_as_whole(&mut versions, text);
        }
    }
}
