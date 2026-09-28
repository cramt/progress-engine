//! Fetch the pinned Delver X build, check it, and lay it out for serving.
//!
//! Each file comes from, in order: this crate's `OUT_DIR` if an earlier build
//! already verified it, the directory `GITAXIAN_PROBE_ASSETS_FROM` names, the
//! archive's npm version for the pin when `GITAXIAN_PROBE_ARCHIVE_TOKEN` is
//! set, or the origin. Whatever the source, it has to hash to the pin, so a
//! directory handed in by a network-less build (a nix fixed-output derivation,
//! a CI cache) is held to the same standard as a download.
//! `GITAXIAN_PROBE_OFFLINE=1` turns "not found locally" into a failure instead
//! of a download.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use facet::Facet;

use sha2::{Digest, Sha256};

#[allow(dead_code)]
mod pin {
    include!("src/pin.rs");
}

const FROM: &str = "GITAXIAN_PROBE_ASSETS_FROM";
const OFFLINE: &str = "GITAXIAN_PROBE_OFFLINE";
/// A GitHub token that can read [`pin::ARCHIVE`]: a classic one with
/// `read:packages`, since GitHub Packages takes no fine-grained token, or a
/// workflow's own `GITHUB_TOKEN` in this repo.
const ARCHIVE_TOKEN: &str = "GITAXIAN_PROBE_ARCHIVE_TOKEN";
/// The npm registry the archive is read from, instead of
/// [`pin::ARCHIVE_REGISTRY`]; for testing against a stand-in.
const ARCHIVE_REGISTRY: &str = "GITAXIAN_PROBE_ARCHIVE_REGISTRY";
const USER_AGENT: &str = "gitaxian-probe-assets/0.1";

type Result<T> = std::result::Result<T, String>;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/pin.rs");
    println!("cargo:rerun-if-env-changed={FROM}");
    println!("cargo:rerun-if-env-changed={OFFLINE}");
    println!("cargo:rerun-if-env-changed={ARCHIVE_TOKEN}");
    println!("cargo:rerun-if-env-changed={ARCHIVE_REGISTRY}");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let served = out.join("assets");
    match build(&out.join("fetched"), &served) {
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

fn build(fetched: &Path, served: &Path) -> Result<()> {
    check_archive_version()?;
    mkdir(fetched)?;
    mkdir(served)?;

    let mut drift = Vec::new();
    for file in pin::PINNED {
        let path = fetched.join(file.name);
        if let Some(actual) = obtain(file, &path)? {
            drift.push((file.name, actual));
        }
    }
    if !drift.is_empty() {
        return Err(explain_drift(&drift));
    }

    for name in pin::SERVED {
        let dst = served.join(name);
        if *name == "model-alpha.dat" {
            unpack_model(
                &fetched.join("model-alpha.7z"),
                &fetched.join("model-alpha.size"),
                &dst,
            )?;
        } else {
            copy(&fetched.join(name), &dst)?;
        }
    }
    Ok(())
}

/// Put a verified copy of `file` at `path`. `Ok(Some(hash))` is a file that
/// was found but hashes to something else - collected, so that one failed build
/// reports every file that moved rather than the first.
fn obtain(file: &pin::Pinned, path: &Path) -> Result<Option<String>> {
    if path.exists() && sha256(&read(path)?) == file.sha256 {
        return Ok(None);
    }

    let bytes = if let Some(dir) = std::env::var_os(FROM) {
        let src = Path::new(&dir).join(file.name);
        println!("cargo:rerun-if-changed={}", src.display());
        read(&src).map_err(|e| format!("{e}\n({FROM} is set, so nothing is downloaded)"))?
    } else if std::env::var_os(OFFLINE).is_some_and(|v| v != "0" && !v.is_empty()) {
        return Err(format!(
            "{} is not in the build cache and {OFFLINE} is set. Point {FROM} at a \
             directory holding the pinned files, or unset {OFFLINE} to download them.",
            file.name
        ));
    } else {
        match archive_token() {
            // The archive has exactly the pinned bytes, so it goes first; the
            // origin is only asked when the archive could not answer.
            Some(token) => from_archive(&token, file.name).or_else(|archived| {
                download(file.name)
                    .map_err(|origin| format!("{archived}\nand from the origin instead: {origin}"))
            })?,
            None => download(file.name)?,
        }
    };

    let actual = sha256(&bytes);
    if actual != file.sha256 {
        return Ok(Some(actual));
    }
    write(path, &bytes)?;
    Ok(None)
}

/// The pin's own npm version, recomputed: `0.0.0-delver-<VERSION>-` and 12
/// hex of the sha256 of the table as `sha256sum` writes it, with anything a
/// semver prerelease refuses turned into `-`. The archive versions its builds
/// by the same rule, so a table edited by hand without its version fails here
/// rather than as a missing package version.
fn check_archive_version() -> Result<()> {
    let sums: String = pin::PINNED
        .iter()
        .map(|f| format!("{}  {}\n", f.sha256, f.name))
        .collect();
    let tag = format!("delver-{}-{}", pin::VERSION, &sha256(sums.as_bytes())[..12]);
    let want: String = "0.0.0-"
        .chars()
        .chain(tag.chars().map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        }))
        .collect();
    if pin::ARCHIVE_VERSION != want {
        return Err(format!(
            "pin.rs's ARCHIVE_VERSION is {}, but its table and VERSION make it {want}. \
             Set it to {want}, or rewrite the pin with assets/repin.py.",
            pin::ARCHIVE_VERSION
        ));
    }
    Ok(())
}

