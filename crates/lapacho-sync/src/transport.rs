use std::collections::HashMap;
use std::sync::Mutex;

use crate::Envelope;

/// Where envelopes go and come from: one mailbox per chain. A transport only
/// ever handles ciphertext. Implementations to come: a WebDAV folder (S9),
/// a direct Bluetooth/LAN link to the phone (S10), Google's Data Layer in the
/// `play` flavor (S12).
pub trait Transport {
    /// Stores `env`, replacing any envelope with the same `sync_id`.
    fn push(&self, env: &Envelope) -> Result<(), String>;
    /// Every envelope in the mailbox.
    // ponytail: all of them each time; a "since" cursor when mailboxes grow.
    fn pull(&self) -> Result<Vec<Envelope>, String>;
    /// Removes the envelope with this `sync_id`; already gone is fine.
    fn delete(&self, sync_id: &str) -> Result<(), String>;
}

/// In memory, for tests: share one between two "devices".
#[derive(Default)]
pub struct MemoryTransport(Mutex<HashMap<String, Envelope>>);

impl Transport for MemoryTransport {
    fn push(&self, env: &Envelope) -> Result<(), String> {
        let mut m = self.0.lock().map_err(|e| e.to_string())?;
        m.insert(env.sync_id.clone(), env.clone());
        Ok(())
    }

    fn pull(&self) -> Result<Vec<Envelope>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.values().cloned().collect())
    }

    fn delete(&self, sync_id: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.remove(sync_id);
        Ok(())
    }
}
