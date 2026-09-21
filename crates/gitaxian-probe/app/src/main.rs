#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

use gitaxian_probe_app::app;

fn main() {
    dioxus_native::launch(app::app)
}
