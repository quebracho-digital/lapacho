//! End-to-end sync policy, envelopes, and transport placeholders for Lapacho.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncState {
    LocalOnly,
    PendingUpload,
    Synced,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEnvelope {
    pub sync_id: String,
    pub payload_ciphertext: Vec<u8>,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_state() {
        let state = SyncState::LocalOnly;
        assert_eq!(state, SyncState::LocalOnly);
    }
}
