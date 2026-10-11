//! P4's exit criterion, in memory: two devices on one chain exchange a clip
//! end to end; a secret passes only when the level allows; the transport
//! never sees plaintext.

use lapacho_core::{PersistLevel, RetentionPolicy, Sensitivity, process_text};
use lapacho_sync::{
    Chain, MAILBOX_TTL_SECS, MemoryTransport, Refusal, SendError, Transport, expires_at, receive,
    seal, send,
};

const R: RetentionPolicy = RetentionPolicy { sensitive_ttl_secs: Some(3600), max_items: 100 };

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn a_clip_crosses_and_the_transport_sees_only_ciphertext() {
    let laptop = Chain::create().unwrap();
    let phone = Chain::from_words(&laptop.words()).unwrap();
    let t = MemoryTransport::default();

    let item = process_text("la reunión es a las 15");
    send(&laptop, "laptop", &item, PersistLevel::None, &R, &t).unwrap();

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
            send(&laptop, "laptop", &secret, level, &R, &t),
            Err(SendError::Refused(Refusal::Level))
        );
    }
    assert!(t.pull().unwrap().is_empty(), "a refused secret reached the transport");

    send(&laptop, "laptop", &secret, PersistLevel::All, &R, &t).unwrap();
    let (got, _) = receive(&phone, "phone", &t).unwrap();
    assert_eq!(got[0].sensitivity, Sensitivity::Secret);
}

#[test]
fn another_chain_gets_nothing() {
    let mine = Chain::create().unwrap();
    let theirs = Chain::create().unwrap();
    let t = MemoryTransport::default();
    send(&mine, "laptop", &process_text("x"), PersistLevel::All, &R, &t).unwrap();
    // Skipped by chain id before any decryption is tried.
    let (got, failed) = receive(&theirs, "phone", &t).unwrap();
    assert!(got.is_empty());
    assert_eq!(failed, 0);
}

#[test]
fn an_expired_envelope_is_deleted_not_delivered() {
    let laptop = Chain::create().unwrap();
    let phone = Chain::from_words(&laptop.words()).unwrap();
    let t = MemoryTransport::default();
    t.push(&seal(&laptop, "laptop", &process_text("old"), now()).unwrap()).unwrap();
    send(&laptop, "laptop", &process_text("new"), PersistLevel::None, &R, &t).unwrap();

    let (got, failed) = receive(&phone, "phone", &t).unwrap();
    assert_eq!((got.len(), failed), (1, 0));
    assert_eq!(got[0].raw_content, "new");
    assert_eq!(t.pull().unwrap().len(), 1, "the expired one is still in the mailbox");
}

#[test]
fn a_secret_leaves_the_mailbox_when_it_leaves_the_disk() {
    let mut secret = process_text("token");
    secret.sensitivity = Sensitivity::Secret;
    secret.timestamp = now() - 600; // copied ten minutes ago, one-hour TTL
    assert_eq!(expires_at(&secret, &R), secret.timestamp + 3600);

    let plain = process_text("hola");
    let e = expires_at(&plain, &R);
    assert!(e >= now() + MAILBOX_TTL_SECS - 5 && e <= now() + MAILBOX_TTL_SECS);

    // Already past its TTL here: it doesn't go out at all.
    secret.timestamp = now() - 7200;
    let t = MemoryTransport::default();
    let c = Chain::create().unwrap();
    assert_eq!(
        send(&c, "laptop", &secret, PersistLevel::All, &R, &t),
        Err(SendError::Refused(Refusal::Expired))
    );
    assert!(t.pull().unwrap().is_empty());
}
