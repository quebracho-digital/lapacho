# Using Lapacho on Android

Lapacho on Android is a **keyboard with a paste strip**: a row of your recent
clips sits above the keys, and tapping one types it into whatever field you are
in. Your history is encrypted on the phone and never leaves it — the app has no
internet permission at all.

> **Status:** early (P0 spike). It works day to day, but it is a debug build
> installed by hand, and the keyboard is deliberately basic (no autocorrect,
> no swipe typing, no emoji).

## Install

1. Download the APK on the phone. Builds are published as
   `https://web.fishman.work/sites/default/files/lapacho-<version>.apk`, each
   with its checksum next to it (`lapacho-<version>.apk.sha256`).
2. Optional, recommended: check the download against the `.sha256` file (any
   hash app on the phone, or `sha256sum` on a computer).
3. Open the APK and allow the install from your browser when Android asks.
   Installing a newer version on top keeps your history.
4. Open **Lapacho** once. The top line shows the installed version — use it to
   confirm an update actually landed.

## Turn on the keyboard

Installing is not enough; Android requires enabling a keyboard by hand:

**Settings → System → Languages & input → On-screen keyboard → Manage
keyboards → Lapacho** (the exact path varies by manufacturer).

Android warns that the keyboard "may be able to collect all the text you
type". It shows that for every third-party keyboard. Lapacho stores copied
clips, never your keystrokes, and cannot send anything anywhere.

To switch keyboards, tap the keyboard icon in the navigation bar while typing
(or long-press the space bar on some phones) and pick Lapacho.

## How capture works

Android only lets the keyboard read the clipboard **while it is on screen**.
So Lapacho has no background monitor: a clip enters the history the next time
the Lapacho keyboard opens after you copy it.

1. Copy something anywhere, as usual.
2. Tap any text field with Lapacho as the active keyboard.
3. The clip is now first in the strip and saved in the history.

The same text copied twice keeps one entry.

## Paste

Tap a clip in the strip: it is typed into the field, exactly as it was copied.
Scroll the strip sideways for older ones (the 20 most recent are shown).

## Search

Tap **🔍** at the start of the strip. While searching, the keys type into the
search instead of the field; the strip shows what you typed and the matching
clips. Tap one to paste it, or press **↵** to paste the first match. **✕**
leaves search without pasting.

Search covers the whole history (the last 100 clips), ignores case and accents
("cancion" finds "Canción"), and every word must appear, in any order. Secrets
never show up in results.

## Passwords and other secrets

Lapacho never stores a password, and never shows one:

- **What counts as a secret.** Anything a password manager marks as sensitive
  when copying (Bitwarden, 1Password… on Android 13+), plus anything Lapacho's
  own classifier recognizes — the same one the desktop app uses: passwords,
  API tokens, private keys, card numbers, recovery codes.
- **Pasting one.** While a secret is on the clipboard, the strip shows
  **🔑 ••••••** first. Tapping it pastes what is on the clipboard at that
  moment. It is not saved and never shown.
- **Password and PIN fields, and incognito tabs.** The strip hides the
  history ("🔒 campo privado: historial oculto") and nothing is captured. If
  something is on the clipboard, the 🔑 ••••••  chip is still there, so a
  copied password can go into a password field.
- **A secret stored anyway** — saved on purpose with GUARDAR in the app, or
  captured by a version before 0.1.7, which had no classifier — is shown as
  🔑 •••••• with a short id. **BORRAR HISTORIAL** removes it.

The history keeps the **last 100 clips** — as far as search reaches — and
trims the oldest on every new one. What is *not* covered yet: there are no
persistence levels as on desktop, no expiry (TTL), and single items cannot be
deleted yet — **Borrar historial** in the app wipes all of it.

## The keyboard

| Key | What it does |
|-----|--------------|
| ⇧ | Next letter in uppercase. **Double tap**: caps lock (⇪). Tap again to release. |
| ´ | Accent for the next vowel: ´ then a → á (with ⇧: Á). Shows **[´]** while waiting. |
| ñ | End of the middle row. |
| ?123 / abc | Switch between letters and numbers/symbols (`< > [ ] { } = _ \| \` `¡ ¿` and more). |
| , and . | Next to the space bar, on both layers. |
| ⌫ / ↵ | Delete / new line. |

Keys vibrate and click according to the phone's own settings (keyboard
vibration, touch sounds); Lapacho has no setting of its own for that.

## The app

The app is also the keyboard's settings: Android's keyboard list opens it from
the settings entry next to Lapacho.

It shows the same history as the keyboard, with a search box on top. Secrets
are masked and carry a short id so two of them can be told apart. **Tap an
item to copy it** back to the clipboard, ready to paste anywhere; a masked one
is copied flagged as sensitive, so it stays masked. **GUARDAR** saves text by
hand. **BORRAR HISTORIAL** deletes every stored item and empties the clipboard
(otherwise the clip still on it would come back the next time the keyboard
opens); it asks first. The list refreshes every time you come
back to the app. Its screen is protected: screenshots, screen recording and the recent-apps
thumbnail come out black.

## Privacy, in short

- No internet permission: nothing can leave the phone.
- History encrypted at rest (AES-256-GCM, key in the Android Keystore).
- Excluded from Google backup.
- Keystrokes are never recorded; only copied clips are.
- Secrets are never stored and never displayed.

## Troubleshooting

- **The strip does not show what I just copied.** Close and reopen the
  keyboard (tap another field): capture only happens when it opens.
- **After an update the old version is still there.** Check the version at the
  top of the app. Each build has its own file name, so a stale download is
  usually a cached page — download again from the versioned link.
- **After installing an update, a different keyboard is active.** Android can
  reset the choice on reinstall; switch back to Lapacho.
