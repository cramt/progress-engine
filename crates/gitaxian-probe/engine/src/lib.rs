//! A Rust host for the Delver X card-recognition engine.
//!
//! The engine ships as a JavaScript glue file and an 8.5 MB WebAssembly blob,
//! both downloaded from a vendor that rebuilds them on its own schedule. What
//! that pair needs is a browser, and only one of this crate's two targets has
//! one, so there are two hosts with two execution models. They share this
//! module's types, the job decoder and the wasm fingerprint, and nothing else.
//!
//! **Native** runs the pair inside a deno_core isolate whose entire host
//! surface is the seventeen ops in `sandbox`: no filesystem, no network, no
//! process, no module loader. The 32-thread pool is OS threads running isolates
//! of their own, turned by a pump. [`Engine::open`] downloads what it needs and
//! blocks until the engine is up; where the files are kept between runs is
//! [`artifacts::ArtifactCache`], `/tmp/gitaxian-probe` unless configured.
//!
//! ```no_run
//! # #[cfg(not(target_arch = "wasm32"))] {
//! use gitaxian_probe_engine::{Engine, EngineConfig};
//!
//! let mut engine = Engine::open(EngineConfig::default())?;
//! let rows = engine.query("SELECT n.name FROM data.cards c \
//!                          JOIN data.names n ON n._id = c.name LIMIT 1")?;
//! engine.close();
//! # }
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! **Web** (`wasm32`) builds none of that. `core.js` and `core.wasm` load as a
//! sidecar beside this crate's own wasm, the pool is the browser's own Web
//! Workers, and every call is a future. The files come from the page's own
//! origin - upstream sends no CORS headers - which is what
//! `gitaxian-probe-assets` is for. The web `Engine` says what the page has to
//! provide.

mod job;
pub mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub mod artifacts;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod pump;
#[cfg(not(target_arch = "wasm32"))]
mod sandbox;
#[cfg(not(target_arch = "wasm32"))]
mod worker;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use artifacts::{
    Artifact, ArtifactCache, ArtifactId, Bundle, DirCache, Origin, Source, Version,
};
#[cfg(not(target_arch = "wasm32"))]
pub use native::{Engine, EngineConfig, ProgressFn};

#[cfg(target_arch = "wasm32")]
pub use web::{Engine, EngineConfig, ProgressFn};

use anyhow::{bail, Result};
use facet::Facet;

/// The build this host was verified against. Import names are positional
/// (FINDINGS.md §5), so a changed import surface means an unverified ABI.
pub const KNOWN_FINGERPRINT: &str = "e7615396c5ece313";

// Engine ABI constants, lifted from the app bundle (FINDINGS.md §7/§12). Each
// host hands them to its glue, so these are the only copy - neither file in js/
// hardcodes its own.
pub(crate) const OUTPUT_SLOTS: u32 = 100;
pub(crate) const SEGMENTATION_MASK_BYTES: u32 = 16384;
// _rec_status(): RUNNING is -1, FINISHED_WITH_DETECTIONS is 1.
pub(crate) const REC_RUNNING: i32 = -1;
pub(crate) const REC_FINISHED_WITH_DETECTIONS: i32 = 1;

/// The constants above, as the glue on either host reads them.
pub(crate) fn abi_json() -> String {
    format!(
        r#"{{"outputSlots":{OUTPUT_SLOTS},"maskBytes":{SEGMENTATION_MASK_BYTES},"recRunning":{REC_RUNNING},"recFinishedWithDetections":{REC_FINISHED_WITH_DETECTIONS}}}"#
    )
}

/// How many frames `recognize` pushes before giving up, and how long it waits
/// for the detector to settle on each. Both are the app's own values
/// (FINDINGS.md §12).
pub(crate) const MAX_FRAMES: f64 = 12.0;
pub(crate) const SETTLE_TIMEOUT_MS: f64 = 4000.0;

/// The JWT that unlocks a gated tier. A newtype so that the only way to reach
/// [`Model::Lambda`] or [`Model::Gamma`] is to have produced one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jwt(String);

impl Jwt {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which weights to boot with.
///
/// The token rides on the variants that need it rather than sitting beside
/// them in the config, so a tokenless Gamma is not a runtime check that fires
/// late in `open` - it does not typecheck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Model {
    Alpha,
    Lambda(Jwt),
    Gamma(Jwt),
}

