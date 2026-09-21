{
  description = "Dioxus Native (Blitz) hello world, cross-compiled to Android from NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
          config = {
            allowUnfree = true; # the Android SDK/NDK are not free software
            android_sdk.accept_license = true;
          };
        };
        lib = pkgs.lib;

        # Android API level we compile *against*, which picks the NDK's
        # per-level clang wrapper and so decides which libc symbols exist.
        # 26 is the real floor: below it `android_properties` fails to link
        # against __system_property_read_callback, and AHardwareBuffer (which
        # wgpu and Skia want) is not there either. Keep in sync with
        # android_min_sdk_version in Dioxus.toml.
        androidApi = "26";

        # arm64 for real phones, x86_64 because that is what dx falls back to
        # when no device is attached and it assumes an emulator. Installing both
        # only costs a rust-std download; nothing is compiled until you name a
        # target. dx aborts with a bare ENOENT if the triple it picked is
        # missing, because it shells out to `rustup`, which a nix toolchain
        # has no reason to provide.
        rustAndroidTargets = ["aarch64-linux-android" "x86_64-linux-android"];

        # dx generates a gradle project pinned to compileSdk 34, and gradle
        # cannot install what is missing because the SDK lives read-only in the
        # nix store — it fails with "The SDK directory is not writable" rather
        # than naming the version it wanted. Everything gradle asks for has to
        # be declared up front.
        buildToolsVersion = "34.0.0";

        androidComposition = pkgs.androidenv.composeAndroidPackages {
          platformVersions = ["34" "35"];
          buildToolsVersions = [buildToolsVersion];
          includeNDK = true;
          includeEmulator = false;
          includeSystemImages = false;
        };

        # `includeNDK` puts the NDK inside the SDK output, so one derivation
        # covers both and they cannot drift apart.
        sdkRoot = "${androidComposition.androidsdk}/libexec/android-sdk";
        ndkRoot = "${sdkRoot}/ndk-bundle";
        ndkBin = "${ndkRoot}/toolchains/llvm/prebuilt/linux-x86_64/bin";

        # Keep in sync with Blitz's own flake — blitz-dom uses edition 2024 and
        # stylo's build scripts are picky about older toolchains.
        rustToolchain = pkgs.rust-bin.stable."1.90.0".default.override {
          extensions = ["rust-src" "rust-analyzer" "clippy"];
          targets = rustAndroidTargets;
        };

        # nixpkgs ships dx 0.7.10, but 0.7's dioxus-native hardcodes the Vello
        # backend, which renders a black screen on Android
        # (https://github.com/DioxusLabs/blitz/issues/377). The `skia` renderer
        # feature only exists from 0.8.0-alpha.1 on, and dx refuses to drive a
        # workspace whose dioxus version it doesn't match — so both move together.
        # Drop this override once nixpkgs carries dioxus-cli >= 0.8.0.
        dx = pkgs.dioxus-cli.overrideAttrs (_: rec {
          version = "0.8.0-alpha.1";
          src = pkgs.fetchCrate {
            pname = "dioxus-cli";
            inherit version;
            hash = "sha256-4x9xTc9FW03ohEhDOe+wJ0EJ4yR8HWFmiEA+hvlLF7Q=";
          };
          cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
            inherit src;
            hash = "sha256-eGGdmI5dvNav2fJmDv/GD7Anfd0lRModfgfEg+Jg3CQ=";
          };
          doCheck = false;
        });

        # Rust env vars are per-target and spelled with the triple embedded in
        # the name, so build them rather than writing four near-identical lines.
        crossEnvFor = target: let
          # aarch64-linux-android -> AARCH64_LINUX_ANDROID
          upper = lib.toUpper (builtins.replaceStrings ["-"] ["_"] target);
          clang = "${ndkBin}/${target}${androidApi}-clang";
        in {
          "CARGO_TARGET_${upper}_LINKER" = clang;
          "CC_${target}" = clang;
          "CXX_${target}" = "${clang}++";
          "AR_${target}" = "${ndkBin}/llvm-ar";
          "RANLIB_${target}" = "${ndkBin}/llvm-ranlib";
        };
      in {
        packages.dx = dx;

        devShells.default = pkgs.mkShell (
          lib.mergeAttrsList (map crossEnvFor rustAndroidTargets)
          // {
            name = "dioxus-native-android";

            packages = [
              rustToolchain
              dx
              pkgs.jdk17 # gradle + aapt2 need a JVM
              pkgs.gradle
              androidComposition.androidsdk
              androidComposition.platform-tools # adb
              pkgs.pkg-config
              pkgs.python3 # stylo's build scripts generate Rust from Python
              pkgs.ninja # skia-bindings, if it falls back to a source build
              pkgs.clang
              pkgs.cargo-ndk
            ];

            # Desktop builds (`cargo run`) dlopen these; the Android build does not.
            buildInputs = with pkgs; [fontconfig wayland libxkbcommon libGL vulkan-loader];

            ANDROID_HOME = sdkRoot;
            ANDROID_SDK_ROOT = sdkRoot;
            ANDROID_NDK_ROOT = ndkRoot;
            ANDROID_NDK_HOME = ndkRoot;
            # rust-skia's build script looks for this exact name, not the two above.
            ANDROID_NDK = ndkRoot;

            JAVA_HOME = "${pkgs.jdk17}";


            # Gradle would otherwise download its own aapt2, a prebuilt binary
            # with an interpreter path that does not exist on NixOS.
            GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${sdkRoot}/build-tools/${buildToolsVersion}/aapt2";

            shellHook = ''
              export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library"
              export LD_LIBRARY_PATH="${lib.makeLibraryPath [pkgs.fontconfig pkgs.wayland pkgs.libxkbcommon pkgs.libGL pkgs.vulkan-loader]}:$LD_LIBRARY_PATH"

              echo "dioxus-native android shell"
              echo "  dx      $(dx --version 2>/dev/null || echo '??')"
              echo "  rustc   $(rustc --version)"
              echo "  ndk     ${ndkRoot}"
              echo
              echo "  desktop : cargo run"
              echo "  android : dx serve --android --renderer native --target aarch64-linux-android"
            '';
          }
        );
      }
    );
}
