# Using Lapacho on Android

Lapacho on Android is a **keyboard with a paste strip**: a row of your recent
clips sits above the keys, and tapping one types it into whatever field you are
in. Your history is encrypted on the phone and never leaves it — the app has no
internet permission at all.

> **Status:** early (P0 spike). It works day to day, but it is a debug build
> installed by hand, and the keyboard is deliberately basic (suggestions and
> corrections you tap, but nothing rewritten by itself, no swipe typing, and a fixed set of emoji rather than a
> picker).

## Install

1. Download the APK on the phone from the download page —
   [web.fishman.work](https://web.fishman.work/) in English,
   [web.fishman.work/es](https://web.fishman.work/es) in Spanish — which lists
   every build with its SHA-256, and the official dictionaries. The files
   themselves are `https://web.fishman.work/sites/default/files/lapacho-<version>.apk`,
   each with its checksum next to it (`lapacho-<version>.apk.sha256`).
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

## Suggestions while you type

From the second letter of a word, the strip above the keys stops showing your
clips and offers up to three words that start with what you typed. Tap one and
it replaces the word, with a space after it. Finish the word — space,
punctuation, or delete — and your clips come back. It is the same single row
doing both jobs, so the keyboard does not grow taller.

Accents are optional while typing: `cancion` offers **canción**, `porq` offers
**porque** and **porqué**. If you start the word with a capital, the
suggestion comes capitalized.

**It only learns what you hand it.** The dictionary is the one that came with
the app — 49 525 Spanish words ordered by how common they are — and it is the
same for everybody. Nothing you type is recorded, counted or kept: suggestions
are looked up and forgotten. The one exception is a word you deliberately
teach it, below.

## Other languages, and your own word lists

Spanish ships inside the APK. Another language — or a list of your own
jargon — is a file you download with your browser and hand to Lapacho: tap
**IDIOMAS → AGREGAR** in the app and pick it from the **Download** folder the
picker opens on. If Android says it cannot read the file, you came through
the picker's *Downloads* shortcut: go **☰ → your phone's name → Download**
instead (details in [`DICTIONARIES.md`](DICTIONARIES.md#installing-one)). The keyboard never downloads
anything itself, because it has no way to.

Every dictionary you add is used **at the same time** as Spanish; there is no
key to switch. With English imported, `wh` offers *what, who, why* and `con`
still offers *con, como*. Up to two imported at once.

A dictionary we publish goes straight in and shows as **oficial ✓**. Any other
file — a list you made, or one from somewhere else — gets a warning with its
SHA-256 first, and imports only if you tap **Importar como personalizado**.
**IDIOMAS** lists them all, marked official or custom; tapping one removes
it.

A dictionary can also add long presses: French's could put é è ê on `e`.
How to make one, from a frequency list or from your own texts:
[`DICTIONARIES.md`](DICTIONARIES.md).

**Corrections.** When nothing completes what you typed, the strip offers what
you probably meant instead: `graicas` → **gracias**, `cuadno` → **cuando**,
`thnaks` → **thanks**. Swapped letters, a missing or extra one, one wrong —
up to two slips. It is only an offer: tap it and it replaces the word, keep
typing and nothing changes. Lapacho never rewrites a word on its own when you
press space. A word it does not know and cannot correct still gets the
**＋** chip to be learned, after the corrections.

There are no suggestions in password fields or incognito tabs, the same rule
the history follows.

## Teaching it a word

Names, jargon, anything the dictionary has never heard of. Write the word;
when the dictionary runs out of completions for it, the strip shows **the word
as you typed it**, with a small **＋**. Hold it — half a second, longer than
the keys — and a chip appears offering **aprender «tu palabra»**. Tap that
chip and the word is learned. Tap anywhere else, or wait five seconds, and
nothing happens.

Nothing is ever learned on its own. Only that word is stored — not the
sentence it was in, not when, not which app — and from then on it is suggested
like any other word, first among its kin.

**PALABRAS** in the app lists every word you taught it, in full. Tap one to
forget it, or **Olvidar todas** to empty the list; the dictionary that came
with the app is not touched. That list is the whole of what the keyboard has
learned about you — which is the point of teaching it one word at a time
instead of letting it watch.

Words the classifier reads as a password or a token are never offered for
learning: a learned word comes back as a suggestion, and a secret must not.

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
| ⇧ | Next letter in uppercase. **Hold it** for caps lock — or double tap, if you are quick. Tap again to release. The key changes colour: grey for one letter, green while locked. |
| ´ | Accent for the next vowel: ´ then a → á (with ⇧: Á). Shows **[´]** while waiting. |
| ñ | **Long press on n** (with ⇧: Ñ). It is no longer a key of its own. |
| @ | **Long press on a**. Also in the symbols layer. |
| ¿ ? ¡ ! | **Long press on .**: the four appear above the key, tap one. |
| ☺ | Emoji: 30 common ones, three rows, in place of the letters. Tap it again to come back. |
| ?123 / abc | Switch between letters and numbers/symbols (`< > [ ] { } = _ \| \` `¡ ¿` and more). |
| , and . | Next to the space bar, on both layers. |
| ⌫ / ↵ | Delete / new line. **Hold ⌫** and it keeps deleting. |

While you are typing a word, the strip above shows suggestions instead of your
clips; see [Suggestions while you type](#suggestions-while-you-type).

The keyboard opens on the layer the field asks for: **numbers** in a numeric
field — a transfer amount, a PIN, a phone number, a date — and the **letters**
everywhere else, with no shift pending, whatever layer you left it on in the
last app. Within a field it stays where you put it: you can type a whole
amount without the keyboard jumping back to the letters after each digit.

A key that has more characters under it shows one of them small, in the
corner: `n` shows ñ, `a` shows @, `.` shows ¿. An imported dictionary can
add more (see [Other languages](#other-languages-and-your-own-word-lists)). Hold the key — a bit under a third of a
second, shorter than Android's usual long press — and they appear above it;
tap the one you want. A long press never types on its own. The row goes away
if you touch anywhere else, or by itself after 5 seconds.

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
- Keystrokes are never recorded; only copied clips are. Suggestions are
  looked up in a fixed dictionary and nothing about them is stored — except
  the words you explicitly teach it, which the app lists and can forget.
- Secrets are never stored and never displayed.

## Troubleshooting

- **The strip does not show what I just copied.** Close and reopen the
  keyboard (tap another field): capture only happens when it opens.
- **After an update the old version is still there.** Check the version at the
  top of the app. Each build has its own file name, so a stale download is
  usually a cached page — download again from the versioned link.
- **After installing an update, a different keyboard is active.** Android can
  reset the choice on reinstall; switch back to Lapacho.
