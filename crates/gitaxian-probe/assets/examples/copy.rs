//! Copy the pinned engine files into a directory a page serves:
//!
//! ```sh
//! cargo run -p gitaxian-probe-assets --example copy -- dist/gitaxian-probe
//! ```

fn main() {
    let Some(dest) = std::env::args_os().nth(1) else {
        eprintln!("usage: copy <dest-dir>");
        std::process::exit(2);
    };
    let dest = std::path::Path::new(&dest);
    if let Err(e) = gitaxian_probe_assets::copy_to(dest) {
        eprintln!("copying into {}: {e}", dest.display());
        std::process::exit(1);
    }
    println!(
        "Delver X {} -> {}",
        gitaxian_probe_assets::VERSION,
        dest.display()
    );
}
