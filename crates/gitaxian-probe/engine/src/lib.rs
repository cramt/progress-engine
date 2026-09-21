//! A Rust host for the Delver X card-recognition engine.
//!
//! The engine ships as a JavaScript glue file and an 8.5 MB WebAssembly blob,
//! both downloaded from a vendor that rebuilds them on its own schedule. This
//! crate runs that pair inside a deno_core isolate whose entire host surface is
//! the seventeen ops in [`sandbox`]: no filesystem, no network, no process, no
//! module loader. The blob reads the artefact files only because the host hands
//! it the bytes, and it reaches nothing else at all.
//!
//! [`Engine::open`] downloads what it needs, so there is nothing to run first.
//! Where those files are kept between runs is [`artifacts::ArtifactCache`], and
//! an unconfigured engine keeps them in `/tmp/gitaxian-probe`.
//!
//! ```no_run
//! use gitaxian_probe_engine::{Engine, EngineConfig};
//!
//! let mut engine = Engine::open(EngineConfig::default())?;
//! let rows = engine.query("SELECT n.name FROM data.cards c \
//!                          JOIN data.names n ON n._id = c.name LIMIT 1")?;
//! engine.close();
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod artifacts;
mod job;
mod pump;
mod sandbox;
pub mod wasm;
mod worker;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use deno_core::{v8, JsRuntime};
use facet::Facet;

pub use artifacts::{
    Artifact, ArtifactCache, ArtifactId, Bundle, DirCache, Origin, Source, Version,
};
pub use sandbox::{Progress, ProgressFn};

use sandbox::{Artifacts, HostState, LogSink, RoleState, Stores};
use worker::{SpawnContext, WorkerRegistry};

/// The build this host was verified against. Import names are positional
/// (FINDINGS.md §5), so a changed import surface means an unverified ABI.
pub const KNOWN_FINGERPRINT: &str = "3411ecc782a61347";

// Engine ABI constants, lifted from the app bundle (FINDINGS.md §7/§12). The
// sandbox reads them through `op_probe_abi`, so these are the only copy - the
// glue in js/ no longer hardcodes its own.
pub(crate) const OUTPUT_SLOTS: u32 = 100;
pub(crate) const SEGMENTATION_MASK_BYTES: u32 = 16384;
// _rec_status(): RUNNING is -1, FINISHED_WITH_DETECTIONS is 1.
pub(crate) const REC_RUNNING: i32 = -1;
pub(crate) const REC_FINISHED_WITH_DETECTIONS: i32 = 1;

/// The JWT that unlocks a gated tier. A newtype so that the only way to reach
/// [`Model::Lambda`] or [`Model::Gamma`] is to have produced one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jwt(String);

impl Jwt {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which weights to boot with.
///
/// The token rides on the variants that need it rather than sitting beside
/// them in the config, so a tokenless Gamma is not a runtime check that fires
/// late in `open` - it does not typecheck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Model {
    Alpha,
    Lambda(Jwt),
    Gamma(Jwt),
}

impl Model {
    /// Which file of weights this is, independently of what unlocks it.
    pub fn tier(&self) -> Tier {
        match self {
            Model::Alpha => Tier::Alpha,
            Model::Lambda(_) => Tier::Lambda,
            Model::Gamma(_) => Tier::Gamma,
        }
    }

    pub fn token(&self) -> Option<&Jwt> {
        match self {
            Model::Alpha => None,
            Model::Lambda(t) | Model::Gamma(t) => Some(t),
        }
    }
}

/// One tier of weights, as the artefact store names it. Separate from [`Model`]
/// because fetching `model-lambda.dat` needs the name and not the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Alpha,
    Lambda,
    Gamma,
}

impl Tier {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Tier::Alpha => "alpha",
            Tier::Lambda => "lambda",
            Tier::Gamma => "gamma",
        }
    }
}