impl Model {
    /// Which file of weights this is, independently of what unlocks it.
    pub fn tier(&self) -> Tier {
        match self {
            Model::Alpha => Tier::Alpha,
            Model::Lambda(_) => Tier::Lambda,
            Model::Gamma(_) => Tier::Gamma,
        }
    }

    pub fn token(&self) -> Option<&Jwt> {
        match self {
            Model::Alpha => None,
            Model::Lambda(t) | Model::Gamma(t) => Some(t),
        }
    }

    /// The tier to boot, or why it cannot be. The glue hardcodes
    /// `_rec_alpha_init` and model id 0, so a gated tier would boot by feeding
    /// lambda or gamma weights to alpha's init and reporting success. Refusing
    /// is the honest answer until the per-tier init export is resolved
    /// host-side (README, *Scope*).
    pub(crate) fn bootable(&self) -> Result<Tier> {
        if self.token().is_some() {
            bail!(
                "{} is gated behind _rec_set_jwt_token and its init export is not resolved \
                 host-side yet; only Model::Alpha can boot",
                self.tier().name()
            );
        }
        Ok(self.tier())
    }
}

/// One tier of weights, as the artefact store names it. Separate from [`Model`]
/// because fetching `model-lambda.dat` needs the name and not the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Alpha,
    Lambda,
    Gamma,
}

impl Tier {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Tier::Alpha => "alpha",
            Tier::Lambda => "lambda",
            Tier::Gamma => "gamma",
        }
    }
}

/// Progress report from the boot sequence.
#[derive(Debug, Clone)]
pub struct Progress {
    pub stage: String,
    pub percent: u32,
    pub message: String,
}

/// Raw RGBA. Decoding is the caller's problem, as it is in the Node harness.
pub struct Image<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl Image<'_> {
    fn check(&self) -> Result<()> {
        let want = self.width as usize * self.height as usize * 4;
        if self.data.len() != want {
            bail!(
                "expected {want} bytes of RGBA for {}x{}, got {}",
                self.width,
                self.height,
                self.data.len()
            );
        }
        Ok(())
    }
}

/// One recognised card. `location` and `centroid` are percentages, not pixels.
#[derive(Facet, Debug, Clone)]
pub struct Detection {
    pub name: String,
    #[facet(default)]
    pub number: String,
    #[facet(rename = "dataId")]
    pub data_id: i64,
    #[facet(rename = "recConf", default)]
    pub rec_conf: i64,
    #[facet(rename = "setConf", default)]
    pub set_conf: i64,
    /// Top-10 candidates from the embedding index, best first.
    #[facet(default)]
    pub similar: Vec<i64>,
    #[facet(default)]
    pub location: Vec<f64>,
    #[facet(default)]
    pub centroid: Vec<f64>,
    #[facet(rename = "imageFilename", default)]
    pub image_filename: Option<String>,
}

/// A catalogue row resolved from a [`Detection::data_id`].
#[derive(Facet, Debug, Clone)]
pub struct Card {
    pub data_id: i64,
    pub name: String,
    pub edition: String,
    pub number: String,
    pub rarity: Option<String>,
    pub artist: Option<String>,
    pub scryfall_id: Option<String>,
    pub release_date: Option<String>,
}

impl Card {
    /// The query `card_by_id` runs, on either host.
    pub(crate) fn sql(data_id: i64) -> String {
        format!(
            "SELECT n.name, e.name, c.number, c.rarity, c.artist, c.scryfall_id, e.release_date \
             FROM data.cards c \
             JOIN data.names n ON n._id = c.name \
             JOIN data.editions e ON e._id = c.edition \
             WHERE c._id = {data_id}"
        )
    }

    /// The row [`Card::sql`] returned, if it returned one.
    pub(crate) fn from_rows(data_id: i64, rows: &[Vec<String>]) -> Option<Card> {
        let row = rows.first()?;
        let text = |i: usize| row.get(i).cloned();
        let or_empty = |i: usize| text(i).unwrap_or_default();
        Some(Card {
            data_id,
            name: or_empty(0),
            edition: or_empty(1),
            number: or_empty(2),
            rarity: text(3),
            artist: text(4),
            scryfall_id: text(5),
            release_date: text(6),
        })
    }
}

#[derive(Facet, Debug, Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// What `boot` reports back once the engine is up.
#[derive(Facet, Debug)]
pub(crate) struct BootInfo {
    pub(crate) version: String,
}
