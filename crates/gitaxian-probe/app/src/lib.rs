pub mod app;

/// NativeActivity's entry point. `dx` builds this crate as a cdylib and the
/// generated MainActivity loads it; nothing calls this on desktop.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    dioxus_native::launch(app::app)
}
