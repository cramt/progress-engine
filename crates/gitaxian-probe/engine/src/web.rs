//! The web host: the engine as the browser it was built for runs it.
//!
//! Nothing here stands in for a browser. `core.js` loads as a classic script,
//! its 32-thread pool is Web Workers, and `core.wasm` instantiates beside this
//! crate's own module as a sidecar. What Rust keeps is what it keeps natively:
//! the fingerprint check and tag patch on `core.wasm`, the ABI constants, the
//! job decoder and the types. The sequence itself is `js/web.js`.
//!
//! # What the page has to provide
//!
//! - **Cross-origin isolation.** The pool shares one `WebAssembly.Memory`, so
//!   the page needs `SharedArrayBuffer`: serve it with
//!   `Cross-Origin-Opener-Policy: same-origin` and
//!   `Cross-Origin-Embedder-Policy: require-corp`. Without them `open` fails
//!   before the engine starts.
//! - **The files, from its own origin.** Delver's origin sends no CORS
//!   headers, and neither does the archive on ghcr.io, so the page serves its
//!   own copy, or proxies the archive: the files as upstream shipped them,
//!   which `gitaxian-probe-assets` lays out, at [`EngineConfig::base`]. The
//!   model comes packed, and is unpacked here, in the page.
//!
//! Every call is a future, and `open` can run on the page or in a dedicated
//! worker. The pool's workers cannot be stopped from outside (FINDINGS.md §6),
//! so [`Engine::close`] frees the engine's buffers and the workers go with the
//! page.

use std::rc::Rc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use facet::Facet;
use js_sys::{Function, Promise, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::{
    job, wasm, Card, Detection, Image, Point, Progress, Tier, KNOWN_FINGERPRINT, MAX_FRAMES,
    SETTLE_TIMEOUT_MS,
};

/// Progress callbacks are `Rc`: the web host is one thread, and a callback is
/// likely to hold something that is not `Send`, like a signal or a DOM node.
pub type ProgressFn = Rc<dyn Fn(Progress)>;

/// The glue's side of each callback: what it calls, as JavaScript sees it.
type DecodeJs = Closure<dyn Fn(Uint8Array) -> Result<String, JsValue>>;
type ProgressJs = Closure<dyn Fn(String, f64, String)>;

#[wasm_bindgen(module = "/js/web.js")]
extern "C" {
    type Probe;

    #[wasm_bindgen(catch)]
    fn open(
        base: &str,
        files: &js_sys::Object,
        wasm: Uint8Array,
        model: Uint8Array,
        catalogue: &js_sys::Array,
        tier: &str,
        abi: JsValue,
        decode: &Function,
        progress: Option<Function>,
        timeout_ms: f64,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(catch, js_name = fetchAll)]
    fn fetch_all(urls: &js_sys::Array, progress: Option<Function>) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, getter)]
    fn version(this: &Probe) -> String;

    #[wasm_bindgen(method, catch)]
    fn query(this: &Probe, sql: &str) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn exec(this: &Probe, sql: &str) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn locate(this: &Probe, rgba: Uint8Array, width: u32, height: u32) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn recognize(
        this: &Probe,
        rgba: Uint8Array,
        width: u32,
        height: u32,
        max_frames: f64,
        settle_timeout_ms: f64,
    ) -> Result<Promise, JsValue>;

    #[wasm_bindgen(method)]
    fn close(this: &Probe);
}

pub struct EngineConfig {
    /// Where the page serves the files `gitaxian-probe-assets` lays out,
    /// resolved against the page: `"probe/"`, `"/static/probe/"`, or a full
    /// URL on the same origin.
    pub base: String,
    /// Where a file is, by name, when it is not at `base` + name: for a page
    /// that fetches each file by its digest, as Meldweb Curator's proxy of the
    /// archive does. Resolved against the page as `base` is.
    pub files: Vec<(String, String)>,
    pub model: Tier,
    /// Run even though core.wasm's import surface no longer matches
    /// [`KNOWN_FINGERPRINT`].
    pub allow_unknown_build: bool,
    pub on_progress: Option<ProgressFn>,
    /// How long any single engine call may take before the host gives up.
    pub timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            base: "gitaxian-probe/".into(),
            files: Vec::new(),
            model: Tier::Alpha,
            allow_unknown_build: false,
            on_progress: None,
            timeout: Duration::from_secs(120),
        }
    }
}

