//! Swipes decoded against the dictionaries the app ships, over the letter
//! layer the keyboard draws. The swipes are synthetic: the line through the
//! word's keys, then the same line with each key missed by a fixed offset —
//! the way a finger lands off-centre — so the score is repeatable.

use lapacho_predict::{Predictor, SwipeKey};
use std::time::Instant;

fn dictionaries() -> (Predictor, String) {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/mobile/android/app/src/main/assets/dict");
    let es = std::fs::read_to_string(format!("{root}/es.txt")).unwrap();
    let en = std::fs::read_to_string(format!("{root}/en.txt")).unwrap();
    (Predictor::new(&[&es, &en], &[]), es)
}

/// The IME's letter rows, in key widths: each row fills the same width with
/// weighted keys (the second row has 9, so its keys are wider; the third
/// starts after a 1.5-wide shift and ends with the ´ key), and a row is
/// 1.2 keys tall (46 dp keys on a ~41 dp column).
fn layout() -> Vec<SwipeKey> {
    let rows: [(&str, f32, f32); 3] = [("qwertyuiop", 0.0, 10.0), ("asdfghjkl", 0.0, 9.0), ("zxcvbnm", 1.5, 9.5)];
    let mut keys = Vec::new();
    for (r, (letters, lead, weights)) in rows.iter().enumerate() {
        let w = 10.0 / weights;
        for (i, c) in letters.chars().enumerate() {
            keys.push((c, (lead + i as f32 + 0.5) * w, (r as f32 + 0.5) * 1.2));
        }
    }
    keys
}

/// The line through `word`'s keys, 8 points per stretch, each key missed by
/// `miss` key widths in a direction that turns from key to key.
fn swipe_of(word: &str, keys: &[SwipeKey], miss: f32) -> Vec<(f32, f32)> {
    let centres: Vec<(f32, f32)> = word
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let k = keys.iter().find(|k| k.0 == c).unwrap();
            let a = i as f32 * 2.4;
            (k.1 + miss * a.cos(), k.2 + miss * a.sin())
        })
        .collect();
    let mut path = vec![centres[0]];
    for w in centres.windows(2) {
        for s in 1..=8 {
            let t = s as f32 / 8.0;
            path.push((w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1)));
        }
    }
    path
}

/// The most frequent Spanish words a swipe can type: two letters or more,
/// every letter on the keys once accents are folded.
fn common_words(es: &str, n: usize) -> Vec<String> {
    es.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().next())
        .map(lapacho_predict::fold)
        .filter(|w| w.chars().count() >= 2 && w.chars().all(|c| c.is_ascii_lowercase()))
        .take(n)
        .collect()
}

/// Share of `words` whose swipe puts the word (or one drawn by the same keys,
/// like `calle`/`cale`) among the first `top` answers.
fn hit_rate(p: &Predictor, words: &[String], miss: f32, top: usize) -> (f32, Vec<String>) {
    let keys = layout();
    let mut missed = Vec::new();
    for w in words {
        let got = p.swipe(&swipe_of(w, &keys, miss), &keys, 1.0, top);
        if !got.iter().any(|g| same_line(&lapacho_predict::fold(g), w)) {
            missed.push(format!("{w} → {got:?}"));
        }
    }
    (1.0 - missed.len() as f32 / words.len() as f32, missed)
}

/// Two words drawn by the same keys in the same order, a doubled letter once.
fn same_line(a: &str, b: &str) -> bool {
    let mut x: Vec<char> = a.chars().collect();
    let mut y: Vec<char> = b.chars().collect();
    x.dedup();
    y.dedup();
    x == y
}

#[test]
fn common_spanish_words_decode_from_their_swipe() {
    let (p, es) = dictionaries();
    let words = common_words(&es, 500);
    for (miss, top, want) in [(0.0, 1, 0.98), (0.0, 3, 0.99), (0.3, 1, 0.92), (0.3, 3, 0.98)] {
        let (rate, missed) = hit_rate(&p, &words, miss, top);
        eprintln!("miss {miss} top {top}: {:.1}% ({} missed) e.g. {:?}", rate * 100.0, missed.len(), &missed[..missed.len().min(12)]);
        assert!(rate >= want, "miss {miss}, top {top}: {rate}");
    }
}

#[test]
fn decoding_a_swipe_is_fast_enough_for_a_keystroke() {
    let (p, _) = dictionaries();
    let keys = layout();
    let t0 = Instant::now();
    for w in ["que", "cuando", "después", "gracias", "necesito", "mañana", "the", "because"] {
        let w = lapacho_predict::fold(w);
        p.swipe(&swipe_of(&w, &keys, 0.3), &keys, 1.0, 3);
    }
    let each = t0.elapsed() / 8;
    eprintln!("swipe: {each:?} each");
    // Debug build; release is several times faster.
    assert!(each.as_millis() < 200, "{each:?}");
}
