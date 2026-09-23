//! Local prediction engine (dictionary prefix completion) for Lapacho.
//! Strictly private, offline-only, zero network I/O.
//!
//! It completes the word being typed out of a fixed word list, and that is
//! the whole of what it knows: there is no user model, nothing is written
//! anywhere, and two people with the same dictionary get the same
//! suggestions. Learning a word is a separate, explicit act (see
//! `docs/DECISIONS.md`); when it lands it adds entries to this list, it does
//! not turn this into something that watches you type.

/// One dictionary entry. [`key`](Entry::key) is [`fold`]ed so that a prefix
/// typed without accents still finds the word ("cancion" → "canción").
struct Entry {
    key: Box<str>,
    word: Box<str>,
    freq: u32,
}

pub struct Predictor {
    /// Sorted by `key`, which is what [`Predictor::suggest`] binary searches.
    ///
    /// ponytail: two heap allocations per word — measured at ~8.5 MB of native
    /// heap for the 49 525-word Spanish list, against ~1.3 MB of actual text.
    /// Fine for a keyboard process that Android kills when it is not on
    /// screen; if it ever is not, the fix is one flat `String` plus `(start,
    /// end)` offsets, not a smaller dictionary.
    words: Vec<Entry>,
}

/// Every dictionary is scaled to this total, so that no language outranks
/// another just because its corpus was bigger: a word's frequency becomes
/// its share of its own list, in parts per billion. Spanish "de" is ~3.5 %
/// of its corpus, so the largest value this produces stays far below a
/// learned word's `u32::MAX`.
const SCALE: f64 = 1e9;

impl Predictor {
    /// Builds from any number of dictionaries plus the words the user taught
    /// it. Each dictionary is `word frequency` lines in any order — they are
    /// sorted here, so a mis-sorted file cannot silently break the search.
    /// A line with just a word counts as frequency 1, lines starting with `#`
    /// are the header (read by the app, not here), and anything else that is
    /// not exactly those shapes is skipped.
    ///
    /// Frequencies are normalized per dictionary ([`SCALE`]) and a word found
    /// in two of them keeps the higher one. A learned word gets `u32::MAX`:
    /// it was asked for by name, so nothing outranks it.
    pub fn new(dictionaries: &[&str], learned: &[&str]) -> Self {
        let mut words: Vec<Entry> = Vec::new();
        for data in dictionaries {
            let parsed: Vec<(&str, u64)> = data.lines().filter_map(parse_line).collect();
            let total = parsed.iter().map(|(_, f)| f).sum::<u64>().max(1) as f64;
            words.extend(parsed.into_iter().map(|(word, freq)| Entry {
                key: fold(word).into(),
                word: word.into(),
                freq: ((freq as f64 / total * SCALE) as u32).max(1),
            }));
        }
        words.extend(learned.iter().filter(|w| !w.trim().is_empty()).map(|w| Entry {
            key: fold(w.trim()).into(),
            word: w.trim().into(),
            freq: u32::MAX,
        }));
        // Highest frequency first within the same word, so dedup keeps it.
        words.sort_unstable_by(|a, b| (&a.key, &a.word, b.freq).cmp(&(&b.key, &b.word, a.freq)));
        words.dedup_by(|later, first| later.word == first.word);
        Predictor { words }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Whether the dictionary already has this word, comparing the way
    /// [`Predictor::suggest`] does — so `cancion` counts as known because
    /// `canción` is in there. That is deliberate: a missing accent is a typo
    /// to correct, not a word to learn.
    pub fn knows(&self, word: &str) -> bool {
        let key = fold(word);
        if key.is_empty() {
            return false;
        }
        let start = self.words.partition_point(|e| *e.key < *key);
        self.words.get(start).is_some_and(|e| *e.key == *key)
    }

    /// The `limit` most frequent words starting with `prefix`, most frequent
    /// first. Case-insensitive and accent-insensitive; a suggestion is
    /// capitalized when the prefix is.
    ///
    /// The word the user already typed is never suggested — it is on screen
    /// already. An accented spelling of it is (that is the point of folding).
    pub fn suggest(&self, prefix: &str, limit: usize) -> Vec<String> {
        let key = fold(prefix);
        if key.is_empty() || limit == 0 {
            return Vec::new();
        }
        let typed = prefix.to_lowercase();
        let start = self.words.partition_point(|e| *e.key < *key);

        let mut best: Vec<&Entry> = Vec::with_capacity(limit + 1);
        for e in self.words[start..].iter().take_while(|e| e.key.starts_with(&key)) {
            if *e.word == *typed {
                continue;
            }
            let at = best.partition_point(|b| b.freq > e.freq);
            if at < limit {
                best.insert(at, e);
                best.truncate(limit);
            }
        }
        best.into_iter().map(|e| apply_case(prefix, &e.word)).collect()
    }
}

/// One dictionary line as `(word, frequency)`, or `None` for the header, a
/// blank line or a malformed one.
fn parse_line(line: &str) -> Option<(&str, u64)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    match line.split_once(char::is_whitespace) {
        None => Some((line, 1)),
        Some((word, freq)) => Some((word, freq.trim().parse().ok()?)),
    }
}

