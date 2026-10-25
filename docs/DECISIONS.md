# Decisions — what was rejected, and why

Approaches that were seriously evaluated for Lapacho and **not** adopted. This
exists so nobody — contributor or maintainer — spends a day re-deriving a dead
end that was already measured.

A rejection here is not "we did not get around to it". Each entry says what was
tried, what the evidence was, what replaced it, and under what conditions it
would be worth reopening.

The counterpart is [`ROADMAP.md`](../ROADMAP.md) for what *is* planned, and
[`README.md`](../README.md#security-model) for what the current code actually
guarantees.

---

## Downloading dictionaries from inside the keyboard

|  |  |
|---|---|
| **Evaluated** | 2026-09-22, designing the predictive keyboard |
| **Rejected** | Same day, on the permission it costs |
| **Replaced by** | One language bundled in the APK; the rest imported from a file |

**The problem it was meant to solve.** Prediction needs a word list per
language. Six languages bundled is an APK nobody wants to download over mobile
data, and a picker that fetches a language on demand is the obvious answer.

**Why it was rejected.** It costs `android.permission.INTERNET`, and *"the app
has no internet permission at all"* is the strongest sentence in
[`USAGE_MOBILE.md`](USAGE_MOBILE.md). A keyboard that can open a socket is a
keyboard the user has to take on trust; one that cannot is a keyboard the user
can verify by reading the manifest. A dictionary is not worth that trade.

`DownloadManager` is the loophole: it downloads from a system process, so an
app *without* `INTERNET` can still enqueue one. It was rejected precisely
because it works — the manifest would keep saying "no internet" while bytes
arrive from the network anyway. The day someone notices, the promise reads as
a technicality rather than a fact.

**What replaced it.** Spanish and English ship inside the APK
(`assets/dict/{es,en}.txt`, ~600 KB each, see the `NOTICE-*.txt` beside
them; English since `0.1.32`, and either can be removed). Any other language
is a file the user downloads **with their browser** and hands to Lapacho
through the system file picker (`ACTION_OPEN_DOCUMENT`), which needs no
permission at all — not network, not storage. Built in `0.1.20`; format and
how to make one in [`DICTIONARIES.md`](DICTIONARIES.md).

The first design had the APK carry the SHA-256 of every dictionary published
with a release and **refuse** anything else. That refuses exactly the custom
dictionaries (jargon, a field's vocabulary) that are also a supported case, so
the hash list stayed but stopped being a gate: since `0.1.21` a file that
matches imports silently as *official*, and one that does not gets a warning
with its hash and a per-file choice to import it as *custom*. Not a setting
that switches the check off: a switch left off is how the next file, the
altered one, goes in without a word.

What guards the parser is the format, for every file — a magic first line, a
`#lang` that is safe as a file name, UTF-8, 8 MB at most — read by code that
only ever builds a word list. The worst a hostile dictionary can do is suggest
words, on screen, in a list the user chose and can remove. The hash answers a
different question: who made the list.

**When to reopen.** If Lapacho is ever distributed through Play, its
per-language asset delivery does this natively and without a permission.

**Worth keeping in mind.** The weight argument turned out to be mostly wrong:
50 000 Spanish words with frequencies are 600 KB of text, ~250 KB inside the
APK. It is *n-gram* models (which word follows which) that cost tens of MB —
and those are exactly what this keyboard does not build.

---

## One language at a time, with a switch key

|  |  |
|---|---|
| **Evaluated** | 2026-09-23, adding imported dictionaries |
| **Rejected** | Same day |
| **Replaced by** | Every active dictionary mixed, each normalized to its own corpus |

**The problem it was meant to solve.** With two dictionaries, which one does
the strip complete from? The usual answer is a 🌐 key that switches between
them.

**Why it was rejected.** The person this keyboard is for writes Spanish with
English in it — `deploy`, `commit`, `the`. A switch means switching mid
sentence, twice, for one word. Mixing costs nothing extra per keystroke (it is
one sorted list either way); what it needs is normalization, or the language
with the bigger corpus buries the other in every prefix they share. Each
file's counts become shares of its own total, so "de" (3.5 % of Spanish) and
"the" (3.3 % of English) compete on equal terms.

**The price.** Shared prefixes get crowded: with English imported, `co` offers
`con, como, come` where Spanish alone offered `cómo` third. Capped at two
imported dictionaries, for memory (~8.5 MB each in the keyboard's process)
more than for this. Since the flat word buffer a list costs ~1.2 MB and the cap is six
languages in all, bundled or imported.

**When to reopen.** If someone writes two languages that never mix, and the
crowding bothers them more than switching would.

---

## Autocorrect that rewrites the word on space

|  |  |
|---|---|
| **Evaluated** | 2026-09-23, adding spell correction |
| **Rejected** | Same day |
| **Replaced by** | Corrections offered as chips in the strip, applied only when tapped |

**The problem it was meant to solve.** `graicas` should become `gracias`
without the user doing anything — the way mainstream keyboards behave.

**Why it was rejected.** It is the keyboard deciding for the user, and it
decides wrong exactly where this user writes: names, jargon, the other
language. Every learned word, every custom dictionary exists because the
dictionary does not know everything; an autocorrect turns each of those gaps
into a word silently replaced. The rest of this keyboard does nothing unless
asked — a long press shows a row and waits, learning takes a hold and a
second tap — and correction follows the same rule.

**The price.** A typo costs one tap instead of zero.

**When to reopen.** If people ask for it — as an opt-in, and only for
corrections the engine is sure of (one edit, far more frequent), never for a
word the user taught it or a custom dictionary holds.

---

## A keyboard that learns while you type

|  |  |
|---|---|
| **Rejected** | By construction — the artifact is the problem, not its storage |
| **Replaced by** | Explicit per-word learning, one deliberate act at a time |

Every keyboard worth using adapts: it picks up your names, your jargon, the
words you actually use. The standard way is to build a personal language model
in the background from what you type.

That model **is a record of everything you typed**, in statistical form.
Encrypting it and keeping it on the phone does not change what it is, and it
cannot be shown to the user in any honest way — nobody can read their own
n-gram table and tell what it gave away. It also flatly contradicts the line
this product is sold on: *keystrokes are never recorded, only copied clips
are.*

What replaces it keeps the useful half. The dictionary is fixed and public, so
suggestions are the same for everyone who has it. Learning is a separate
gesture on a single word: long-press the word you typed, a chip offers to
learn it, you tap it. What gets stored is that word and nothing else — no
context, no timestamp, no app it was typed in — in a list the companion app
shows in full, with a delete button next to each entry.

The claim then becomes checkable instead of vague: *Lapacho only learns the
words you teach it, one at a time, and you can see all of them.*

The cost is real and accepted: no next-word prediction, and the keyboard stays
worse than Gboard at guessing. That is the trade this product exists to make.

**Since then (2026-10-25).** The half of next-word prediction that needs no
model shipped: a fixed, public table of default phrases (`buen ` → *día*),
the same for everyone — see `docs/DICTIONARIES.md#phrases`. The half that
learns is *not* adopted; a way to have it without the objection above is
proposed in [Learning mode](#learning-mode--a-keyboard-that-learns-only-while-it-is-red-proposed).

---

## Learning mode — a keyboard that learns only while it is red (proposed)

|  |  |
|---|---|
| **Proposed** | 2026-10-25, after the default phrases shipped |
| **Status** | **Open — not adopted, nothing built.** Needs the maintainer's decision, and a rewrite of the promise below if yes |
| **Would partly replace** | [A keyboard that learns while you type](#a-keyboard-that-learns-while-you-type) |

**What that entry objected to.** A personal model built in the background *is*
a record of everything typed, in statistical form, and nobody can read their
own n-gram table and tell what it gave away. The objection is about learning
**invisibly and continuously**. It is not about learning as such, which is what
makes a keyboard worth using.

**The proposal.** Keep the table, remove the invisibility. Learning becomes a
**mode** the user turns on, impossible to miss while it is on, limited to what
can be shown in full, and off again by itself. The product line moves from
*"keystrokes are never recorded"* to something just as checkable and a little
weaker:

> Lapacho records word pairs only while the keyboard is red; only pairs of
> words it already knew; you can read all of them, and erase them.

### The mode

- **Off by default.** Turned on by one explicit tap (a key in the actions row;
  where is open). There is no setting that leaves it on permanently.
- **Visible while on.** The keyboard's background turns red and the strip
  carries a fixed label, *● APRENDIENDO*. Red alone is not enough — it fails
  for colour-blind users and is overridden by some skins — so the label is not
  optional. Skins cannot hide either.
- **Ends by itself:** when the screen turns off, and one hour after it was
  turned on. A mode people forget is the failure that matters here, so the
  default errs towards asking again. Closing the keyboard does **not** end it
  (that would be unusable); a private field **pauses** it (below).

### What it records

- **Pairs only:** previous word → next word, plus a count. No triples, no
  timestamp, no app name, no field type. A pair never spans a full stop, a
  line break or a change of field: the context is reset there.
- **Only words the keyboard already knew** — in the dictionary or in the
  taught words. This is the strongest filter and costs nothing: passwords,
  tokens, IDs, card and account numbers, and every random string are not
  dictionary words, so they never become a pair. Digits are never recorded.
  What it learns is how *you* chain ordinary words (`buen` → `equipo`), not
  what you type that is yours alone.
- **A pair is suggested only after it has been seen twice**, so something typed
  once, ever, is stored at worst as an inert count and is never shown back.

### Near private data

A reminder after the fact does not un-record anything, so the protection comes
first and the notice reports it:

- In a private field (`privateField`: passwords, PINs, incognito, the same test
  that already hides the clips) the mode **pauses**: nothing is recorded, the
  strip turns grey with *⏸ pausado*, and it resumes by itself in the next
  ordinary field.
- A word the classifier reads as a secret (`classify()`, the same one the clip
  history uses) is dropped, the context is reset, and the strip says briefly
  *🔒 no aprendido*. The pair on either side of it goes with it.
- Being red is the standing reminder; these two are the events.

### Where it lives, and who can see it

- A table next to the taught words in `lapacho-core`, written only while the
  mode is on. App-private storage, `allowBackup="false"` (already set), never
  in the log (counts only, as now) and **never in `lapacho-sync`** — a test
  must fail if a sync payload could carry it.
- The companion app lists **every** pair, searchable by first word, with a
  delete button on each, and **Borrar todo**. If what the viewer shows and what
  is stored can differ, the claim above is false; a test compares the row count
  with what the viewer lists.
- It ranks above the default phrases: a pair you taught beats the table's
  guess, and the table fills in where you taught nothing.

### What it costs

- **A manual mode yields a small table.** Most people will not keep it on, so
  this learns your own stock phrases (`buen día equipo`), not the continuous
  adaptation of Gboard. That is the same trade as before, narrower.
- **The promise gets weaker.** `docs/USAGE_MOBILE.md` ("Keystrokes are never
  recorded; only copied clips are") and the README line change, and so does the
  sentence in the entry above that this product is sold on. Existing users read
  a different claim after the update, so it needs a line in the release notes,
  not only here.
- **A pair table is still a table of phrases.** The filters keep out secrets,
  not meaning: *"me duele la"* is made of ordinary words. What stops that from
  being a leak is that you chose to turn it on, saw it on, and can read it.

### Alternative worth weighing first

**Teach it from text you hand over**, instead of from keystrokes: pick a file
or paste something you wrote (sent mail, notes), and the companion builds the
pairs once, on the phone, and throws the text away. No keystroke is ever
recorded, no mode to forget, and the same filters apply. It gives up
learning *as you go*, and gains that the promise does not change at all. The
two are not exclusive; this one is cheaper and could come first.

### How it would be checked

Unit tests, in this order of importance: nothing is recorded in a private
field or with the mode off; a digit, a secret word or an unknown word records
nothing and resets the context; the mode ends at screen-off and at the hour; a
pair is not suggested before its second sighting; the viewer lists exactly
what is stored; *Borrar todo* leaves no rows; a sync payload and a backup
contain none of it. Then on a device, red and grey states by eye.

### Open questions for the maintainer

1. **Adopt, or do the "teach it from text" version first?** (Or neither.)
2. Where does the toggle live — a key in the actions row, a long press on the
   strip, or only in the companion?
3. One hour, or shorter? End at screen-off, at keyboard close, or neither?
4. Two sightings before a pair is suggested: right, or three?
5. Encrypt the table with the Keystore key that already protects the history?
   It does not make the table more honest, but a stolen file is useless.
6. Are the words *APRENDIENDO* / *pausado* / *no aprendido* the right ones, in
   a keyboard that is otherwise written in the phone's language?

**When to reject for good.** If, a few weeks after it ships, the mode is
almost never turned on, it is complexity and a weaker promise for nothing: take
it out, and keep the default phrases.

---

## A swipe-typing library instead of our own decoder

|  |  |
|---|---|
| **Rejected** | 2026-09-30, adding swipe typing to the keyboard |
| **Replaced by** | `Predictor::swipe` in `lapacho-predict`: SHARK2's location channel over the dictionary the keyboard already holds |

The candidates, as of 2026-09: Google's gesture library and FUTO's are closed or
not free, which rules them out here. In Rust, `super-swipe-type` (a port of
CleverKeys' ONNX transformer) is English-only, one author, months old, and
would bring a model runtime into a keyboard process; nothing else on crates.io
is more than a name. Outside Rust, FlorisBoard's statistical classifier and
OpenSwipe are Kotlin tied to their own keyboards, and HeliBoard's NLnet-funded
free gesture library is still collecting data (until 2026-11-30), with no
released language or licence to build on.

What tipped it is that the missing layer was small. The decoder needs a word
list with frequencies, which Lapacho already loads for suggestions, plus the
line through each word's keys compared with the finger's: ~150 lines, no
dependency, no model, nothing leaving the phone and nothing stored. On
synthetic swipes over the 500 commonest Spanish words it puts the word first
99 % of the time on the key centres, and among the three offered 99.6 % with
every key missed by a third of a key (`tests/swipe.rs`), in ~4 ms on the
emulator.

**When to reopen.** If HeliBoard's library ships under a free licence with a
Rust or C API and decodes real swipes measurably better — or if real swipes
from phones show ours failing in ways the missing SHARK2 shape channel would
not fix.

---

## A second keyboard row for suggestions

|  |  |
|---|---|
| **Rejected** | 2026-09-22, implementing the suggestion strip |
| **Replaced by** | The one strip, which suggestions take over while a word is being typed |

The paste strip and the suggestion strip both want the row above the keys. The
obvious fix is to show both, one above the other.

It was rejected because it makes every user pay height — on a phone the
keyboard is already half the screen — for two features that are never needed
in the same instant: while a word is being typed there is nothing to paste
into it, and the moment the word ends (space, punctuation, or a delete) the
clips come back. Suggestions also stay hidden until the second letter, so a
single keystroke never takes the clips away.

**When to reopen.** If usage shows people pasting mid-word often enough to
matter, or if a tablet layout makes the height cheap.

---

## `mlockall` — locking the whole process into RAM

|  |  |
|---|---|
| **Evaluated** | 2026-08-04 — implemented, installed and tested the same day |
| **Rejected** | 2026-08-04 — reverted in `020fac8` |
| **Replaced by** | `crates/lapacho-core/src/locked_ring.rs` |

**The problem it was meant to solve.** Clipboard history is encrypted at rest,
but the live session buffer holds plaintext by necessity. The kernel can page
that plaintext to a swap device the app does not control and that is usually not
encrypted — so `zeroize`-ing on eviction accomplishes nothing if a copy left for
disk beforehand. On a real machine the process had 52 MB of itself already in
swap while the only locked page was the AES key.

**Why it was rejected.** It kills the app. Installing a build with
`mlockall(MCL_CURRENT | MCL_FUTURE | MCL_ONFAULT)` made Lapacho die on startup
with `memory allocation of 704260 bytes failed`.

`MCL_ONFAULT` controls page *population*, not *accounting*. The kernel still
charges the full virtual reservation against `RLIMIT_MEMLOCK` at `mmap` time.
Measured on the target host: with `mlockall` in effect, a 2 GB `PROT_NONE`
`MAP_NORESERVE` reservation that is **never touched** moves `VmLck` from
3200 kB to 2100352 kB.

WebKit reserves several GB of address space across its heaps. With `MCL_FUTURE`
in effect, crossing the limit makes subsequent allocations fail outright. No
finite `RLIMIT_MEMLOCK` survives that, and requiring `RLIMIT_MEMLOCK=unlimited`
on every user's machine is not something a desktop app gets to demand.

A guard that skipped `mlockall` below a threshold did **not** save it: the
threshold was sized against real usage (~100 MB RSS) when the quantity that
matters is reserved address space, which is orders of magnitude larger.

**What replaced it.** A small arena the app owns: one contiguous 800 KB
allocation (25 slots × 32 KB), locked once at startup, never grown and never
unlocked early. This is what the internal design record (§11 step 4) specified
before the shortcut was attempted. It covers text; images do not fit a slot on
purpose, because covering them would mean locking >100 MB permanently. See
[`README.md`](../README.md#content-at-rest-and-content-in-memory) for the full
scope and its limits.

**When to reopen.** Not on a configuration change — this is not a tuning
problem, it is the interaction between `MCL_FUTURE` and WebKit's memory
reservation model. If Lapacho ever leaves the Tauri/WebKit shell, it becomes
worth re-evaluating.

**Lesson worth keeping.** The change was signed off on a verification that
proved the wrong thing: that locking memory worked (`mlockall` returned 0, and
touching 20 MB grew `VmLck` by 20 MB) rather than that *the app survived with it
enabled*. Verifying the mechanism is not verifying the effect. The valid test is
running what the user runs.

---

## Per-`String` `mlock` in the session buffer

|  |  |
|---|---|
| **Evaluated** | 2026-08-04, during the `LockedRing` design |
| **Rejected** | Never implemented — rejected on the page-aliasing argument |
| **Replaced by** | The same contiguous arena |

Locking each payload as it arrives looks simpler than a pre-allocated arena. It
is not safe. `mlock` operates on whole pages, and two small allocations
routinely share one, so `munlock`-ing an evicted item can unlock a page that
still holds another live secret.

A `Vec<String>` makes it worse: reallocation moves the payload to memory that
was never locked, silently. Hence a fixed-capacity backing store allocated once
as a boxed slice, with slots carved out of memory that is already locked and
nothing unlocked until the process exits.

---

## Encrypting the session buffer in RAM

|  |  |
|---|---|
| **Rejected** | By construction — it does not do what it appears to do |

Keeping the buffer ciphertext in memory requires keeping the key in memory next
to it, so anything that can read the process can read both. It costs a
decrypt on every search, render and paste in exchange for no additional
guarantee against the attacker it appears to address.

What is actually achievable is narrower and is what the code does: keep the
plaintext window short (`zeroize` on eviction) and keep those pages out of swap
(the locked arena).

---

## Sharing more than the rules across platforms

|  |  |
|---|---|
| **Rejected** | 2026-10-11, planning iPhone, Wear OS and open watches (design, nothing measured) |
| **Replaced by** | The rules once in the Rust crates; a native, thin app per platform that calls them |

**The problem it was meant to solve.** Each new target (iPhone, Wear OS,
Pebble, Bangle.js) looks like a copy of the Android app, and the reflex is a
base app the targets inherit from, or one cross-platform UI framework for all.

**Why it was rejected.** There is nothing to inherit across Rust, Kotlin,
Swift and a watch's C or JavaScript; what is worth sharing is knowledge, not
screens. What a secret is, what each paranoia level allows, what a plugin
receives and what may sync live once in `lapacho-core`, `-predict` and
`-sync`, and every app calls them — the bridge (`rust-bridge`, UniFFI)
already builds a `staticlib` and UniFFI also emits Swift, so iOS reuses it.
Moving Android's history store into the core (`763b72e`) is the pattern.
Screens stay native (Leptos, Compose, SwiftUI): three UIs for three
platforms is not duplication, and a shared UI framework would make all three
worse.

- **Platform needs go through interfaces the platform implements** (reading
  the clipboard, the secure key store, the sync transport) — UniFFI callback
  interfaces. The core never names Android or iOS.
- **An interface only with two real implementations.** The sync transport
  (LAN, relay) qualifies; a single plugin does not.
- **Small watches don't run the core.** A Pebble or a Bangle.js has no room
  for it; they display what the phone already decided to show, so the
  paranoia rules are never copied onto a watch. Phone and watches speak one
  message format (list clips, reveal one, run a plugin), defined once in the
  core.
- `apps/mobile/android/rust-bridge` moves to `crates/lapacho-ffi` when iOS
  starts, not before.

**When to reopen.** If a rule turns up implemented twice (in Kotlin and
Swift, say), it moves into the core. If two platforms' screens turn out to
be the same screen kept in sync by hand for months, a shared UI layer is
worth measuring then.

## Reaching watches through Gadgetbridge's broadcast intents

|  |  |
|---|---|
| **Rejected** | 2026-10-11, looking for a way to support open watches |
| **Replaced by** | *Lapacho Watch*, a plugin app holding the Bluetooth permission and talking to a small Lapacho app on each watch |

Gadgetbridge already talks to Bangle.js, PineTime, Pebble, Amazfit and more,
and other apps can push data through it with Android broadcast intents. A
broadcast reaches any app on the phone that listens for it, so every clip
sent to the watch would be readable by any installed app: a clipboard leak
by design (a user on the Espruino forum flagged the same route as a security
risk). The keyboard keeps no Bluetooth permission either way; the plugin app
carries it, and uninstalling it takes it away.

**When to reopen.** If Gadgetbridge offers a channel restricted to one
calling app (a bound service checking the caller's signature, say).
