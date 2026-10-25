//! Default phrases: what usually follows the words just typed ("buen" →
//! "día", "gracias por" → "todo"). A fixed, public table, the same for
//! everyone who has it — no user model, nothing written anywhere, exactly as
//! with the word list (see `docs/DECISIONS.md`).

use crate::fold;
use std::collections::HashMap;

/// Words of context a key may have. Two is what a greeting needs ("buen día"
/// → …) and what a table written by hand can cover; a third would be a
/// corpus, not a table.
const MAX_KEY_WORDS: usize = 2;

pub struct PhraseBook {
    /// Folded context, one or two words joined by a space → phrases, best first.
    next: HashMap<String, Vec<String>>,
}

impl PhraseBook {
    /// Each file is `context > phrase | phrase | …` lines, best phrase first
    /// (see `docs/DICTIONARIES.md`). The context is one or two words; case
    /// and accents are folded away, so `mañana` and `Manana` are one key.
    /// Lines starting with `#`, blank lines and anything else that is not that
    /// shape are skipped. A context in two files gets both lists, each phrase
    /// once.
    pub fn new(files: &[&str]) -> Self {
        let mut next: HashMap<String, Vec<String>> = HashMap::new();
        for line in files.iter().flat_map(|f| f.lines()).map(str::trim) {
            let Some((key, phrases)) = line.split_once('>').filter(|_| !line.starts_with('#')) else { continue };
            let words: Vec<String> = key.split_whitespace().map(word_key).collect();
            if words.is_empty() || words.len() > MAX_KEY_WORDS || words.iter().any(String::is_empty) {
                continue;
            }
            let list = next.entry(words.join(" ")).or_default();
            for p in phrases.split('|').map(str::trim).filter(|p| !p.is_empty()) {
                if !list.iter().any(|q| q == p) {
                    list.push(p.to_string());
                }
            }
        }
        PhraseBook { next }
    }

    /// Up to `limit` phrases that continue `before`, the text up to the
    /// cursor: those for its last two words first, then for the last one.
    ///
    /// Only once a word is finished — `before` must end in a space — because
    /// while one is being typed the word list's `suggest` has the strip. A
    /// sentence mark or a line break ends the context: nothing follows
    /// "gracias." but a new sentence, which this table knows nothing about.
    pub fn next(&self, before: &str, limit: usize) -> Vec<String> {
        let tail = &before[before.trim_end().len()..];
        if tail.is_empty() || tail.contains('\n') {
            return Vec::new();
        }
        let mut tokens = before.split_whitespace().rev();
        let Some(last) = tokens.next().filter(|t| !ends_sentence(t)) else { return Vec::new() };
        let last = word_key(last);
        if last.is_empty() {
            return Vec::new();
        }
        let mut keys = Vec::with_capacity(2);
        if let Some(prev) = tokens.next().filter(|t| !ends_sentence(t)).map(word_key).filter(|k| !k.is_empty()) {
            keys.push(format!("{prev} {last}"));
        }
        keys.push(last);

        let mut out: Vec<String> = Vec::new();
        for p in keys.iter().filter_map(|k| self.next.get(k)).flatten() {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out.truncate(limit);
        out
    }
}

/// A token as a table key: folded, a typographic apostrophe made plain
/// ("I’m" = "I'm"), and the punctuation around it ("¿cómo", "hola,") gone.
fn word_key(token: &str) -> String {
    fold(&token.replace('’', "'")).trim_matches(|c: char| !c.is_alphanumeric()).to_string()
}

fn ends_sentence(token: &str) -> bool {
    token.ends_with(['.', '!', '?', '…'])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "\
# a comment > not a rule
buen > día | días | fin de semana
buen día > ¿cómo estás? | a todos
cómo > estás | andás | te va
i'm > sorry | on my way
nonsense without arrow
> nothing before the arrow
one two three > too many words
";

    fn book() -> PhraseBook {
        PhraseBook::new(&[TABLE])
    }

    #[test]
    fn a_finished_word_gets_its_phrases_in_file_order() {
        assert_eq!(book().next("Hola, buen ", 5), vec!["día", "días", "fin de semana"]);
    }

    #[test]
    fn two_words_of_context_come_before_one() {
        let b = PhraseBook::new(&["día > de nada\nbuen día > ¿cómo estás?"]);
        assert_eq!(b.next("buen día ", 5), vec!["¿cómo estás?", "de nada"]);
    }

    #[test]
    fn limit_cuts_the_list() {
        assert_eq!(book().next("buen ", 2), vec!["día", "días"]);
        assert!(book().next("buen ", 0).is_empty());
    }

    #[test]
    fn a_word_still_being_typed_is_left_to_the_word_list() {
        assert!(book().next("buen", 3).is_empty());
        assert!(book().next("", 3).is_empty());
    }

    #[test]
    fn a_sentence_mark_or_a_line_break_ends_the_context() {
        assert!(book().next("buen día. ", 3).is_empty());
        assert!(book().next("¿buen? ", 3).is_empty());
        assert!(book().next("buen \n", 3).is_empty());
        // The mark one word back cuts the two-word key, not the one-word one.
        assert_eq!(book().next("Hola. Buen ", 1), vec!["día"]);
        assert_eq!(PhraseBook::new(&["a b > x\nb > y"]).next("a. b ", 5), vec!["y"]);
    }

    #[test]
    fn case_accents_and_punctuation_do_not_matter() {
        assert_eq!(book().next("¿COMO ", 1), vec!["estás"]);
        assert_eq!(book().next("Cómo ", 1), vec!["estás"]);
        assert_eq!(book().next("hola, buen ", 1), vec!["día"]);
    }

    #[test]
    fn a_typographic_apostrophe_is_the_plain_one() {
        assert_eq!(book().next("I’m ", 5), vec!["sorry", "on my way"]);
    }

    #[test]
    fn malformed_lines_and_comments_are_skipped() {
        let b = book();
        assert!(b.next("a ", 3).is_empty());
        assert!(b.next("three ", 3).is_empty());
        assert!(b.next("comment ", 3).is_empty());
    }

    #[test]
    fn two_files_mix_and_a_phrase_is_not_repeated() {
        let b = PhraseBook::new(&["ok > dale | gracias", "ok > gracias | thanks"]);
        assert_eq!(b.next("ok ", 5), vec!["dale", "gracias", "thanks"]);
    }
}
