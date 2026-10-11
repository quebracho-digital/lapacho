//! Against a real WebDAV server; skipped unless `LAPACHO_WEBDAV_URL` is set:
//!
//! ```sh
//! docker run --rm -d -p 8099:80 rclone/rclone:1.68.2 serve webdav /data --addr :80 --user u --pass p
//! LAPACHO_WEBDAV_URL=http://localhost:8099 LAPACHO_WEBDAV_USER=u LAPACHO_WEBDAV_PASS=p \
//!   cargo test -p lapacho-sync --features webdav --test webdav
//! ```
#![cfg(feature = "webdav")]

use lapacho_core::{PersistLevel, RetentionPolicy, process_text};
use lapacho_sync::{Chain, Transport, WebDav, receive, seal, send};

const R: RetentionPolicy = RetentionPolicy { sensitive_ttl_secs: Some(3600), max_items: 100 };

#[test]
fn two_devices_through_a_webdav_folder() {
    let Ok(url) = std::env::var("LAPACHO_WEBDAV_URL") else {
        eprintln!("LAPACHO_WEBDAV_URL not set, skipping");
        return;
    };
    let user = std::env::var("LAPACHO_WEBDAV_USER").ok();
    let pass = std::env::var("LAPACHO_WEBDAV_PASS").ok();
    let dav = |c: &Chain| WebDav::new(&url, c.id(), user.as_deref(), pass.as_deref());

    let laptop = Chain::create().unwrap(); // a fresh chain = an empty mailbox
    let phone = Chain::from_words(&laptop.words()).unwrap();

    assert!(dav(&phone).pull().unwrap().is_empty(), "missing mailbox should read as empty");

    send(&laptop, "laptop", &process_text("por webdav"), PersistLevel::None, &R, &dav(&laptop)).unwrap();
    // Same clip again: same file, overwritten, not a second one.
    send(&laptop, "laptop", &process_text("por webdav"), PersistLevel::None, &R, &dav(&laptop)).unwrap();
    assert_eq!(dav(&laptop).pull().unwrap().len(), 1);

    let (got, failed) = receive(&phone, "phone", &dav(&phone)).unwrap();
    assert_eq!((got.len(), failed), (1, 0));
    assert_eq!(got[0].raw_content, "por webdav");

    // An expired envelope is deleted from the server by whoever pulls next.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    dav(&laptop).push(&seal(&laptop, "laptop", &process_text("vieja"), now).unwrap()).unwrap();
    assert_eq!(dav(&laptop).pull().unwrap().len(), 2);
    receive(&phone, "phone", &dav(&phone)).unwrap();
    assert_eq!(dav(&laptop).pull().unwrap().len(), 1);
}

#[test]
fn wrong_password_is_an_error_not_an_empty_mailbox() {
    let Ok(url) = std::env::var("LAPACHO_WEBDAV_URL") else { return };
    let c = Chain::create().unwrap();
    let dav = WebDav::new(&url, c.id(), Some("u"), Some("wrong"));
    assert!(dav.pull().is_err());
}
