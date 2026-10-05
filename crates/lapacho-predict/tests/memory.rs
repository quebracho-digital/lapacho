//! How much heap the shipped dictionaries cost once loaded. The keyboard runs
//! in a process Android kills under memory pressure, so this is the budget
//! that decides how many languages can be active at once.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use lapacho_predict::Predictor;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn the_shipped_dictionaries_cost_little_more_than_their_words() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/mobile/android");
    let es = std::fs::read_to_string(format!("{root}/app/src/main/assets/dict/es.txt")).unwrap();
    let en = std::fs::read_to_string(format!("{root}/app/src/main/assets/dict/en.txt")).unwrap();

    let before = LIVE.load(Ordering::Relaxed);
    let t = Instant::now();
    let p = Predictor::new(&[&es, &en], &[]);
    let took = t.elapsed();
    let heap = LIVE.load(Ordering::Relaxed) - before;

    // What the words themselves weigh, without the frequencies.
    let text: usize = [&es, &en].iter().flat_map(|d| d.lines()).filter(|l| !l.starts_with('#'))
        .map(|l| l.split_whitespace().next().unwrap_or("").len()).sum();
    eprintln!("{} words, {heap} bytes of heap for {text} bytes of words, built in {took:?}", p.len());
    assert!(heap < 3 * text as isize, "{heap} bytes for {text} bytes of words");
}
