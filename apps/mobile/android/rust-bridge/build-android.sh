#!/usr/bin/env bash
# Builds the bridge for a device (arm64-v8a) and the emulator (x86_64), and
# generates its Kotlin bindings. Called by the :app Gradle build; OUT is where
# it expects jniLibs/ and kotlin/.
set -euo pipefail
out="$1"
cd "$(dirname "$0")"
target="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

cargo ndk -t arm64-v8a -t x86_64 -o "$out/jniLibs" build -p lapacho-mobile-bridge --release

# Bindings come from an unstripped host build: the release profile strips the
# symbol table uniffi reads its metadata from, and on a stripped .so the
# generator finds nothing and still exits 0. The metadata is the same on every
# architecture.
cargo build -q -p lapacho-mobile-bridge
cargo run -q -p lapacho-mobile-bridge --features bindgen --bin uniffi-bindgen -- \
    generate --library "$target/debug/liblapacho_mobile_bridge.so" \
    --language kotlin --no-format --out-dir "$out/kotlin"
