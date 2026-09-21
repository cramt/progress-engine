//! A Rust host for the Delver X card-recognition engine.
//!
//! The engine ships as a JavaScript glue file and an 8.5 MB WebAssembly blob,
//! both downloaded from a vendor that rebuilds them on its own schedule. This
//! crate runs that pair inside a deno_core isolate whose entire host surface is
//! the fourteen ops in [`sandbox`]: no filesystem, no network, no process, no
//! module loader. The blob reads the artefact files only because the host hands
//! it the bytes, and it reaches nothing else at all.
//!
//! [`Engine::open`] downloads what it needs, so there is nothing to run first.
//! Where those files are kept between runs is [`artifacts::ArtifactCache`], and
//! an unconfigured engine keeps them in `/tmp/delver-engine`.
//!
//! ```no_run
//! use delver_engine::{Engine, EngineConfig};
//!
//! let mut engine = Engine::open(EngineConfig::default())?;
//! let rows = engine.query("SELECT n.name FROM data.cards c \
//!                          JOIN data.names n ON n._id = c.name LIMIT 1")?;
//! engine.close();
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod artifacts;
mod pump;
mod sandbox;
pub mod wasm;
mod worker;

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use deno_core::{v8, JsRuntime};
use serde::{Deserialize, Serialize};

pub use artifacts::{
    Artifact, ArtifactCache, ArtifactId, Bundle, DirCache, Origin, Source, Version,
};
pub use sandbox::Progress;

use sandbox::{Artifacts, HostState, LogSink, Role, Stores};
use worker::{SpawnContext, WorkerRegistry};

/// The build this host was verified against. Import names are positional
/// (FINDINGS.md §5), so a changed import surface means an unverified ABI.
pub const KNOWN_FINGERPRINT: &str = "3411ecc782a61347";

// Engine ABI constants, lifted from the app bundle (FINDINGS.md §7/§12). The
// sandbox reads them through `op_delver_abi`, so these are the only copy - the
// glue in js/ no longer hardcodes its own.
pub(crate) const OUTPUT_SLOTS: u32 = 100;
pub(crate) const SEGMENTATION_MASK_BYTES: u32 = 16384;
// _rec_status(): RUNNING is -1, FINISHED_WITH_DETECTIONS is 1.
pub(crate) const REC_RUNNING: i32 = -1;
pub(crate) const REC_FINISHED_WITH_DETECTIONS: i32 = 1;

/// The tokenless tier, and the two JWT-gated ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    Alpha,
    Lambda,
    Gamma,
}

impl Model {
    fn name(self) -> &'static str {
        match self {
            Model::Alpha => "alpha",
            Model::Lambda => "lambda",
            Model::Gamma => "gamma",
        }
    }
}

pub struct EngineConfig {
    /// Where the engine's files come from and where they are kept between
    /// runs. The default downloads them from the origin into
    /// `/tmp/delver-engine`.
    pub source: Source,
    pub model: Model,
    /// Required for [`Model::Lambda`] and [`Model::Gamma`].
    pub token: Option<String>,
    /// Size of the engine's thread pool hint. The pool itself is fixed at 32
    /// by the build; this is what `navigator.hardwareConcurrency` reports.
    pub cores: u32,
    /// Run even though core.wasm's import surface no longer matches
    /// [`KNOWN_FINGERPRINT`].
    pub allow_unknown_build: bool,
    pub on_progress: Option<Rc<dyn Fn(Progress)>>,
    /// How long any single engine call may take before the host gives up.
    pub timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            source: Source::default(),
            model: Model::Alpha,
            token: None,
            cores: std::thread::available_parallelism().map_or(4, |n| n.get() as u32),
            allow_unknown_build: false,
            on_progress: None,
            timeout: Duration::from_secs(120),
        }
    }
}

