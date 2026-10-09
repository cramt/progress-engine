//! The browser's way into the copy of Scryfall (ADR-0030, ADR-0031).
//!
//! The copy itself, its format, builder and lookups, is
//! `chip_scryfall::copy`. What is here is the worker's handle on it: one copy
//! being built and one answering, kept in this thread, and the printing a
//! card named by name gets, which is the default `[printings]` rules'
//! (ADR-0026) since the worker has no `meldweb.toml` to read.
//!
//! The `store_*` functions are the worker's handle on
//! `chip_scryfall::copy::store`: which slot to open or build into, whether a
//! copy is due a look, and what meta to write. The worker does the reading,
//! writing and downloading they name, and decides none of it.

use std::cell::RefCell;

use chip_scryfall::copy::store::{Build, Meta, Plan, Store, UnixMs, LOOK, META, UNREADABLE};
use chip_scryfall::copy::{Builder, Found, ScryfallCopy, Wanted};
use facet::Facet;
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

use crate::preference::Preference;

thread_local! {
    static BUILDING: RefCell<Option<Builder>> = const { RefCell::new(None) };
    static COPY: RefCell<Option<ScryfallCopy>> = const { RefCell::new(None) };
    static RANKING: Preference = Preference::parse(None).expect("the default rules parse");
    static STORE: RefCell<Store> = RefCell::new(Store::new());
    static PLANNED: RefCell<Option<Build>> = const { RefCell::new(None) };
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

/// What [`store_plan`] decided, for the worker to carry out.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum StoreStep {
    /// Scryfall has nothing newer. Write `meta` to the meta file if given.
    Current {
        #[facet(default, skip_serializing_if = Option::is_none)]
        meta: Option<String>,
    },
    /// Scryfall's file is one this build already refused.
    Refused { message: String },
    /// A copy is begun: feed it the bulk files, then write
    /// [`copy_finish`]'s text to `file`, then [`store_built`]'s to the meta
    /// file. If feeding or finishing refuses the file, write
    /// [`store_unreadable`]'s text to the unreadable file instead.
    Build { file: String },
}

fn kept(meta: Option<String>) -> Option<Meta> {
    meta.and_then(|m| Meta::parse(&m).ok())
}

fn planned() -> Result<Build, JsError> {
    PLANNED
        .take()
        .ok_or_else(|| JsError::new("no copy of Scryfall was planned"))
}

/// `Date.now()`, which is a whole number of milliseconds.
fn unix_ms(now: f64) -> UnixMs {
    UnixMs(now as u64)
}

/// The file naming the slot that holds a whole copy.
#[wasm_bindgen]
pub fn store_meta_file() -> String {
    META.into()
}

/// The file remembering a Default Cards file this build refused.
#[wasm_bindgen]
pub fn store_unreadable_file() -> String {
    UNREADABLE.into()
}

/// How often an open tab asks [`store_due`], in milliseconds.
#[wasm_bindgen]
pub fn store_look_ms() -> f64 {
    LOOK.0 as f64
}

/// The slot file to open, given the meta file's text: the copy it names, if
/// it is one this build reads and newer than the one answering.
#[wasm_bindgen]
pub fn store_newer(meta: Option<String>) -> Option<String> {
    let kept = kept(meta);
    STORE.with_borrow(|s| s.newer(kept.as_ref()).map(|k| k.slot().file().into()))
}

/// The copy `meta`, which [`store_newer`] named, is open and answering.
#[wasm_bindgen]
pub fn store_hold(meta: &str) -> Result<(), JsError> {
    let meta = Meta::parse(meta).map_err(|e| JsError::new(&e.to_string()))?;
    STORE.with_borrow_mut(|s| s.hold(meta));
    Ok(())
}

/// Whether the copy answering is old enough to ask `/bulk-data` about.
#[wasm_bindgen]
pub fn store_due(now: f64) -> bool {
    STORE.with_borrow(|s| s.due(unix_ms(now)))
}

/// What to do about Scryfall's listing, a [`StoreStep`] as JSON: `meta` is
/// the meta file's text read in this look, `listing` the `updated_at`
/// `/bulk-data` gives for Default Cards, and `unreadable` the unreadable
/// file's text.
#[wasm_bindgen]
pub fn store_plan(
    meta: Option<String>,
    listing: &str,
    unreadable: Option<String>,
    now: f64,
) -> String {
    let kept = kept(meta);
    let plan = STORE
        .with_borrow_mut(|s| s.plan(kept.as_ref(), listing, unreadable.as_deref(), unix_ms(now)));
    let step = match plan {
        Plan::Current { write } => StoreStep::Current {
            meta: write.map(|m| m.to_text()),
        },
        Plan::Refused { updated_at } => StoreStep::Refused {
            message: format!("this copy's reader cannot read Scryfall's file of {updated_at}"),
        },
        Plan::Build(build) => {
            let file = build.slot().file().into();
            BUILDING.set(Some(Builder::new(build.updated_at())));
            PLANNED.set(Some(build));
            StoreStep::Build { file }
        }
    };
    facet_json::to_string(&step).expect("a step serialises")
}

/// The copy [`store_plan`] began is finished and answering, and its text is
/// in its slot: the meta file's text, to write now.
#[wasm_bindgen]
pub fn store_built(now: f64) -> Result<String, JsError> {
    let build = planned()?;
    Ok(STORE.with_borrow_mut(|s| s.built(build, unix_ms(now)).to_text()))
}

/// The copy [`store_plan`] began refused Scryfall's file: the unreadable
/// file's text, to write now.
#[wasm_bindgen]
pub fn store_unreadable() -> Result<String, JsError> {
    let build = planned()?;
    Ok(STORE.with_borrow_mut(|s| s.unreadable(build)))
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
