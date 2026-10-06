//! The published Hebrew and Italian dictionaries, through the engine: a
//! script with no case and no accents to fold, and a Latin one with graves.

use lapacho_predict::{Predictor, SwipeKey};

fn dict(lang: &str) -> Predictor {
    let path = format!("{}/../../apps/mobile/android/dictionaries/{lang}.txt", env!("CARGO_MANIFEST_DIR"));
    Predictor::new(&[&std::fs::read_to_string(path).unwrap()], &[])
}

#[test]
fn hebrew_completes_corrects_and_swipes() {
    let p = dict("he");
    assert!(p.len() > 40_000);
    assert!(p.knows("שלום"));
    assert!(p.suggest("שלו", 3).contains(&"שלום".to_string()), "{:?}", p.suggest("שלו", 3));
    // A regular mem where the word ends in a final one is the same word,
    // offered the way a missing accent is.
    assert_eq!(p.suggest("שלומ", 1), vec!["שלום"]);
    assert_eq!(p.correct("תודא", 1), vec!["תודה"]);

    // The #rows of he.txt on a 10-wide grid, a key per unit.
    let rows = ["קראטוןםפ", "שדגכעיחלךף", "זסבהנמצתץ"];
    let keys: Vec<SwipeKey> = rows.iter().enumerate()
        .flat_map(|(r, row)| row.chars().enumerate().map(move |(i, c)| (c, i as f32 + r as f32 * 0.5, r as f32)))
        .collect();
    let at = |c: char| keys.iter().find(|k| k.0 == c).map(|k| (k.1, k.2)).unwrap();
    for word in ["שלום", "אמא", "מים", "כמה"] {
        let path: Vec<(f32, f32)> = word.chars().map(at).collect();
        assert_eq!(p.swipe(&path, &keys, 1.0, 1), vec![word], "{word}: a final and a regular letter are two keys");
    }
}

#[test]
fn italian_finds_the_grave_without_typing_it() {
    let p = dict("it");
    assert_eq!(p.suggest("perch", 1), vec!["perché"]);
    assert!(p.suggest("citt", 3).contains(&"città".to_string()), "{:?}", p.suggest("citt", 3));
}
