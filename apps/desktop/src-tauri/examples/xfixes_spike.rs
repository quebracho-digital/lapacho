//! Spike to test background X11 clipboard monitoring using XFixes events
//! (without the process having window focus).
//!
//! Run with:
//!   cargo run -p lapacho-desktop --example xfixes_spike
//!
//! Then, in another app (e.g. browser, terminal with xclip, etc.) copy text.
//! The spike should print "CLIPBOARD CHANGE DETECTED" to stdout immediately,
//! even if this process has no focus.

use std::io;

use clipboard_master::{CallbackResult, ClipboardHandler, Master};

struct Handler;

impl ClipboardHandler for Handler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        println!("CLIPBOARD CHANGE DETECTED");
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: io::Error) -> CallbackResult {
        eprintln!("Clipboard error: {}", error);
        CallbackResult::Next
    }
}

fn main() {
    println!("Starting XFixes clipboard spike with clipboard-master (v4)...");
    println!("Copy text in another app (no focus on this) to test.");
    let mut master = Master::new(Handler).expect("create new monitor");
    // Blocks until error or shutdown (use Ctrl-C to stop).
    master.run().expect("run monitor");
}
