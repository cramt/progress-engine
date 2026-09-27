//! The native host: the engine inside deno_core, with a browser built out of
//! seventeen ops around it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use deno_core::{v8, JsRuntime};
use facet::Facet;

use crate::artifacts::{Bundle, Source};
pub use crate::sandbox::ProgressFn;
use crate::sandbox::{self, Artifacts, HostState, LogSink, RoleState, Stores};
use crate::worker::{self, SpawnContext, WorkerRegistry};
use crate::{
    pump, BootInfo, Card, Detection, Image, Model, Point, Tier, KNOWN_FINGERPRINT, MAX_FRAMES,
    SETTLE_TIMEOUT_MS,
};

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
        let tier = config.model.bootable()?;

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
        let rows = self.query(&Card::sql(data_id))?;
        Ok(Card::from_rows(data_id, &rows))
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
