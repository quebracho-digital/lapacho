//! Fixed-capacity, page-locked ring buffer for the plaintext that the session
//! buffer has to keep around.
//!
//! # Why this exists
//!
//! Clipboard content is encrypted at rest in SQLite, but the live session
//! buffer necessarily holds plaintext — the app has to search it, render it and
//! paste it back. The kernel is free to page that plaintext out to a swap
//! device we do not control and that is very often not encrypted, and
//! `zeroize`-ing on eviction does nothing about a copy that left for disk
//! beforehand.
//!
//! # Why not `mlockall`
//!
//! Locking the whole process was tried and reverted (`020fac8`): under WebKit
//! it kills the app. `MCL_ONFAULT` controls page *population*, not
//! *accounting* — the kernel still charges the full virtual reservation
//! against `RLIMIT_MEMLOCK` at `mmap` time, and WebKit reserves several GB of
//! address space, so no finite limit is survivable. Measured: a 2 GB
//! `PROT_NONE` reservation that is never touched moved `VmLck` from 3200 kB to
//! 2100352 kB.
//!
//! So the lock has to be small, owned, and permanent — which is what
//! `ARQUITECTURA_REFACTOREO.md` §11 step 4 specified in the first place: a
//! pre-allocated fixed-capacity ring, locked once, never grown.
//!
//! # Why one allocation and never unlocked
//!
//! `mlock` works on whole pages. Locking each `String` as it arrives would
//! mean `munlock`-ing pages that may still hold *another* live secret, because
//! two small allocations routinely share a page. One contiguous region, locked
//! once for the lifetime of the process, has no such aliasing: slots are carved
//! out of memory that is already locked, and nothing is ever unlocked early.
//!
//! A `Vec` that reallocates would move the payload to unlocked memory behind
//! our back, so the backing store is allocated once as a boxed slice and never
//! resized.
//!
//! # What this does *not* cover
//!
//! - **Only what the caller stores here.** In Lapacho that is `raw_content`,
//!   the field that holds the actual secret. Values larger than one slot are
//!   rejected (see [`LockedRing::store`]) and the caller must decide what to do
//!   — in practice images, which are not what the secret hints classify.
//! - **Copies handed out to callers.** [`LockedRing::get`] returns a borrow, but
//!   anything the caller builds from it (a `String` for the UI, a menu label)
//!   lives in ordinary memory. Those copies are short-lived by construction;
//!   the copy this protects is the one that sits idle for hours, which is
//!   precisely the one the kernel chooses to swap.
//!
//! Neither limitation is hidden: [`LockedRing::is_locked`] reports whether the
//! lock actually took, so the caller can say so out loud instead of assuming.

use zeroize::Zeroize;

/// One entry's bookkeeping. The payload itself lives in the arena.
struct Entry {
    id: String,
    slot: usize,
    len: usize,
}

/// A fixed-capacity keyed store whose backing memory is locked into RAM.
///
/// Newest-first: [`store`](Self::store) puts an entry at the front, and when
/// the ring is full the oldest entry is evicted (and scrubbed) to make room.
pub struct LockedRing {
    /// Held for the lifetime of the ring. Dropping it unlocks the region, so it
    /// is never released early.
    ///
    /// **Declared before `arena` on purpose.** Rust drops fields in
    /// declaration order, so this releases the lock while the memory is still
    /// allocated. With the order reversed, `munlock` runs against a freed
    /// allocation — which debug builds catch as a panic inside `region`, and
    /// release builds do not catch at all, quietly unlocking pages the
    /// allocator may already have handed to something else. `crypto::Cipher`
    /// has the same constraint and the same comment.
    #[cfg(feature = "mlock")]
    _lock: Option<region::LockGuard>,
    /// `capacity * slot_bytes`, allocated once and never resized: a realloc
    /// would silently move plaintext into unlocked memory.
    arena: Box<[u8]>,
    slot_bytes: usize,
    locked: bool,
    /// Newest first.
    entries: Vec<Entry>,
    free: Vec<usize>,
}

impl LockedRing {
    /// Allocates and locks `capacity * slot_bytes` of memory.
    ///
    /// Locking is best-effort: if `RLIMIT_MEMLOCK` does not allow it the ring
    /// still works, and [`is_locked`](Self::is_locked) returns `false`. A
    /// clipboard manager that refuses to start is worse than one that starts
    /// and reports it could not lock — the same policy `crypto::Cipher` uses
    /// for the key.
    pub fn new(capacity: usize, slot_bytes: usize) -> Self {
        assert!(capacity > 0 && slot_bytes > 0, "empty ring is a bug, not a config");
        let arena = vec![0u8; capacity * slot_bytes].into_boxed_slice();

        #[cfg(feature = "mlock")]
        let (_lock, locked) = match region::lock(arena.as_ptr(), arena.len()) {
            Ok(guard) => (Some(guard), true),
            Err(e) => {
                eprintln!(
                    "lapacho: no se pudo fijar en RAM el buffer de sesión ({e}); \
                     el portapapeles puede terminar en swap"
                );
                (None, false)
            }
        };
        #[cfg(not(feature = "mlock"))]
        let locked = false;

        Self {
            #[cfg(feature = "mlock")]
            _lock,
            arena,
            slot_bytes,
            locked,
            entries: Vec::with_capacity(capacity),
            free: (0..capacity).collect(),
        }
    }

