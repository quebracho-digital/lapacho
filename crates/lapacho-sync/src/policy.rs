use lapacho_core::{ClipboardItem, PersistLevel};

/// Why an item may not leave the device. Shown to the user next to the
/// disabled "Sync to my devices".
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Images don't sync in v1: a few MB per clip is not what the queue is for.
    Image,
    /// The persistence level keeps this sensitivity off the disk, so it
    /// doesn't leave the device either.
    Level,
}

impl Refusal {
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::Image => "Images don't sync",
            Refusal::Level => "Your privacy level keeps this item on this device",
        }
    }
}

/// The second gate after local persistence (§5.3): an item syncs only if the
/// active level would write it to disk. `PersistLevel::persists` is the one
/// rule for both, so the two can't drift. Stricter than §5.3's draft matrix,
/// which let `Personal` sync under `None`: a level that won't store a phone
/// number shouldn't send it to another device either.
///
/// The per-item vault doesn't open this gate: it overrides what reaches this
/// device's disk, not what leaves it.
pub fn may_sync(item: &ClipboardItem, level: PersistLevel) -> Result<(), Refusal> {
    if item.content_type == "image" {
        return Err(Refusal::Image);
    }
    if !level.persists(item.sensitivity) {
        return Err(Refusal::Level);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lapacho_core::{Sensitivity, process_text};

    fn item(s: Sensitivity) -> ClipboardItem {
        let mut i = process_text("x");
        i.sensitivity = s;
        i
    }

    #[test]
    fn matrix() {
        use PersistLevel::*;
        use Sensitivity as S;
        let allowed = |l, s| may_sync(&item(s), l).is_ok();
        assert!(allowed(None, S::None));
        assert!(!allowed(None, S::Personal));
        assert!(!allowed(None, S::Credential));
        assert!(!allowed(None, S::Secret));
        assert!(allowed(Sensitive, S::Credential));
        assert!(!allowed(Sensitive, S::Secret));
        assert!(allowed(All, S::Secret));
    }

    #[test]
    fn vault_does_not_open_the_gate() {
        let mut i = item(Sensitivity::Secret);
        i.vaulted = true;
        assert_eq!(may_sync(&i, PersistLevel::Sensitive), Err(Refusal::Level));
    }

    #[test]
    fn images_never() {
        let mut i = item(Sensitivity::None);
        i.content_type = "image".into();
        assert_eq!(may_sync(&i, PersistLevel::All), Err(Refusal::Image));
    }
}
