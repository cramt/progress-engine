//! Getting the engine's files, and keeping them between runs.
//!
//! Everything the engine is made of - the Emscripten glue, the wasm blob, the
//! card catalogue and one file of weights per model tier - is served
//! unauthenticated from a single origin. This module is the whole of that
//! download: [`Bundle::fetch`] returns every byte [`crate::Engine`] needs, out
//! of a cache where it can and off the network where it must.
//!
//! Where the bytes are kept is the caller's choice. [`ArtifactCache`] is the
//! seam; [`DirCache`] is the only implementation here, and an unconfigured
//! engine caches in `/tmp/delver-engine`.
//!
//! Upstream rebuilds on its own schedule, so the build string in `version.txt`
//! is the cache namespace: a rebuild is a miss rather than a stale hit, and the
//! nine bytes it costs to ask are the cheapest request of the run.

use std::fmt;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};

use crate::{Model, Progress};

/// Where the artefacts are served from.
pub const DEFAULT_ORIGIN: &str = "https://mtg.delver.app";

/// Refuse a response larger than this. The biggest artefact upstream serves is
/// a ~40 MB archive, so anything near this is the origin misbehaving rather
/// than a build that grew.
const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;

// ---- what there is to fetch ------------------------------------------------

/// An upstream build string, as `version.txt` spells it - `1.76.beta`.
///
/// Only [`Version::parse`] constructs one, and it refuses anything that is not
/// a single safe path segment: the value arrives off the network and a
/// filesystem cache turns it straight into a directory name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version(String);

