//! The Emscripten pthread pool, as real OS threads running real V8 isolates.
//!
//! The engine preallocates 32 threads at startup and `run()` does not complete
//! until every one of them has acknowledged the shared memory and the compiled
//! module, so the pool cannot be faked. Each worker is its own isolate with its
//! own copy of the sandbox; what travels between them is a structured clone
//! carrying the shared `WebAssembly.Memory` through deno_core's cross-isolate
//! stores.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use deno_core::v8;

use crate::sandbox::{build_runtime, Artifacts, HostState, LogSink, Role, Stores};

/// A one-way queue with a parked reader. Plain `mpsc` would do for delivery,
/// but a worker that has nothing to do must be able to sleep until it does
/// rather than spin, and 32 spinning threads is not a rounding error.
#[derive(Clone)]
pub struct Mailbox(Arc<(Mutex<VecDeque<Vec<u8>>>, Condvar)>);

impl Mailbox {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(VecDeque::new()), Condvar::new())))
    }

    pub fn push(&self, msg: Vec<u8>) {
        let (queue, wake) = &*self.0;
        queue.lock().unwrap().push_back(msg);
        wake.notify_all();
    }

    pub fn pop(&self) -> Option<Vec<u8>> {
        self.0 .0.lock().unwrap().pop_front()
    }

    /// Park until something arrives or `timeout` elapses.
    fn wait(&self, timeout: Duration) {
        let (queue, wake) = &*self.0;
        let guard = queue.lock().unwrap();
        if !guard.is_empty() {
            return;
        }
        let _ = wake.wait_timeout(guard, timeout).unwrap();
    }

    /// Wake a parked reader without delivering anything - used to break a
    /// worker out of its sleep when it is being shut down.
    fn poke(&self) {
        self.0 .1.notify_all();
    }
}

/// The worker end of the pair.
pub struct WorkerPort {
    to_main: Mailbox,
    from_main: Mailbox,
}

impl WorkerPort {
    pub fn post(&self, msg: Vec<u8>) {
        self.to_main.push(msg);
    }
    pub fn recv(&self) -> Option<Vec<u8>> {
        self.from_main.pop()
    }
}

/// Everything a worker thread needs to stand up its own sandbox.
pub struct SpawnContext {
    pub artifacts: Arc<Artifacts>,
    pub stores: Stores,
    pub cores: u32,
    /// Shared clock origin, so `performance.now()` means the same thing in
    /// every isolate. The engine compares timestamps across threads.
    pub epoch: Instant,
    /// The engine's log sink, so a pthread's `console` output reaches the same
    /// place the main isolate's does instead of dying with the thread.
    pub logs: LogSink,
}

struct WorkerHandle {
    to_worker: Mailbox,
    from_worker: Mailbox,
    isolate: Arc<Mutex<Option<v8::IsolateHandle>>>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

pub struct WorkerRegistry {
    ctx: Arc<SpawnContext>,
    next: u32,
    workers: HashMap<u32, WorkerHandle>,
}

impl WorkerRegistry {
    pub fn new(ctx: Arc<SpawnContext>) -> Self {
        Self {
            ctx,
            next: 1,
            workers: HashMap::new(),
        }
    }

    pub fn spawn(&mut self, name: &str) -> Result<u32> {
        let id = self.next;
        self.next += 1;

        let to_worker = Mailbox::new();
        let from_worker = Mailbox::new();
        let stop = Arc::new(AtomicBool::new(false));
        let isolate = Arc::new(Mutex::new(None));

        let port = WorkerPort {
            to_main: from_worker.clone(),
            from_main: to_worker.clone(),
        };
        let ctx = self.ctx.clone();
        let thread_stop = stop.clone();
        let thread_isolate = isolate.clone();
        let inbox = to_worker.clone();

        let tag = format!("{name}{id}");
        let join = std::thread::Builder::new()
            .name(format!("delver-{name}-{id}"))
            .spawn(move || run_worker(ctx, tag, port, inbox, thread_stop, thread_isolate))?;

        self.workers.insert(
            id,
            WorkerHandle {
                to_worker,
                from_worker,
                isolate,
                stop,
                join: Some(join),
            },
        );
        Ok(id)
    }

    pub fn post(&mut self, id: u32, msg: Vec<u8>) {
        if let Some(worker) = self.workers.get(&id) {
            worker.to_worker.push(msg);
        }
    }

    pub fn recv(&mut self, id: u32) -> Option<Vec<u8>> {
        self.workers.get(&id).and_then(|w| w.from_worker.pop())
    }

    pub fn terminate(&mut self, id: u32) {
        if let Some(worker) = self.workers.remove(&id) {
            stop_worker(worker);
        }
    }

    /// Tear the whole pool down. Nothing on the Module object exposes PThread,
    /// so this registry is the only handle on the threads that exists; without
    /// it they outlive the engine.
    pub fn shutdown(&mut self) {
        for (_, worker) in self.workers.drain() {
            stop_worker(worker);
        }
    }

    pub fn count(&self) -> usize {
        self.workers.len()
    }
}

fn stop_worker(mut worker: WorkerHandle) {
    worker.stop.store(true, Ordering::SeqCst);
    // A pool thread spends its life blocked in Atomics.wait inside the wasm
    // thread body, so the flag alone never reaches it - V8 has to interrupt.
    if let Some(handle) = worker.isolate.lock().unwrap().as_ref() {
        handle.terminate_execution();
    }
    worker.to_worker.poke();
    if let Some(join) = worker.join.take() {
        let _ = join.join();
    }
}

fn run_worker(
    ctx: Arc<SpawnContext>,
    tag: String,
    port: WorkerPort,
    inbox: Mailbox,
    stop: Arc<AtomicBool>,
    isolate_slot: Arc<Mutex<Option<v8::IsolateHandle>>>,
) {
    let host = HostState {
        artifacts: ctx.artifacts.clone(),
        cores: ctx.cores,
        epoch: ctx.epoch,
        progress: None,
        image: None,
        workers: None,
        port: Some(port),
        logs: ctx.logs.clone(),
        tag,
    };

    let tokio = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[delver] worker tokio runtime failed: {e}");
            return;
        }
    };
    let _guard = tokio.enter();

    let mut js = match build_runtime(Role::Pthread, ctx.stores.clone(), host) {
        Ok(js) => js,
        Err(e) => {
            eprintln!("[delver] worker sandbox failed to start: {e}");
            return;
        }
    };
    *isolate_slot.lock().unwrap() = Some(js.v8_isolate().thread_safe_handle());

    let pump = match crate::pump::pump_handle(&mut js) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[delver] worker pump missing: {e}");
            return;
        }
    };

    while !stop.load(Ordering::Relaxed) {
        match crate::pump::pump_once(&mut js, &pump) {
            // A terminated isolate unwinds out of the pump; that is shutdown,
            // not an error.
            Err(_) => break,
            Ok(next_timer_ms) => {
                let idle = Duration::from_millis(1);
                let wait = if next_timer_ms < 0.0 {
                    idle
                } else {
                    idle.min(Duration::from_micros(
                        (next_timer_ms * 1000.0).max(0.0) as u64
                    ))
                };
                if wait > Duration::ZERO {
                    inbox.wait(wait);
                }
            }
        }
    }

    // Let the isolate come back to a usable state so its runtime can drop
    // cleanly rather than tripping over a pending termination.
    js.v8_isolate().cancel_terminate_execution();
}
