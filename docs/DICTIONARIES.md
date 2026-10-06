# Keyboard dictionaries — format, mixing, and making your own

The Android keyboard suggests words from **dictionaries**: plain-text word
lists with frequencies. Spanish ships inside the APK. Any other language — or
a list of your own jargon, names, or a field's vocabulary — is a file you
import in the app. The keyboard has no network permission, so it never
downloads one; you bring it (why: [`DECISIONS.md`](DECISIONS.md)).

## Installing one

1. Get the file onto the phone: download it with the browser, or copy it
   over USB. Official ones are on the download page, as
   `lapacho-dict-<lang>-<hash>.dict` — `.dict` rather than `.txt` so the
   browser saves the file instead of opening it as a page of text.
2. In the Lapacho app, tap **LANGUAGES → ADD**. The picker opens on the
   phone's **Download** folder; pick the file there.
3. If it is one of ours, it goes straight in. If it is not, the app says so
   first — see below.
4. The keyboard uses it the next time it opens.

**If it says Android will not let it read the file**
(`SecurityException: …providers.downloads has no access to content://media/…`),
the file was reached through the picker's **Downloads** shortcut. On some
phones (seen on a Pixel) that shortcut hands the request to the media store,
which refuses a non-media file the browser saved. The same file through the
phone's storage reads fine: in the picker, **☰ → the phone's name →
Download**. That is where the picker opens by default since `0.1.23`.

