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
      #     <base>/v<ver>/librusty_v8_simdutf_release_<target>.a.gz
      v8Version = "150.4.0";

      # denoland publishes nothing for Android: v150.4.0 carries 72 assets and
      # not one matches *-linux-android, and CI stopped building it in
      # denoland/rusty_v8#1558 (merged 2024-08-02). Plenty of aarch64 assets
      # exist - the missing axis is bionic, not the architecture, and a glibc
      # archive will not link against the NDK.
      #
      # aidant's fork rebuilds the same upstream tags with Android and iOS
      # turned back on, and is what the upstream tracking issue points people
      # at. Its v150.4.0 is our exact `v8` version and carries the simdutf
      # variant deno_core needs, so this is a different publisher of the same
      # artefact rather than a downgrade. It is one person's fork offered with
      # "no promises for support", which the pinned hash below contains: the
      # bytes cannot change under us, they can only stop being fetchable.
      #
      # Remove `v8AndroidBase` and go back to `v8Base` for every target the day
      # denoland ships Android assets again.
      # https://github.com/denoland/rusty_v8/issues/1640
      v8Base = "https://github.com/denoland/rusty_v8/releases/download";
      v8AndroidBase = "https://github.com/aidant/rusty_v8/releases/download";

      # The rust target triple for this system. Both prebuilt archives below are
      # published per triple and spell it the same way.
      rustTarget =
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

      v8ArchiveFor = base: target: hash:
        pkgs.stdenvNoCC.mkDerivation {
          pname = "librusty_v8";
          version = v8Version;
          src = pkgs.fetchurl {
            url = "${base}/v${v8Version}/librusty_v8_simdutf_release_${target}.a.gz";
            inherit hash;
          };
          # RUSTY_V8_ARCHIVE wants the unpacked .a, not the .gz it ships as.
          dontUnpack = true;
          nativeBuildInputs = [pkgs.gzip];
          installPhase = "gzip -dc $src > $out";
        };

      librustyV8 = v8ArchiveFor v8Base rustTarget v8Hash;

      # Cross builds need a second file the host build does not. Normally the
      # `v8` crate finds `src_binding_*.rs` beside the archive it downloaded;
      # with RUSTY_V8_ARCHIVE pointing into the store there is no "beside", so
      # the path is passed explicitly too. Plain .rs, not gzipped.
      librustyV8AndroidBinding = pkgs.fetchurl {
        url = "${v8AndroidBase}/v${v8Version}/src_binding_simdutf_release_aarch64-linux-android.rs";
        hash = "sha256-dyeCauR5vbZF6Acjn7EtH44uI956bPFvXuWSaQ0dhQY=";
      };

      # One archive per ABI, because RUSTY_V8_ARCHIVE is read verbatim and
      # carries no target templating - hence one devshell each. aarch64 is the
      # phone, x86_64 is the emulator. Both come from the same release, and the
      # two `src_binding` files happen to be byte-identical, so the x86_64 one
      # is not fetched twice.
      librustyV8Android =
        v8ArchiveFor v8AndroidBase "aarch64-linux-android"
        "sha256-Di0djEEBzG/3JC3XJSg15jiE0run6cjGRjRD7mYl/ug=";

      librustyV8AndroidX86 =
        v8ArchiveFor v8AndroidBase "x86_64-linux-android"
        "sha256-exOLoP+QVOrHrQm/E96Do3LSR4iTXBAzueMkPPNUrFw=";

      librustyV8AndroidX86Binding = pkgs.fetchurl {
        url = "${v8AndroidBase}/v${v8Version}/src_binding_simdutf_release_x86_64-linux-android.rs";
        hash = "sha256-dyeCauR5vbZF6Acjn7EtH44uI956bPFvXuWSaQ0dhQY=";
      };

      # deno_core reaches Android and then refuses to compile for it:
      # `uv_compat/tty.rs` gates `mod global_termios` on #[cfg(unix)], Android
      # is unix, and its errno arm covers only macos and linux - and in Rust
      # `target_os = "android"` is not `target_os = "linux"`. Bionic spells the
      # accessor `__errno` where glibc has `__errno_location`, so the fix is
      # nine lines.
      #
      # Cargo cannot apply a diff: `[patch.crates-io]` only redirects a
      # dependency to another *source*. So nix patches the crate and hands
      # cargo the result as a path, and `vendor/deno_core` below is a symlink
      # into the store rather than anything committed. It is applied on every
      # target, not just Android, so host and phone compile the same source;
      # the new arm is inert off Android.
      #
      # 0.412.0 is the newest release, so there is no version to bump to
      # instead. Remove this once upstream carries an android arm - the file is
      # libs/core/uv_compat/tty.rs, in the deno monorepo since deno_core was
      # merged into it.
      # https://github.com/denoland/deno
      denoCorePatched = pkgs.applyPatches {
        name = "deno_core-0.412.0-android";
        src = pkgs.fetchCrate {
          pname = "deno_core";
          version = "0.412.0";
          hash = "sha256-KSDpcScyZCLattgUXEFMkf4I0YCPuJu8tdz/gzSwnS0=";
        };
        patches = [./patches/deno_core-android-errno.patch];
      };

      # Both the devshell and the crane builds need `vendor/deno_core` to exist
      # before cargo reads the workspace manifest, so the link is made the same
      # way in both rather than only in the shell.
      linkPatchedDenoCore = ''
        mkdir -p vendor
        ln -sfn ${denoCorePatched} vendor/deno_core
      '';

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

      # gitaxian-probe-app links Skia through skia-bindings, whose build script
      # downloads a prebuilt archive and, when that fails, falls back to
      # fetching the whole Skia source tree and building it. In a nix sandbox
      # both downloads fail, and the error you see is the second one - so this
      # looks like "it wants to build Skia from source" when it only wants the
      # prebuilt it could not reach.
      #
      # `SKIA_BINARIES_URL` takes a file:// URL and reads it straight off disk,
      # so the prebuilt becomes a fixed-output derivation like librusty_v8.
      #
      # The key is not guessable: it is the rust-skia commit, the target triple
      # and the *resolved* cargo feature set, in that order. Read it off a real
      # build rather than deriving it -
      #   cat target/debug/build/skia-bindings-*/out/skia/key.txt
      # - and the build also prints the whole URL as `FROM: ...`. When
      # skia-bindings or the feature set moves, both the key and these hashes
      # change together.
      skiaTag = "0.97.2";
      skiaKey = "da8fc6731fc439bc3b6a";
      skiaFeatures = "gl-jpegd-jpege-pdf-textlayout";
      skiaHash =
        {
          x86_64-linux = "sha256-7nf70Bg+hU4pcnZwXk6GhYN8bH0DBEcslxRfzY9/LPw=";
          aarch64-linux = "sha256-JYfcrxGqtoDvhjfUGS/HelB8keOoi+u3nXmTpP76HRs=";
          x86_64-darwin = "sha256-/pLmaRaUek1maiTQWAQ09CWFhT0iHSrwBqUqcrVbKDs=";
          aarch64-darwin = "sha256-xMXVBZq5ImqvPVM3qP1C7w5C6f48vDyNpDELSjoeQlQ=";
        }
        .${system};

      skiaBinaries = pkgs.fetchurl {
        url = "https://github.com/rust-skia/skia-binaries/releases/download/${skiaTag}/skia-binaries-${skiaKey}-${rustTarget}-${skiaFeatures}.tar.gz";
        hash = skiaHash;
      };

      commonArgs = {
        inherit src;
        strictDeps = true;
        env = {
          RUSTY_V8_ARCHIVE = "${librustyV8}";
          SKIA_BINARIES_URL = "file://${skiaBinaries}";
        };
        preConfigure = linkPatchedDenoCore;
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
      # wrapper and so decides which libc symbols exist. 26 used to be the
      # floor - below it `android_properties` will not link against
      # __system_property_read_callback, and AHardwareBuffer, which wgpu and
      # skia both want, is not there either.
      #
      # V8 raised it to 28. Its bundled libc++ and libc++abi call
      # `aligned_alloc`, which bionic declares `__INTRODUCED_IN(28)`, so at 26
      # `operator new(size_t, align_val_t)` and
      # `__aligned_malloc_with_fallback` link against nothing. Keep in sync
      # with android_min_sdk_version in crates/gitaxian-probe/app/Dioxus.toml.
      androidApi = "28";

      # dx generates a gradle project pinned to compileSdk 34, and gradle
      # cannot install what is missing because the SDK is read-only in the nix
      # store - it fails with "The SDK directory is not writable" rather than
      # naming the version it wanted. Everything gradle asks for is declared.
      androidBuildTools = "34.0.0";

      androidComposition = androidPkgs.androidenv.composeAndroidPackages {
        platformVersions = ["34" "35"];
        buildToolsVersions = [androidBuildTools];
        includeNDK = true;

        # The emulator is x86_64, so it is a different ABI from the phone and
        # needs its own V8 archive and its own shell - see
        # `devShells.android-emulator`. The host has /dev/kvm and vmx, so the
        # image runs accelerated rather than interpreting arm64.
        #
        # Note this does not make `--device` optional: dx calls
        # start_simulators() whenever it is unset, and that path boots the
        # *first* AVD rather than talking to whatever is plugged in.
        includeEmulator = true;
        includeSystemImages = true;
        systemImageTypes = ["default"];
        abiVersions = ["x86_64"];
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
      #
      # Underscores, not the dashes of the triple. cc-rs reads either spelling,
      # but `CC_aarch64-linux-android` is not a valid shell identifier, so
      # `nix develop` cannot export it and it silently does not exist - the
      # build then falls through to the host `CC` and dies in glibc headers
      # looking for gnu/stubs-32.h. It stayed hidden because dx computes its
      # own target_cc/target_cxx/ar_path and passes them to cargo itself, so
      # only a bare `cargo --target aarch64-linux-android` ever saw it.
      androidCrossEnv = target: let
        under = builtins.replaceStrings ["-"] ["_"] target;
        upper = pkgs.lib.toUpper under;
        clang = "${ndkBin}/${target}${androidApi}-clang";
      in {
        "CARGO_TARGET_${upper}_LINKER" = clang;
        "CC_${under}" = clang;
        "CXX_${under}" = "${clang}++";
        "AR_${under}" = "${ndkBin}/llvm-ar";
        "RANLIB_${under}" = "${ndkBin}/llvm-ranlib";
      };

      # One shell per Android ABI. They differ only in which V8 archive and
      # compiler-rt they point at, but `RUSTY_V8_ARCHIVE` is read verbatim with
      # no target templating, so one shell cannot serve both: `.#android` is
      # the phone and `.#android-emulator` is the x86_64 AVD.
      androidShellFor = {
        target,
        arch,
        v8Archive,
        v8Binding,
      }:
        pkgs.mkShell (
          pkgs.lib.mergeAttrsList (map androidCrossEnv androidTargets)
          // {
            name = "gitaxian-probe-android-${arch}";

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

            # The Android V8, not the host one the default shell uses. A cargo
            # build for the host triple from inside this shell will therefore
            # fail to link - that is the trade for these not being per-target.
            RUSTY_V8_ARCHIVE = "${v8Archive}";
            RUSTY_V8_SRC_BINDING_PATH = "${v8Binding}";

            # Two complete libc++ implementations end up in one .so: the prebuilt
            # V8 was built by Chromium against its own bundled third_party/libc++
            # and carries it inside the archive, while skia-bindings asks for the
            # NDK's with `vec!["log", "android", "c++_static", "c++abi"]`
            # (build_support/platform/android.rs). Every std::logic_error and
            # std::runtime_error symbol is then defined twice and ld.lld refuses.
            #
            # This tells the linker to keep the first definition and drop the
            # rest, which is an ODR violation. It survives here only because Skia
            # and V8 never hand each other C++ objects - they meet through Rust -
            # so each uses its own copy internally and the duplicated symbols are
            # exception types neither throws across the boundary.
            #
            # The actual fix is a V8 built with use_custom_libcxx=false so it
            # shares the NDK runtime, which means building V8 rather than using a
            # prebuilt. Drop this the day that archive exists.
            RUSTFLAGS = "-C link-arg=-Wl,--allow-multiple-definition";

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
              ${linkPatchedDenoCore}

              # V8's CpuFeatures::FlushICache calls __clear_cache, which
              # lives in compiler-rt rather than libc. rustc passes
              # -nodefaultlibs, so the clang driver never adds compiler-rt on its
              # own and the symbol goes undefined - but only when linking an
              # executable. A cdylib link appears to succeed because shared
              # objects tolerate undefined symbols, and dx builds the bin.
              #
              # The archive is passed by path rather than -L/-l because the clang
              # major version sits in the directory name and would otherwise be
              # another thing to bump by hand.
              # https://github.com/denoland/rusty_v8/issues/1640
              builtins_archive=$(echo ${ndkBin}/../lib/clang/*/lib/linux/libclang_rt.builtins-${arch}-android.a)
              export RUSTFLAGS="$RUSTFLAGS -C link-arg=$builtins_archive"

              echo "gitaxian-probe android shell (${target})"
              echo "  dx     $(dx --version 2>/dev/null || echo '??')"
              echo "  rustc  $(rustc --version)"
              echo "  ndk    ${ndkRoot}"
              echo
              echo "  cd crates/gitaxian-probe/app"
              echo "  desktop : cargo run -p gitaxian-probe-app"
              echo "  android : dx serve --android --renderer native --target ${target} --device"
            '';
          }
        );

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

        shellHook = linkPatchedDenoCore;
      };

      devShells.android = androidShellFor {
        target = "aarch64-linux-android";
        arch = "aarch64";
        v8Archive = librustyV8Android;
        v8Binding = librustyV8AndroidBinding;
      };

      devShells.android-emulator = androidShellFor {
        target = "x86_64-linux-android";
        arch = "x86_64";
        v8Archive = librustyV8AndroidX86;
        v8Binding = librustyV8AndroidX86Binding;
      };
    });
}
