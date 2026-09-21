//! Driving the sandbox.
//!
//! Nothing in the engine is reachable through a plain call-and-return: the
//! bootstrap awaits a 32-worker handshake, jobs land as files announced by an
//! event, and recognition settles over several frames. So the host does not
//! "call JavaScript", it turns a crank - deliver queued worker messages, fire
//! due timers, let V8 drain its microtasks - until the promise it is waiting on
//! settles.

use std::task::Poll;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use deno_core::{v8, JsRuntime, PollEventLoopOptions};

/// How long the main isolate idles between turns when no timer is due sooner.
/// It holds no work of its own while the pool runs, so this is how often it
/// looks at the pool's progress rather than a wait on anything local.
pub const MAIN_IDLE: Duration = Duration::from_micros(200);

pub fn pump_handle(js: &mut JsRuntime) -> Result<v8::Global<v8::Function>> {
    let global = js.execute_script("probe:pump-handle", "globalThis.__probePump")?;
    deno_core::scope!(scope, js);
    let local = v8::Local::new(scope, global);
    let func = v8::Local::<v8::Function>::try_from(local)
        .map_err(|_| anyhow!("__probePump is not a function"))?;
    Ok(v8::Global::new(scope, func))
}

/// How long to idle after a turn of the crank: until the next timer is due,
/// capped so that a message arriving without one is still noticed promptly.
///
/// Both loops that turn the crank need this and used to spell it out
/// separately, against a bare `f64` in which a negative value meant "nothing
/// scheduled" and everything else meant milliseconds.
pub fn idle_for(next_timer: Option<Duration>, cap: Duration) -> Duration {
    next_timer.map_or(cap, |next| next.min(cap))
}

/// One turn of the crank. Returns how long until the next timer is due, or
/// `None` when the isolate has nothing scheduled.
pub fn pump_once(js: &mut JsRuntime, pump: &v8::Global<v8::Function>) -> Result<Option<Duration>> {
    let next = {
        deno_core::scope!(scope, js);
        let func = v8::Local::new(scope, pump);
        let recv = v8::undefined(scope).into();
        let result = func
            .call(scope, recv, &[])
            .ok_or_else(|| anyhow!("pump call failed - isolate terminated"))?;
        // The glue reports milliseconds until the next timer, or a negative
        // number when it has none.
        let ms = result.number_value(scope).unwrap_or(-1.0);
        (ms >= 0.0).then(|| Duration::from_micros((ms * 1000.0) as u64))
    };

    // A noop waker is right here: we re-poll on our own schedule rather than
    // waiting to be woken, because the engine's progress is driven by the
    // crank above as much as by V8's own queues.
    let waker = deno_core::futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    if let Poll::Ready(Err(e)) = js.poll_event_loop(&mut cx, PollEventLoopOptions::default()) {
        bail!("{e}");
    }
    Ok(next)
}

pub enum Settled {
    Pending,
    Value(String),
    Failed(String),
}

/// Inspect a value returned by an engine method. They hand back promises of
/// JSON strings; anything already settled is read straight out.
pub fn settled(js: &mut JsRuntime, value: &v8::Global<v8::Value>) -> Settled {
    deno_core::scope!(scope, js);
    let local = v8::Local::new(scope, value);
    let Ok(promise) = v8::Local::<v8::Promise>::try_from(local) else {
        return Settled::Value(local.to_rust_string_lossy(scope));
    };
    match promise.state() {
        v8::PromiseState::Pending => Settled::Pending,
        v8::PromiseState::Fulfilled => {
            Settled::Value(promise.result(scope).to_rust_string_lossy(scope))
        }
        v8::PromiseState::Rejected => {
            Settled::Failed(promise.result(scope).to_rust_string_lossy(scope))
        }
    }
}

/// Crank until `value` settles, or until `timeout` runs out.
pub fn run_until(
    js: &mut JsRuntime,
    pump: &v8::Global<v8::Function>,
    value: v8::Global<v8::Value>,
    timeout: Duration,
) -> Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let next = pump_once(js, pump)?;
        match settled(js, &value) {
            Settled::Value(v) => return Ok(v),
            Settled::Failed(e) => bail!("{e}"),
            Settled::Pending => {}
        }
        if std::time::Instant::now() > deadline {
            bail!("timed out after {timeout:?} waiting for the engine");
        }
        // Sleeping at all matters: the main isolate is idle while 32 worker
        // threads do the actual work, and spinning here starves them.
        let wait = idle_for(next, MAIN_IDLE);
        if wait > Duration::ZERO {
            std::thread::sleep(wait);
        }
    }
}
