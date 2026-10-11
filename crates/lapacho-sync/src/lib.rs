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

use lapacho_core::{ClipboardItem, PersistLevel, RetentionPolicy, Sensitivity};

/// How long an envelope waits in the mailbox for the other devices: a week,
/// so a device that was off for a few days still gets it.
pub const MAILBOX_TTL_SECS: u64 = 7 * 24 * 3600;

/// When `item`'s envelope must leave the mailbox. A credential or secret
/// leaves no later than it leaves this device's disk — the local TTL counts
/// from when it was copied, so the remote one does too (§5.3, rule 3).
pub fn expires_at(item: &ClipboardItem, retention: &RetentionPolicy) -> u64 {
    let mailbox = envelope::now_secs().saturating_add(MAILBOX_TTL_SECS);
    match (item.sensitivity, retention.sensitive_ttl_secs) {
        (Sensitivity::Credential | Sensitivity::Secret, Some(ttl)) => {
            mailbox.min(item.timestamp.saturating_add(ttl))
        }
        _ => mailbox,
    }
}

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
    retention: &RetentionPolicy,
    transport: &dyn Transport,
) -> Result<String, SendError> {
    may_sync(item, level).map_err(SendError::Refused)?;
    let expires = expires_at(item, retention);
    if expires <= envelope::now_secs() {
        return Err(SendError::Refused(Refusal::Expired));
    }
    let env = seal(chain, device, item, expires).map_err(SendError::Failed)?;
    transport.push(&env).map_err(SendError::Failed)?;
    Ok(env.sync_id)
}

/// Pulls the chain's envelopes, deletes the expired ones from the transport
/// (whoever sent them: any device of the chain cleans up), and opens those
/// from other devices. Returns
/// the items, ready for `HistoryRepo::save` (which applies this device's
/// `PersistLevel` and dedups what it already has), and how many envelopes
/// failed to open: tampered, or sealed with another key. A failed delete fails
/// the whole call, so an expired secret left on the server doesn't go unseen.
pub fn receive(
    chain: &Chain,
    device: &str,
    transport: &dyn Transport,
) -> Result<(Vec<ClipboardItem>, usize), String> {
    let mut items = Vec::new();
    let mut failed = 0;
    let now = envelope::now_secs();
    for env in transport.pull()? {
        // Read from the header without opening: whoever can forge it can
        // already delete files from the folder.
        if env.expires_at <= now {
            transport.delete(&env.sync_id)?;
            continue;
        }
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
