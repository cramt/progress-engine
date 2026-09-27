// The Delver X build these assets are pinned to. Shared by build.rs and the
// library through `include!`, so the table the build checks downloads against
// is the table the library reports.
//
// Upstream serves only its current build, from one origin, with no history.
// When it ships a new one these hashes stop matching what is served and a
// fresh build fails; the failure prints the replacement table.

/// The build string `version.txt` carries.
pub const VERSION: &str = "1.83.beta";

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
        sha256: "a62860d0722990fd83ca5e74db7ea0c4860797b86734dd4546d04869b38c0f71",
    },
    Pinned {
        name: "core.js",
        sha256: "b2726070abe302427dff168134ecfe8b289a0ad806cae1cf647b6d188a82db65",
    },
    Pinned {
        name: "core.wasm",
        sha256: "5172360a5c9196523a85b67bb665978c8249cc09ad2985ec10774306859f66c6",
    },
    Pinned {
        name: "data.7z",
        sha256: "9aeae631e87d7157e9adf3790d45fe573042d57f20754962ecb33e98e0035050",
    },
    Pinned {
        name: "data.md5",
        sha256: "b956491fadb38e368c99be563b04ea89dd27e9212b68ea127e65971ee5e5641e",
    },
    Pinned {
        name: "data.size",
        sha256: "5380d19560ccf7e76e09ccaa3e1c05143454e87e88ba3a51e3b6809cdc9fae06",
    },
    Pinned {
        name: "model-alpha.7z",
        sha256: "f0d3e2d90c8abe3b53937ddd29b0998d7889c086a1b691bae5ddcac3385ba8ef",
    },
    Pinned {
        name: "model-alpha.size",
        sha256: "b9f6bae6a9f2be6dbe4c8665304bda7d29b94df6b49d55bd8e6a85a9e0c816b5",
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
