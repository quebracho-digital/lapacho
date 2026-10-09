//! Prints the request the built-in `terms` plugin makes for the text on stdin:
//! the same prompt and fence a user pastes into their assistant, for
//! terms-eval's prompt-injection set. Usage: terms_request < terms.txt
use std::io::Read;

fn main() {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).expect("text on stdin");
    print!("{}", lapacho_core::terms::assistant_request(&text));
}
