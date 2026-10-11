//! P4's exit criterion, in memory: two devices on one chain exchange a clip
//! end to end; a secret passes only when the level allows; the transport
//! never sees plaintext.

use lapacho_core::{PersistLevel, Sensitivity, process_text};
use lapacho_sync::{Chain, MemoryTransport, Refusal, SendError, Transport, receive, send};

#[test]
fn a_clip_crosses_and_the_transport_sees_only_ciphertext() {
    let laptop = Chain::create().unwrap();
    let phone = Chain::from_words(&laptop.words()).unwrap();
    let t = MemoryTransport::default();

    let item = process_text("la reunión es a las 15");
    send(&laptop, "laptop", &item, PersistLevel::None, &t).unwrap();

    for env in t.pull().unwrap() {
        let json = serde_json::to_string(&env).unwrap();
        assert!(!json.contains("reunión"), "plaintext reached the transport");
    }

    let (got, failed) = receive(&phone, "phone", &t).unwrap();
    assert_eq!(failed, 0);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].raw_content, "la reunión es a las 15");

    // The sender doesn't get its own clip back.
    assert!(receive(&laptop, "laptop", &t).unwrap().0.is_empty());
}

#[test]
fn a_secret_passes_only_when_the_level_allows() {
    let laptop = Chain::create().unwrap();
    let phone = Chain::from_words(&laptop.words()).unwrap();
    let t = MemoryTransport::default();

    let mut secret = process_text("token");
    secret.sensitivity = Sensitivity::Secret;

    for level in [PersistLevel::None, PersistLevel::Sensitive] {
        assert_eq!(
            send(&laptop, "laptop", &secret, level, &t),
            Err(SendError::Refused(Refusal::Level))
        );
    }
    assert!(t.pull().unwrap().is_empty(), "a refused secret reached the transport");

    send(&laptop, "laptop", &secret, PersistLevel::All, &t).unwrap();
    let (got, _) = receive(&phone, "phone", &t).unwrap();
    assert_eq!(got[0].sensitivity, Sensitivity::Secret);
}

#[test]
fn another_chain_gets_nothing() {
    let mine = Chain::create().unwrap();
    let theirs = Chain::create().unwrap();
    let t = MemoryTransport::default();
    send(&mine, "laptop", &process_text("x"), PersistLevel::All, &t).unwrap();
    // Skipped by chain id before any decryption is tried.
    let (got, failed) = receive(&theirs, "phone", &t).unwrap();
    assert!(got.is_empty());
    assert_eq!(failed, 0);
}
