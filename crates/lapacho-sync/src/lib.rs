//! Multi-device sync: a small end-to-end encrypted queue of clipboard
//! envelopes. Design: `docs/ARQUITECTURA_MOBILE_ANDROID.md` §5.
//!
//! - [`Chain`]: the shared secret every device of one user holds, created on
//!   the first device and joined on the others with 24 words (Brave-style).
//! - [`may_sync`]: what may leave the device, decided by the same
//!   `PersistLevel` that decides what reaches the disk.
//! - [`Envelope`]: one clip sealed for the chain. A transport (a WebDAV
//!   folder, a Bluetooth link, memory in tests) only ever sees these.
//! - [`send`] / [`receive`]: the whole round trip over a [`Transport`].

mod chain;
mod envelope;
mod policy;
mod transport;
#[cfg(feature = "webdav")]
mod webdav;

pub use chain::Chain;
pub use envelope::{Envelope, open, seal};
pub use policy::{Refusal, may_sync};
pub use transport::{MemoryTransport, Transport};
#[cfg(feature = "webdav")]
pub use webdav::WebDav;

use lapacho_core::{ClipboardItem, PersistLevel};

/// Why [`send`] didn't send.
#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    /// The policy forbids it; nothing left the device.
    Refused(Refusal),
    /// Sealing or the transport failed; the item stays local.
    Failed(String),
}

/// Seals `item` for the chain and pushes it, if `level` allows it to leave.
/// Called on an explicit user act ("Sync to my devices"): the stored
/// `sync_eligible` flag is not consent, so it isn't read here. Returns the
/// item's `sync_id`, for the caller to store with it.
// ponytail: one push, no outbox or retry; a failed send returns `Failed` and
// the item stays local. A persistent queue with backoff belongs to the app that
// runs sync (Lapacho Sync on Android), which knows when the network is back.
pub fn send(
    chain: &Chain,
    device: &str,
    item: &ClipboardItem,
    level: PersistLevel,
    transport: &dyn Transport,
) -> Result<String, SendError> {
    may_sync(item, level).map_err(SendError::Refused)?;
    let env = seal(chain, device, item).map_err(SendError::Failed)?;
    transport.push(&env).map_err(SendError::Failed)?;
    Ok(env.sync_id)
}

/// Pulls the chain's envelopes and opens those from other devices. Returns
/// the items, ready for `HistoryRepo::save` (which applies this device's
/// `PersistLevel` and dedups what it already has), and how many envelopes
/// failed to open: tampered, or sealed with another key.
pub fn receive(
    chain: &Chain,
    device: &str,
    transport: &dyn Transport,
) -> Result<(Vec<ClipboardItem>, usize), String> {
    let mut items = Vec::new();
    let mut failed = 0;
    for env in transport.pull()? {
        if env.sender == device || env.chain_id != chain.id() {
            continue;
        }
        match open(chain, &env) {
            Ok(item) => items.push(item),
            Err(_) => failed += 1,
        }
    }
    Ok((items, failed))
}
