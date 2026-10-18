//! Every SVG in `symbols/` compiled in, so the library needs no disk or
//! network to set a mana symbol, in a binary or in the browser.

use std::fmt::Write;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("symbols");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    files.sort();
    let mut out = String::from("&[\n");
    for path in files {
        // `U_P.svg` is {U/P}: a slash cannot be in a file name.
        let code = path
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .replace('_', "/");
        writeln!(out, "    ({code:?}, include_bytes!({:?})),", path.display()).unwrap();
    }
    out.push(']');
    let target = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("symbols.rs");
    std::fs::write(target, out).unwrap();
}
