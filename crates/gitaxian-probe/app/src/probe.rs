//! Where the app meets `gitaxian-probe-engine`.
//!
//! The engine is the same crate the CLI examples use; what differs on a phone
//! is only where its artefacts are allowed to live.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use gitaxian_probe_engine::{DirCache, Engine, EngineConfig};

/// An unconfigured engine caches under `/tmp`, which on Android is not
/// writable by an app. `android_main` records the activity's private directory
/// here before any UI runs, and [`config`] redirects the cache into it.
/// Unset on desktop, where the default is already right.
static CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn set_cache_root(path: PathBuf) {
    let _ = CACHE_ROOT.set(path);
}

fn config() -> EngineConfig {
    let mut config = EngineConfig::default();
    if let Some(root) = CACHE_ROOT.get() {
        config.source.cache = Arc::new(DirCache::new(root.join("engine")));
    }
    config
}

/// Bring the engine up, reporting what happened in one line either way.
///
/// The first call downloads tens of megabytes, so this is deliberately not run
/// at startup.
pub fn open_and_describe() -> String {
    match Engine::open(config()) {
        Ok(mut engine) => {
            let line = format!(
                "engine {} · fingerprint {} · {} workers",
                engine.version(),
                engine.fingerprint(),
                engine.worker_count()
            );
            engine.close();
            line
        }
        Err(error) => format!("failed: {error}"),
    }
}
