#!/usr/bin/env bash
# Builds Lapacho Voice's bridge for a device (arm64-v8a) and the emulator
# (x86_64), copies in sherpa-onnx's own shared libraries (Android only links
# them dynamically), and generates the Kotlin bindings. Called by the
# :plugin-voice Gradle build; OUT is where it expects jniLibs/ and kotlin/.
set -euo pipefail
out="$1"
cd "$(dirname "$0")"
target="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

cargo ndk -t arm64-v8a -t x86_64 -o "$out/jniLibs" build -p lapacho-voice-bridge --release

# sherpa-onnx-sys downloads its Android archive into the target directory;
# the bridge links two of its libraries dynamically.
for abi in arm64-v8a x86_64; do
    for lib in libonnxruntime.so libsherpa-onnx-c-api.so; do
        cp "$target/sherpa-onnx-prebuilt/jniLibs/$abi/$lib" "$out/jniLibs/$abi/"
    done
done

# Bindings from an unstripped host build, as in rust-bridge/build-android.sh.
# Its own target directory: with the Android archive already in the shared
# one, sherpa-onnx-sys 1.13.8 doesn't fetch the host's libraries and the
# link fails.
host="$target/voice-bindgen"
CARGO_TARGET_DIR="$host" cargo build -q -p lapacho-voice-bridge
CARGO_TARGET_DIR="$host" cargo run -q -p lapacho-voice-bridge --features bindgen --bin uniffi-bindgen -- \
    generate --library "$host/debug/liblapacho_voice_bridge.so" \
    --language kotlin --no-format --out-dir "$out/kotlin"
