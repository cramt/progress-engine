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

pub fn pump_handle(js: &mut JsRuntime) -> Result<v8::Global<v8::Function>> {
    let global = js.execute_script("delver:pump-handle", "globalThis.__delverPump")?;
    deno_core::scope!(scope, js);
    let local = v8::Local::new(scope, global);
    let func = v8::Local::<v8::Function>::try_from(local)
        .map_err(|_| anyhow!("__delverPump is not a function"))?;
    Ok(v8::Global::new(scope, func))
}

/// One turn of the crank. Returns milliseconds until the next timer is due, or
/// -1 when the isolate has nothing scheduled.
pub fn pump_once(js: &mut JsRuntime, pump: &v8::Global<v8::Function>) -> Result<f64> {
    let next = {
        deno_core::scope!(scope, js);
        let func = v8::Local::new(scope, pump);
        let recv = v8::undefined(scope).into();
        let result = func
            .call(scope, recv, &[])
            .ok_or_else(|| anyhow!("pump call failed - isolate terminated"))?;
        result.number_value(scope).unwrap_or(-1.0)
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

/// Inspect a value returned by `execute_script`. Engine methods hand back
/// promises of JSON strings; anything already settled is read straight out.
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
            bail!("timed out after {:?} waiting for the engine", timeout);
        }
        // Sleeping at all matters: the main isolate is idle while 32 worker
        // threads do the actual work, and spinning here starves them.
        let idle = Duration::from_micros(200);
        let wait = if next < 0.0 {
            idle
        } else {
            idle.min(Duration::from_micros((next * 1000.0).max(0.0) as u64))
        };
        if wait > Duration::ZERO {
            std::thread::sleep(wait);
        }
    }
}
