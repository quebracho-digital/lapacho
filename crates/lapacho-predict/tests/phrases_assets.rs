//! The phrase tables the app ships. A line that is mistyped does not fail to
//! load, it silently never matches — so the shape of every line is checked
//! here, and a few real contexts are looked up end to end.

use lapacho_predict::{fold, PhraseBook};
use std::collections::HashSet;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/mobile/android/app/src/main/assets/phrases");

fn read(lang: &str) -> String {
    std::fs::read_to_string(format!("{ROOT}/{lang}.txt")).unwrap()
}

#[test]
fn every_line_is_a_context_that_can_match() {
    for lang in ["es", "en"] {
        let mut seen = HashSet::new();
        for (n, line) in read(lang).lines().enumerate().map(|(n, l)| (n + 1, l.trim())) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = format!("{lang}.txt:{n}: {line}");
            let (key, phrases) = line.split_once('>').unwrap_or_else(|| panic!("no `>` — {at}"));
            let words: Vec<&str> = key.split_whitespace().collect();
            assert!((1..=2).contains(&words.len()), "context is 1 or 2 words — {at}");
            // Case and accents are folded away on load, but a mark inside a
            // word never is, and typed text never has one there to match.
            for w in &words {
                assert!(w.chars().all(|c| c.is_alphanumeric() || c == '\''), "context has punctuation — {at}");
            }
            assert!(seen.insert(fold(&words.join(" "))), "context twice in one file — {at}");
            let list: Vec<&str> = phrases.split('|').map(str::trim).collect();
            assert!(list.iter().all(|p| !p.is_empty() && p.chars().count() <= 32), "empty or long phrase — {at}");
            // Tapping a phrase types it plus a space: a leading mark would land after the space.
            assert!(list.iter().all(|p| p.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '¿' || c == '¡')),
                "phrase starts with punctuation — {at}");
        }
    }
}

#[test]
fn the_examples_that_asked_for_this() {
    let es = read("es");
    let en = read("en");
    let book = PhraseBook::new(&[&es, &en]);
    assert_eq!(book.next("buen ", 3).first().map(String::as_str), Some("día"));
    assert!(book.next("Buen día ", 3).contains(&"¿cómo estás?".to_string()));
    assert!(book.next("¿Cómo ", 3).contains(&"estás".to_string()));
    assert!(book.next("Gracias por ", 3).contains(&"todo".to_string()));
    assert!(book.next("Good morning ", 3).contains(&"everyone".to_string()));
    // Nothing mid-word, nothing after a full stop.
    assert!(book.next("buen", 3).is_empty());
    assert!(book.next("Gracias. ", 3).is_empty());
}
