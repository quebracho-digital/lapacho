# Changelog

All notable changes to Lapacho are recorded here. Newest first.

## Unreleased

### Security — advanced XSS detectors

Added three new threat detectors for advanced XSS attack vectors:

- **DataUrlScript:** detects `data:text/html,<script>...` and base64-encoded SVG
  with embedded `<script>` (base64 of `<script>` = `c2NyaXB0`).
- **SvgEventHandler:** detects `<svg>` with `onload`, `onerror`, or other event
  handlers (including nested elements like `<svg><circle onerror=evil()>`).
- **ImgOnError:** detects `<img>` with `onerror` handler — a classic XSS vector
  that triggers when the image fails to load.

### Performance — tray rebuild reduced from ~300 ms to ~150 ms

The system-tray rebuild was loading the entire history from the database on
every clipboard capture. This meant decrypting ~100 rows twice per rebuild
(one for the menu, one for the indicator icon).

- **Tray now shows only the session buffer** (up to 25 items): the items the
  user copied this session. This keeps the tray instant and avoids database I/O.
- **Older items remain accessible** via the main window ("Abrir Lapacho…")
  which searches the full history.
- **Expected latency reduction:** from 280–310 ms to ~150 ms per capture.

### Security — the session buffer's plaintext no longer reaches swap

Clipboard history is encrypted at rest, but the live session buffer holds
plaintext by necessity — the app searches it, renders it and pastes it back.
Nothing kept that plaintext out of swap, and swap is usually not encrypted. On a
real machine the process had 52 MB of itself already on disk while the only
locked page was the AES key.

- **Payloads now live in a page-locked arena** (`LockedRing`): one contiguous
  allocation, 25 slots of 32 KB, 800 KB locked once at startup and never grown.
  Sized from a real history where the largest text item was 10.7 KB.
- **Images are deliberately not covered.** They average ~1 MB and covering them
  would mean locking >100 MB permanently. Oversized items stay in the buffer
  unlocked rather than being dropped, and the coverage is reported rather than
  assumed.
- **Eviction now scrubs every field that carries the payload**, not just
  `raw_content`: `display_content` (which *is* the payload for non-sensitive
  items) and any user-set `title` were being left in freed memory. A separate
  path — marking an item as secret — skipped the scrubbing entirely.
- **Process-wide `mlockall` is rejected.** It was tried and it kills the app
  under WebKit: `MCL_ONFAULT` controls page population, not accounting, so the
  kernel charges the multi-GB address space against `RLIMIT_MEMLOCK`.

### Added — diagnostics that survive autostart

Launched from autostart there is no terminal, so everything the app reported
went nowhere and a startup crash left no trace. The desktop entry now redirects
to `~/.local/state/lapacho/lapacho.log`, keeping the previous run as `.log.1`.
Per-capture timings are behind `LAPACHO_TRACE=1` so the log stays readable. No
clipboard content is ever written to it.

### Known issue — tray rebuild latency

A capture takes ~300 ms to show up, and now the numbers say where it goes:
storage is 16–18 ms and the tray menu rebuild is 281–308 ms. `get_tray_items`
decrypts 100 rows of history on every rebuild. Not yet fixed.


### ⚠️ Security notice — sensitive items may still be on disk from before you switched to Paranoia

**Who is affected:** anyone who ran Lapacho at the `Balanced` or `All`
persistence level and later switched to `Paranoia`, **and** has the sensitive
TTL turned off.

**What happens:** the persistence level only governs *new* writes. Switching to
`Paranoia` does not remove what earlier levels already wrote, and nothing else
purges it either — the sensitive TTL is the only mechanism that would have, and
when it is set to `off` that check never runs. The result is credentials and
secrets sitting in the history while the UI says `Paranoia`, which most people
read as "nothing sensitive is on disk".

This is not a leak of plaintext: content is still encrypted at rest with your
master key. The problem is that the app's own labelling led you to believe the
data was already gone.

**How to check.** Count what is stored, by sensitivity (no content is printed):

```bash
sqlite3 ~/.local/share/digital.quebracho.lapacho/history.db \
  "SELECT sensitivity, COUNT(*) FROM history GROUP BY sensitivity;"
```

