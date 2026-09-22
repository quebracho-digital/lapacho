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

impl Predictor {
    /// Builds from `word frequency` lines, one per line, in any order —
    /// they are sorted here, so a mis-sorted asset cannot silently break the
    /// search. Lines that are not exactly two fields are skipped.
    pub fn from_text(data: &str) -> Self {
        let mut words: Vec<Entry> = data
            .lines()
            .filter_map(|line| {
                let (word, freq) = line.split_once(' ')?;
                let word = word.trim();
                if word.is_empty() {
                    return None;
                }
                Some(Entry {
                    key: fold(word).into(),
                    word: word.into(),
                    freq: freq.trim().parse().ok()?,
                })
            })
            .collect();
        words.sort_unstable_by(|a, b| a.key.cmp(&b.key));
        Predictor { words }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
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

/// Lowercase, with Spanish diacritics removed, so that the dictionary can be
/// searched by what is easiest to type.
///
/// ponytail: a table, not Unicode NFD — the bundled dictionary is Spanish and
/// this is every mark it uses. A language with other marks (French, Polish)
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

    const DICT: &str = "que 100\nquerido 80\nquería 90\ncanción 70\ncancion 5\nqué 95\nalto 10";

    #[test]
    fn suggests_most_frequent_first() {
        let p = Predictor::from_text(DICT);
        assert_eq!(p.suggest("quer", 2), vec!["quería", "querido"]);
    }

    #[test]
    fn typed_word_is_not_suggested_but_its_accented_form_is() {
        let p = Predictor::from_text(DICT);
        let hits = p.suggest("que", 5);
        assert!(!hits.contains(&"que".to_string()), "{hits:?}");
        assert_eq!(hits.first().map(String::as_str), Some("qué"));
    }

    #[test]
    fn a_prefix_without_accents_finds_the_accented_word() {
        let p = Predictor::from_text(DICT);
        assert_eq!(p.suggest("canci", 1), vec!["canción"]);
    }

    #[test]
    fn capitalized_prefix_capitalizes_the_suggestion() {
        let p = Predictor::from_text(DICT);
        assert_eq!(p.suggest("Alt", 1), vec!["Alto"]);
    }

    #[test]
    fn empty_prefix_and_unknown_prefix_suggest_nothing() {
        let p = Predictor::from_text(DICT);
        assert!(p.suggest("", 3).is_empty());
        assert!(p.suggest("xyz", 3).is_empty());
    }

    #[test]
    fn unsorted_and_malformed_lines_do_not_break_the_search() {
        let p = Predictor::from_text("zeta 1\nalfa 2\nsin_frecuencia\n\nbeta x\ngamma 3\n");
        assert_eq!(p.len(), 3);
        assert_eq!(p.suggest("z", 1), vec!["zeta"]);
        assert_eq!(p.suggest("a", 1), vec!["alfa"]);
    }
}