impl Version {
    pub fn parse(raw: &str) -> Result<Self> {
        let v = raw.trim();
        let usable = !v.is_empty()
            && v.len() <= 64
            && v != "."
            && v != ".."
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
        if !usable {
            bail!("upstream reported build {v:?}, which is not a usable name");
        }
        Ok(Self(v.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One file the engine needs, under the name the cache knows it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Artifact {
    /// The Emscripten glue.
    CoreJs,
    /// The engine, as upstream ships it. [`crate::Engine`] patches its own copy.
    CoreWasm,
    /// The packed card catalogue - the engine unpacks this one itself.
    Catalogue,
    /// md5 and byte count of the *unpacked* catalogue. `_install` refuses to
    /// run unless they sit next to the archive (FINDINGS.md §8).
    CatalogueMd5,
    CatalogueSize,
    /// The weights for one tier, unpacked. Upstream serves them packed.
    Model(Model),
    /// Byte count of the unpacked weights. There is a `model-<tier>.md5`
    /// upstream as well, but FINDINGS.md §1 has it as an opaque build token
    /// rather than a digest of anything served, so it is not fetched: the size
    /// is the only sidecar that can actually check a download.
    ModelSize(Model),
    /// The build string. Never cached - it is the key everything else is
    /// cached under, so there is nowhere to put it.
    Version,
}

/// How the origin serves an artefact.
enum Remote {
    Verbatim(&'static str),
    /// A single-file LZMA2 archive that has to come out before the engine can
    /// be handed the contents.
    Packed(&'static str),
}

impl Artifact {
    /// The name the cache stores this under, which is also the name upstream
    /// serves it under for everything that needs no unpacking.
    pub fn file_name(self) -> &'static str {
        match self {
            Artifact::CoreJs => "core.js",
            Artifact::CoreWasm => "core.wasm",
            Artifact::Catalogue => "data.7z",
            Artifact::CatalogueMd5 => "data.md5",
            Artifact::CatalogueSize => "data.size",
            Artifact::Version => "version.txt",
            Artifact::Model(Model::Alpha) => "model-alpha.dat",
            Artifact::Model(Model::Lambda) => "model-lambda.dat",
            Artifact::Model(Model::Gamma) => "model-gamma.dat",
            Artifact::ModelSize(Model::Alpha) => "model-alpha.size",
            Artifact::ModelSize(Model::Lambda) => "model-lambda.size",
            Artifact::ModelSize(Model::Gamma) => "model-gamma.size",
        }
    }

    fn remote(self) -> Remote {
        match self {
            Artifact::Model(Model::Alpha) => Remote::Packed("model-alpha.7z"),
            Artifact::Model(Model::Lambda) => Remote::Packed("model-lambda.7z"),
            Artifact::Model(Model::Gamma) => Remote::Packed("model-gamma.7z"),
            other => Remote::Verbatim(other.file_name()),
        }
    }
}

/// An artefact together with the build it belongs to - the cache's key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactId {
    pub version: Version,
    pub artifact: Artifact,
}

// ---- where it is kept ------------------------------------------------------

/// Where fetched artefacts live between runs.
///
/// One obligation, and it is what lets [`Bundle::fetch`] treat a hit as final:
/// what [`put`](Self::put) stored is what [`get`](Self::get) returns, byte for
/// byte, or `get` reports a miss. A half-written entry must never read as a
/// hit - which for anything on a filesystem means writing elsewhere and
/// renaming into place, as [`DirCache`] does.
pub trait ArtifactCache: Send + Sync {
    fn get(&self, id: &ArtifactId) -> Result<Option<Vec<u8>>>;

    fn put(&self, id: &ArtifactId, bytes: &[u8]) -> Result<()>;

    /// The newest build this cache holds anything for.
    ///
    /// Consulted only when the origin cannot be reached, so that an engine
    /// which has run once keeps running with no network. Returning `None` is
    /// fine and just means an unreachable origin is fatal.
    fn newest_version(&self) -> Result<Option<Version>> {
        Ok(None)
    }
}

/// An [`ArtifactCache`] backed by one directory per upstream build, at
/// `<root>/<version>/<name>`.
pub struct DirCache {
    root: PathBuf,
}

impl DirCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `/tmp/delver-engine`, where an unconfigured engine caches - or the
    /// platform temp directory on anything without a `/tmp`.
    ///
    /// Deliberately not `$TMPDIR`: `nix develop` points that at a fresh
    /// directory per invocation, so a cache under it would be thrown away
    /// between runs of exactly the command this crate is usually run from.
    pub fn temp() -> Self {
        let tmp = Path::new("/tmp");
        let root = if tmp.is_dir() {
            tmp.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        Self::new(root.join("delver-engine"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, id: &ArtifactId) -> PathBuf {
        self.root
            .join(id.version.as_str())
            .join(id.artifact.file_name())
    }
}

impl Default for DirCache {
    fn default() -> Self {
        Self::temp()
    }
}

impl ArtifactCache for DirCache {
    fn get(&self, id: &ArtifactId) -> Result<Option<Vec<u8>>> {
        let path = self.path(id);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn put(&self, id: &ArtifactId, bytes: &[u8]) -> Result<()> {
        let path = self.path(id);
        let dir = path
            .parent()
            .expect("an artefact path always has its version directory as parent");
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

        // Written next to the target and renamed in, so a run killed mid-write
        // leaves a stray `.partial` rather than a truncated cache hit.
        let partial = dir.join(format!(
            ".{}.{}.partial",
            id.artifact.file_name(),
            std::process::id()
        ));
        std::fs::write(&partial, bytes)
            .with_context(|| format!("writing {}", partial.display()))?;
        std::fs::rename(&partial, &path)
            .with_context(|| format!("moving {} into {}", partial.display(), path.display()))?;
        Ok(())
    }

    fn newest_version(&self) -> Result<Option<Version>> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("listing {}", self.root.display())),
        };

        let mut newest: Option<(SystemTime, Version)> = None;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(version) = name.to_str().and_then(|n| Version::parse(n).ok()) else {
                continue;
            };
            let touched = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            if newest.as_ref().is_none_or(|(seen, _)| touched > *seen) {
                newest = Some((touched, version));
            }
        }
        Ok(newest.map(|(_, version)| version))
    }
}

// ---- where it comes from ---------------------------------------------------

/// The server the artefacts are downloaded from.
#[derive(Debug, Clone)]
pub struct Origin {
    /// Base URL. A trailing slash is ignored.
    pub base: String,
    /// The origin fronts a browser app and turns away clients that do not look
    /// like one, so this claims to be a browser.
    pub user_agent: String,
    /// Ceiling on a whole transfer, not just the handshake - the largest
    /// artefact here is tens of megabytes.
    pub timeout: Duration,
}

impl Default for Origin {
    fn default() -> Self {
        Self {
            base: DEFAULT_ORIGIN.to_string(),
            user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                         Chrome/140 Safari/537.36"
                .to_string(),
            timeout: Duration::from_secs(300),
        }
    }
}

impl Origin {
    fn download(&self, name: &str, report: Option<&dyn Fn(Progress)>) -> Result<Vec<u8>> {
        let url = format!("{}/{name}", self.base.trim_end_matches('/'));
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(self.user_agent.clone())
            .timeout_global(Some(self.timeout))
            .build()
            .into();

        let mut response = agent
            .get(&url)
            .call()
            .with_context(|| format!("GET {url}"))?;
        let total: Option<u64> = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());

        let mut body = response.body_mut().as_reader();
        let mut out = Vec::with_capacity(total.unwrap_or(0).min(MAX_DOWNLOAD) as usize);
        let mut chunk = vec![0u8; 256 * 1024];
        let mut announced = 0;
        loop {
            let read = body
                .read(&mut chunk)
                .with_context(|| format!("reading the body of {url}"))?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..read]);
            if out.len() as u64 > MAX_DOWNLOAD {
                bail!("{url} is larger than the {MAX_DOWNLOAD} byte ceiling");
            }
            if let Some(report) = report {
                // Ten reports per file rather than one per chunk: a caller that
                // prints a line each time should get ten, not two hundred.
                let (step, progress) = step(name, out.len(), total);
                if step > announced {
                    announced = step;
                    report(progress);
                }
            }
        }
        Ok(out)
    }
}

