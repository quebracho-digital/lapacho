# Keyboard dictionaries — format, mixing, and making your own

The Android keyboard suggests words from **dictionaries**: plain-text word
lists with frequencies. Spanish ships inside the APK. Any other language — or
a list of your own jargon, names, or a field's vocabulary — is a file you
import in the app. The keyboard has no network permission, so it never
downloads one; you bring it (why: [`DECISIONS.md`](DECISIONS.md)).

## Installing one

1. Get the file onto the phone: download it with the browser, or copy it
   over USB.
2. In the Lapacho app, tap **IDIOMAS → AGREGAR** and pick the file.
3. If it is one of ours, it goes straight in. If it is not, the app says so
   first — see below.
4. The keyboard uses it the next time it opens.

**Official or custom.** The APK carries the SHA-256 of every dictionary we
publish. A file that matches imports without a question and is listed as
**oficial ✓**. One that does not — your own list, one published after your
version of the app, or one somebody altered — gets a warning with its full
hash and two buttons: **Importar como personalizado** or **Cancelar**. The app
cannot tell those three cases apart, so it asks, every time, per file; there
is no setting that turns the check off for good. Either way the file must pass
the [format](#format) check.

**IDIOMAS** lists what the keyboard is using — the bundled Spanish, then each
imported file as *oficial ✓* or *personalizado*, with the start of its
SHA-256. Tap an imported one to remove it. Up to **two** imported
dictionaries at a time, on top of Spanish; importing one whose `#lang` is
already there replaces it.

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
| `#name <text>` | no | What **IDIOMAS** shows. Defaults to the `id`. |
| `#alternates <key:chars> …` | no | Characters added to a key's long press, space-separated pairs. The key is one letter `a`–`z`, 1 to 8 characters after it. `#alternates e:éèêë c:ç` makes holding `e` offer é è ê ë. |

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
8.5 MB of memory while it is open, which is why only two can be imported.

## How dictionaries mix

All active dictionaries are used at once — there is no language switch. Type
`wh` with English imported and you get `what`, `who`, `why`; type `con` and
Spanish still answers.

To keep a big corpus from burying a small one, each file's frequencies are
turned into **shares of that file's total** before mixing. "de" is about 3.5 %
of the Spanish list and "the" about 3.3 % of the English one, so they compete
on equal terms even though the English corpus is bigger.
A word in two files keeps its higher share. A word you taught the keyboard
(PALABRAS) always comes first.

Which has a consequence for small custom lists: **the fewer words in a file,
the larger each one's share.** A 100-word list with equal frequencies gives
each word 1 % — more than all but a handful of everyday words. That is usually
what you want from jargon (typing `ku` should offer `kubectl`), but if a small
list keeps pushing common words aside, give it real frequencies, or give its
everyday words (the ones it shares with Spanish) low ones.

## Making one from a frequency list

[hermitdave/FrequencyWords](https://github.com/hermitdave/FrequencyWords)
(MIT) has lists for ~50 languages, counted over subtitles. It is where the
bundled Spanish comes from. English, for example:

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
