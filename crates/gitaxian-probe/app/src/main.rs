#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

use gitaxian_probe_app::{app, probe};

// Android runs this too. dioxus-desktop's `start_app` sets up the JNI bindings
// that the generated activity calls, then dlsym's `main` and calls it - so
// unlike the Blitz shell there is no separate android_main entry point.
fn main() {
    probe::init_cache_root();
    dioxus::launch(app::app)
}