/// How far along a download is, and which reporting step that falls in - a
/// report goes out only when the step advances.
///
/// The origin puts a `content-length` on the archives but serves `core.js` and
/// `core.wasm` chunked without one, so there is not always a percentage to
/// give. Those count megabytes instead of guessing at a denominator.
fn step(name: &str, done: usize, total: Option<u64>) -> (u64, Progress) {
    let (step, percent, message) = match total {
        Some(total) if total > 0 => {
            let percent = (done as u64 * 100 / total).min(100) as u32;
            (percent as u64 / 10, percent, name.to_string())
        }
        _ => {
            let mib = done as u64 / (1 << 20);
            (mib / 4, 0, format!("{name} ({mib} MB)"))
        }
    };
    (
        step,
        Progress {
            stage: "fetch".into(),
            percent,
            message,
        },
    )
}

/// Where artefacts come from, and where they are kept.
pub struct Source {
    pub cache: Arc<dyn ArtifactCache>,
    pub origin: Origin,
    /// Never touch the network: every artefact must already be cached, and the
    /// build is whatever the cache last saw.
    pub offline: bool,
}

impl Default for Source {
    fn default() -> Self {
        Self {
            cache: Arc::new(DirCache::temp()),
            origin: Origin::default(),
            offline: false,
        }
    }
}

impl Source {
    /// The build to fetch.
    ///
    /// Asks the origin, and falls back to the newest build the cache holds when
    /// it cannot be reached - an engine that has run once keeps running with no
    /// network.
    pub fn version(&self) -> Result<Version> {
        if self.offline {
            return self.cache.newest_version()?.ok_or_else(|| {
                anyhow!("the source is offline and its cache holds no build to fall back to")
            });
        }

        let fresh = self
            .origin
            .download(Artifact::Version.file_name(), None)
            .and_then(|bytes| Version::parse(&String::from_utf8_lossy(&bytes)));
        match fresh {
            Ok(version) => Ok(version),
            Err(unreachable) => match self.cache.newest_version()? {
                Some(version) => Ok(version),
                None => Err(unreachable.context("asking the origin which build to fetch")),
            },
        }
    }

