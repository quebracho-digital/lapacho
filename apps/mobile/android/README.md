# Lapacho mobile — Android (P0 spike)

Scaffolded, **not yet built or run** — this environment has no Android
SDK/NDK/Gradle installed. Everything below needs verification on a machine
with Android Studio (or SDK + `adb` + an emulator/device).

## What this is

The P0 spike from `docs/ARQUITECTURA_MOBILE_ANDROID.md` §9: prove that
encrypted on-disk storage — not process memory — is what survives Android
killing things, and that both the companion app and the keyboard (IME) can
read/write it as two processes of the same app.

- **No Rust yet.** `lapacho-core` behind `uniffi` is P1. This spike is plain
  Kotlin: `HistoryRepo` (SQLite) + `LapachoCipher` (AES-256-GCM, key in the
  Android Keystore). Schema mirrors `lapacho_core::storage::SqliteRepo` by
  hand; expect a hand-kept-mirror drift until P1 replaces it.
- **One app, two processes.** `:app` module contains both `MainActivity`
  (companion) and `LapachoIme` (the keyboard service), the latter declared
  with `android:process=":ime"` in `AndroidManifest.xml`. Same UID, same
  `filesDir`, same Keystore alias — no IPC/ContentProvider needed to share
  the DB (see docs §4.3).
- **"Paste keyboard", not a Gboard replacement.** Per
  `docs/DEBATE_ARQUITECTURA_MOBILE.md`, the IME's real feature is the paste
  strip (tap a history item → commits raw text). The row of letter keys below
  it exists only to satisfy the literal P0 requirement ("IME that types
  characters") — no shift, no symbols, no autocorrect. Don't read it as an
  attempt at a full keyboard.

## Module layout

```
apps/mobile/android/
  storage/    # library module: HistoryRepo, LapachoCipher, Types — shared by app + IME
  app/        # companion Activity + LapachoIme service (separate :ime process)
```

## Before you can build

1. Install Android Studio (bundles SDK + a Gradle-compatible JDK), or the
   command-line SDK tools + JDK 17.
2. Generate the Gradle wrapper once (not checked in — needs network/Gradle
   installed to fetch): from `apps/mobile/android/`:
   ```
   gradle wrapper --gradle-version 8.10.2
   ```
3. Then the usual:
   ```
   ./gradlew :app:assembleDebug
   ./gradlew :app:installDebug   # needs a running emulator or device via adb
   ```

## Running the P0 checklist (docs §9)

1. Install the app, open it, type something into the `EditText`, tap
   "Guardar". Confirm it appears in the list below (companion process, own
   read).
2. Settings → System → Languages & input → On-screen keyboard → enable
   "Lapacho" as an input method. Switch to it in any text field (long-press
   the keyboard-switch icon, or the on-screen picker).
3. Confirm the paste strip at the top of the Lapacho keyboard shows the item
   you just saved — cross-process read, no restart needed.
4. `adb shell am force-stop digital.quebracho.lapacho` (kills **both**
   processes — companion and `:ime` share the app, so this is the closest adb
   equivalent to what LMK does to either one independently). Reopen the
   keyboard: the paste strip must still show the item. This is the actual
   proof the spike exists for.
5. Cold-start timing: `adb logcat -s LapachoIme` while switching into the
   keyboard after a force-stop. `onCreateInputView` logs the time from
   `onCreate`; `onStartInputView` logs the `loadTopN` query time separately.
   P0 exit criterion is "measure it", not a specific target yet.

## Known gaps (intentional, P0 scope only)

- `contentId()` in `LapachoCipher.kt` is a plain SHA-256, not the keyed hash
  desktop uses (`crypto::content_id`). Fine for a single-device spike; **must**
  be replaced by calling into `lapacho-core` once the uniffi bridge (P1)
  exists, so mobile and desktop agree on the same id for the same content —
  required for sync dedup in P4.
- No classification (`classify_sensitivity`), no `PersistLevel`/TTL settings
  UI, no ingest pipeline. Companion writes everything as `Sensitivity.NONE` /
  `PersistLevel.ALL`. All of this is P1 (`lapacho-core` behind uniffi).
- No biometric gate, no image support, no prediction, no sync. Explicit
  non-goals for P0–P3 per docs §10.
