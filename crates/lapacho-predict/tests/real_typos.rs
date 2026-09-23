//! Corrections against the dictionaries the app actually ships: the bundled
//! Spanish and the official English. Typos are real ones — swaps, a dropped
//! or doubled letter, a phonetic spelling — and the words that must *not* be
//! corrected are as much a part of the check as the ones that must.

use lapacho_predict::Predictor;
use std::time::Instant;

fn dictionaries() -> Predictor {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/mobile/android");
    let es = std::fs::read_to_string(format!("{root}/app/src/main/assets/dict/es.txt")).unwrap();
    let en = std::fs::read_to_string(format!("{root}/dictionaries/en.txt")).unwrap();
    Predictor::new(&[&es, &en], &[])
}

#[test]
fn corrects_real_typos_and_leaves_real_words_alone() {
    let p = dictionaries();
    let first = |w: &str| p.correct(w, 3).into_iter().next();
    let expected = [
        ("maniana", "mañana"), ("qeu", "que"), ("tambein", "también"), ("hoal", "hola"),
        ("graicas", "gracias"), ("porqeu", "porque"), ("cuadno", "cuando"), ("dodne", "dónde"),
        ("tengi", "tengo"), ("qiero", "quiero"), ("ahroa", "ahora"), ("despeus", "después"),
        ("siempr", "siempre"), ("nescesito", "necesito"), ("trabjo", "trabajo"), ("ecsamen", "examen"),
        ("Graicas", "Gracias"), ("teh", "the"), ("becuase", "because"), ("wiht", "with"),
        ("thnaks", "thanks"), ("recieve", "receive"), ("definately", "definitely"),
    ];
    let wrong: Vec<String> = expected.iter()
        .filter(|(typo, want)| first(typo).as_deref() != Some(*want))
        .map(|(typo, want)| format!("{typo} → {:?}, want {want}", first(typo)))
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");

    for real in ["estoy", "perro", "pero", "vaca", "casa", "nadia", "esta", "the", "house"] {
        assert_eq!(first(real), None, "{real} is a word, it must not be corrected");
    }
}

#[test]
fn a_correction_is_cheap_enough_for_a_keystroke() {
    let p = dictionaries();
    let t0 = Instant::now();
    for _ in 0..20 {
        for w in ["maniana", "cuadno", "siempr", "definately", "ecsamen"] {
            std::hint::black_box(p.correct(w, 3));
        }
    }
    let per_call_ms = t0.elapsed().as_secs_f64() * 1e3 / 100.0;
    // Generous for a debug build on a loaded CI box; release on the laptop
    // is ~1 ms, and the phone is budgeted at a few times that.
    assert!(per_call_ms < 50.0, "{per_call_ms:.2} ms per correction");
}
