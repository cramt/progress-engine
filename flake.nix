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

      # gitaxian-probe-engine embeds V8 through deno_core. The `v8` crate's
      # build script downloads a prebuilt libv8 from GitHub unless RUSTY_V8_*
      # points it somewhere, and a nix build has no network - so the archive
      # becomes a fixed-output derivation and the build reads it from the
      # store. Building V8 from source instead (V8_FROM_SOURCE=1) takes hours.
      #
      # The version is the `v8` crate version in Cargo.lock and the two have to
      # match exactly, and the archive variant has to match the v8 features
      # deno_core turns on - it asks for simdutf and nothing else, so the plain
      # `release` asset links but leaves every simdutf__* symbol undefined.
      # When either moves, re-prefetch:
      #   nix store prefetch-file --json \
      #     https://github.com/denoland/rusty_v8/releases/download/v<ver>/librusty_v8_simdutf_release_<target>.a.gz
      v8Version = "150.4.0";
      v8Target =
        {
          x86_64-linux = "x86_64-unknown-linux-gnu";
          aarch64-linux = "aarch64-unknown-linux-gnu";
          x86_64-darwin = "x86_64-apple-darwin";
          aarch64-darwin = "aarch64-apple-darwin";
        }
        .${system};
      v8Hash =
        {
          x86_64-linux = "sha256-9IdiyhDR8fxgWkQcWuQw7Izh6egPFNePvELLh4wwtHY=";
          aarch64-linux = "sha256-U54oOBWjlqV5bzKFi0LlF7hY66rqqtBdAykO6MhkpSc=";
          x86_64-darwin = "sha256-p1AnH+xrIRRX7Qpc99LqsZJLJlYhqC2oarlZ1v8II+Q=";
          aarch64-darwin = "sha256-Wu/9jVoMG3msHXCvg9WxkJllX9nGRaeU3EPxAfd5g4w=";
        }
        .${system};

      librustyV8 = pkgs.stdenvNoCC.mkDerivation {
        pname = "librusty_v8";
        version = v8Version;
        src = pkgs.fetchurl {
          url = "https://github.com/denoland/rusty_v8/releases/download/v${v8Version}/librusty_v8_simdutf_release_${v8Target}.a.gz";
          hash = v8Hash;
        };
        # RUSTY_V8_ARCHIVE wants the unpacked .a, not the .gz it ships as.
        dontUnpack = true;
        nativeBuildInputs = [pkgs.gzip];
        installPhase = "gzip -dc $src > $out";
      };

      # crane's default source filter keeps only Rust and Cargo files, which
      # drops the fixtures the tests read: decklists, the checked-in Scryfall
      # index and the bulk records chip-scryfall is tested against. Without them
      # the build fails late, during compilation, with a confusing "no such
      # file" for a file that plainly exists.
      #
      # gitaxian-probe-engine adds .js to that list: its four sandbox files are
      # include_str!'d into the binary, so they are sources the compiler needs
      # and not data the tests read.
      #
      # Criteria files need no entry of their own: they are TOML now, and
      # filterCargoSources keeps every .toml file.
      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          (builtins.match ".*\\.(json|jsonl|txt|js)$" path != null)
          || (craneLib.filterCargoSources path type);
        name = "source";
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
        env.RUSTY_V8_ARCHIVE = "${librustyV8}";
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
      };

      devShells.default = craneLib.devShell {
        packages = with pkgs; [
          rust-analyzer
          rustfmt
          clippy
          cargo-nextest
          jq
          # gitaxian-probe: the tests and examples decode card scans to raw
          # RGBA with `magick`, .fixtures/fetch-cards.sh parses Scryfall's JSON
          # with node and fetches with curl, and wasm-tools validates the tag
          # patch src/wasm.rs applies to core.wasm.
          imagemagick
          nodejs_24
          wasm-tools
          curl
        ];
        RUSTY_V8_ARCHIVE = "${librustyV8}";
      };
    });
}
