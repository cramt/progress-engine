#!/usr/bin/env bash
# Build an APK carrying both binaries: the app, and the engine host it spawns.
#
# The order is not incidental. `dx build` wipes jniLibs/<abi> for the ABI it is
# building before dropping libmain.so in, so the engine host has to be copied
# in afterwards, and gradle has to be re-run to pick it up. Doing it the other
# way round silently ships an APK without the engine.
#
# Usage: build-apk.sh <rust-target>   e.g. aarch64-linux-android
set -euo pipefail

target="${1:?usage: build-apk.sh <rust-target>}"
case "$target" in
  aarch64-linux-android) abi=arm64-v8a ;;
  x86_64-linux-android)  abi=x86_64 ;;
  *) echo "unknown target: $target" >&2; exit 1 ;;
esac

root=$(git rev-parse --show-toplevel)
gradle_project="$root/target/dx/gitaxian-probe-app/debug/android/app"
jnilibs="$gradle_project/app/src/main/jniLibs/$abi"

echo "==> engine host for $target"
cargo build -p gitaxian-probe-engine --bin probe-host --target "$target"

echo "==> app for $target"
(cd "$root/crates/gitaxian-probe/app" && dx build --android --target "$target")

echo "==> adding libprobehost.so"
install -Dm755 "$root/target/$target/debug/probe-host" "$jnilibs/libprobehost.so"

echo "==> repackaging"
(cd "$gradle_project" && ./gradlew assembleDebug --offline -q)

apk="$gradle_project/app/build/outputs/apk/debug/app-debug.apk"
echo "==> $apk"
unzip -l "$apk" | grep -E '\.so$' | awk '{printf "    %.0f MB  %s\n", $1/1048576, $4}'
