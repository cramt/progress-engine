//! The sandbox: a V8 isolate with exactly the host surface `core.js` needs and
//! nothing else.
//!
//! deno_core hands us a bare isolate - no filesystem, no network, no process,
//! no module loader. Every capability below is one the Emscripten glue cannot
//! run without, and the list is the whole security story: the downloaded blob
//! reaches the host only through these ops.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use anyhow::{bail, Result};
use deno_core::{
    op2, CompiledWasmModuleStore, Extension, JsRuntime, OpState, RuntimeOptions,
    SharedArrayBufferStore,
};
use deno_error::JsErrorBox;

use crate::artifacts::Bundle;
use crate::worker::{WorkerPort, WorkerRegistry};

/// Progress report from the boot sequence.
#[derive(Debug, Clone)]
pub struct Progress {
    pub stage: String,
    pub percent: u32,
    pub message: String,
}

/// Where `console` inside the sandbox ends up.
///
/// Every isolate shares one of these, so a line from a pthread lands in the
/// same place as a line from the main isolate. Buffered for `take_logs`, and
/// mirrored to stderr when `PROBE_LOG` is set - a buffer you can only read
/// once the call returns is no use when the call is the thing that hung.
///
/// The tee is the host's decision, not new reach for the blob: `op_probe_log`
/// was already there, and stdout/stderr stay unreachable from inside.
#[derive(Clone, Default)]
pub struct LogSink(Arc<Mutex<Vec<String>>>);

