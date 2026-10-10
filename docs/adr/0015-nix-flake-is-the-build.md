# The Nix flake is the only build and CI path

CI is `nix flake check`, which runs fmt, clippy, test and build for every product, and the devshell carries the whole toolchain. Every step CI takes is a derivation, so the cache holds all of it: a commit rebuilds only what its change reaches. The Nix sandbox has no network, so anything downloaded has to arrive as a fixed-output derivation pinned by hash.

Gitaxian Probe downloads V8 and Delver's engine at build time, so it is a cargo workspace of its own, and a plain `cargo` at the root never resolves either. The flake still builds and checks all of it. Delver's files arrive pinned by `crates/gitaxian-probe/assets/pin.json`, whose hashes are also their blob digests in the public archive on ghcr.io. rusty_v8's prebuilt static library arrives at the version the probe's `Cargo.lock` names, handed to the v8 crate as `RUSTY_V8_ARCHIVE`. The scans the accuracy tests read are Scryfall's, pinned in `engine/.fixtures/cards.nix`. The native tests run against a cache seeded from the pin, and the web check runs in nixpkgs' headless Chromium.

## Considered Options

- **Building V8 from source.** Rejected: it takes hours.
- **Leaving the native host and the web check outside the flake**, as until 2026-10. Rejected once the prebuilt V8 could be fetched by hash: the web check's workflow compiled wasm-bindgen-cli from source on every run and cached through rust-cache, and nothing ran the native accuracy test.

See [CLAUDE.md: Build and test](../../CLAUDE.md#build-and-test) and `flake.nix`.
