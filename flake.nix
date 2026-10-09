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
    terranix = {
      url = "github:terranix/terranix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    pnpm2nix = {
      url = "github:cramt/pnpm2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  # Gitaxian Probe is a cargo workspace of its own (crates/gitaxian-probe/),
  # built here in full: the assets its pin names and the prebuilt V8 its native
  # host links, each a fixed-output fetch, so nothing it builds or tests reaches
  # the network on its own.

  outputs = {
    nixpkgs,
    crane,
    flake-utils,
    rust-overlay,
    terranix,
    pnpm2nix,
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
      #
      # Meldweb Curator's web app and worker are left out too, so a TypeScript
      # edit does not rebuild the Rust, except for the one file meldweb-wasm's
      # test compares against the types it generates.
      #
      # docs/ (the baselines are JSON) and the pnpm workspace's package.json
      # match the extensions but no Rust reads them, so they are left out too:
      # a baseline refresh or a pnpm bump would otherwise rebuild every crate.
      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          !(pkgs.lib.hasPrefix (toString ./docs) path)
          && path != toString ./package.json
          && (builtins.match ".*/crates/gitaxian-probe(/.*)?$" path == null)
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
          # The colour-identity sigils a deck tile draws
          ./assets
          (pkgs.lib.fileset.difference ./crates/meldweb-curator/web
            (pkgs.lib.fileset.maybeMissing ./crates/meldweb-curator/web/src/wasm/pkg))
          ./crates/meldweb-curator/worker/package.json
          ./crates/meldweb-curator/worker/biome.json
          ./crates/meldweb-curator/worker/tsconfig.json
          ./crates/meldweb-curator/worker/src
          # The scanner's build names the engine files it fetches by this pin
          ./crates/gitaxian-probe/assets/pin.json
        ];
      };
      # node_modules come from pnpm2nix, which fetches each package by the
      # integrity pnpm-lock.yaml already records, so there is no deps hash to
      # keep: the lockfile is the pin. The worker is a package here because
      # the root check and test run it too.
      meldwebWorkspace = pnpm2nix.lib.${system}.mkPnpmWorkspace {
        workspace = ./.;
        appSrc = _: meldwebWebSrc;
        packages = ["crates/meldweb-curator/worker"];
        apps = [
          {
            name = "meldweb-web";
            path = "crates/meldweb-curator/web";
            version = "0.1.0";
            extraNativeBuildInputs = [pkgs.biome];
          }
        ];
        nodejs = pkgs.nodejs;
      };
      meldwebWeb = meldwebWorkspace.apps.meldweb-web.overrideAttrs (old: {
        env =
          old.env
          // {
            MELDWEB_WASM_PREBUILT = "1";
            # The scanner's API; the site never carries the engine's files,
            # which the worker proxies from the archive
            MELDWEB_PROBE_PREBUILT = "${probeBindgen}";
          };
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

      # Gitaxian Probe's web build. The pin (assets/pin.json) names every file
      # of the Delver X build by sha256, the engine's and each tier's model, and on ghcr.io that sha256 is the
      # file's blob digest, so each file is a fixed-output derivation whose
      # hash is the pin: it may reach the network, and nothing unpinned gets
      # in. The registry wants a token even for a public blob, and hands an
      # anonymous one to anybody, which plain fetchurl cannot ask for.
      probePin = builtins.fromJSON (builtins.readFile ./crates/gitaxian-probe/assets/pin.json);
      probePinned = probePin.engine ++ builtins.concatMap (tier: tier.model) (builtins.attrValues probePin.tiers);
      probeBlob = file:
        pkgs.runCommand "delver-x-${file.name}" {
          nativeBuildInputs = [pkgs.curl pkgs.jq];
          SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
          impureEnvVars = pkgs.lib.fetchers.proxyImpureEnvVars;
          outputHashMode = "flat";
          outputHashAlgo = "sha256";
          outputHash = file.sha256;
        } ''
          repo=cramt/delver-x
          token=$(curl -fsS --retry 3 "https://ghcr.io/token?scope=repository:$repo:pull" | jq -r .token)
          curl -fsSL --retry 3 -H "Authorization: Bearer $token" -o $out \
            "https://ghcr.io/v2/$repo/blobs/sha256:${file.sha256}"
        '';
      probeFiles = pkgs.linkFarm "delver-x-${probePin.version}" (map (file: {
          inherit (file) name;
          path = probeBlob file;
        })
        probePinned);

      # The probe's own workspace, without its build output, the fixtures
      # fetched for its tests, or the archive's publisher.
      probeSrc = pkgs.lib.cleanSourceWith {
        src = ./crates/gitaxian-probe;
        filter = path: type:
          builtins.match ".*/crates/gitaxian-probe/(target|archive|engine/\\.fixtures)(/.*)?$" path == null;
        name = "gitaxian-probe-source";
      };
      probeArgs = {
        src = probeSrc;
        strictDeps = true;
        version = "0.1.0";
        # The assets crate's build script takes the files from here and still
        # checks every one against the pin; offline, it cannot fall back to a
        # download.
        GITAXIAN_PROBE_ASSETS_FROM = probeFiles;
        GITAXIAN_PROBE_OFFLINE = "1";
      };

      # The served directory: the pinned files as upstream shipped them, as
      # the assets crate lays them out. Its tests check each one is there and that
      # version.txt says what the pin does.
      probeAssetsArgs =
        probeArgs
        // {
          pname = "gitaxian-probe-assets";
          cargoExtraArgs = "-p gitaxian-probe-assets";
        };
      probeAssets = craneLib.buildPackage (probeAssetsArgs
        // {
          cargoArtifacts = craneLib.buildDepsOnly probeAssetsArgs;
          installPhaseCommand = ''
            cargo run --release --offline -p gitaxian-probe-assets --example copy -- $out
          '';
        });

      # The probe's JavaScript API for a page: the wasm and its glue. The same
      # wasm-bindgen as meldweb-wasm, which the probe's Cargo.lock pins to. It
      # needs no engine file, so Meldweb Curator's site builds from it alone.
      probeBindgenArgs =
        # Not the engine's files: naming them would make every site build
        # fetch the 42 MB it never carries
        builtins.removeAttrs probeArgs ["GITAXIAN_PROBE_ASSETS_FROM"]
        // {
          pname = "gitaxian-probe-bindgen";
          cargoExtraArgs = "-p gitaxian-probe-bindgen";
          CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
          doCheck = false;
        };
      probeBindgen = craneLib.buildPackage (probeBindgenArgs
        // {
          cargoArtifacts = craneLib.buildDepsOnly probeBindgenArgs;
          nativeBuildInputs = [wasmBindgen];
          installPhaseCommand = ''
            wasm-bindgen --target web --out-dir $out \
              target/wasm32-unknown-unknown/release/gitaxian_probe_bindgen.wasm
          '';
        });
      # The API beside the files it serves: pkg/ the API, gitaxian-probe/ the
      # engine's files, for a page that serves its own copy.
      probeWeb = pkgs.runCommand "gitaxian-probe-web" {} ''
        mkdir $out
        ln -s ${probeBindgen} $out/pkg
        ln -s ${probeAssets} $out/gitaxian-probe
      '';

      # The native host links rusty_v8's prebuilt static library, which the v8
      # crate's build script downloads unless RUSTY_V8_ARCHIVE names a copy.
      # The version is Cargo.lock's, so a v8 bump fails here on the hash rather
      # than linking a library built for another one. deno_core turns on
      # simdutf, which is the archive's name suffix.
      probeLock = builtins.fromTOML (builtins.readFile ./crates/gitaxian-probe/Cargo.lock);
      v8Version = (pkgs.lib.findFirst (p: p.name == "v8") (throw "the probe's Cargo.lock has no v8") probeLock.package).version;
      rustyV8 = pkgs.fetchurl {
        url = "https://github.com/denoland/rusty_v8/releases/download/v${v8Version}/librusty_v8_simdutf_release_${pkgs.stdenv.hostPlatform.rust.rustcTarget}.a.gz";
        hash =
          {
            x86_64-linux = "sha256-9IdiyhDR8fxgWkQcWuQw7Izh6egPFNePvELLh4wwtHY=";
          }.${
            system
          } or (throw "no rusty_v8 ${v8Version} hash for ${system}");
      };
      probeNativeArgs =
        probeArgs
        // {
          pname = "gitaxian-probe";
          RUSTY_V8_ARCHIVE = rustyV8;
        };
      probeCargoArtifacts = craneLib.buildDepsOnly probeNativeArgs;

      # The six scans the accuracy numbers are measured on, framed the way a
      # camera sees a card. The scans are Scryfall's, pinned by hash: a rescan
      # upstream breaks only a machine that has not fetched them yet, and the
      # fix is the new hash, since the frames are what FINDINGS.md's numbers
      # were measured on.
      probeFrames = pkgs.runCommand "gitaxian-probe-frames" {nativeBuildInputs = [pkgs.imagemagick];} ''
        mkdir $out
        ${pkgs.lib.concatMapStrings (card: ''
            magick ${pkgs.fetchurl {inherit (card) url hash;}} -resize 55% -background '#2b2b30' \
              -gravity center -extent 1280x960 $out/${card.slug}-frame.jpg
          '')
          (import ./crates/gitaxian-probe/engine/.fixtures/cards.nix)}
      '';

      # The engine's tests fetch it from Delver and fall back to the cache in
      # /tmp/gitaxian-probe when that fails, which in the sandbox it always
      # does. Seeding the cache from the pin makes them test the pinned build,
      # and PROBE_REQUIRE_ENGINE turns any skip into a failure, so a green run
      # is the accuracy numbers holding. The cache keeps the models unpacked.
      probeTest = craneLib.cargoTest (probeNativeArgs
        // {
          cargoArtifacts = probeCargoArtifacts;
          nativeBuildInputs = [pkgs.imagemagick pkgs.p7zip];
          PROBE_REQUIRE_ENGINE = "1";
          preCheck = ''
            mkdir -p engine/.fixtures
            cp ${probeFrames}/*-frame.jpg engine/.fixtures/
            cache=/tmp/gitaxian-probe/${probePin.version}
            mkdir -p $cache
            for f in ${probeFiles}/*; do
              name=$(basename $f)
              case $name in
                model-*.7z) 7z e -so $f ''${name%.7z}.dat > $cache/''${name%.7z}.dat ;;
                *) cp $f $cache/$name ;;
              esac
            done
          '';
        });

      # The web host's acceptance check: the native accuracy numbers, in
      # headless Chromium, on the page and inside a module worker, per tier.
      probeWebCheckArgs =
        probeBindgenArgs
        // {
          pname = "gitaxian-probe-web-check";
          cargoExtraArgs = "-p gitaxian-probe-web-check";
        };
      probeWebCheckWasm = craneLib.buildPackage (probeWebCheckArgs
        // {
          cargoArtifacts = craneLib.buildDepsOnly probeWebCheckArgs;
          nativeBuildInputs = [wasmBindgen];
          installPhaseCommand = ''
            wasm-bindgen --target web --out-dir $out \
              target/wasm32-unknown-unknown/release/gitaxian_probe_web_check.wasm
          '';
        });
      probeWebCheckPage = pkgs.lib.fileset.toSource {
        root = ./crates/gitaxian-probe/web-check;
        fileset = pkgs.lib.fileset.unions (map (f: ./crates/gitaxian-probe/web-check + "/${f}") [
          "index.html"
          "frames.js"
          "worker.js"
          "serve.py"
          "drive.mjs"
        ]);
      };
      probeWebCheck =
        pkgs.runCommand "gitaxian-probe-web-check" {
          nativeBuildInputs = [pkgs.python3 pkgs.nodejs];
          # playwright from nixpkgs, with the browsers built for that version
          NODE_PATH = "${pkgs.playwright-test}/lib/node_modules";
          PLAYWRIGHT_BROWSERS_PATH = pkgs.playwright-driver.browsers;
        } ''
          export HOME=$TMPDIR
          site=$TMPDIR/site
          mkdir -p $site/fixtures
          ln -s ${probeWebCheckWasm} $site/pkg
          ln -s ${probeAssets} $site/gitaxian-probe
          cp ${probeFrames}/*-frame.jpg $site/fixtures/
          cp ${probeWebCheckPage}/{index.html,frames.js,worker.js} $site/

          python3 ${probeWebCheckPage}/serve.py $site 8791 &
          server=$!
          trap 'kill $server 2>/dev/null' EXIT
          until python3 -c 'import socket; socket.create_connection(("127.0.0.1", 8791))' 2>/dev/null; do
            sleep 0.1
          done
          for tier in alpha lambda gamma; do
            node ${probeWebCheckPage}/drive.mjs "http://127.0.0.1:8791/?tier=$tier"
            node ${probeWebCheckPage}/drive.mjs "http://127.0.0.1:8791/?tier=$tier&worker"
          done | tee $out
        '';
      # Meldweb Curator's deploy: the worker's own source (not its node_modules
      # or the dev symlink to a site) with this flake's built site as its assets.
      curatorWorker = pkgs.lib.fileset.toSource {
        root = ./crates/meldweb-curator/worker;
        fileset = pkgs.lib.fileset.unions [
          ./crates/meldweb-curator/worker/src
          ./crates/meldweb-curator/worker/wrangler.toml
        ];
      };
      curatorDeploy = pkgs.callPackage ./crates/meldweb-curator/infra/deploy.nix {
        worker = curatorWorker;
        site = meldwebWeb;
      };

      # wrangler.toml names the worker and its account, so tofu reads them from
      # there rather than keeping a second copy
      curatorWrangler = builtins.fromTOML (builtins.readFile ./crates/meldweb-curator/worker/wrangler.toml);
      infraConfig = terranix.lib.terranixConfiguration {
        inherit system;
        modules = [./crates/meldweb-curator/infra/infra.nix];
        extraArgs = {
          curator =
            import ./crates/meldweb-curator/infra/config.nix
            // {
              accountId = curatorWrangler.account_id;
              workerName = curatorWrangler.name;
            };
          deploy = curatorDeploy;
        };
      };

      tofu = pkgs.opentofu.withPlugins (p: [p.cloudflare_cloudflare]);

      # State is remote, so a throwaway working dir per run is enough
      infra = pkgs.writeShellApplication {
        name = "infra";
        runtimeInputs = [tofu pkgs.curl pkgs.jq];
        text = ''
          # Every secret is in 1Password's Homelab vault, as ~/nixconf's infra
          # does it: the token is the service account opnix reads with, and
          # `op` is the system's, as it is unfree and wrapped there
          OP_SERVICE_ACCOUNT_TOKEN=$(cat /etc/opnix-token)
          export OP_SERVICE_ACCOUNT_TOKEN
          CLOUDFLARE_API_TOKEN=$(op read 'op://Homelab/MeldwebCurator/cloudflareApiToken')
          TF_VAR_github_client_secret=$(op read 'op://Homelab/MeldwebCurator/githubClientSecret')
          # State lives where nixconf's does: the terraformremotestate database on luna
          PG_CONN_STR="postgres://terraformremotestate:$(op read 'op://Homelab/TerraformRemoteState/password')@$(op read 'op://Homelab/Infrastructure/lunaInternalAddress'):5432"
          export CLOUDFLARE_API_TOKEN TF_VAR_github_client_secret PG_CONN_STR

          token=$(curl -fsS -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
            https://api.cloudflare.com/client/v4/accounts/${curatorWrangler.account_id}/tokens/verify)
          # verify says "active" even before not_before; every real call then fails
          if jq -e '.result.not_before // empty | fromdateiso8601 > now' <<<"$token" >/dev/null; then
            echo "CLOUDFLARE_API_TOKEN is not valid until $(jq -r .result.not_before <<<"$token")" >&2
            exit 1
          fi

          work=$(mktemp -d)
          trap 'rm -rf "$work"' EXIT
          cp ${infraConfig} "$work/config.tf.json"
          tofu -chdir="$work" init -input=false >/dev/null
          tofu -chdir="$work" "$@"
        '';
      };
    in {
      packages = {
        default = gauntlet;
        meldweb-web = meldwebWeb;
        gitaxian-probe-assets = probeAssets;
        gitaxian-probe-web = probeWeb;
        gitaxian-probe-frames = probeFrames;
        infra-config = infraConfig;
      };

      # `nix run .#infra -- plan|apply` is the one command that ships Meldweb
      # Curator: the worker, the site, the client secret and meldweb.cramt.dk.
      # deploy-curator is only the worker and site, with the secret uploaded
      # when GITHUB_CLIENT_SECRET is set and kept when it isn't.
      apps.deploy-curator = {
        type = "app";
        meta.description = "Deploy Meldweb Curator's site and worker with wrangler";
        program = pkgs.lib.getExe curatorDeploy;
      };
      apps.infra = {
        type = "app";
        meta.description = "Run OpenTofu on Meldweb Curator's Cloudflare stack";
        program = pkgs.lib.getExe infra;
      };

      checks = {
        inherit gauntlet;
        meldweb-web = meldwebWeb;
        gitaxian-probe-web = probeWeb;
        gitaxian-probe-test = probeTest;
        gitaxian-probe-web-check = probeWebCheck;
        gitaxian-probe-clippy = craneLib.cargoClippy (probeNativeArgs
          // {
            cargoArtifacts = probeCargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
        gitaxian-probe-fmt = craneLib.cargoFmt {
          src = probeSrc;
          pname = "gitaxian-probe";
          version = "0.1.0";
        };
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