fn archive_token() -> Option<String> {
    std::env::var(ARCHIVE_TOKEN).ok().filter(|t| !t.is_empty())
}

/// The registry's document for the package: every version it holds.
#[derive(Facet)]
struct Packument {
    versions: HashMap<String, Manifest>,
}

#[derive(Facet)]
struct Manifest {
    dist: Dist,
}

#[derive(Facet)]
struct Dist {
    tarball: String,
}

/// One file of the pinned build from the archive's version for it. The
/// tarball is downloaded and unpacked once per build.
fn from_archive(token: &str, name: &str) -> Result<Vec<u8>> {
    static FILES: OnceLock<Result<HashMap<String, Vec<u8>>>> = OnceLock::new();
    let where_ = format!("{}@{}", pin::ARCHIVE, pin::ARCHIVE_VERSION);
    let files = FILES
        .get_or_init(|| archived_files(token).map_err(|e| format!("reading {where_}: {e}")))
        .as_ref()
        .map_err(Clone::clone)?;
    files
        .get(name)
        .cloned()
        .ok_or_else(|| format!("{where_} has no {name}"))
}

fn archived_files(token: &str) -> Result<HashMap<String, Vec<u8>>> {
    let registry = std::env::var(ARCHIVE_REGISTRY).unwrap_or_else(|_| pin::ARCHIVE_REGISTRY.into());
    let url = format!(
        "{}/{}",
        registry.trim_end_matches('/'),
        pin::ARCHIVE.replace('/', "%2f")
    );
    let json = get_authorized(&url, token, "application/json")?;
    let packument: Packument =
        facet_json::from_str(&String::from_utf8_lossy(&json)).map_err(|e| format!("{url}: {e}"))?;
    let tarball = &packument
        .versions
        .get(pin::ARCHIVE_VERSION)
        .ok_or("the registry has no such version")?
        .dist
        .tarball;
    let gz = get_authorized(tarball, token, "application/octet-stream")?;

    // npm puts everything under `package/`.
    let mut files = HashMap::new();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(gz.as_slice()));
    for entry in tar.entries().map_err(|e| format!("{tarball}: {e}"))? {
        let mut entry = entry.map_err(|e| format!("{tarball}: {e}"))?;
        let path = entry.path().map_err(|e| format!("{tarball}: {e}"))?;
        let Some(name) = path
            .strip_prefix("package")
            .ok()
            .and_then(|p| p.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("{tarball}: {e}"))?;
        files.insert(name, bytes);
    }
    Ok(files)
}

/// A GET with the token. The tarball download may redirect to storage that
/// refuses a second credential, and ureq drops `Authorization` on a redirect.
fn get_authorized(url: &str, token: &str, accept: &str) -> Result<Vec<u8>> {
    let mut response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", accept)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{url}: {e}"))?;
    Ok(bytes)
}

fn download(name: &str) -> Result<Vec<u8>> {
    let url = format!("{}/{name}", pin::ORIGIN);
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

/// The weights as upstream ships them are one LZMA2 member; the size sidecar is
/// the byte count of what comes out, and the only check upstream's sidecars
/// can actually make (FINDINGS.md §1).
fn unpack_model(archive: &Path, size: &Path, dst: &Path) -> Result<()> {
    let want: u64 = String::from_utf8_lossy(&read(size)?)
        .trim()
        .parse()
        .map_err(|e| format!("{}: {e}", size.display()))?;
    if fs::metadata(dst).is_ok_and(|m| m.len() == want) {
        return Ok(());
    }

    let file = fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let mut reader = sevenz_rust2::ArchiveReader::new(file, sevenz_rust2::Password::empty())
        .map_err(|e| format!("opening {}: {e}", archive.display()))?;
    let name = dst.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let mut out = None;
    reader
        .for_each_entries(|entry, r| {
            if entry.name() != name {
                return Ok(true);
            }
            let mut buf = Vec::with_capacity(want as usize);
            r.read_to_end(&mut buf)?;
            out = Some(buf);
            Ok(false)
        })
        .map_err(|e| format!("unpacking {}: {e}", archive.display()))?;
    let bytes = out.ok_or_else(|| format!("{} holds no {name}", archive.display()))?;
    if bytes.len() as u64 != want {
        return Err(format!(
            "{} unpacked to {} bytes, but its size sidecar says {want}",
            archive.display(),
            bytes.len()
        ));
    }
    write(dst, &bytes)
}

fn explain_drift(drift: &[(&str, String)]) -> String {
    let mut msg = format!(
        "the files served no longer match the {} pin.\n\n\
         Upstream serves only its current build, so this is what a new Delver X \
         release looks like. To build the pinned one anyway, set {ARCHIVE_TOKEN} to \
         a classic GitHub token with read:packages, which can read {}. If the new build is wanted, pin it \
         from the archive with assets/repin.py (the engine README, *The archive*), \
         and check the engine's KNOWN_FINGERPRINT still holds. What is served now:\n",
        pin::VERSION,
        pin::ARCHIVE,
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
