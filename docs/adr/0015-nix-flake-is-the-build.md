# The Nix flake is the only build and CI path

CI is `nix flake check`, which runs fmt, clippy, test and build, and the devshell carries the whole toolchain. The Nix sandbox has no network, so prebuilt V8 and Skia are fixed-output derivations pinned by hash, and deno_core's Android fix is applied by the flake. `vendor/deno_core` is an uncommitted store symlink, so a bare `cargo build` outside `nix develop` fails. That is deliberate.

## Considered Options

- **Building V8 from source.** Rejected: it takes hours.
- **A cargo `[patch]` alone for deno_core.** Rejected: cargo cannot apply a diff.

See [CLAUDE.md: Build and test](../../CLAUDE.md#build-and-test) and `flake.nix`.
