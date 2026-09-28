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
      # cannot reach a build that does not include them. So is the GPU spike
      # (#65), a workspace of its own for the same reason.
      #
      # Meldweb Curator's web app and worker are left out too, so a TypeScript
      # edit does not rebuild the Rust, except for the one file meldweb-wasm's
      # test compares against the types it generates.
      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          (builtins.match ".*/crates/gitaxian-probe(/.*)?$" path == null)
          && (builtins.match ".*/crates/ichormoon-gauntlet/gpu-spike(/.*)?$" path == null)
          && (builtins.match ".*/crates/meldweb-curator/worker(/.*)?$" path == null)
          && (
            (builtins.match ".*/crates/meldweb-curator/web(/.*)?$" path == null)
            || (builtins.match ".*/crates/meldweb-curator/web(/src(/deck\\.gen\\.ts)?)?$" path != null)
          )
          && ((builtins.match ".*\\.(json|jsonl|txt|ts)$" path != null)
            || (craneLib.filterCargoSources path type));
        name = "source";
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
      };

      # Must be the version meldweb-wasm pins wasm-bindgen to, exactly: the CLI
      # refuses a module built by any other. Bump both together.
      wasmBindgen = pkgs.wasm-bindgen-cli_0_2_126;

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      gauntlet = craneLib.buildPackage (commonArgs // {inherit cargoArtifacts;});

      # chip-decklist for the browser: the `wasm` profile from Cargo.toml, then
      # wasm-bindgen's JS glue. Only this package's dependency tree is built for
      # wasm32, because Gauntlet's (ureq, rustls) would not compile there.
      meldwebWasmArgs =
        commonArgs
        // {
          pname = "meldweb-wasm";
          cargoExtraArgs = "-p meldweb-wasm";
          CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
          CARGO_PROFILE = "wasm";
          doCheck = false;
        };
      meldwebWasm = craneLib.buildPackage (meldwebWasmArgs
        // {
          cargoArtifacts = craneLib.buildDepsOnly meldwebWasmArgs;
          nativeBuildInputs = [wasmBindgen];
          installPhaseCommand = ''
            wasm-bindgen --target web --out-dir $out \
              target/wasm32-unknown-unknown/wasm/meldweb_wasm.wasm
          '';
        });

      # The web app's check, test and build, as the one derivation CI runs. The
      # source is the pnpm workspace plus the deck its smoke page imports. The
      # root `pnpm check` and `pnpm test` cover every workspace package, so the
      # worker's biome, tsc and vitest run here too.
      meldwebWebSrc = pkgs.lib.fileset.toSource {
        root = ./.;
        fileset = pkgs.lib.fileset.unions [
          ./package.json
          ./pnpm-lock.yaml
          ./pnpm-workspace.yaml
          ./decks/lantern.deck.toml
          ./decks/loam.deck.toml
          (pkgs.lib.fileset.difference ./crates/meldweb-curator/web
            (pkgs.lib.fileset.maybeMissing ./crates/meldweb-curator/web/src/wasm/pkg))
          ./crates/meldweb-curator/worker/package.json
          ./crates/meldweb-curator/worker/biome.json
          ./crates/meldweb-curator/worker/tsconfig.json
          ./crates/meldweb-curator/worker/src
        ];
      };
      meldwebWeb = pkgs.stdenvNoCC.mkDerivation (finalAttrs: {
        pname = "meldweb-web";
        version = "0.1.0";
        src = meldwebWebSrc;
        pnpmDeps = pkgs.fetchPnpmDeps {
          inherit (finalAttrs) pname version src;
          fetcherVersion = 4;
          # Regenerate after any pnpm-lock.yaml change: set lib.fakeHash, build,
          # and paste the hash the failure reports.
          hash = "sha256-3VLWAXJ+bEbwjfDi1aswWianzz4qwt5owtwYz4VjR4U=";
        };
        nativeBuildInputs = [pkgs.nodejs pkgs.pnpm pkgs.pnpmConfigHook pkgs.biome];
        MELDWEB_WASM_PREBUILT = "1";
        buildPhase = ''
          runHook preBuild
          mkdir -p crates/meldweb-curator/web/src/wasm
          cp -r --no-preserve=mode ${meldwebWasm} crates/meldweb-curator/web/src/wasm/pkg
          pnpm check
          pnpm test
          pnpm build
          runHook postBuild
        '';
        installPhase = ''
          cp -r crates/meldweb-curator/web/dist $out
        '';
      });
    in {
      packages = {
        default = gauntlet;
        meldweb-web = meldwebWeb;
      };

      # Deploys Meldweb Curator: this flake's built site as the worker's static
      # assets, and the worker itself. Run from the repo root, logged in to
      # Cloudflare (or with CLOUDFLARE_API_TOKEN set), after the one-time
      # `wrangler secret put GITHUB_CLIENT_SECRET` (see the worker's README).
      apps.deploy-curator = {
        type = "app";
        meta.description = "Deploy Meldweb Curator's site and worker with wrangler";
        program = toString (pkgs.writeShellScript "deploy-curator" ''
          set -eu
          cd "$(${pkgs.git}/bin/git rev-parse --show-toplevel)"
          ln -sfn ${meldwebWeb} crates/meldweb-curator/worker/site
          exec ${pkgs.wrangler}/bin/wrangler deploy \
            --config crates/meldweb-curator/worker/wrangler.toml "$@"
        '');
      };

      checks = {
        inherit gauntlet;
        meldweb-web = meldwebWeb;
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

      devShells.gpu-spike = let
        # CUDA is unfree, so only this shell's nixpkgs allows it and nothing
        # CI builds ever sees it.
        cuda = (import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        }).cudaPackages;
        # What cubecl-cuda compiles kernels with at run time: NVRTC, and the
        # headers its generated source includes, found through CUDA_PATH.
        cudaForCubecl = pkgs.symlinkJoin {
          name = "cuda-for-cubecl";
          paths = builtins.concatMap (p: map (o: p.${o}) p.outputs) [
            cuda.cuda_nvrtc
            cuda.cuda_cudart
            cuda.cuda_cccl
          ];
        };
      in
        craneLib.devShell {
          # Issue #65's spike (crates/ichormoon-gauntlet/gpu-spike), a workspace of
          # its own. libcuda comes from the host's driver, not from here: on
          # NixOS that is /run/opengl-driver/lib.
          CUDA_PATH = cudaForCubecl;
          # cubecl-cpu links the prebuilt LLVM its build downloads, which needs a
          # libstdc++ at run time.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [cudaForCubecl pkgs.stdenv.cc.cc.lib] + ":/run/opengl-driver/lib";
        };

      devShells.default = craneLib.devShell {
        packages = with pkgs; [
          rust-analyzer
          rustfmt
          clippy
          cargo-nextest
          jq
          nodejs
          pnpm
          # From nixpkgs rather than npm: the npm package execs a prebuilt glibc
          # binary the Nix sandbox cannot run. The same copy serves CI.
          biome
          wasmBindgen
          # Deploys the worker (crates/meldweb-curator/worker/); never an npm dep.
          wrangler
        ];
      };
    });
}
