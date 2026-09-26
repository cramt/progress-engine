{
  description = "Draw-probability tests for Magic: The Gathering decklists";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  # Gitaxian Probe is parked outside the cargo workspace (see the root
  # Cargo.toml), and everything this flake carried only for it went with it:
  # the prebuilt librusty_v8 and Skia fixed-output derivations, the
  # Android-patched deno_core symlinked into vendor/, the WebView libraries, and
  # the `android` and `android-emulator` shells. Bringing the probe back means
  # restoring them from the commit that parked it - the V8 hashes are pinned to
  # the `v8` crate version and its simdutf variant, so check both still match.
  # patches/deno_core-android-errno.patch stays in the tree for that day.

  outputs = {
    nixpkgs,
    crane,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay)];
      pkgs = import nixpkgs {inherit system overlays;};

      rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      # crane's default source filter keeps only Rust and Cargo files, which
      # drops the fixtures the tests read: decklists, the checked-in Scryfall
      # index and the bulk records chip-scryfall is tested against. Without them
      # the build fails late, during compilation, with a confusing "no such
      # file" for a file that plainly exists.
      #
      # Criteria files need no entry of their own: they are TOML now, and
      # filterCargoSources keeps every .toml file.
      #
      # The parked probe is left out of the source altogether, so its manifests
      # cannot reach a build that does not include them.
      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          (builtins.match ".*/crates/gitaxian-probe(/.*)?$" path == null)
          && ((builtins.match ".*\\.(json|jsonl|txt)$" path != null)
            || (craneLib.filterCargoSources path type));
        name = "source";
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      gauntlet = craneLib.buildPackage (commonArgs // {inherit cargoArtifacts;});
    in {
      packages.default = gauntlet;

      checks = {
        inherit gauntlet;
        clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
        fmt = craneLib.cargoFmt {inherit src;};
        test = craneLib.cargoTest (commonArgs // {inherit cargoArtifacts;});
        # The independent Python checker (checker/): deals real games from the
        # committed decks and fails when an engine answer falls outside its
        # 99.9% interval (wider for a sampled one, which carries its own
        # error). Standard library only, so python3 is all it needs.
        checker =
          pkgs.runCommand "gauntlet-checker" {
            nativeBuildInputs = [pkgs.python3];
            PYTHONDONTWRITEBYTECODE = "1";
          } ''
            # The unit tests first: HANDS.md's rocks-and-dorks hands (26-33).
            CHECKER_DECKS=${./decks} python3 -m unittest discover -s ${./checker}
            python3 ${./checker}/compare.py \
              --gauntlet ${gauntlet}/bin/gauntlet \
              --decks ${./decks}
            touch $out
          '';
      };

      devShells.default = craneLib.devShell {
        packages = with pkgs; [
          rust-analyzer
          rustfmt
          clippy
          cargo-nextest
          jq
        ];
      };
    });
}