/// Raw RGBA. Decoding is the caller's problem, as it is in the Node harness.
pub struct Image<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl Image<'_> {
    fn check(&self) -> Result<()> {
        let want = self.width as usize * self.height as usize * 4;
        if self.data.len() != want {
            bail!(
                "expected {want} bytes of RGBA for {}x{}, got {}",
                self.width,
                self.height,
                self.data.len()
            );
        }
        Ok(())
    }
}

/// One recognised card. `location` and `centroid` are percentages, not pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub name: String,
    #[serde(default)]
    pub number: String,
    #[serde(rename = "dataId")]
    pub data_id: i64,
    #[serde(rename = "recConf", default)]
    pub rec_conf: i64,
    #[serde(rename = "setConf", default)]
    pub set_conf: i64,
    /// Top-10 candidates from the embedding index, best first.
    #[serde(default)]
    pub similar: Vec<i64>,
    #[serde(default)]
    pub location: Vec<f64>,
    #[serde(default)]
    pub centroid: Vec<f64>,
    #[serde(rename = "imageFilename", default)]
    pub image_filename: Option<String>,
}

/// A catalogue row resolved from a [`Detection::data_id`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub data_id: i64,
    pub name: String,
    pub edition: String,
    pub number: String,
    pub rarity: Option<String>,
    pub artist: Option<String>,
    pub scryfall_id: Option<String>,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub struct Engine {
    js: JsRuntime,
    pump: v8::Global<v8::Function>,
    // Held only so the worker threads outlive nothing they need; the registry
    // itself lives in the isolate's OpState.
    _tokio: tokio::runtime::Runtime,
    logs: LogSink,
    timeout: Duration,
    version: String,
    fingerprint: String,
    model: Model,
    closed: bool,
}

