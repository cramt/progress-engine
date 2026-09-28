// The Delver X build these assets are pinned to. Shared by build.rs and the
// library through `include!`, so the table the build checks downloads against
// is the table the library reports.
//
// Upstream serves only its current build, from one origin, with no history.
// When it ships a new one these hashes stop matching what is served and a
// fresh build fails; the failure prints the replacement table.

/// The build string `version.txt` carries.
pub const VERSION: &str = "1.89.beta";

/// Where upstream serves them.
pub const ORIGIN: &str = "https://mtg.delver.app";

/// One file as upstream serves it, and the sha256 it has to have.
pub struct Pinned {
    pub name: &'static str,
    pub sha256: &'static str,
}

/// Every file fetched. Only the alpha tier: lambda and gamma are gated behind a
/// token and the engine refuses to boot them anyway.
pub const PINNED: &[Pinned] = &[
    Pinned {
        name: "version.txt",
        sha256: "862f14ee0649380f28479e412558e7a9b55c32655def449fb69ab9515c5d0d56",
    },
    Pinned {
        name: "core.js",
        sha256: "d4de616e9f3485b2dd7ea3800d938a8e002d266bc87dcbf18a63e4e43a3b19c9",
    },
    Pinned {
        name: "core.wasm",
        sha256: "fb8ce43fc99febf288f2123233602822184a3539d7b362d9576563f0c28bac50",
    },
    Pinned {
        name: "data.7z",
        sha256: "46e75f46fd18a6d041178c6f9d65f9b935d285e84b8cbe3b37ac8a844aa80b87",
    },
    Pinned {
        name: "data.md5",
        sha256: "b05a686500d97843d3549bda84059f57d6d555344a958573efbaf8f3474b963e",
    },
    Pinned {
        name: "data.size",
        sha256: "334d6ac44ca2d1166721d7214de39220de3b27d7999c2681eee562be0e0b49a7",
    },
    Pinned {
        name: "model-alpha.7z",
        sha256: "e380ee6364a777c3e37c9b6245f36c8552699911eccc300f98abf2bf671b4552",
    },
    Pinned {
        name: "model-alpha.size",
        sha256: "e1b467adce40a5df58de2b7bf347d0b0e33bb2f25c7c02b96e97a3264a475b0c",
    },
];

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
