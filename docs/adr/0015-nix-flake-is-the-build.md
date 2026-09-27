# The Nix flake is the only build and CI path

CI is `nix flake check`, which runs fmt, clippy, test and build, and the devshell carries the whole toolchain. The Nix sandbox has no network, so anything downloaded has to arrive as a fixed-output derivation pinned by hash. Gitaxian Probe, which downloads both V8 and Delver's engine at build time, is therefore a cargo workspace of its own outside the flake; plain `cargo` builds Gauntlet too, and the flake stays its CI path.

## Considered Options

- **Building V8 from source.** Rejected: it takes hours.

See [CLAUDE.md: Build and test](../../CLAUDE.md#build-and-test) and `flake.nix`.
