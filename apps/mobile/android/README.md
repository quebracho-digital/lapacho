# Lapacho mobile — Android (P0 spike)

Developer notes. How to *use* the app: [`docs/USAGE_MOBILE.md`](../../../docs/USAGE_MOBILE.md).

## What this is

The P0 spike from `docs/ARQUITECTURA_MOBILE_ANDROID.md` §9, grown into
something usable day to day: encrypted on-disk storage shared by the companion
app and the keyboard (IME), which capture and paste the clipboard.

- **One app, two processes.** `:app` contains both `MainActivity` (companion)
  and `LapachoIme` (the keyboard), the latter declared with
  `android:process=":ime"`. Same UID, same `filesDir`, same Keystore alias —
  no IPC needed to share the DB (docs §4.3).
- **Storage is still Kotlin.** `HistoryRepo` (SQLite) + `LapachoCipher`
  (AES-256-GCM, key in the Android Keystore), schema mirrored from
  `lapacho_core::storage::SqliteRepo` by hand.
- **Classification is Rust.** `lapacho-core` is compiled for Android through
  `rust-bridge/` (uniffi) and the app calls `classify_sensitivity`, so a
  password is recognized by the same rules as on desktop. The rest of the
  migration (storage, keyed ids, persistence levels) is
  [`docs/MIGRACION_MOBILE_RUST.md`](../../../docs/MIGRACION_MOBILE_RUST.md).
- **"Paste keyboard", not a Gboard replacement.** The IME's real feature is the
  paste strip. The keys (letters, ñ, dead-key acute, shift/caps lock, a
  numbers/symbols layer) are enough to type with, nothing more — no
  autocorrect, no swipe, no emoji.

## Module layout

```
apps/mobile/android/
  storage/      # library module: HistoryRepo, LapachoCipher, Types
  app/          # companion Activity + LapachoIme service (separate :ime process)
  rust-bridge/  # lapacho-core behind uniffi; build-android.sh builds it for :app
```

## Building

Needs JDK 17, the Android SDK (platform 35, build-tools 35) and NDK, plus Rust
with `cargo-ndk` and the targets `aarch64-linux-android` and
`x86_64-linux-android`. The Gradle wrapper is not checked in; generate it once
with `gradle wrapper --gradle-version 8.10.2`.

```
./gradlew :app:assembleDebug        # also builds the Rust bridge
./gradlew :app:testDebugUnitTest    # JVM tests (keyboard logic)
cargo test -p lapacho-mobile-bridge # bridge tests, from the repo root
```

`preBuild` runs `rust-bridge/build-android.sh`, which builds the `.so` for
`arm64-v8a` (devices) and `x86_64` (emulator) and generates the Kotlin
bindings into `app/build/generated/rust/`. The APK is limited to those two
ABIs.

## Releasing a build

Bump `versionCode`/`versionName` in `app/build.gradle.kts` (the version is
shown in the app, which is how a user tells a cached download from a new one),
then publish the APK under a **versioned** file name together with its
checksum:

```
lapacho-<versionName>.apk
lapacho-<versionName>.apk.sha256
```

Never reuse a file name: the download URL sits behind a CDN that caches APKs.

## Manual checks

- **Cross-process read.** Save an item in the companion, open the keyboard:
  the strip shows it. `adb shell am force-stop digital.quebracho.lapacho`,
  reopen the keyboard: still there.
- **Cold start.** `adb logcat -s LapachoIme` logs the time to the input view
  and the `loadTopN` query time.
- **Secrets.** Copy a password-like string from a normal field: the strip
  shows 🔑 •••••• and logcat shows no capture. In an `<input type=password>`
  the history is hidden and the chip still pastes.

Emulator gotchas (window required for the IME, `force-stop` disabling the
keyboard, `uiautomator dump` not seeing the IME, FLAG_SECURE blacking out
screenshots of the companion) are the usual reasons a check "fails".

## Known gaps

- `contentId()` is a plain SHA-256, not desktop's keyed hash; mobile and
  desktop give the same content different ids until storage moves to Rust.
  Required for sync dedup (P4).
- No persistence levels, no TTL, no delete from the UI. The history is capped
  at `HISTORY_MAX` (100), trimmed after every write; search covers exactly
  that, so nothing is kept out of reach.
- Acting on an item is copy (app) or paste (keyboard) only. `MainActivity.copy`
  is the single entry point where desktop-style plugins will hang.
- No biometric gate, no image support, no prediction, no sync — non-goals for
  P0–P3 per docs §10.