Any row other than `None` is a sensitive item on disk.

**How to clean it up.** Back up first, then delete the sensitive rows you did
not explicitly ask to keep:

```bash
cp -r ~/.local/share/digital.quebracho.lapacho{,.bak-$(date +%F)}
sqlite3 ~/.local/share/digital.quebracho.lapacho/history.db \
  "DELETE FROM history WHERE sensitivity != 'None' AND pinned = 0 AND vaulted = 0;"
```

Items you pinned (📌) or put in the vault (🗄) are kept — those were deliberate.

**Fixed:** lowering the persistence level now purges what the new level forbids,
so switching to `Paranoia` *is* retroactive. Pick the level again (even the one
already selected) to clean up an existing history; the UI reports how many items
were removed. Vaulted items (🗄) are kept — that flag is an explicit per-item
override of the level. Note that plain pinning (📌) is **not** an override and
will not save a sensitive item from the purge, which is why the manual SQL above
spares pinned rows but the app does not.

### Added

- **Per-item title** (🏷). Name an item so you can find it by what it *is*
  rather than by its content — the title is searchable. Stored encrypted, like
  the content: a title such as "prod DB password" gives away as much as the
  value it names.
- **Pin** (📌). Marks an item as kept: exempt from the history size cap, and it
  no longer consumes one of the capped slots, so pinning cannot evict the rest
  of your history. Pinned items now sort to the top of the list and the tray.
- **Vault** (💾 → 🗄). Forces an item onto disk *even when the active
  persistence level forbids it*, and exempts it from the sensitive TTL. This is
  the one deliberate hole in the persistence policy, and it only opens for one
  item at a time, by an explicit act.
  - On sensitive items the button arms first (💾 → ⚠ → confirm), so a secret is
    never one stray click away from disk.
  - Removing an item from the vault **re-applies the active level immediately**:
    if the level forbids it, it leaves the disk right then, not at the next
    cleanup.
  - The persistence selector now reads "Paranoia (salvo bóveda)" / "Balanced
    (salvo bóveda)" — the mode no longer promises more than it delivers.
- **"Buscar…" entry in the tray menu**, above "Open Lapacho…". A native
  GTK/AppIndicator menu cannot host a text field, so this opens the window with
  the search box focused and its contents selected.

### Fixed

- **The window opened behind whatever you were using.** Opening Lapacho from
  the tray or the global shortcut showed the window but left it stacked below
  the focused app, so you had to hunt for it. `show` + `set_focus` is not
  enough: a hidden window may also be minimized, and window managers with
  focus-stealing prevention ignore a focus request coming from a tray click or
  a shortcut rather than from direct interaction. Both paths now unminimize and
  force the raise through a stacking request.
- **The global shortcut now lands the cursor in the search box.** Opening by
  shortcut is nearly always "I want to find something", so it no longer costs a
  click and an aim.
- **Pinning an old item appeared to do nothing.** The list ordered strictly by
  timestamp, so a pinned item stayed buried at whatever position its date gave
  it. Kept items now sort first, and the 100-item query limit can no longer
  drop them.
- **Titles were not searchable from the UI.** The desktop search path filters
  its own merged list and never called `HistoryRepo::search`, so the title was
  only matched by a code path the UI does not use.
- **Looking up one item decrypted the whole history.** Resolving an id loaded
  and decrypted up to 100 rows to return one. It is now a single-row query —
  which matters most on Android, where the IME does this on every paste.

### Changed

- The mobile UniFFI bridge returns a typed error (`InvalidKey` / `Storage` /
  `NotFound`) instead of opaque strings, so Kotlin can tell "unlock the vault
  again" from "storage is broken". `get_raw_content` now returns an error when
  an item is missing rather than an empty value, so the IME cannot paste blank.

### Known limitations

- Titling or pinning an item that the active level never wrote to disk (a
  sensitive item under `Paranoia`) only holds in memory and is lost on restart.
  Put it in the vault (💾) first if you want it to survive.
- None of the above has been exercised in a real GUI session yet.
