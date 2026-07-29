# Changelog

All notable changes to Lapacho are recorded here. Newest first.

## Unreleased

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

**Still open:** changing the persistence level does not offer to purge what the
new level forbids. Until it does, switching to `Paranoia` is not retroactive.
Tracked in `ROADMAP.md`.

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