    /// Whether the backing memory is actually locked into RAM.
    ///
    /// Exists so callers can report the real state instead of assuming the
    /// guarantee holds — the exact mistake that put "mlock + zeroize" in the
    /// security doctrine for months while neither was complete.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Largest value a single slot can hold.
    pub fn max_len(&self) -> usize {
        self.slot_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stores `data` under `id` at the front, replacing any previous value for
    /// that id. Evicts (and scrubs) the oldest entry when full.
    ///
    /// Returns `false` — storing nothing — when `data` does not fit in a slot.
    /// The caller must not silently treat that as success: the value is simply
    /// not protected, and it is the caller's job to decide whether to keep it
    /// elsewhere and to be honest about it.
    pub fn store(&mut self, id: &str, data: &[u8]) -> bool {
        if data.len() > self.slot_bytes {
            self.remove(id); // a stale short value must not survive a long one
            return false;
        }
        self.remove(id);
        if self.free.is_empty() {
            if let Some(oldest) = self.entries.pop() {
                self.scrub(oldest.slot);
                self.free.push(oldest.slot);
            }
        }
        let slot = self.free.pop().expect("capacity > 0 guarantees a free slot");
        self.slot_mut(slot)[..data.len()].copy_from_slice(data);
        self.entries.insert(
            0,
            Entry { id: id.to_string(), slot, len: data.len() },
        );
        true
    }

    /// Borrows the stored bytes for `id`, if present.
    pub fn get(&self, id: &str) -> Option<&[u8]> {
        let e = self.entries.iter().find(|e| e.id == id)?;
        Some(&self.arena[e.slot * self.slot_bytes..][..e.len])
    }

    /// Removes `id`, scrubbing its slot. No-op when absent.
    pub fn remove(&mut self, id: &str) {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            let e = self.entries.remove(pos);
            self.scrub(e.slot);
            self.free.push(e.slot);
        }
    }

    /// Removes everything, scrubbing every slot in use.
    pub fn clear(&mut self) {
        while let Some(e) = self.entries.pop() {
            self.scrub(e.slot);
            self.free.push(e.slot);
        }
    }

    /// Zeroizes a whole slot, not just the bytes last written: a shorter value
    /// landing on a slot must not leave the tail of a longer predecessor
    /// readable.
    fn scrub(&mut self, slot: usize) {
        self.slot_mut(slot).zeroize();
    }

    fn slot_mut(&mut self, slot: usize) -> &mut [u8] {
        let start = slot * self.slot_bytes;
        &mut self.arena[start..start + self.slot_bytes]
    }
}

impl Drop for LockedRing {
    fn drop(&mut self) {
        // Scrub before the allocation is released and before the lock guard
        // (declared after `arena`, dropped after it) lets the pages go.
        self.arena.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring() -> LockedRing {
        LockedRing::new(3, 16)
    }

    #[test]
    fn stores_and_reads_back() {
        let mut r = ring();
        assert!(r.store("a", b"hunter2"));
        assert_eq!(r.get("a"), Some(&b"hunter2"[..]));
        assert_eq!(r.get("nope"), None);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn oversized_values_are_rejected_not_truncated() {
        let mut r = ring();
        // Silently storing a prefix would hand back a corrupted secret.
        assert!(!r.store("big", &[b'x'; 17]));
        assert_eq!(r.get("big"), None);
    }

    #[test]
    fn an_oversized_value_evicts_the_stale_short_one() {
        // Otherwise `get` would keep answering with an outdated value for an id
        // whose real content is now something else entirely.
        let mut r = ring();
        assert!(r.store("a", b"short"));
        assert!(!r.store("a", &[b'x'; 17]));
        assert_eq!(r.get("a"), None);
    }

    #[test]
    fn replacing_an_id_does_not_consume_a_second_slot() {
        let mut r = ring();
        for _ in 0..10 {
            assert!(r.store("a", b"same-id"));
        }
        assert_eq!(r.len(), 1);
        assert_eq!(r.get("a"), Some(&b"same-id"[..]));
    }

    #[test]
    fn evicts_oldest_when_full_and_scrubs_it() {
        let mut r = ring();
        r.store("a", b"aaa");
        r.store("b", b"bbb");
        r.store("c", b"ccc");
        r.store("d", b"ddd"); // pushes "a" out
        assert_eq!(r.len(), 3);
        assert_eq!(r.get("a"), None);
        assert_eq!(r.get("d"), Some(&b"ddd"[..]));
        // The evicted payload is gone from the arena, not merely unreferenced.
        assert!(!r.arena.windows(3).any(|w| w == b"aaa"));
    }

    #[test]
    fn remove_scrubs_the_arena() {
        let mut r = ring();
        r.store("a", b"hunter2");
        r.remove("a");
        assert_eq!(r.get("a"), None);
        assert!(!r.arena.windows(7).any(|w| w == b"hunter2"));
    }

    #[test]
    fn clear_scrubs_every_slot() {
        let mut r = ring();
        r.store("a", b"aaa");
        r.store("b", b"bbb");
        r.clear();
        assert!(r.is_empty());
        assert!(r.arena.iter().all(|&b| b == 0));
    }

    #[test]
    fn a_short_value_does_not_leave_a_longer_predecessors_tail_readable() {
        let mut r = LockedRing::new(1, 16);
        r.store("a", b"aaaaaaaaaaaaaa");
        r.store("a", b"bb");
        assert_eq!(r.get("a"), Some(&b"bb"[..]));
        assert!(!r.arena.windows(3).any(|w| w == b"aaa"));
    }

    #[test]
    fn slots_are_reused_after_removal() {
        let mut r = ring();
        for i in 0..20 {
            let id = format!("id{i}");
            assert!(r.store(&id, b"x"));
            r.remove(&id);
        }
        assert!(r.is_empty());
        assert!(r.store("final", b"ok"));
    }

    #[cfg(feature = "mlock")]
    #[test]
    fn reports_whether_it_actually_locked() {
        // Not asserting `true`: RLIMIT_MEMLOCK may forbid it in a container.
        // The point is that the answer is observable rather than assumed.
        let r = ring();
        let _ = r.is_locked();
    }
}