impl Engine {
    /// Bring up a fully initialised engine: catalogue queryable, recogniser
    /// running, model loaded. Returns only once all of that holds, so the
    /// engine handed back is never half-built.
    pub fn open(config: EngineConfig) -> Result<Self> {
        let bundle = Bundle::fetch(&config.source, config.model, config.on_progress.as_deref())
            .context("fetching the engine")?;
        let artifacts = Arc::new(Artifacts::new(
            bundle,
            config.allow_unknown_build,
            KNOWN_FINGERPRINT,
        )?);

        let fingerprint = artifacts.fingerprint.clone();
        let stores = Stores::default();
        let epoch = Instant::now();
        let logs = LogSink::default();

        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .context("building the host tokio runtime")?;
        let _guard = tokio.enter();

        let spawn = Arc::new(SpawnContext {
            artifacts: artifacts.clone(),
            stores: stores.clone(),
            cores: config.cores,
            epoch,
            logs: logs.clone(),
        });

        let host = HostState {
            artifacts,
            cores: config.cores,
            epoch,
            progress: config.on_progress.clone(),
            image: None,
            workers: Some(WorkerRegistry::new(spawn)),
            port: None,
            logs: logs.clone(),
            tag: "main".into(),
        };

        let mut js = sandbox::build_runtime(Role::Main, stores, host)?;
        let pump = pump::pump_handle(&mut js)?;

        let boot = js.execute_script(
            "delver:boot",
            format!(
                "__delver.boot({{ model: {} }})",
                json_string(config.model.name()),
            ),
        )?;
        // Boot unpacks a 44 MB catalogue and loads a 34 MB model, so it gets
        // more room than an ordinary call.
        let info = pump::run_until(
            &mut js,
            &pump,
            boot,
            config.timeout.max(Duration::from_secs(120)),
        )
        .context("booting the engine")?;
        let info: serde_json::Value = serde_json::from_str(&info)?;

        drop(_guard);
        Ok(Self {
            js,
            pump,
            _tokio: tokio,
            logs,
            timeout: config.timeout,
            version: info["version"].as_str().unwrap_or("unknown").to_string(),
            fingerprint,
            model: config.model,
            closed: false,
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

    pub fn model(&self) -> Model {
        self.model
    }

    /// How many pthread isolates the engine currently has alive. The build
    /// preallocates 32 and `run()` blocks until every one has acknowledged the
    /// shared memory, so a booted engine always reports the full pool.
    pub fn worker_count(&self) -> usize {
        let state = self.js.op_state();
        let mut state = state.borrow_mut();
        state
            .borrow_mut::<HostState>()
            .workers
            .as_ref()
            .map_or(0, |w| w.count())
    }

    /// Anything any isolate wrote to `console`, drained. Each line carries the
    /// isolate that produced it. Set `DELVER_LOG=1` to have them printed to
    /// stderr as they happen as well, which is what you want when the thing you
    /// are debugging never returns.
    pub fn take_logs(&self) -> Vec<String> {
        self.logs.take()
    }

    fn call(&mut self, code: String) -> Result<String> {
        if self.closed {
            bail!("engine is closed");
        }
        let _guard = self._tokio.enter();
        let value = self.js.execute_script("delver:call", code)?;
        pump::run_until(&mut self.js, &self.pump, value, self.timeout)
    }

    /// Read-only SQL. `data.` is Delver's 124k-card catalogue, `main` the local
    /// collection in user.db, so queries must be schema-qualified.
    pub fn query(&mut self, sql: &str) -> Result<Vec<Vec<serde_json::Value>>> {
        let out = self.call(format!("__delver.query({})", json_string(sql)))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Statement execution (INSERT/UPDATE/...).
    pub fn exec(&mut self, sql: &str) -> Result<serde_json::Value> {
        let out = self.call(format!("__delver.exec({})", json_string(sql)))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Resolve the `dataId` a recognition result carries into catalogue columns.
    pub fn card_by_id(&mut self, data_id: i64) -> Result<Option<Card>> {
        let rows = self.query(&format!(
            "SELECT n.name, e.name, c.number, c.rarity, c.artist, c.scryfall_id, e.release_date \
             FROM data.cards c \
             JOIN data.names n ON n._id = c.name \
             JOIN data.editions e ON e._id = c.edition \
             WHERE c._id = {data_id}"
        ))?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let text = |i: usize| row.get(i).and_then(|v| v.as_str()).map(str::to_string);
        Ok(Some(Card {
            data_id,
            name: text(0).unwrap_or_default(),
            edition: text(1).unwrap_or_default(),
            number: text(2).unwrap_or_default(),
            rarity: text(3),
            artist: text(4),
            scryfall_id: text(5),
            release_date: text(6),
        }))
    }

    /// Identify the cards in a still image.
    ///
    /// The engine is built around a live camera feed, so this pushes the frame
    /// through the streaming detector until it settles. It needs to see the
    /// whole card against a contrasting background - a tight, edge-to-edge crop
    /// gives the detector no boundary to find and returns nothing.
    pub fn recognize(&mut self, image: &Image) -> Result<Vec<Detection>> {
        image.check()?;
        self.stage_image(image.data)?;
        let out = self.call(format!(
            "__delver.recognize({}, {}, 12, 4000)",
            image.width, image.height
        ))?;
        Ok(serde_json::from_str(&out)?)
    }

    /// Find the card quad without identifying it - the crop detector.
    pub fn locate(&mut self, image: &Image) -> Result<Option<Vec<Point>>> {
        image.check()?;
        self.stage_image(image.data)?;
        let out = self.call(format!(
            "__delver.locate({}, {})",
            image.width, image.height
        ))?;
        Ok(serde_json::from_str(&out)?)
    }

    fn stage_image(&mut self, data: &[u8]) -> Result<()> {
        if self.closed {
            bail!("engine is closed");
        }
        let state = self.js.op_state();
        state.borrow_mut().borrow_mut::<HostState>().image = Some(data.to_vec());
        Ok(())
    }

    /// Release the engine and tear down its thread pool. Without this the 32
    /// preallocated pthreads outlive the engine.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _guard = self._tokio.enter();
        let _ = self.js.execute_script("delver:close", "__delver.close()");
        let state = self.js.op_state();
        let mut state = state.borrow_mut();
        if let Some(workers) = state.borrow_mut::<HostState>().workers.as_mut() {
            workers.shutdown();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}