    /// One artefact, from the cache if it is there and from the origin if not.
    pub fn get(
        &self,
        version: &Version,
        artifact: Artifact,
        report: Option<&dyn Fn(Progress)>,
    ) -> Result<Vec<u8>> {
        if artifact == Artifact::Version {
            return Ok(version.as_str().as_bytes().to_vec());
        }

        let id = ArtifactId {
            version: version.clone(),
            artifact,
        };
        if let Some(bytes) = self.cache.get(&id)? {
            return Ok(bytes);
        }
        if self.offline {
            bail!(
                "{} is not cached for build {version} and the source is offline",
                artifact.file_name()
            );
        }

        let bytes = match artifact.remote() {
            Remote::Verbatim(name) => self.origin.download(name, report)?,
            Remote::Packed(name) => {
                let packed = self.origin.download(name, report)?;
                unpack(&packed, artifact.file_name())
                    .with_context(|| format!("unpacking {name}"))?
            }
        };
        self.cache.put(&id, &bytes)?;
        Ok(bytes)
    }
}

// ---- the result ------------------------------------------------------------

/// Every byte the engine needs, for one model tier.
///
/// [`Bundle::fetch`] is the only way to build one, so there is no
/// half-populated bundle to hold: every artefact is present and the weights
/// have been checked against their sidecars.
pub struct Bundle {
    pub version: Version,
    pub tier: Model,
    pub core_js: String,
    /// `core.wasm` as upstream ships it, unpatched.
    pub core_wasm: Vec<u8>,
    /// `data.7z`, still packed. The engine unpacks the catalogue itself, into
    /// its own in-memory filesystem.
    pub catalogue: Vec<u8>,
    pub catalogue_md5: Vec<u8>,
    pub catalogue_size: Vec<u8>,
    /// The unpacked weights for [`Bundle::tier`].
    pub weights: Vec<u8>,
}

impl Bundle {
    pub fn fetch(source: &Source, tier: Model, report: Option<&dyn Fn(Progress)>) -> Result<Self> {
        let version = source.version()?;
        let get = |artifact| source.get(&version, artifact, report);

        let core_js = String::from_utf8(get(Artifact::CoreJs)?).context("core.js is not UTF-8")?;
        let core_wasm = get(Artifact::CoreWasm)?;
        let catalogue = get(Artifact::Catalogue)?;
        let catalogue_md5 = get(Artifact::CatalogueMd5)?;
        let catalogue_size = get(Artifact::CatalogueSize)?;
        let weights = get(Artifact::Model(tier))?;

        // Checked on every open, not just after a download. Weights that came
        // out short do not crash the engine - they come back as confident
        // recognitions of the wrong card, and the sidecar is the only thing
        // upstream ships that can say so.
        check_weights(&weights, &get(Artifact::ModelSize(tier))?, tier)?;

        Ok(Self {
            version,
            tier,
            core_js,
            core_wasm,
            catalogue,
            catalogue_md5,
            catalogue_size,
            weights,
        })
    }

    /// Bytes for the name the sandbox asks for, or `None`.
    ///
    /// This *is* the artefact allowlist. A bundle holds exactly the files the
    /// engine is allowed to read, so there is no path for it to traverse out of
    /// and no name for it to get wrong.
    pub fn file(&self, name: &str) -> Option<&[u8]> {
        Some(match name {
            "data.7z" => &self.catalogue,
            "data.md5" => &self.catalogue_md5,
            "data.size" => &self.catalogue_size,
            "version.txt" => self.version.as_str().as_bytes(),
            _ if name == Artifact::Model(self.tier).file_name() => &self.weights,
            _ => return None,
        })
    }
}

