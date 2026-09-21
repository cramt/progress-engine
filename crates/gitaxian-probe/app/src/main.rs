#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

use dioxus_native_android_hello::app;

fn main() {
    dioxus_native::launch(app::app)
}
