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

      # gitaxian-probe-app is Dioxus Native: on the desktop it paints through
      # skia and dlopens the graphics stack, and fontconfig is found with
      # pkg-config at build time rather than at run time.
      desktopGraphics = with pkgs; [fontconfig wayland libxkbcommon libGL vulkan-loader];

      commonArgs = {
        inherit src;
        strictDeps = true;
        env.RUSTY_V8_ARCHIVE = "${librustyV8}";
        nativeBuildInputs = with pkgs; [
          pkg-config
          python3 # stylo generates Rust from Python in its build scripts
        ];
        buildInputs = desktopGraphics;
      };

      # The Android toolchain, kept out of the default shell: it is an unfree
      # SDK and about a gigabyte, and only gitaxian-probe-app wants it. Enter
      # it with `nix develop .#android`.
      androidPkgs = import nixpkgs {
        inherit system overlays;
        config = {
          allowUnfree = true; # the Android SDK and NDK are not free software
          android_sdk.accept_license = true;
        };
      };

      # The API level to compile *against*: it picks the NDK's per-level clang
      # wrapper and so decides which libc symbols exist. 26 is the real floor -
      # below it `android_properties` will not link against
      # __system_property_read_callback, and AHardwareBuffer, which wgpu and
      # skia both want, is not there either. Keep in sync with
      # android_min_sdk_version in crates/gitaxian-probe/app/Dioxus.toml.
      androidApi = "26";

      # dx generates a gradle project pinned to compileSdk 34, and gradle
      # cannot install what is missing because the SDK is read-only in the nix
      # store - it fails with "The SDK directory is not writable" rather than
      # naming the version it wanted. Everything gradle asks for is declared.
      androidBuildTools = "34.0.0";

      androidComposition = androidPkgs.androidenv.composeAndroidPackages {
        platformVersions = ["34" "35"];
        buildToolsVersions = [androidBuildTools];
        includeNDK = true;
        includeEmulator = false;
        includeSystemImages = false;
      };

      # `includeNDK` puts the NDK inside the SDK output, so one derivation
      # covers both and they cannot drift apart.
      sdkRoot = "${androidComposition.androidsdk}/libexec/android-sdk";
      ndkRoot = "${sdkRoot}/ndk-bundle";
      ndkBin = "${ndkRoot}/toolchains/llvm/prebuilt/linux-x86_64/bin";

      androidTargets = ["aarch64-linux-android" "x86_64-linux-android"];

      # nixpkgs ships dx 0.7.x, whose dioxus-native hardcodes the Vello
      # backend, and Vello renders a black screen on Android
      # (https://github.com/DioxusLabs/blitz/issues/377). The `skia` renderer
      # feature only exists from 0.8.0-alpha.1, and dx refuses to drive a
      # workspace whose dioxus version it does not match - so both move
      # together. Drop this once nixpkgs carries dioxus-cli >= 0.8.0.
      dx = androidPkgs.dioxus-cli.overrideAttrs (_: rec {
        version = "0.8.0-alpha.1";
        src = androidPkgs.fetchCrate {
          pname = "dioxus-cli";
          inherit version;
          hash = "sha256-4x9xTc9FW03ohEhDOe+wJ0EJ4yR8HWFmiEA+hvlLF7Q=";
        };
        cargoDeps = androidPkgs.rustPlatform.fetchCargoVendor {
          inherit src;
          hash = "sha256-eGGdmI5dvNav2fJmDv/GD7Anfd0lRModfgfEg+Jg3CQ=";
        };
        doCheck = false;
      });

      # Rust's cross-compilation env vars embed the triple in the name, so
      # build them rather than writing five near-identical lines per target.
      androidCrossEnv = target: let
        upper = pkgs.lib.toUpper (builtins.replaceStrings ["-"] ["_"] target);
        clang = "${ndkBin}/${target}${androidApi}-clang";
      in {
        "CARGO_TARGET_${upper}_LINKER" = clang;
        "CC_${target}" = clang;
        "CXX_${target}" = "${clang}++";
        "AR_${target}" = "${ndkBin}/llvm-ar";
        "RANLIB_${target}" = "${ndkBin}/llvm-ranlib";
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
          # gitaxian-probe-app: pkg-config finds fontconfig at build time,
          # python3 runs stylo's generators, ninja is skia-bindings' fallback
          # when it cannot use a prebuilt.
          pkg-config
          python3
          ninja
          # gitaxian-probe: the tests and examples decode card scans to raw
          # RGBA with `magick`, .fixtures/fetch-cards.sh parses Scryfall's JSON
          # with node and fetches with curl, and wasm-tools validates the tag
          # patch src/wasm.rs applies to core.wasm.
          imagemagick
          nodejs_24
          wasm-tools
          curl
        ];
        buildInputs = desktopGraphics;

        RUSTY_V8_ARCHIVE = "${librustyV8}";

        # dioxus-native dlopens these at run time, and nothing on the link line
        # points at them.
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath desktopGraphics;
      };

      devShells.android = pkgs.mkShell (
        pkgs.lib.mergeAttrsList (map androidCrossEnv androidTargets)
        // {
          name = "gitaxian-probe-android";

          packages = [
            rustToolchain
            dx
            androidPkgs.jdk17 # gradle and aapt2 need a JVM
            androidPkgs.gradle
            androidComposition.androidsdk
            androidComposition.platform-tools # adb
            pkgs.pkg-config
            pkgs.python3 # stylo generates Rust from Python
            pkgs.ninja # skia-bindings, if it falls back to a source build
            pkgs.clang
            pkgs.cargo-ndk
          ];
          buildInputs = desktopGraphics;

          ANDROID_HOME = sdkRoot;
          ANDROID_SDK_ROOT = sdkRoot;
          ANDROID_NDK_ROOT = ndkRoot;
          ANDROID_NDK_HOME = ndkRoot;
          # rust-skia's build script looks for this exact name, not the two above.
          ANDROID_NDK = ndkRoot;
          JAVA_HOME = "${androidPkgs.jdk17}";

          # Gradle would otherwise download its own aapt2, a prebuilt binary
          # whose interpreter path does not exist on NixOS.
          GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${sdkRoot}/build-tools/${androidBuildTools}/aapt2";

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath desktopGraphics;

          shellHook = ''
            echo "gitaxian-probe android shell"
            echo "  dx     $(dx --version 2>/dev/null || echo '??')"
            echo "  rustc  $(rustc --version)"
            echo "  ndk    ${ndkRoot}"
            echo
            echo "  cd crates/gitaxian-probe/app"
            echo "  desktop : cargo run -p gitaxian-probe-app"
            echo "  android : dx serve --android --renderer native --target aarch64-linux-android"
          '';
        }
      );
    });
}