impl LogSink {
    pub(crate) fn push(&self, tag: &str, line: String) {
        let line = format!("[{tag}] {line}");
        if log_to_stderr() {
            eprintln!("{line}");
        }
        self.0.lock().unwrap().push(line);
    }

    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

fn log_to_stderr() -> bool {
    static LIVE: OnceLock<bool> = OnceLock::new();
    *LIVE.get_or_init(|| std::env::var("PROBE_LOG").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// The engine's files, in memory, plus the two things the host derives from
/// them before the blob is allowed to see anything.
pub struct Artifacts {
    pub bundle: Bundle,
    /// core.wasm with its two internal tags exported (FINDINGS.md §10). The
    /// bundle keeps the bytes upstream actually shipped.
    pub wasm: Vec<u8>,
    pub fingerprint: String,
}

impl Artifacts {
    pub fn new(bundle: Bundle, allow_unknown_build: bool, known: &str) -> Result<Self> {
        let fingerprint = crate::wasm::import_fingerprint(&bundle.core_wasm)?;
        if fingerprint != known && !allow_unknown_build {
            bail!(
                "core.wasm import surface is {fingerprint}, expected {known}. The engine was \
                 rebuilt and its ABI is unverified; re-check FINDINGS.md, then set \
                 allow_unknown_build to run anyway."
            );
        }

        Ok(Self {
            wasm: crate::wasm::export_internal_tags(&bundle.core_wasm)?,
            bundle,
            fingerprint,
        })
    }

    /// What `op_probe_artifact` hands back. The bundle is the allowlist: it
    /// holds exactly the files the engine may read, so an unknown name - any
    /// traversal, any absolute path - has nothing to resolve to.
    fn read(&self, name: &str) -> Vec<u8> {
        self.bundle
            .file(name)
            .map(<[u8]>::to_vec)
            .unwrap_or_default()
    }
}

/// Progress callbacks are `Arc` and `Send` so that `EngineConfig` can be built
/// on one thread and handed to the thread the engine will live on - which a
/// GUI has to do, because `JsRuntime` pins its isolate to its thread.
pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

/// What an isolate can reach that depends on which isolate it is.
///
/// The main isolate owns the pool, reports progress and stages frames; a
/// pthread isolate has a port and none of that. Held as four `Option` fields
/// those were sixteen combinations with two worth having, and every op that
/// wanted one paid an unwrap for a state the other role could not be in. As a
/// sum it is the role itself, which is why `build_runtime` no longer takes a
/// `Role` beside the state that already says what the role is.
pub enum RoleState {
    Main {
        workers: WorkerRegistry,
        progress: Option<ProgressFn>,
        /// The frame staged for the next recognition call. Taken, not copied:
        /// an image is used once.
        image: Option<Vec<u8>>,
    },
    Pthread {
        port: WorkerPort,
    },
}

/// Everything an isolate's ops can reach.
pub struct HostState {
    pub artifacts: Arc<Artifacts>,
    /// What `navigator.hardwareConcurrency` reports inside the sandbox. This
    /// does not size the pool; the build fixes that at 32.
    pub reported_concurrency: u32,
    pub epoch: Instant,
    pub logs: LogSink,
    /// Which isolate this is, prefixed onto its log lines: `main`, or `pthread3`.
    pub tag: String,
    pub role: RoleState,
}

impl HostState {
    pub fn workers(&mut self) -> Option<&mut WorkerRegistry> {
        match &mut self.role {
            RoleState::Main { workers, .. } => Some(workers),
            RoleState::Pthread { .. } => None,
        }
    }

    fn port(&self) -> Option<&WorkerPort> {
        match &self.role {
            RoleState::Pthread { port } => Some(port),
            RoleState::Main { .. } => None,
        }
    }

    fn take_image(&mut self) -> Option<Vec<u8>> {
        match &mut self.role {
            RoleState::Main { image, .. } => image.take(),
            RoleState::Pthread { .. } => None,
        }
    }

    fn progress(&self) -> Option<ProgressFn> {
        match &self.role {
            RoleState::Main { progress, .. } => progress.clone(),
            RoleState::Pthread { .. } => None,
        }
    }
}

/// Cross-isolate stores. These are what let a structured clone carry the shared
/// `WebAssembly.Memory` and the compiled module from the main isolate into a
/// pthread isolate - the whole reason the 32-thread pool can exist at all.
#[derive(Clone, Default)]
pub struct Stores {
    pub sab: SharedArrayBufferStore,
    pub wasm: CompiledWasmModuleStore,
}

// ---- ops -------------------------------------------------------------------

#[op2(fast)]
fn op_probe_log(state: &mut OpState, #[smi] level: u8, #[string] message: String) {
    let host = state.borrow::<HostState>();
    let line = if level == 0 {
        message
    } else {
        format!("[warn] {message}")
    };
    host.logs.push(&host.tag, line);
}

#[op2(fast)]
fn op_probe_now(state: &mut OpState) -> f64 {
    state.borrow::<HostState>().epoch.elapsed().as_secs_f64() * 1000.0
}

/// Wall-clock milliseconds at the shared epoch. Emscripten's
/// `_emscripten_get_now` is `performance.timeOrigin + performance.now()`, and
/// every isolate has to agree on both halves: the engine compares these
/// timestamps across threads.
#[op2(fast)]
fn op_probe_time_origin(state: &mut OpState) -> f64 {
    let epoch = state.borrow::<HostState>().epoch;
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    now_unix - epoch.elapsed().as_secs_f64() * 1000.0
}

#[op2(fast)]
#[smi]
fn op_probe_cores(state: &mut OpState) -> u32 {
    state.borrow::<HostState>().reported_concurrency
}

#[op2(fast)]
fn op_probe_random(#[buffer] out: &mut [u8]) {
    // The engine seeds its RNG through crypto.getRandomValues; nothing here is
    // used for anything the host cares about cryptographically.
    for byte in out.iter_mut() {
        *byte = rand_byte();
    }
}

fn rand_byte() -> u8 {
    use std::cell::Cell;
    thread_local! {
        static SEED: Cell<u64> = const { Cell::new(0) };
    }
    SEED.with(|seed| {
        let mut x = seed.get();
        if x == 0 {
            x = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9e37_79b9_7f4a_7c15)
                | 1;
        }
        // xorshift64*: the engine only needs unpredictable-enough bytes.
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        seed.set(x);
        (x >> 33) as u8
    })
}

#[op2]
#[buffer]
fn op_probe_artifact(state: &mut OpState, #[string] name: String) -> Vec<u8> {
    state.borrow::<HostState>().artifacts.read(&name)
}

#[op2]
#[buffer]
fn op_probe_wasm(state: &mut OpState) -> Vec<u8> {
    state.borrow::<HostState>().artifacts.wasm.clone()
}

/// The engine ABI constants, so the glue reads them from here rather than
/// keeping its own copy. Per-tier config (init export, model id) is resolved in
/// Rust too and passed to `boot`; this is the tier-independent half.
#[op2]
#[string]
fn op_probe_abi() -> String {
    format!(
        r#"{{"outputSlots":{},"maskBytes":{},"recRunning":{},"recFinishedWithDetections":{}}}"#,
        crate::OUTPUT_SLOTS,
        crate::SEGMENTATION_MASK_BYTES,
        crate::REC_RUNNING,
        crate::REC_FINISHED_WITH_DETECTIONS,
    )
}

#[op2(fast)]
fn op_probe_progress(
    state: &mut OpState,
    #[string] stage: String,
    #[smi] percent: u32,
    #[string] message: String,
) {
    let Some(report) = state.borrow::<HostState>().progress() else {
        return;
    };
    report(Progress {
        stage,
        percent,
        message,
    });
}

/// Decode one `main.output` / `worker_N.output` blob into JSON.
///
/// The bytes live in the wasm filesystem, so the glue is the only one who can
/// read them - but MessagePack is the host's to parse (see [`crate::job`]), not
/// something to walk tag by tag in JavaScript.
#[op2]
#[string]
fn op_probe_decode_job(state: &mut OpState, #[buffer] bytes: &[u8]) -> Result<String, JsErrorBox> {
    crate::job::decode_to_json(bytes).map_err(|e| {
        // The glue's drain loop treats any throw as "not ready yet" and backs
        // off, so without this a malformed result is an unexplained timeout.
        let host = state.borrow::<HostState>();
        host.logs.push(&host.tag, format!("{e:#}"));
        JsErrorBox::generic(format!("{e:#}"))
    })
}

/// Hand the sandbox the frame the host staged for this call. Taken, not
/// copied: an image is used once.
#[op2]
#[buffer]
fn op_probe_take_image(state: &mut OpState) -> Vec<u8> {
    state
        .borrow_mut::<HostState>()
        .take_image()
        .unwrap_or_default()
}

// ---- worker ops (main isolate) ---------------------------------------------

#[op2(fast)]
#[smi]
fn op_probe_worker_spawn(state: &mut OpState, #[string] name: String) -> Result<u32, JsErrorBox> {
    let workers = state
        .borrow_mut::<HostState>()
        .workers()
        .ok_or_else(|| JsErrorBox::generic("this isolate cannot spawn workers"))?;
    workers
        .spawn(&name)
        .map_err(|e| JsErrorBox::generic(e.to_string()))
}

#[op2(fast)]
fn op_probe_worker_post(
    state: &mut OpState,
    #[smi] id: u32,
    #[buffer] data: &[u8],
) -> Result<(), JsErrorBox> {
    let workers = state
        .borrow_mut::<HostState>()
        .workers()
        .ok_or_else(|| JsErrorBox::generic("this isolate has no workers"))?;
    workers.post(id, data.to_vec());
    Ok(())
}

/// Next message from that worker, or an empty buffer when none is pending.
#[op2]
#[buffer]
fn op_probe_worker_recv(state: &mut OpState, #[smi] id: u32) -> Vec<u8> {
    state
        .borrow_mut::<HostState>()
        .workers()
        .and_then(|w| w.recv(id))
        .unwrap_or_default()
}

#[op2(fast)]
fn op_probe_worker_terminate(state: &mut OpState, #[smi] id: u32) {
    if let Some(workers) = state.borrow_mut::<HostState>().workers() {
        workers.terminate(id);
    }
}

// ---- worker ops (pthread isolate) ------------------------------------------

#[op2(fast)]
fn op_probe_self_post(state: &mut OpState, #[buffer] data: &[u8]) {
    if let Some(port) = state.borrow::<HostState>().port() {
        port.post(data.to_vec());
    }
}

#[op2]
#[buffer]
fn op_probe_self_recv(state: &mut OpState) -> Vec<u8> {
    state
        .borrow::<HostState>()
        .port()
        .and_then(|p| p.recv())
        .unwrap_or_default()
}

// ---- runtime construction --------------------------------------------------

const OPS: &[deno_core::OpDecl] = &[
    op_probe_log(),
    op_probe_now(),
    op_probe_time_origin(),
    op_probe_cores(),
    op_probe_random(),
    op_probe_artifact(),
    op_probe_wasm(),
    op_probe_abi(),
    op_probe_progress(),
    op_probe_take_image(),
    op_probe_decode_job(),
    op_probe_worker_spawn(),
    op_probe_worker_post(),
    op_probe_worker_recv(),
    op_probe_worker_terminate(),
    op_probe_self_post(),
    op_probe_self_recv(),
];

/// deno_core's own builtins ship with the isolate. Most are plumbing the
/// runtime needs - structured clone, promise inspection, type predicates - but
/// a handful reach the host, and the blob has no business with any of them.
/// The resource table this host registers nothing into is closed off too, so
/// the read/write ops cannot become a capability if something later does.
const DISABLED_BUILTINS: &[&str] = &[
    "op_panic", // aborts the host process
    "op_print", // host stdout/stderr
    "op_pipe",
    "op_is_terminal",
    "op_read",
    "op_read_all",
    "op_read_sync",
    "op_write",
    "op_write_all",
    "op_write_sync",
    "op_shutdown",
    "op_close",
    "op_try_close",
    "op_resources",
];

pub fn build_runtime(stores: Stores, host: HostState) -> Result<JsRuntime> {
    let core_js = host.artifacts.bundle.core_js.clone();
    let is_main = matches!(host.role, RoleState::Main { .. });
    let ext = Extension {
        name: "gitaxian_probe",
        ops: std::borrow::Cow::Borrowed(OPS),
        op_state_fn: Some(Box::new(move |state: &mut OpState| {
            state.put(host);
        })),
        middleware_fn: Some(Box::new(|op| {
            if DISABLED_BUILTINS.contains(&op.name) {
                op.disable()
            } else {
                op
            }
        })),
        ..Default::default()
    };

    let mut js = JsRuntime::new(RuntimeOptions {
        extensions: vec![ext],
        shared_array_buffer_store: Some(stores.sab),
        compiled_wasm_module_store: Some(stores.wasm),
        ..Default::default()
    });

    js.execute_script("probe:bootstrap.js", include_str!("../js/bootstrap.js"))?;
    if is_main {
        js.execute_script(
            "probe:main-prelude.js",
            include_str!("../js/main-prelude.js"),
        )?;
    } else {
        js.execute_script(
            "probe:worker-prelude.js",
            include_str!("../js/worker-prelude.js"),
        )?;
    }

    // The untrusted blob. It is a classic script, so `createCore` lands on the
    // global; in a pthread isolate it self-invokes off globalThis.name.
    js.execute_script("probe:core.js", core_js)?;

    if is_main {
        js.execute_script("probe:engine.js", include_str!("../js/engine.js"))?;
    }
    Ok(js)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(script: &str) -> Option<String> {
        let source = crate::Source::default();
        let bundle = match Bundle::fetch(&source, crate::Tier::Alpha, None) {
            Ok(bundle) => bundle,
            Err(e) => {
                eprintln!("skipped: could not get the engine: {e:#}");
                return None;
            }
        };
        let artifacts = Arc::new(Artifacts::new(bundle, false, crate::KNOWN_FINGERPRINT).ok()?);
        let spawn = Arc::new(crate::worker::SpawnContext {
            artifacts: artifacts.clone(),
            stores: Stores::default(),
            reported_concurrency: 4,
            epoch: Instant::now(),
            logs: Default::default(),
            failures: Default::default(),
        });
        let host = HostState {
            artifacts,
            reported_concurrency: 4,
            epoch: Instant::now(),
            logs: Default::default(),
            tag: "probe".into(),
            // A pthread isolate would self-invoke core.js; the main role only
            // defines it, which is all this needs. Nothing here spawns.
            role: RoleState::Main {
                workers: WorkerRegistry::new(spawn),
                progress: None,
                image: None,
            },
        };
        let mut js = build_runtime(Stores::default(), host).ok()?;
        let value = js
            .execute_script("probe:probe", script.to_string())
            .unwrap();
        deno_core::scope!(scope, &mut js);
        Some(deno_core::v8::Local::new(scope, value).to_rust_string_lossy(scope))
    }

    /// The point of the port. If any of these come back defined, the blob has
    /// reach it is not supposed to have.
    #[test]
    fn the_sandbox_has_no_ambient_capabilities() {
        let script = r#"JSON.stringify(
            ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "process",
             "require", "module", "importScripts", "indexedDB", "localStorage",
             "sessionStorage", "caches", "window", "document", "Deno.readFile",
             "Deno.writeFile", "Deno.env", "Deno.run", "Deno.connect"]
              .filter((path) => path.split(".")
                .reduce((o, k) => (o == null ? undefined : o[k]), globalThis) !== undefined))"#;
        let Some(found) = probe(script) else {
            eprintln!("skipped: no engine available");
            return;
        };
        assert_eq!(
            found, "[]",
            "the sandbox exposes host capabilities: {found}"
        );

        // The deno_core builtins that reach the host are disabled, not merely
        // unused: calling one is a no-op rather than a way out.
        let disabled = probe(
            "(() => { try { Deno.core.ops.op_panic('escape'); return 'op_panic ran'; }              catch { return 'threw'; } })()",
        )
        .unwrap();
        assert_ne!(disabled, "op_panic ran");
    }

    #[test]
    #[ignore = "diagnostic: prints the full op surface"]
    fn dump_all_ops() {
        if let Some(all) = probe("Object.keys(Deno.core.ops).sort().join(\"\\n\")") {
            println!("{all}");
        }
    }

    /// Everything the blob can reach into the host with, enumerated. Ops that
    /// are not `op_probe_*` come from deno_core itself and are isolate
    /// plumbing - structured clone, promise inspection, the resource table
    /// (into which this host registers nothing).
    #[test]
    fn the_host_surface_is_the_declared_ops() {
        let Some(found) = probe(
            "Object.keys(Deno.core.ops).filter(n => n.startsWith('op_probe_')).sort().join(',')",
        ) else {
            eprintln!("skipped: no engine available");
            return;
        };
        let mut declared: Vec<_> = OPS.iter().map(|op| op.name).collect();
        declared.sort_unstable();
        assert_eq!(found, declared.join(","));
    }
}
