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

  # Gitaxian Probe is a cargo workspace of its own (crates/gitaxian-probe/) and
  # is not built here: its native host links a prebuilt V8 and its assets crate
  # downloads Delver's engine at build time, and the Nix sandbox has no network
  # for either. Bringing it in means fixed-output derivations for both - the V8
  # one pinned to the `v8` crate version and its simdutf variant, as it was
  # before the probe was parked, and GITAXIAN_PROBE_ASSETS_FROM pointed at one
  # holding the files crates/gitaxian-probe/assets/src/pin.rs names.

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
      # The probe is left out of the source altogether, so its manifests
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
