//! Fetch the pinned Delver X build, check it, and lay it out for serving.
//!
//! The pin is `pin.json`: the build's version, its archive tag, and every
//! file's sha256. Each file comes from, in order: this crate's `OUT_DIR` if an
//! earlier build already verified it, the directory `GITAXIAN_PROBE_ASSETS_FROM`
//! names (the flake hands in one it fetched itself), the public archive on
//! ghcr.io, or the origin. Whatever the source, it has to hash to the pin, so a
//! directory handed in is held to the same standard as a download.
//! `GITAXIAN_PROBE_OFFLINE=1` turns "not found locally" into a failure instead
//! of a download.

use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use facet::Facet;

use sha2::{Digest, Sha256};

#[allow(dead_code)]
mod layout {
    include!("src/layout.rs");
}

const FROM: &str = "GITAXIAN_PROBE_ASSETS_FROM";
const OFFLINE: &str = "GITAXIAN_PROBE_OFFLINE";
/// The registry the archive is read from, instead of
/// [`layout::ARCHIVE_REGISTRY`]; for testing against a stand-in.
const ARCHIVE_REGISTRY: &str = "GITAXIAN_PROBE_ARCHIVE_REGISTRY";
const USER_AGENT: &str = "gitaxian-probe-assets/0.1";

type Result<T> = std::result::Result<T, String>;

#[derive(Facet)]
struct Pin {
    version: String,
    tag: String,
    files: Vec<Pinned>,
}

#[derive(Facet)]
struct Pinned {
    name: String,
    sha256: String,
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=pin.json");
    println!("cargo:rerun-if-changed=src/layout.rs");
    println!("cargo:rerun-if-env-changed={FROM}");
    println!("cargo:rerun-if-env-changed={OFFLINE}");
    println!("cargo:rerun-if-env-changed={ARCHIVE_REGISTRY}");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let served = out.join("assets");
    match read_pin().and_then(|pin| {
        write(&out.join("pin.rs"), pin_rs(&pin).as_bytes())?;
        build(&pin, &out.join("fetched"), &served)
    }) {
        Ok(()) => println!(
            "cargo:rustc-env=GITAXIAN_PROBE_ASSETS_DIR={}",
            served.display()
        ),
        Err(e) => {
            // Cargo shows a failed build script's stderr as it is; a panic
            // would bury the message under a backtrace.
            eprintln!("\ngitaxian-probe-assets: {e}\n");
            std::process::exit(1);
        }
    }
}

fn read_pin() -> Result<Pin> {
    let json = fs::read_to_string("pin.json").map_err(|e| format!("reading pin.json: {e}"))?;
    let pin: Pin = facet_json::from_str(&json).map_err(|e| format!("pin.json: {e}"))?;
    check_tag(&pin)?;
    Ok(pin)
}

/// The pin as the library's constants, so the table the build checks
/// downloads against is the table the library reports.
fn pin_rs(pin: &Pin) -> String {
    let mut rs = format!(
        "/// The build string `version.txt` carries.\n\
         pub const VERSION: &str = {:?};\n\n\
         /// This build's tag in [`ARCHIVE_REPOSITORY`].\n\
         pub const ARCHIVE_TAG: &str = {:?};\n\n\
         /// One file as upstream serves it, and the sha256 it has to have: also\n\
         /// its blob's digest in the archive.\n\
         pub struct Pinned {{\n    pub name: &'static str,\n    pub sha256: &'static str,\n}}\n\n\
         /// Every file fetched, in `pin.json`'s order.\n\
         pub const PINNED: &[Pinned] = &[\n",
        pin.version, pin.tag
    );
    for f in &pin.files {
        let _ = writeln!(
            rs,
            "    Pinned {{ name: {:?}, sha256: {:?} }},",
            f.name, f.sha256
        );
    }
    rs.push_str("];\n");
    rs
}

fn build(pin: &Pin, fetched: &Path, served: &Path) -> Result<()> {
    mkdir(fetched)?;
    mkdir(served)?;

    let mut drift = Vec::new();
    for file in &pin.files {
        let path = fetched.join(&file.name);
        if let Some(actual) = obtain(file, &path)? {
            drift.push((file.name.as_str(), actual));
        }
    }
    if !drift.is_empty() {
        return Err(explain_drift(pin, &drift));
    }

    for name in layout::SERVED {
        copy(&fetched.join(name), &served.join(name))?;
    }
    Ok(())
}

