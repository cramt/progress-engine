pub mod app;
pub mod probe;

/// NativeActivity's entry point. `dx` builds this crate as a cdylib and the
/// generated MainActivity loads it; nothing calls this on desktop.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    // Before the UI, because the engine's default cache is /tmp and an Android
    // app cannot write there.
    if let Some(dir) = android_app.internal_data_path() {
        probe::set_cache_root(dir);
    }
    dioxus_native::set_android_app(android_app);
    dioxus_native::launch(app::app)
}