fn check_weights(weights: &[u8], size: &[u8], tier: Model) -> Result<()> {
    let want: usize = String::from_utf8_lossy(size)
        .trim()
        .parse()
        .with_context(|| {
            format!(
                "{} is not a byte count",
                Artifact::ModelSize(tier).file_name()
            )
        })?;
    if weights.len() != want {
        bail!(
            "{} unpacked to {} bytes, its sidecar says {want} - the cached copy is damaged; \
             clear the artefact cache and open again",
            Artifact::Model(tier).file_name(),
            weights.len()
        );
    }
    Ok(())
}

/// Pull the single named file out of one of upstream's LZMA2 archives.
fn unpack(packed: &[u8], want: &str) -> Result<Vec<u8>> {
    let mut reader =
        sevenz_rust2::ArchiveReader::new(Cursor::new(packed), sevenz_rust2::Password::empty())
            .map_err(|e| anyhow!("{e}"))
            .context("reading the archive header")?;

    let mut found = None;
    let mut failed = None;
    reader
        .for_each_entries(|entry, contents| {
            if entry.name() != want {
                return Ok(true);
            }
            let mut out = Vec::with_capacity(entry.size() as usize);
            match contents.read_to_end(&mut out) {
                Ok(_) => found = Some(out),
                Err(e) => failed = Some(e),
            }
            Ok(false)
        })
        .map_err(|e| anyhow!("{e}"))?;

    if let Some(e) = failed {
        return Err(e).context("decompressing the entry");
    }
    found.ok_or_else(|| anyhow!("the archive holds no {want}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_has_to_be_one_safe_path_segment() {
        assert_eq!(
            Version::parse(" 1.76.beta\n").unwrap().as_str(),
            "1.76.beta"
        );
        for hostile in ["", "..", ".", "1.7/../../etc", "a b", "~/x", "\u{5}"] {
            assert!(Version::parse(hostile).is_err(), "accepted {hostile:?}");
        }
    }

    #[test]
    fn a_bundle_hands_out_only_the_engines_own_files() {
        let bundle = Bundle {
            version: Version::parse("1.76.beta").unwrap(),
            tier: Model::Alpha,
            core_js: String::new(),
            core_wasm: Vec::new(),
            catalogue: b"packed".to_vec(),
            catalogue_md5: Vec::new(),
            catalogue_size: Vec::new(),
            weights: b"weights".to_vec(),
        };
        assert_eq!(bundle.file("data.7z"), Some(&b"packed"[..]));
        assert_eq!(bundle.file("model-alpha.dat"), Some(&b"weights"[..]));
        assert_eq!(bundle.file("version.txt"), Some(&b"1.76.beta"[..]));
        // A tier that is not loaded, and the usual ways out of a directory.
        for denied in ["model-gamma.dat", "../core.js", "/etc/passwd", "user.db"] {
            assert_eq!(bundle.file(denied), None, "served {denied}");
        }
    }

    #[test]
    fn a_dir_cache_round_trips_and_reports_a_miss() {
        let root = std::env::temp_dir().join(format!("delver-cache-test-{}", std::process::id()));
        let cache = DirCache::new(&root);
        let id = ArtifactId {
            version: Version::parse("1.76.beta").unwrap(),
            artifact: Artifact::CatalogueMd5,
        };

        assert!(cache.get(&id).unwrap().is_none());
        assert!(cache.newest_version().unwrap().is_none());

        cache.put(&id, b"876189c4e3eb0f63cb72b147caadbb03").unwrap();
        assert_eq!(
            cache.get(&id).unwrap().as_deref(),
            Some(&b"876189c4e3eb0f63cb72b147caadbb03"[..])
        );
        assert_eq!(cache.newest_version().unwrap(), Some(id.version.clone()));
        // The partial file the write went through must not survive it.
        let left: Vec<_> = std::fs::read_dir(root.join("1.76.beta"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, ["data.md5"]);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