**Official or custom.** The APK carries the SHA-256 of every dictionary we
publish. A file that matches imports without a question and is listed as
**oficial ✓**. One that does not — your own list, one published after your
version of the app, or one somebody altered — gets a warning with its full
hash and two buttons: **Import as custom** or **Cancel**. The app
cannot tell those three cases apart, so it asks, every time, per file; there
is no setting that turns the check off for good. Either way the file must pass
the [format](#format) check.

**LANGUAGES** lists what the keyboard is using — the bundled ones (Spanish
and English), then each imported file as *oficial ✓* or *personalizado*,
with the start of its SHA-256. Tap any of them to remove it, bundled ones
included; **ADD** offers a removed bundled language back before asking for a
file. Up to **six** languages at a time (each costs ~1.2 MB of keyboard
memory per 50 000 words), at least one. Importing one whose `#lang` is already there replaces
it — for a bundled language too, so a newer official English replaces the
one inside the APK.

## Format

UTF-8 text, one entry per line, with a header at the top:

```text
#lapacho-dict 1
#lang en
#name English
#alternates e:éè c:ç
the 22761659
you 28787591
GitHub 1200
kubectl
```

**Header** — `#` lines at the top, before the first word:

| Line | Required | Meaning |
|------|----------|---------|
| `#lapacho-dict 1` | yes, **first line** | Marks the file as a dictionary. Anything else is refused. |
| `#lang <id>` | yes | Identifies it: lowercase letters, digits and `-`, up to 32 (`en`, `pt-br`, `es-medicina`). Also its file name inside the app, so a second file with the same `id` replaces the first. `es` is taken by the bundled one. |
| `#name <text>` | no | What **LANGUAGES** shows. Defaults to the `id`. |
| `#alternates <key:chars> …` | no | Characters added to a key's long press, space-separated pairs. The key is one letter `a`–`z`, 1 to 8 characters after it. `#alternates e:éèêë c:ç` makes holding `e` offer é è ê ë. |
| `#rows <row> <row> …` | no | The letter keys, top row first: 2 to 4 rows of up to 12 lowercase letters, no letter twice. `#rows azertyuiop qsdfghjklm wxcvbn` is French AZERTY. Without it the language types on the keyboard's own QWERTY (with the ´ dead key). See [How dictionaries mix](#how-dictionaries-mix). |

Alternates from every active dictionary are merged, each character once, after
the ones the keyboard always has (`@` on `a`). Spanish's header is where `ñ`
on `n` comes from.

**Entries** — `word frequency`, separated by a space or tab:

- The frequency is a whole number: how often the word appears, in any unit.
  Only the proportions matter (see [Mixing](#how-dictionaries-mix)).
- A line with just a word counts as frequency 1.
- Order does not matter; the keyboard sorts on load.
- A malformed line (`word notanumber`, three fields) is skipped, not fatal.
- Write the word as it should be typed: `GitHub` comes out as `GitHub`. A
  suggestion is capitalized when you start the word with a capital, never
  lowered.
- Accents are part of the word (`canción`), but the lookup ignores them:
  typing `cancion` finds it. The same goes for ç, ã, õ and the umlauts.

**Limits.** At most 8 MB — room for a few hundred thousand words; the bundled
Spanish is 50 000 words in 640 KB. Each 50 000 words cost the keyboard about
1.2 MB of memory while it is open; at most six languages are active at once,
bundled ones included, because each one more is read before the first
suggestion.

## How dictionaries mix

All active dictionaries **that type on the same keys** are used at once —
there is no language switch. Type `wh` with English imported and you get
`what`, `who`, `why`; type `con` and Spanish still answers.

A dictionary with its own `#rows` is a different **layout**, and a 🌐 key
appears next to ?123 to cycle through them (it stays hidden while there is
only one). Languages on the same rows share a layout: Spanish, English and
Portuguese all live on QWERTY, so they never need the key; a French AZERTY
does. Suggestions, corrections, swipe and long presses come only from the
dictionaries of the layout on screen. The keyboard starts on the first
dictionary's layout and keeps the last one picked from one app to the next.

To keep a big corpus from burying a small one, each file's frequencies are
turned into **shares of that file's total** before mixing. "de" is about 3.5 %
of the Spanish list and "the" about 3.3 % of the English one, so they compete
on equal terms even though the English corpus is bigger.
A word in two files keeps its higher share. A word you taught the keyboard
(WORDS) always comes first.

Which has a consequence for small custom lists: **the fewer words in a file,
the larger each one's share.** A 100-word list with equal frequencies gives
each word 1 % — more than all but a handful of everyday words. That is usually
what you want from jargon (typing `ku` should offer `kubectl`), but if a small
list keeps pushing common words aside, give it real frequencies, or give its
everyday words (the ones it shares with Spanish) low ones.

## Making one from a frequency list

[hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords)
(MIT) has lists for ~50 languages, counted over subtitles. It is where the
bundled Spanish and English come from. English, for example:

```sh
curl -sfLO https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/en/en_50k.txt
{
  printf '#lapacho-dict 1\n#lang en\n#name English\n'
  # Plain lowercase words only; of the one-letter ones, keep "a" and "i".
  awk '$1 ~ /^[a-z]+$/ && (length($1) > 1 || $1 ~ /^[ai]$/) { print $1, $2 }' en_50k.txt
} > en.txt
sha256sum en.txt
```

For a language with accents, widen the pattern to its letters —
Portuguese: `/^[a-zàáâãçéêíóôõú]+$/`, and add `#alternates` for the ones the
layout lacks (`#alternates c:ç a:ãáàâ o:õóô e:éê`). The filter matters: the
raw lists carry names, numbers and subtitle noise, and every word in the file
is one the keyboard may offer.

### The official ones

Every official dictionary is built by
`apps/mobile/android/dictionaries/build.py`, one recipe per language: the
header, which entries count as words, which one-letter words to keep, and how
to mend the subtitles' missing accents.

```sh
curl -sfLO https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/fr/fr_50k.txt
python3 build.py fr fr_50k.txt > fr.txt
```

It reproduces the committed files byte for byte, so a change to a recipe shows
up as a new hash, and the official-hash test catches it.

| Lang | Layout | Long press | Missing accents |
|------|--------|------------|-----------------|
| `en` | QWERTY | — | — |
| `it` | QWERTY | è é à ì ò ù | merged if under 2× (final vowel only) |
| `pt-br` | QWERTY | ç ã á â à õ ó ô é ê í ú | merged if under 0.1× |
| `fr` | AZERTY | é è ê ë à â æ ç î ï ô œ ù û ü ÿ | merged if under 0.1× |
| `de` | QWERTZ (ä ö ü are keys) | ß on s | merged if under 0.1× |
| `ru` | ЙЦУКЕН | ё on е | none: е for ё is normal spelling, the engine folds them |
| `he` | Israeli standard | — | none (no accents); final letters fold to regular |

**Why two cuts.** The subtitles drop accents. Italian drops the final one so
often that `perche` beats `perché`, and `piu`, `cosi` and `gia` about match
the right spelling. Real Italian pairs have the plain word far more common
(`e`/`è` 2.9×, `si`/`sì` 3.2×, `la`/`là` 89×). So a plain word **under 2×**
the accented one is a typo: it gives its count to the accented spelling and is
dropped. In Portuguese, French and German the typos are rare (`voce`, `ca`,
`fur`, about 1 % of the right spelling) and the real pairs are close to even
(`e`/`é` 0.85, `a`/`à` 0.89, `wurde`/`würde` 0.81). Italian's cut would
delete real words there, so they merge only **under 0.1×**.

**Hebrew** keeps entries made only of the 27 letters (22 plus the 5 final
forms) and longer than one letter. The engine treats a final letter and its
regular form as one (ם = מ, as `ó` = `o`), so `שלומ` completes to `שלום`;
swipe still tells them apart as two keys. Niqqud is not folded yet: the list
has none, and a word typed with it won't match. **German**'s source list is
lowercased, so nouns come without their capital. **Arabic** is not offered
yet: it needs the strip read right to left and harakat folded.

## Making one from your own texts

Your own writing — notes, documentation, an exported chat — makes a list of
the words you actually use, counted by how often you use them. This runs on
your computer; the texts never go near the phone, only the word list does.

```sh
{
  printf '#lapacho-dict 1\n#lang es-mio\n#name Mis palabras\n'
  grep -ohP '\p{L}+' textos/*.txt |
    gawk '{ c[tolower($0)]++ } END { for (w in c) print w, c[w] }' |
    sort -k2,2nr
} > es-mio.txt
```

- `grep -P '\p{L}+'` takes runs of letters in any script, accents included.
- **`gawk`, not `awk`**: Debian's default awk is `mawk`, whose `tolower`
  does not understand UTF-8 and turns `ÑANDÚ` into `ÑandÚ`.
- `tolower` lowers everything; fix the words that need their capitals
  (`quebrachOS`, `GitHub`) by hand afterwards, or drop the `tolower`.
- Read the result before importing it. It is a list of the words in those
  texts; a name or a password that was in them is now in the list, and the
  keyboard will suggest it. Delete what should not be there.

A short hand-written list works too — one word per line, no frequencies:

```text
#lapacho-dict 1
#lang es-obra
#name Obra
encofrado
hormigonado
viga
```

## Adding an official dictionary (maintainers)

1. Build it with a recipe above and commit it to
   `apps/mobile/android/dictionaries/<lang>.txt`, with a `NOTICE-<lang>.txt`
   saying where the words came from and under what licence.
2. Add its SHA-256 to `OFFICIAL_DICTIONARIES` in `Predict.kt`.
   `PredictTest.officialHashesMatchTheCommittedDictionaries` fails until the
   list and the committed files agree exactly — both ways.
3. Publish that exact file next to the APK. Users on older builds see it as
   custom until they update.

## Checking a file before importing it

The app refuses a file with a message saying why (no header, invalid `#lang`,
not UTF-8, too big, no words). To catch it earlier:

```sh
head -1 en.txt                              # must be: #lapacho-dict 1
grep -m1 '^#lang' en.txt                    # the id
grep -vc '^#' en.txt                        # how many entries
iconv -f utf-8 -t utf-8 en.txt >/dev/null   # fails on invalid UTF-8
```
