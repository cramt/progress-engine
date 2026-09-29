// Where the pinned build comes from and how it is laid out for serving. Shared
// by build.rs and the library through `include!`; the pin itself is
// `pin.json`, which build.rs turns into the library's VERSION, ARCHIVE_TAG and
// PINNED.

/// Where upstream serves its current build, and only that one.
pub const ORIGIN: &str = "https://mtg.delver.app";

/// The registry that keeps every build, once upstream has stopped serving it.
pub const ARCHIVE_REGISTRY: &str = "https://ghcr.io";

/// The public OCI artifact in [`ARCHIVE_REGISTRY`] holding every build: one
/// tag per build, each file its own blob, addressed by the sha256 the pin
/// gives it. `crates/gitaxian-probe/archive/` publishes it.
pub const ARCHIVE_REPOSITORY: &str = "cramt/delver-x";

/// What ends up in the asset directory, which is what a page serves. The same
/// names as upstream, except that the weights arrive unpacked: the engine
/// unpacks its own catalogue but not its model, so something has to, and doing
/// it at build time keeps an LZMA decoder out of the page.
pub const SERVED: &[&str] = &[
    "version.txt",
    "core.js",
    "core.wasm",
    "data.7z",
    "data.md5",
    "data.size",
    "model-alpha.dat",
];
