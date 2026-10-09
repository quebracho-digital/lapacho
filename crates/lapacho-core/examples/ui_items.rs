//! Prints, as the desktop's `get_history` would return them, the history items
//! that copying each file's text would make: the ingest pipeline (classifier,
//! sanitizers) on real input, for the UI's XSS check
//! (apps/desktop/ui/tests/xss.py). Usage: ui_items <file>...
use lapacho_core::ingest::process_text;
use lapacho_core::types::UIClipboardItem;

fn main() {
    let items: Vec<UIClipboardItem> = std::env::args()
        .skip(1)
        .map(|path| {
            let mut item = process_text(&std::fs::read_to_string(&path).expect("readable file"));
            item.id = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
            UIClipboardItem::from(item)
        })
        .collect();
    println!("{}", serde_json::to_string(&items).unwrap());
}