pub struct EngineConfig {
    /// Where the engine's files come from and where they are kept between
    /// runs. The default downloads them from the origin into
    /// `/tmp/gitaxian-probe`.
    pub source: Source,
    pub model: Model,
    /// What `navigator.hardwareConcurrency` reports inside the sandbox. It does
    /// not size the pool: this build preallocates 32 pthreads whatever it says.
    pub reported_concurrency: u32,
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
            source: Source::default(),
            model: Model::Alpha,
            reported_concurrency: std::thread::available_parallelism()
                .map_or(4, |n| n.get() as u32),
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
#[derive(Facet, Debug, Clone)]
pub struct Detection {
    pub name: String,
    #[facet(default)]
    pub number: String,
    #[facet(rename = "dataId")]
    pub data_id: i64,
    #[facet(rename = "recConf", default)]
    pub rec_conf: i64,
    #[facet(rename = "setConf", default)]
    pub set_conf: i64,
    /// Top-10 candidates from the embedding index, best first.
    #[facet(default)]
    pub similar: Vec<i64>,
    #[facet(default)]
    pub location: Vec<f64>,
    #[facet(default)]
    pub centroid: Vec<f64>,
    #[facet(rename = "imageFilename", default)]
    pub image_filename: Option<String>,
}

/// A catalogue row resolved from a [`Detection::data_id`].
#[derive(Facet, Debug, Clone)]
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

#[derive(Facet, Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// What `boot` reports back once the engine is up.
#[derive(Facet, Debug)]
struct BootInfo {
    version: String,
}

/// One argument to an engine method. The host never builds JavaScript source,
/// so these are the only shapes a call can carry.
enum Arg<'a> {
    Str(&'a str),
    Num(f64),
}

pub struct Engine {
    js: JsRuntime,
    /// The `__probe` object, looked up once. Calls are property lookups and
    /// invocations on this, never freshly compiled source.
    api: v8::Global<v8::Object>,
    pump: v8::Global<v8::Function>,
    /// Entered around every call: the ops and the worker pool are built on
    /// deno_core's event loop, which needs a reactor in scope.
    tokio: tokio::runtime::Runtime,
    logs: LogSink,
    timeout: Duration,
    version: String,
    fingerprint: String,
    tier: Tier,
    closed: bool,
}

impl Engine {
    /// Bring up a fully initialised engine: catalogue queryable, recogniser
    /// running, model loaded. Returns only once all of that holds, so the
    /// engine handed back is never half-built.
    pub fn open(config: EngineConfig) -> Result<Self> {
        // The glue hardcodes `_rec_alpha_init` and model id 0, so a gated tier
        // would boot by feeding lambda or gamma weights to alpha's init and
        // reporting success. Refusing is the honest answer until the per-tier
        // init export is resolved here (README, *Scope*).
        if config.model.token().is_some() {
            bail!(
                "{} is gated behind _rec_set_jwt_token and its init export is not resolved \
                 host-side yet; only Model::Alpha can boot",
                config.model.tier().name()
            );
        }
        let tier = config.model.tier();

        let bundle = Bundle::fetch(&config.source, tier, config.on_progress.as_deref())
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
            reported_concurrency: config.reported_concurrency,
            epoch,
            logs: logs.clone(),
            failures: Default::default(),
        });
        let failures = spawn.failures.clone();

        let host = HostState {
            artifacts,
            reported_concurrency: config.reported_concurrency,
            epoch,
            logs: logs.clone(),
            tag: "main".into(),
            role: RoleState::Main {
                workers: WorkerRegistry::new(spawn),
                progress: config.on_progress.clone(),
                image: None,
            },
        };

        let mut js = sandbox::build_runtime(stores, host)?;
        let pump = pump::pump_handle(&mut js)?;
        let api = api_handle(&mut js)?;

        // Boot unpacks a 44 MB catalogue and loads a 34 MB model, so it gets
        // more room than an ordinary call.
        let boot_timeout = config.timeout.max(Duration::from_secs(120));
        let boot = call_method(&mut js, &api, "boot", &[Arg::Str(tier.name())])?;
        let info = pump::run_until(&mut js, &pump, boot, boot_timeout)
            .map_err(|e| worker::explain(e, &failures))
            .context("booting the engine")?;
        let info: BootInfo =
            facet_json::from_str(&info).map_err(|e| anyhow!("parsing the boot report: {e}"))?;

