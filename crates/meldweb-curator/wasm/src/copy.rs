//! The browser's way into the copy of Scryfall (ADR-0030, ADR-0031).
//!
//! The copy itself, its format, builder and lookups, is
//! `chip_scryfall::copy`. What is here is the worker's handle on it: one copy
//! being built and one answering, kept in this thread, and the printing a
//! card named by name gets, which is the default `[printings]` rules'
//! (ADR-0026) since the worker has no `meldweb.toml` to read.

use std::cell::RefCell;

use chip_scryfall::copy::{Builder, Found, ScryfallCopy, Wanted, FORMAT};
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

use crate::preference::Preference;

thread_local! {
    static BUILDING: RefCell<Option<Builder>> = const { RefCell::new(None) };
    static COPY: RefCell<Option<ScryfallCopy>> = const { RefCell::new(None) };
    static RANKING: Preference = Preference::parse(None).expect("the default rules parse");
}

fn with_copy<T>(f: impl FnOnce(&ScryfallCopy, &Preference) -> T) -> Result<T, JsError> {
    RANKING
        .with(|ranking| COPY.with_borrow(|c| c.as_ref().map(|c| f(c, ranking))))
        .ok_or_else(|| JsError::new("the copy of Scryfall is not loaded"))
}

fn building(f: impl FnOnce(&mut Builder)) -> Result<(), JsError> {
    BUILDING
        .with_borrow_mut(|b| b.as_mut().map(f))
        .ok_or_else(|| JsError::new("no copy of Scryfall is being built"))
}

/// [`FORMAT`], for the page to tell a stored copy it cannot read.
#[wasm_bindgen]
pub fn copy_format() -> u32 {
    FORMAT
}

/// Starts a new copy from Default Cards written at `updated_at`.
#[wasm_bindgen]
pub fn copy_begin(updated_at: &str) {
    BUILDING.set(Some(Builder::new(updated_at)));
}

/// Default Cards' lines, whole ones only, in any number of calls.
#[wasm_bindgen]
pub fn copy_feed_cards(lines: &str) -> Result<(), JsError> {
    building(|b| b.cards(lines))
}

/// Oracle Tags' lines, whole ones only, in any number of calls.
#[wasm_bindgen]
pub fn copy_feed_tags(lines: &str) -> Result<(), JsError> {
    building(|b| b.tags(lines))
}

/// Ends the copy being built, makes it the one answering, and returns it as
/// the text to keep. A file with lines that were not cards is refused: a
/// copy missing cards would answer for them with nothing.
#[wasm_bindgen]
pub fn copy_finish() -> Result<String, JsError> {
    let builder = BUILDING
        .take()
        .ok_or_else(|| JsError::new("no copy of Scryfall is being built"))?;
    if let Some(first) = builder.unreadable().first() {
        return Err(JsError::new(&format!(
            "{} lines of Scryfall's bulk data were not cards, the first: {first}",
            builder.unreadable().len()
        )));
    }
    let text = builder.finish().to_text().map_err(|e| JsError::new(&e))?;
    let copy = ScryfallCopy::load(&text).map_err(|e| JsError::new(&e))?;
    COPY.set(Some(copy));
    Ok(text)
}

/// Makes `stored`, what [`copy_finish`] returned once, the copy answering,
/// and says when Scryfall wrote it.
#[wasm_bindgen]
pub fn copy_load(stored: &str) -> Result<String, JsError> {
    let copy = ScryfallCopy::load(stored).map_err(|e| JsError::new(&e))?;
    let at = copy.updated_at().to_string();
    COPY.set(Some(copy));
    Ok(at)
}

/// Each of `wanted`, a JSON list of [`Wanted`], as a [`Found`] or null.
#[wasm_bindgen]
pub fn copy_lookup(wanted: &str) -> Result<String, JsError> {
    let wanted: Vec<Wanted> =
        facet_json::from_str(wanted).map_err(|e| JsError::new(&e.to_string()))?;
    with_copy(|c, ranking| {
        let found: Vec<Option<Found>> = wanted.iter().map(|w| c.find(w, ranking)).collect();
        facet_json::to_string(&found).expect("printings serialise")
    })
}

/// Reads up to `n` more of the copy for search ([`ScryfallCopy::warm`]);
/// whether all of it is read.
#[wasm_bindgen]
pub fn copy_warm(n: usize) -> Result<bool, JsError> {
    with_copy(|c, _| c.warm(n))
}

/// [`ScryfallCopy::prints`], as JSON.
#[wasm_bindgen]
pub fn copy_prints(uri: &str) -> Result<String, JsError> {
    with_copy(|c, _| facet_json::to_string(&c.prints(uri)).expect("printings serialise"))
}

/// [`ScryfallCopy::search`], as JSON.
#[wasm_bindgen]
pub fn copy_search(query: &str, offset: usize, limit: usize) -> Result<String, JsError> {
    with_copy(|c, ranking| {
        facet_json::to_string(&c.search(query, offset, limit, ranking)).expect("serialises")
    })
}

/// [`ScryfallCopy::autocomplete`], as JSON.
#[wasm_bindgen]
pub fn copy_autocomplete(query: &str) -> Result<String, JsError> {
    with_copy(|c, _| facet_json::to_string(&c.autocomplete(query)).expect("names serialise"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!("../../../reality-chip/scryfall/tests/fixtures/scryfall-copy.jsonl");

    /// A card named by name gets the printing the printing picker ranks
    /// first by the same rules, and the default rules put paper first.
    #[test]
    fn a_name_gets_the_printing_the_default_rules_rank_first() {
        let mut b = Builder::new("now");
        b.cards(FIXTURE);
        let copy = ScryfallCopy::load(&b.finish().to_text().unwrap()).unwrap();
        let ranking = Preference::parse(None).unwrap();
        for name in ["Lightning Bolt", "Sol Ring", "Island", "Fire // Ice"] {
            let best = copy
                .find(&Wanted::Name { name: name.into() }, &ranking)
                .unwrap();
            let all = copy.prints(best.prints.as_deref().unwrap());
            assert!(all.len() > 1, "{name} has one printing to pick from");
            let facts: Vec<_> = all.iter().map(|p| p.facts.clone()).collect();
            let (first, _) = ranking.rank(&facts)[0];
            assert_eq!(all[first].printing.id, best.id, "{name}");
            assert_eq!(facts[first].digital, Some(false), "{name}");
        }
    }
}
