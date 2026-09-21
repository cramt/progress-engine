//! One process, one engine, spoken to over stdio.
//!
//! V8 will not run inside an Android app's own process. The same binary, the
//! same archive and the same app sandbox all work when the engine gets a
//! process to itself, and abort inside `Snapshot::Initialize` with `Check
//! failed: AllowHeapAllocationInRelease::IsAllowed()` when it does not - under
//! both the Blitz and the WebView shells, so it is not about the renderer.
//! This host is that process.
//!
//! The wire types are in `gitaxian_probe_engine::proto`, shared with the app
//! so the two ends cannot drift.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use gitaxian_probe_engine::{DirCache, Engine, EngineConfig};
use gitaxian_probe_proto::{Request, Response};

/// Where the engine caches. The app passes its own private directory, since
/// this process inherits no useful default - `/tmp` does not exist on Android.
fn config(cache_root: Option<PathBuf>) -> EngineConfig {
    let mut config = EngineConfig::default();
    if let Some(root) = cache_root {
        config.source.cache = Arc::new(DirCache::new(root));
    }
    config
}

fn respond(out: &mut impl Write, response: &Response) {
    let _ = writeln!(out, "{}", response.encode());
    let _ = out.flush();
}

/// Raise the executable's `PT_TLS` alignment to what Android's arm64 loader
/// demands. See `src/tls_align.c` for why this cannot be done in Rust.
#[cfg(all(target_os = "android", target_arch = "aarch64"))]
unsafe extern "C" {
    fn probe_tls_align_anchor() -> *mut core::ffi::c_char;
}

fn main() {
    // Touch it, so the object carrying the over-aligned thread-local is
    // pulled out of the archive rather than dropped as unused.
    #[cfg(all(target_os = "android", target_arch = "aarch64"))]
    unsafe {
        std::hint::black_box(probe_tls_align_anchor());
    }

    // facet stores its "this is a wide pointer" flag in bit 63, on the stated
    // assumption that user-space addresses never set it. Android arm64 breaks
    // that: every heap pointer comes back tagged `0xb4` in the top byte, so
    // facet reads ordinary thin pointers as wide and panics in
    // `as_mut_byte_ptr` - including inside the engine's own msgpack job
    // decode, which is not code this host can route around.
    //
    // Turning heap tagging off for this process clears the top byte and the
    // panic with it. It costs the hardening that tagging buys, in a process
    // whose entire job is running a sandboxed blob - so it is a trade, and it
    // goes away when facet stops claiming bit 63.
    //
    // `android:allowNativeHeapPointerTagging="false"` in the manifest does
    // *not* do this: it was set, present in the built manifest, and the panic
    // was unchanged.
    //
    // https://github.com/facet-rs/facet/issues/2659
    #[cfg(all(target_os = "android", target_arch = "aarch64"))]
    unsafe {
        unsafe extern "C" {
            /// bionic's `mallopt`, for `M_BIONIC_SET_HEAP_TAGGING_LEVEL`.
            fn mallopt(param: core::ffi::c_int, value: core::ffi::c_int) -> core::ffi::c_int;
        }
        // (-204, 0) is M_BIONIC_SET_HEAP_TAGGING_LEVEL, M_HEAP_TAGGING_LEVEL_NONE.
        unsafe { mallopt(-204, 0) };
    }

    let cache_root = std::env::args().nth(1).map(PathBuf::from);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut engine: Option<Engine> = None;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }

        let request = Request::decode(&line);

        let response = match request.op.as_str() {
            "open" => match Engine::open(config(cache_root.clone())) {
                Ok(opened) => {
                    let message = format!(
                        "engine {} · fingerprint {} · {} workers",
                        opened.version(),
                        opened.fingerprint(),
                        opened.worker_count()
                    );
                    engine = Some(opened);
                    Response::ok(message)
                }
                Err(error) => Response::err(error.to_string()),
            },
            "query" => match engine.as_mut() {
                None => Response::err("not open"),
                Some(engine) => match engine.query(&request.sql) {
                    Ok(rows) => Response {
                        ok: true,
                        message: format!("{} rows", rows.len()),
                        rows,
                    },
                    Err(error) => Response::err(error.to_string()),
                },
            },
            "close" => {
                if let Some(mut engine) = engine.take() {
                    engine.close();
                }
                Response::ok("closed")
            }
            other => Response::err(format!("unknown op: {other}")),
        };

        respond(&mut stdout, &response);
    }

    if let Some(mut engine) = engine.take() {
        engine.close();
    }
}
