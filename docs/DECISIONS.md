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

**What replaced it.** Spanish ships inside the APK
(`assets/dict/es.txt`, ~600 KB, see its `NOTICE-es.txt`). Any other language
is a file the user downloads **with their browser** and hands to Lapacho
through the system file picker (`ACTION_OPEN_DOCUMENT`), which needs no
permission at all — not network, not storage. The APK carries the SHA-256 of
the dictionaries published with each release and refuses anything that does
not match, so an imported file is data we published, not an arbitrary blob.

**When to reopen.** If Lapacho is ever distributed through Play, its
per-language asset delivery does this natively and without a permission.

**Worth keeping in mind.** The weight argument turned out to be mostly wrong:
50 000 Spanish words with frequencies are 600 KB of text, ~250 KB inside the
APK. It is *n-gram* models (which word follows which) that cost tens of MB —
and those are exactly what this keyboard does not build.

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
