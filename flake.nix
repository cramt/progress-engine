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

      # rusty_v8's build script downloads a prebuilt V8 at build time, which a
      # Nix sandbox has no network for. Pre-fetch both artefacts and point the
      # documented environment variables at them, so the build script finds them
      # already on disk and never reaches for the network.
      #
      # This is not a bug to wait out: rusty_v8 ships V8 as a prebuilt release
      # asset by design, because building it from source needs depot_tools and
      # roughly an hour. See https://github.com/denoland/rusty_v8#binary-build.
      #
      # Removal condition: drop this block if the crate ever gains a vendored
      # build that works offline, or if nixpkgs starts packaging librusty_v8 at
      # a matching version.
      #
      # Pinned to the `v8` crate version in Cargo.lock. Bumping deno_core means
      # bumping RUSTY_V8_VERSION and both hashes together; a mismatch shows up
      # as the build script reaching for the network again and failing.
      # The asset name encodes the crate's enabled features, and deno_core turns
      # on `simdutf`. Fetching the plain `_release_` variant links far enough to
      # look right and then fails at the very end with undefined `simdutf__*`
      # symbols, so the `_simdutf_` variant is the one that matters.
      RUSTY_V8_VERSION = "150.4.0";
      v8Target = "x86_64-unknown-linux-gnu";
      v8Variant = "simdutf_release";
      v8Release = "https://github.com/denoland/rusty_v8/releases/download/v${RUSTY_V8_VERSION}";
      librusty_v8 = pkgs.fetchurl {
        url = "${v8Release}/librusty_v8_${v8Variant}_${v8Target}.a.gz";
        hash = "sha256-9IdiyhDR8fxgWkQcWuQw7Izh6egPFNePvELLh4wwtHY=";
      };
      v8SrcBinding = pkgs.fetchurl {
        url = "${v8Release}/src_binding_${v8Variant}_${v8Target}.rs";
        hash = "sha256-dyeCauR5vbZF6Acjn7EtH44uI956bPFvXuWSaQ0dhQY=";
      };

      # crane's default source filter keeps only Rust and Cargo files, which
      # drops three things this build genuinely needs: the JavaScript bootstrap
      # that pe-js embeds with include_str!, and the decklist and index fixtures
      # the tests read. Without them the build fails late, during compilation,
      # with a confusing "no such file" for a file that plainly exists.
      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          (builtins.match ".*\\.(js|json|txt)$" path != null)
          || (craneLib.filterCargoSources path type);
        name = "source";
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
        RUSTY_V8_ARCHIVE = librusty_v8;
        RUSTY_V8_SRC_BINDING_PATH = v8SrcBinding;
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      progress-engine = craneLib.buildPackage (commonArgs // {inherit cargoArtifacts;});
    in {
      packages.default = progress-engine;

      checks = {
        inherit progress-engine;
        clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
        fmt = craneLib.cargoFmt {inherit src;};
        test = craneLib.cargoTest (commonArgs // {inherit cargoArtifacts;});
      };

      devShells.default = craneLib.devShell {
        packages = with pkgs; [rust-analyzer rustfmt clippy cargo-nextest jq];
      };
    });
}
