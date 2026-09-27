# The Nix flake is the only build and CI path

CI is `nix flake check`, which runs fmt, clippy, test and build, and the devshell carries the whole toolchain. The Nix sandbox has no network, so the parked probe's prebuilt V8 was a fixed-output derivation pinned by hash. With Gitaxian Probe parked outside the workspace (and its Android app deleted), plain `cargo` builds the rest too; the flake stays the CI path.

## Considered Options

- **Building V8 from source.** Rejected: it takes hours.

See [CLAUDE.md: Build and test](../../CLAUDE.md#build-and-test) and `flake.nix`.