pub struct Engine {
    probe: Probe,
    version: String,
    fingerprint: String,
    tier: Tier,
    closed: bool,
    // The glue holds these as plain functions; they are owned here so they
    // live exactly as long as the engine that calls them.
    _decode: DecodeJs,
    _progress: Option<ProgressJs>,
}

impl Engine {
    /// Bring up a fully initialised engine: catalogue queryable, recogniser
    /// running, model loaded. Resolves only once all of that holds, so the
    /// engine handed back is never half-built.
    pub async fn open(config: EngineConfig) -> Result<Self> {
        let tier = config.model;
        // Checked first because nothing later fails cleanly without it: core.js
        // throws inside a Worker postMessage and the boot never settles.
        if let Some(why) = isolation_problem() {
            bail!("{why}");
        }
        let base = if config.base.ends_with('/') {
            config.base
        } else {
            format!("{}/", config.base)
        };

        let files = js_sys::Object::new();
        for (name, url) in &config.files {
            js_sys::Reflect::set(&files, &name.into(), &url.into()).map_err(js_error)?;
        }
        let url = |name: &str| {
            config
                .files
                .iter()
                .find(|(n, _)| n == name)
                .map_or_else(|| format!("{base}{name}"), |(_, url)| url.clone())
        };

        let progress = config.on_progress.map(|report| {
            ProgressJs::new(move |stage: String, percent: f64, message: String| {
                report(Progress {
                    stage,
                    percent: percent.clamp(0.0, 100.0) as u32,
                    message,
                })
            })
        });
        let progress_fn = || {
            progress
                .as_ref()
                .map(|c| c.as_ref().unchecked_ref::<Function>().clone())
        };

        // Every file the boot reads comes down in one go: the wait is the
        // slowest file rather than the sum, and the page sees bytes arrive.
        let packed_name = format!("model-{}.7z", tier.name());
        let size_name = format!("model-{}.size", tier.name());
        let names = [
            "core.wasm",
            &packed_name,
            &size_name,
            "data.7z",
            "data.md5",
            "data.size",
            "version.txt",
        ];
        let urls: js_sys::Array = names.iter().map(|n| JsValue::from(url(n))).collect();
        let fetched: js_sys::Array = settle(fetch_all(&urls, progress_fn()))
            .await
            .context("fetching the engine")?
            .unchecked_into();
        let bytes = |i: u32| Uint8Array::new(&fetched.get(i)).to_vec();
        let catalogue = fetched.slice(3, 7);

        let model = unpack_model(&bytes(1), &bytes(2), tier)?;
        let (fingerprint, patched) =
            wasm::admit(&bytes(0), KNOWN_FINGERPRINT, config.allow_unknown_build)?;

        let decode =
            Closure::<dyn Fn(Uint8Array) -> Result<String, JsValue>>::new(|bytes: Uint8Array| {
                job::decode_to_json(&bytes.to_vec())
                    .map_err(|e| js_sys::Error::new(&format!("{e:#}")).into())
            });

        let abi = js_sys::JSON::parse(&crate::abi_json()).map_err(js_error)?;
        let probe: Probe = settle(open(
            &base,
            &files,
            Uint8Array::from(patched.as_slice()),
            Uint8Array::from(model.as_slice()),
            &catalogue,
            tier.name(),
            abi,
            decode.as_ref().unchecked_ref(),
            progress_fn(),
            config.timeout.as_millis() as f64,
        ))
        .await
        .context("booting the engine")?
        .unchecked_into();

        Ok(Self {
            version: probe.version(),
            probe,
            fingerprint,
            tier,
            closed: false,
            _decode: decode,
            _progress: progress,
        })
    }