/// Put a verified copy of `file` at `path`. `Ok(Some(hash))` is a file that
/// was found but hashes to something else - collected, so that one failed build
/// reports every file that moved rather than the first.
fn obtain(file: &Pinned, path: &Path) -> Result<Option<String>> {
    if path.exists() && sha256(&read(path)?) == file.sha256 {
        return Ok(None);
    }

    let bytes = if let Some(dir) = std::env::var_os(FROM) {
        let src = Path::new(&dir).join(&file.name);
        println!("cargo:rerun-if-changed={}", src.display());
        read(&src).map_err(|e| format!("{e}\n({FROM} is set, so nothing is downloaded)"))?
    } else if std::env::var_os(OFFLINE).is_some_and(|v| v != "0" && !v.is_empty()) {
        return Err(format!(
            "{} is not in the build cache and {OFFLINE} is set. Point {FROM} at a \
             directory holding the pinned files, or unset {OFFLINE} to download them.",
            file.name
        ));
    } else {
        // The archive has exactly the pinned bytes, so it goes first; the
        // origin is only asked when the archive could not answer.
        from_archive(&file.sha256).or_else(|archived| {
            download(&file.name).map_err(|origin| {
                format!(
                    "{}: {archived}\nand from the origin instead: {origin}",
                    file.name
                )
            })
        })?
    };

    let actual = sha256(&bytes);
    if actual != file.sha256 {
        return Ok(Some(actual));
    }
    write(path, &bytes)?;
    Ok(None)
}

/// The pin's tag, recomputed: `delver-<version>-` and 12 hex of the sha256 of
/// the table as `sha256sum` writes it. The archive tags its builds by the same
/// rule, so a table edited by hand without its tag fails here.
fn check_tag(pin: &Pin) -> Result<()> {
    let sums: String = pin
        .files
        .iter()
        .map(|f| format!("{}  {}\n", f.sha256, f.name))
        .collect();
    let want = format!("delver-{}-{}", pin.version, &sha256(sums.as_bytes())[..12]);
    if pin.tag != want {
        return Err(format!(
            "pin.json's tag is {}, but its files and version make it {want}. \
             Set it to {want}, or rewrite the pin with assets/repin.py.",
            pin.tag
        ));
    }
    Ok(())
}

#[derive(Facet)]
struct Token {
    token: String,
}

/// One blob of the archive, by its digest, which is the pinned sha256. The
/// registry wants a token even for a public package's blobs, but hands an
/// anonymous one to anybody; it is asked for once per build.
fn from_archive(sha256: &str) -> Result<Vec<u8>> {
    static TOKEN: OnceLock<Result<String>> = OnceLock::new();
    let registry =
        std::env::var(ARCHIVE_REGISTRY).unwrap_or_else(|_| layout::ARCHIVE_REGISTRY.into());
    let registry = registry.trim_end_matches('/');
    let repo = layout::ARCHIVE_REPOSITORY;
    let token = TOKEN
        .get_or_init(|| {
            let url = format!("{registry}/token?scope=repository:{repo}:pull");
            let json = get(&url, None)?;
            facet_json::from_str::<Token>(&String::from_utf8_lossy(&json))
                .map(|t| t.token)
                .map_err(|e| format!("{url}: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)?;
    get(
        &format!("{registry}/v2/{repo}/blobs/sha256:{sha256}"),
        Some(token),
    )
}

/// A GET, with a bearer token when there is one. The blob redirects to storage
/// that refuses a second credential, and ureq drops `Authorization` on a
/// redirect.
fn get(url: &str, token: Option<&str>) -> Result<Vec<u8>> {
    let mut request = ureq::get(url).header("User-Agent", USER_AGENT);
    if let Some(token) = token {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request.call().map_err(|e| format!("{url}: {e}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{url}: {e}"))?;
    Ok(bytes)
}

fn download(name: &str) -> Result<Vec<u8>> {
    let url = format!("{}/{name}", layout::ORIGIN);
    let mut response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("downloading {url}: {e}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading {url}: {e}"))?;
    Ok(bytes)
}

fn explain_drift(pin: &Pin, drift: &[(&str, String)]) -> String {
    let mut msg = format!(
        "the files found no longer match the {} pin.\n\n\
         The archive serves blobs by digest, so this is the origin serving a new \
         Delver X release while the archive could not be reached. If the new \
         build is wanted, pin it from the archive with assets/repin.py (the \
         engine README, *The archive*), and check the engine's KNOWN_FINGERPRINT \
         still holds. What was found:\n",
        pin.version,
    );
    for (name, actual) in drift {
        let _ = write!(msg, "\n    {name}: sha256 = \"{actual}\"");
    }
    msg
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))
}

/// Written beside the target and renamed, so an interrupted build never leaves
/// a truncated file that a later one would have to hash to find out.
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let partial = path.with_extension("partial");
    fs::write(&partial, bytes).map_err(|e| format!("writing {}: {e}", partial.display()))?;
    fs::rename(&partial, path).map_err(|e| format!("renaming into {}: {e}", path.display()))
}

fn copy(src: &Path, dst: &Path) -> Result<()> {
    if fs::metadata(dst).ok().map(|m| m.len()) == fs::metadata(src).ok().map(|m| m.len())
        && read(dst)? == read(src)?
    {
        return Ok(());
    }
    write(dst, &read(src)?)
}

fn mkdir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))
}
