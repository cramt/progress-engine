//! Where the app meets `gitaxian-probe-engine`.
//!
//! On the desktop the engine is just a library call. On Android it is not: V8
//! aborts inside `Snapshot::Initialize` when it shares the app's process,
//! under both the Blitz and the WebView shells, while the same binary in the
//! same app sandbox works perfectly with a process to itself. So Android talks
//! to `probe-host` over stdio instead, shipped inside the APK as
//! `libprobehost.so` because `nativeLibraryDir` is the only place an app is
//! allowed to exec from.

use std::path::PathBuf;
use std::sync::OnceLock;

/// The app's private directory, which is where the engine may cache. `/tmp`
/// does not exist on Android, and an unconfigured engine would try it.
static CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Point the cache at somewhere writable before anything opens the engine.
///
/// The Blitz shell got this from the `AndroidApp` handed to `android_main`.
/// The WebView shell has no such handle - `main` is called by a dlsym from
/// dioxus-desktop's JNI trampoline - so the app's private directory is derived
/// from the process name instead, which on Android's main process is the
/// package name. Reading it out of the activity over JNI would be more
/// correct; this needs no JNI and no new dependency.
pub fn init_cache_root() {
    #[cfg(target_os = "android")]
    {
        if let Ok(cmdline) = std::fs::read_to_string("/proc/self/cmdline") {
            let package = cmdline.trim_end_matches(char::from(0));
            if !package.is_empty() {
                let files = PathBuf::from("/data/data").join(package).join("files");
                let _ = std::fs::create_dir_all(&files);
                let _ = CACHE_ROOT.set(files);
            }
        }
    }
}

/// Bring the engine up and describe it in one line, whatever that takes on
/// this platform.
///
/// Blocking, and slow the first time - it downloads tens of megabytes - so
/// callers must keep it off the thread that paints.
pub fn open_and_describe() -> String {
    match describe() {
        Ok(line) => line,
        Err(error) => format!("failed: {error}"),
    }
}

#[cfg(not(target_os = "android"))]
fn describe() -> Result<String, String> {
    use std::sync::Arc;

    use gitaxian_probe_engine::{DirCache, Engine, EngineConfig};

    let mut config = EngineConfig::default();
    if let Some(root) = CACHE_ROOT.get() {
        config.source.cache = Arc::new(DirCache::new(root.join("engine")));
    }
    let mut engine = Engine::open(config).map_err(|error| error.to_string())?;
    let line = format!(
        "engine {} · fingerprint {} · {} workers",
        engine.version(),
        engine.fingerprint(),
        engine.worker_count()
    );
    engine.close();
    Ok(line)
}

#[cfg(target_os = "android")]
fn describe() -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    use gitaxian_probe_proto::{Request, Response};

    let host = host_path()?;
    let cache = CACHE_ROOT
        .get()
        .map(|root| root.join("engine").display().to_string())
        .unwrap_or_default();

    let mut child = Command::new(&host)
        .arg(&cache)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Captured, not discarded: when the host dies the panic on its stderr
        // is the only account of why, and "host closed without answering" on
        // its own has cost hours.
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn {}: {error}", host.display()))?;

    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
    let mut stderr = child.stderr.take().ok_or("no stderr")?;

    writeln!(stdin, "{}", Request::open().encode()).map_err(|error| error.to_string())?;
    stdin.flush().map_err(|error| error.to_string())?;

    let mut line = String::new();
    let read = stdout
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;

    // Single-shot for now: the host is torn down with its answer. Keeping one
    // alive across calls is what `recognize` will want, and is why the
    // protocol is a stream rather than a one-off.
    let _ = child.kill();
    let _ = child.wait();

    if read == 0 {
        let mut complaint = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut complaint);
        let last = complaint
            .lines()
            .filter(|line| !line.trim().is_empty())
            .next_back()
            .unwrap_or("no output on stderr");
        return Err(format!("host closed without answering: {last}"));
    }

    let response = Response::decode(line.trim())?;
    if response.ok {
        Ok(response.message)
    } else {
        Err(response.message)
    }
}

/// Where the APK's native libraries were unpacked, which is the only directory
/// an Android app may exec from.
///
/// There is nothing to ask for it: this process is a zygote fork of
/// `app_process`, so `current_exe` points at the zygote rather than at us. But
/// our own code lives in `libmain.so`, which is mapped, so the loader has
/// already written the answer into `/proc/self/maps`.
#[cfg(target_os = "android")]
fn host_path() -> Result<PathBuf, String> {
    let maps = std::fs::read_to_string("/proc/self/maps")
        .map_err(|error| format!("read maps: {error}"))?;

    for line in maps.lines() {
        let Some(start) = line.find('/') else {
            continue;
        };
        let path = std::path::Path::new(line[start..].trim_end());
        if path.file_name().is_some_and(|name| name == "libmain.so") {
            if let Some(dir) = path.parent() {
                return Ok(dir.join("libprobehost.so"));
            }
        }
    }
    Err("libmain.so not found in /proc/self/maps".into())
}