    /// Upstream build version, from `version.txt`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The import-surface fingerprint this run was checked against.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    fn alive(&self) -> Result<()> {
        if self.closed {
            bail!("engine is closed");
        }
        Ok(())
    }

    async fn call_json<T: for<'a> Facet<'a>>(
        &self,
        method: &str,
        call: Result<Promise, JsValue>,
    ) -> Result<T> {
        let out = settle(call).await?;
        let out = out
            .as_string()
            .ok_or_else(|| anyhow!("{method} returned something other than JSON"))?;
        facet_json::from_str(&out).map_err(|e| anyhow!("parsing the result of {method}: {e}"))
    }

    /// Read-only SQL. `data.` is Delver's 124k-card catalogue, `main` the local
    /// collection in user.db, so queries must be schema-qualified.
    ///
    /// Every column arrives as text whatever its declared type, which is the
    /// engine's doing and not a loss of information here.
    pub async fn query(&mut self, sql: &str) -> Result<Vec<Vec<String>>> {
        self.alive()?;
        self.call_json("query", self.probe.query(sql)).await
    }

    /// Statement execution (INSERT/UPDATE/...).
    pub async fn exec(&mut self, sql: &str) -> Result<facet_value::Value> {
        self.alive()?;
        self.call_json("exec", self.probe.exec(sql)).await
    }

    /// Resolve the `dataId` a recognition result carries into catalogue columns.
    pub async fn card_by_id(&mut self, data_id: i64) -> Result<Option<Card>> {
        let rows = self.query(&Card::sql(data_id)).await?;
        Ok(Card::from_rows(data_id, &rows))
    }

    /// Identify the cards in a still image.
    ///
    /// The engine is built around a live camera feed, so this pushes the frame
    /// through the streaming detector until it settles. It needs to see the
    /// whole card against a contrasting background - a tight, edge-to-edge crop
    /// gives the detector no boundary to find and returns nothing.
    pub async fn recognize(&mut self, image: &Image<'_>) -> Result<Vec<Detection>> {
        self.alive()?;
        image.check()?;
        let call = self.probe.recognize(
            Uint8Array::from(image.data),
            image.width,
            image.height,
            MAX_FRAMES,
            SETTLE_TIMEOUT_MS,
        );
        self.call_json("recognize", call).await
    }

    /// Find the card quad without identifying it - the crop detector.
    pub async fn locate(&mut self, image: &Image<'_>) -> Result<Option<Vec<Point>>> {
        self.alive()?;
        image.check()?;
        let call = self
            .probe
            .locate(Uint8Array::from(image.data), image.width, image.height);
        self.call_json("locate", call).await
    }

    /// Release the engine's buffers. [`Drop`] does this too.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.probe.close();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

/// Await a promise the glue returned, or the error it threw getting there.
async fn settle(call: Result<Promise, JsValue>) -> Result<JsValue> {
    JsFuture::from(call.map_err(js_error)?)
        .await
        .map_err(js_error)
}

fn js_error(e: JsValue) -> anyhow::Error {
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        return anyhow!("{}", String::from(err.message()));
    }
    anyhow!("{}", e.as_string().unwrap_or_else(|| format!("{e:?}")))
}

/// Why the page cannot run the engine's thread pool, if it cannot.
fn isolation_problem() -> Option<&'static str> {
    let isolated = js_sys::Reflect::get(&js_sys::global(), &"crossOriginIsolated".into())
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    (!isolated).then_some(
        "the page is not cross-origin isolated, so the engine's thread pool has no \
         SharedArrayBuffer; serve it with Cross-Origin-Opener-Policy: same-origin and \
         Cross-Origin-Embedder-Policy: require-corp",
    )
}

/// The weights out of upstream's archive, checked against the size sidecar:
/// short weights do not crash the engine, they recognise the wrong card.
fn unpack_model(packed: &[u8], size: &[u8], tier: Tier) -> Result<Vec<u8>> {
    let name = format!("model-{}.dat", tier.name());
    let weights =
        crate::packed::unpack(packed, &name).with_context(|| format!("unpacking {name}"))?;
    let want: usize = String::from_utf8_lossy(size)
        .trim()
        .parse()
        .with_context(|| format!("model-{}.size is not a byte count", tier.name()))?;
    if weights.len() != want {
        bail!(
            "{name} unpacked to {} bytes, its sidecar says {want}",
            weights.len()
        );
    }
    Ok(weights)
}