        drop(_guard);
        Ok(Self {
            js,
            api,
            pump,
            tokio,
            logs,
            timeout: config.timeout,
            version: info.version,
            fingerprint,
            tier,
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

    pub fn tier(&self) -> Tier {
        self.tier
    }

    /// How many pthread isolates the engine currently has alive. The build
    /// preallocates 32 and `run()` blocks until every one has acknowledged the
    /// shared memory, so a booted engine always reports the full pool.
    pub fn worker_count(&self) -> usize {
        let state = self.js.op_state();
        let mut state = state.borrow_mut();
        state
            .borrow_mut::<HostState>()
            .workers()
            .map_or(0, |w| w.count())
    }

    /// Anything any isolate wrote to `console`, drained. Each line carries the
    /// isolate that produced it. Set `PROBE_LOG=1` to have them printed to
    /// stderr as they happen as well, which is what you want when the thing you
    /// are debugging never returns.
    pub fn take_logs(&self) -> Vec<String> {
        self.logs.take()
    }

    fn call(&mut self, method: &str, args: &[Arg]) -> Result<String> {
        if self.closed {
            bail!("engine is closed");
        }
        let _guard = self.tokio.enter();
        let value = call_method(&mut self.js, &self.api, method, args)?;
        pump::run_until(&mut self.js, &self.pump, value, self.timeout)
    }

    fn call_json<T: for<'a> Facet<'a>>(&mut self, method: &str, args: &[Arg]) -> Result<T> {
        let out = self.call(method, args)?;
        facet_json::from_str(&out).map_err(|e| anyhow!("parsing the result of {method}: {e}"))
    }

    /// Read-only SQL. `data.` is Delver's 124k-card catalogue, `main` the local
    /// collection in user.db, so queries must be schema-qualified.
    ///
    /// Every column arrives as text whatever its declared type, which is the
    /// engine's doing and not a loss of information here.
    pub fn query(&mut self, sql: &str) -> Result<Vec<Vec<String>>> {
        self.call_json("query", &[Arg::Str(sql)])
    }

    /// Statement execution (INSERT/UPDATE/...).
    pub fn exec(&mut self, sql: &str) -> Result<facet_value::Value> {
        self.call_json("exec", &[Arg::Str(sql)])
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
        let text = |i: usize| row.get(i).cloned();
        let or_empty = |i: usize| text(i).unwrap_or_default();
        Ok(Some(Card {
            data_id,
            name: or_empty(0),
            edition: or_empty(1),
            number: or_empty(2),
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
        self.call_json(
            "recognize",
            &[
                Arg::Num(image.width as f64),
                Arg::Num(image.height as f64),
                Arg::Num(MAX_FRAMES),
                Arg::Num(SETTLE_TIMEOUT_MS),
            ],
        )
    }

    /// Find the card quad without identifying it - the crop detector.
    pub fn locate(&mut self, image: &Image) -> Result<Option<Vec<Point>>> {
        image.check()?;
        self.stage_image(image.data)?;
        self.call_json(
            "locate",
            &[Arg::Num(image.width as f64), Arg::Num(image.height as f64)],
        )
    }

    fn stage_image(&mut self, data: &[u8]) -> Result<()> {
        if self.closed {
            bail!("engine is closed");
        }
        let state = self.js.op_state();
        let mut state = state.borrow_mut();
        let host = state.borrow_mut::<HostState>();
        let RoleState::Main { image, .. } = &mut host.role else {
            bail!("only the main isolate stages frames");
        };
        *image = Some(data.to_vec());
        Ok(())
    }

    /// Release the engine and tear down its thread pool.
    ///
    /// [`Drop`] does this too, so calling it is a way to choose *when* the 32
    /// pthreads go away rather than something the caller owes.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _guard = self.tokio.enter();
        let _ = call_method(&mut self.js, &self.api, "close", &[]);
        let state = self.js.op_state();
        let mut state = state.borrow_mut();
        if let Some(workers) = state.borrow_mut::<HostState>().workers() {
            workers.shutdown();
        }
    }
}

/// How many frames `recognize` pushes before giving up, and how long it waits
/// for the detector to settle on each. Both are the app's own values
/// (FINDINGS.md §12).
const MAX_FRAMES: f64 = 12.0;
const SETTLE_TIMEOUT_MS: f64 = 4000.0;

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

fn api_handle(js: &mut JsRuntime) -> Result<v8::Global<v8::Object>> {
    let global = js.execute_script("probe:api-handle", "globalThis.__probe")?;
    deno_core::scope!(scope, js);
    let local = v8::Local::new(scope, global);
    let object = v8::Local::<v8::Object>::try_from(local)
        .map_err(|_| anyhow!("__probe is not an object"))?;
    Ok(v8::Global::new(scope, object))
}

/// Invoke one method on the `__probe` object.
///
/// The host used to reach the engine by `format!`-ing a line of JavaScript and
/// compiling it, which put every argument through a quoting step and paid for a
/// fresh script per call. Arguments are v8 values now, so neither happens.
fn call_method(
    js: &mut JsRuntime,
    api: &v8::Global<v8::Object>,
    method: &str,
    args: &[Arg],
) -> Result<v8::Global<v8::Value>> {
    deno_core::scope!(scope, js);
    let api = v8::Local::new(scope, api);
    let key = v8::String::new(scope, method).ok_or_else(|| anyhow!("out of v8 string space"))?;
    let found = api
        .get(scope, key.into())
        .ok_or_else(|| anyhow!("__probe.{method} could not be read"))?;
    let func = v8::Local::<v8::Function>::try_from(found)
        .map_err(|_| anyhow!("__probe.{method} is not a function"))?;

    let argv = args
        .iter()
        .map(|arg| match arg {
            Arg::Str(s) => v8::String::new(scope, s)
                .map(|s| s.into())
                .ok_or_else(|| anyhow!("out of v8 string space")),
            Arg::Num(n) => Ok(v8::Number::new(scope, *n).into()),
        })
        .collect::<Result<Vec<v8::Local<v8::Value>>>>()?;

    let result = func
        .call(scope, api.into(), &argv)
        .ok_or_else(|| anyhow!("__probe.{method} threw"))?;
    Ok(v8::Global::new(scope, result))
}