/// Lowercase, with diacritics removed, so that the dictionary can be
/// searched by what is easiest to type.
///
/// ponytail: a table, not Unicode NFD — it covers Spanish, English,
/// Portuguese, French and German. A language with other marks (Polish, Czech)
/// needs `unicode-normalization` here, not more rows.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            let folded = match c {
                'á' | 'à' | 'ä' | 'â' => 'a',
                'é' | 'è' | 'ë' | 'ê' => 'e',
                'í' | 'ì' | 'ï' | 'î' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' => 'o',
                'ú' | 'ù' | 'ü' | 'û' => 'u',
                'ã' => 'a',
                'õ' => 'o',
                'ç' => 'c',
                'ñ' => 'n',
                other => other,
            };
            folded.to_lowercase()
        })
        .collect()
}

/// Gives `word` the capitalization of `prefix`: `Que` → `Querido`. Only the
/// first letter, because that is the only case a keyboard can infer.
fn apply_case(prefix: &str, word: &str) -> String {
    if !prefix.chars().next().is_some_and(char::is_uppercase) {
        return word.to_string();
    }
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    if let Some(c) = chars.next() {
        out.extend(c.to_uppercase());
    }
    out.push_str(chars.as_str());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(dict: &str) -> Predictor {
        Predictor::new(&[dict], &[])
    }

    const DICT: &str = "que 100\nquerido 80\nquería 90\ncanción 70\ncancion 5\nqué 95\nalto 10";

    #[test]
    fn suggests_most_frequent_first() {
        let p = one(DICT);
        assert_eq!(p.suggest("quer", 2), vec!["quería", "querido"]);
    }

    #[test]
    fn typed_word_is_not_suggested_but_its_accented_form_is() {
        let p = one(DICT);
        let hits = p.suggest("que", 5);
        assert!(!hits.contains(&"que".to_string()), "{hits:?}");
        assert_eq!(hits.first().map(String::as_str), Some("qué"));
    }

    #[test]
    fn a_prefix_without_accents_finds_the_accented_word() {
        let p = one(DICT);
        assert_eq!(p.suggest("canci", 1), vec!["canción"]);
    }

    #[test]
    fn capitalized_prefix_capitalizes_the_suggestion() {
        let p = one(DICT);
        assert_eq!(p.suggest("Alt", 1), vec!["Alto"]);
    }

    #[test]
    fn empty_prefix_and_unknown_prefix_suggest_nothing() {
        let p = one(DICT);
        assert!(p.suggest("", 3).is_empty());
        assert!(p.suggest("xyz", 3).is_empty());
    }

    #[test]
    fn knows_what_is_in_the_dictionary_accents_aside() {
        let p = one(DICT);
        assert!(p.knows("que"));
        assert!(p.knows("QUE"), "case is not a different word");
        assert!(p.knows("cancion"), "a missing accent is a typo, not a new word");
        assert!(!p.knows("quebrachos"));
        assert!(!p.knows(""));
    }

    #[test]
    fn a_learned_word_outranks_the_dictionary() {
        let p = Predictor::new(&[DICT], &["quebrachos"]);
        assert!(p.knows("quebrachos"));
        assert_eq!(p.suggest("que", 1), vec!["quebrachos"]);
    }

    #[test]
    fn header_bare_words_and_malformed_lines() {
        let p = one("#lapacho-dict 1\n#alternates n:ñ\nzeta 1\nalfa 2\nsolita\n\nbeta x\ngamma 3\n");
        assert_eq!(p.len(), 4, "zeta, alfa, solita, gamma");
        assert_eq!(p.suggest("z", 1), vec!["zeta"]);
        assert_eq!(p.suggest("a", 1), vec!["alfa"]);
        assert!(p.knows("solita"), "a word with no frequency is still a word");
        assert!(!p.knows("#lapacho-dict"));
    }

    #[test]
    fn dictionaries_are_ranked_by_share_not_by_corpus_size() {
        // English's corpus is 100x bigger; "the" and "de" are both half of
        // their own list, so neither buries the other's second word.
        let es = "de 50\ndesde 30\notro 20";
        let en = "the 5000\nthere 3000\ndesk 2000";
        let p = Predictor::new(&[es, en], &[]);
        assert_eq!(p.suggest("des", 2), vec!["desde", "desk"]);
    }

    #[test]
    fn a_word_in_two_dictionaries_is_suggested_once() {
        let p = Predictor::new(&["no 10\nnos 90", "no 95\nnot 5"], &[]);
        assert_eq!(p.len(), 3);
        assert_eq!(p.suggest("n", 3), vec!["no", "nos", "not"]);
    }

    #[test]
    fn folds_the_marks_of_the_other_supported_languages() {
        assert_eq!(fold("Français"), "francais");
        assert_eq!(fold("não"), "nao");
    }
}
