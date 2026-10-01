//! The web host as a JavaScript API.
//!
//! [`gitaxian_probe_engine::Engine`] is a Rust type, so a page written in
//! TypeScript cannot hold one. This wraps it in a class wasm-bindgen can hand
//! out, and does on the Rust side the one thing every such page wants after a
//! recognition: resolve each detection, and the engine's own runners-up, to
//! catalogue rows carrying a `scryfall_id` - the join key to everything else in
//! the family.
//!
//! Meldweb Curator's scan dialog is the first caller. What the page must serve
//! and how it must be served is the engine's web host's contract, unchanged:
//! cross-origin isolation, and the `gitaxian-probe-assets` directory at `base`.
//!
//! Empty on native targets, so the workspace still builds and tests there.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use facet::Facet;
use gitaxian_probe_engine::{Card, Engine, EngineConfig, Image};
use js_sys::{Function, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

/// One card found in a frame.
#[derive(Facet)]
struct Found {
    /// The engine's pick.
    card: Card,
    /// The engine's own confidences, 0-100: in the card, and in the printing.
    rec_conf: i64,
    set_conf: i64,
    /// The embedding index's other candidates, best first. The engine reads no
    /// collector number, so a same-art reprint is usually in here rather than
    /// in `card` (FINDINGS.md §12).
    alternatives: Vec<Card>,
}

/// A booted engine. Calls are serialised: one runs at a time, and a call made
/// while another is running is refused rather than queued.
#[wasm_bindgen]
pub struct Scanner {
    engine: Rc<RefCell<Option<Engine>>>,
    version: String,
}

#[wasm_bindgen]
impl Scanner {
    /// Boot the engine from the files served at `base`. `onProgress`, if given,
    /// is called as `(stage, percent, message)`. `files`, if given, is
    /// `{ name: url }` for files that are not at `base` + name. Resolves to a
    /// `Scanner` once the catalogue is queryable and the model loaded.
    pub fn open(
        base: String,
        on_progress: Option<Function>,
        files: Option<js_sys::Object>,
    ) -> Promise {
        future_to_promise(async move {
            let files = files
                .map(|f| {
                    js_sys::Object::entries(&f)
                        .iter()
                        .filter_map(|entry| {
                            let pair = js_sys::Array::from(&entry);
                            Some((pair.get(0).as_string()?, pair.get(1).as_string()?))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let report = on_progress.map(|f| {
                Rc::new(move |p: gitaxian_probe_engine::Progress| {
                    let _ = f.call3(
                        &JsValue::NULL,
                        &p.stage.into(),
                        &p.percent.into(),
                        &p.message.into(),
                    );
                }) as gitaxian_probe_engine::ProgressFn
            });
            let engine = Engine::open(EngineConfig {
                base,
                files,
                on_progress: report,
                ..EngineConfig::default()
            })
            .await
            .map_err(to_js)?;
            Ok(Scanner {
                version: engine.version().to_owned(),
                engine: Rc::new(RefCell::new(Some(engine))),
            }
            .into())
        })
    }

    /// Delver X's build string.
    #[wasm_bindgen(getter)]
    pub fn version(&self) -> String {
        self.version.clone()
    }

    /// Identify the cards in one RGBA frame. Resolves to a JSON array of
    /// `{ card, rec_conf, set_conf, alternatives }`, empty when nothing was
    /// found. The whole card has to be in frame against a contrasting
    /// background: a tight crop gives the detector no edge to find.
    // The borrow is held across the awaits on purpose: it is the lock that
    // makes a second call fail fast instead of interleaving with the first.
    #[allow(clippy::await_holding_refcell_ref)]
    pub fn scan(&self, rgba: Uint8Array, width: u32, height: u32) -> Promise {
        let engine = self.engine.clone();
        let rgba = rgba.to_vec();
        future_to_promise(async move {
            let mut slot = engine
                .try_borrow_mut()
                .map_err(|_| JsError::new("the scanner is busy with another frame"))?;
            let engine = slot
                .as_mut()
                .ok_or_else(|| JsError::new("the scanner is closed"))?;
            let image = Image {
                data: &rgba,
                width,
                height,
            };
            let found = scan(engine, &image).await.map_err(to_js)?;
            let json = facet_json::to_string(&found)
                .map_err(|e| JsError::new(&format!("encoding the result: {e}")))?;
            Ok(json.into())
        })
    }

    /// Release the engine's buffers. Its thread pool lives as long as the page.
    pub fn close(&self) {
        if let Ok(mut slot) = self.engine.try_borrow_mut() {
            slot.take();
        }
    }
}

async fn scan(engine: &mut Engine, image: &Image<'_>) -> anyhow::Result<Vec<Found>> {
    let mut out = Vec::new();
    for detection in engine.recognize(image).await? {
        let Some(card) = engine.card_by_id(detection.data_id).await? else {
            continue;
        };
        let mut alternatives = Vec::new();
        for id in detection.similar {
            if id == detection.data_id {
                continue;
            }
            if let Some(alt) = engine.card_by_id(id).await? {
                alternatives.push(alt);
            }
        }
        out.push(Found {
            card,
            rec_conf: detection.rec_conf,
            set_conf: detection.set_conf,
            alternatives,
        });
    }
    Ok(out)
}

fn to_js(e: anyhow::Error) -> JsValue {
    js_sys::Error::new(&format!("{e:#}")).into()
}
