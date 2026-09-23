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
    /// `key`'s length in characters, so a correction can skip words of the
    /// wrong length without decoding them. Sits in the struct's padding.
    chars: u8,
    /// Which letters `key` contains, see [`letter_mask`].
    letters: u32,
}

impl Entry {
    fn new(word: &str, freq: u32) -> Self {
        let key = fold(word);
        let chars = key.chars().count().min(u8::MAX as usize) as u8;
        let letters = letter_mask(&key);
        Entry { key: key.into(), word: word.into(), freq, chars, letters }
    }
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
            words.extend(parsed.into_iter().map(|(word, freq)| {
                Entry::new(word, ((freq as f64 / total * SCALE) as u32).max(1))
            }));
        }
        words.extend(learned.iter().filter(|w| !w.trim().is_empty()).map(|w| Entry::new(w.trim(), u32::MAX)));
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

/// A known word is only corrected when a neighbour one edit away is this many
/// times more frequent. The lists come from subtitles and carry their typos
/// (`qeu` is in the Spanish one, 74 000 times rarer than `que`), but a real
/// word next to a common one must be left alone: `perro`/`pero` is ×37,
/// `vaca`/`vaya` ×18, `nadia`/`nada` ×325.
const KNOWN_TYPO_RATIO: u64 = 1000;

/// Shorter than this, nearly every word is one edit from dozens of others,
/// and a correction is a guess.
const MIN_CORRECT: usize = 3;

impl Predictor {
    /// Words the user probably meant by `word`, best first — for a word that
    /// is misspelled, not unfinished (that is [`Predictor::suggest`]).
    ///
    /// Candidates are one edit away (insertion, deletion, substitution, or
    /// two letters swapped — the commonest slip on a phone), or two if
    /// nothing is one away; ranked by distance, then frequency. Accents are
    /// ignored the way the rest of the engine ignores them, so the accented
    /// spelling of a typed word is a completion, not a correction.
    ///
    /// A word the dictionary knows is only corrected towards a far more
    /// frequent neighbour ([`KNOWN_TYPO_RATIO`]), and then at one edit only.
    ///
    /// ponytail: only words sharing the first letter are scanned — a first
    /// letter is rarely the one mistyped, and it cuts the scan ~20×; within
    /// it, [`letter_mask`] drops most words before any table is built.
    /// Measured at 0.03–1.3 ms on the laptop against 84 000 words (es + en). Two words run
    /// together (`porfavor`) are not split. If either matters, a deletion
    /// index (SymSpell) is the upgrade, at several MB of memory.
    pub fn correct(&self, word: &str, limit: usize) -> Vec<String> {
        let key: Vec<char> = fold(word).chars().collect();
        let Some(&first) = key.first() else { return Vec::new() };
        if key.len() < MIN_CORRECT || limit == 0 {
            return Vec::new();
        }
        let key_str: String = key.iter().collect();
        let letters = letter_mask(&key_str);
        let known = self.freq_of(&key_str);

        // Every key starting with the same letter.
        let mut start_buf = [0u8; 4];
        let first_str = first.encode_utf8(&mut start_buf);
        let start = self.words.partition_point(|e| *e.key < *first_str);
        let range = self.words[start..].iter().take_while(|e| e.key.starts_with(first));

        let max = if known.is_some() { 1 } else { 2 };
        let mut hits: Vec<(usize, &Entry)> = Vec::new();
        let mut cand: Vec<char> = Vec::new();
        let mut rows = Rows::default();
        for e in range {
            if *e.key == *key_str
                || (e.chars as usize).abs_diff(key.len()) > max
                || (e.letters ^ letters).count_ones() as usize > 2 * max
            {
                continue;
            }
            if let Some(typed) = known
                && (e.freq as u64) < typed as u64 * KNOWN_TYPO_RATIO
            {
                continue;
            }
            cand.clear();
            cand.extend(e.key.chars());
            if let Some(d) = rows.distance(&key, &cand, max) {
                hits.push((d, e));
            }
        }
        // Nothing one edit away is what earns a look two away.
        if let Some(best) = hits.iter().map(|h| h.0).min() {
            hits.retain(|h| h.0 == best);
        }
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.freq.cmp(&a.1.freq)));
        // One spelling per key: "tambien" and "también" are the same answer.
        let mut seen: Vec<&str> = Vec::new();
        hits.into_iter()
            .filter(|(_, e)| {
                let fresh = !seen.contains(&&*e.key);
                seen.push(&e.key);
                fresh
            })
            .take(limit)
            .map(|(_, e)| apply_case(word, &e.word))
            .collect()
    }

    /// The highest frequency among the spellings of a folded key, if any.
    fn freq_of(&self, key: &str) -> Option<u32> {
        let start = self.words.partition_point(|e| *e.key < *key);
        self.words[start..].iter().take_while(|e| *e.key == *key).map(|e| e.freq).max()
    }
}

/// The set of letters in `key`, one bit each (`char mod 32`, so every
/// alphabet folds into the same 32 bits).
///
/// It bounds the edit distance from below, which is what makes a correction
/// cheap: one edit changes at most two bits of the set (a substitution drops
/// one letter and adds another; a swap changes none), so two words whose
/// masks differ in more than `2 × max` bits cannot be `max` edits apart and
/// are skipped without building a table. Two letters sharing a bit only make
/// masks look closer, never farther, so the bound stays true. Measured: most
/// of a first-letter range is dropped by this alone.
fn letter_mask(key: &str) -> u32 {
    key.chars().fold(0, |m, c| m | 1 << (c as u32 % 32))
}

/// The three rows of the distance table, kept between candidates: a scan
/// compares thousands of words, and allocating per word was most of its time.
#[derive(Default)]
struct Rows {
    before: Vec<usize>,
    prev: Vec<usize>,
    cur: Vec<usize>,
}

impl Rows {
    /// Optimal string alignment distance between `a` and `b` — Levenshtein
    /// plus a swap of two adjacent letters counted as one edit — or `None` as
    /// soon as it is certain to exceed `max`.
    fn distance(&mut self, a: &[char], b: &[char], max: usize) -> Option<usize> {
        if a.len().abs_diff(b.len()) > max {
            return None;
        }
        let n = b.len() + 1;
        for row in [&mut self.before, &mut self.prev, &mut self.cur] {
            row.clear();
            row.resize(n, 0);
        }
        // `before` is the row a swap looks back to.
        for (j, v) in self.prev.iter_mut().enumerate() {
            *v = j;
        }
        for i in 1..=a.len() {
            self.cur[0] = i;
            let mut row_min = i;
            for j in 1..=b.len() {
                let cost = usize::from(a[i - 1] != b[j - 1]);
                let mut d = (self.prev[j - 1] + cost).min(self.prev[j] + 1).min(self.cur[j - 1] + 1);
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    d = d.min(self.before[j - 2] + 1);
                }
                self.cur[j] = d;
                row_min = row_min.min(d);
            }
            if row_min > max {
                return None;
            }
            std::mem::swap(&mut self.before, &mut self.prev);
            std::mem::swap(&mut self.prev, &mut self.cur);
        }
        Some(self.prev[b.len()]).filter(|&d| d <= max)
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

    #[test]
    fn a_swap_of_two_letters_is_one_edit() {
        // grasas is two substitutions away; gracias is two as plain
        // Levenshtein too — and would lose to the more frequent grasas.
        let p = Predictor::new(&["gracias 90\ngrasas 95"], &[]);
        assert_eq!(p.correct("graicas", 1), vec!["gracias"]);
    }

    #[test]
    fn nearer_beats_more_frequent_and_frequency_breaks_ties() {
        // cuando (a swap) and cuadro (a substitution) are one edit away;
        // cuadrado, two, is dropped however common.
        let p = Predictor::new(&["cuando 1000\ncuadro 10\ncuadrado 5000"], &[]);
        assert_eq!(p.correct("cuadno", 3), vec!["cuando", "cuadro"]);
    }

    #[test]
    fn two_edits_only_when_nothing_is_one_away() {
        let p = Predictor::new(&["necesito 10\nnecesita 5"], &[]);
        assert_eq!(p.correct("nesesito", 2), vec!["necesito"]);
        assert_eq!(p.correct("nescesito", 2), vec!["necesito"], "two edits, nothing at one");
        assert_eq!(p.correct("nxcxsxto", 2), Vec::<String>::new(), "three edits");
    }

    #[test]
    fn a_known_word_is_corrected_only_towards_a_far_more_common_one() {
        let p = Predictor::new(&["que 1000000\nqeu 1\npero 37\nperro 1"], &[]);
        assert_eq!(p.correct("qeu", 1), vec!["que"], "a typo the corpus kept");
        assert!(p.correct("perro", 1).is_empty(), "a real word next to a common one");
    }

    #[test]
    fn keeps_the_capital_and_gives_one_spelling_per_word() {
        let p = Predictor::new(&["también 90\ntambien 10"], &[]);
        assert_eq!(p.correct("Tambein", 3), vec!["También"]);
    }

    #[test]
    fn short_words_and_learned_ones_are_left_alone() {
        let p = Predictor::new(&["de 100\nte 90\nquebrados 50"], &["quebrachos"]);
        assert!(p.correct("dr", 3).is_empty());
        assert!(p.correct("quebrachos", 3).is_empty());
    }

    #[test]
    fn the_letter_mask_never_rules_out_a_real_neighbour() {
        // Each edit moves the mask by at most two bits; a swap by none.
        for (a, b, max) in [("cuadno", "cuando", 1), ("graicas", "gracias", 1), ("maniana", "manana", 1),
            ("resivir", "recibir", 2), ("teh", "the", 1), ("ab", "xy", 2)]
        {
            let d = Rows::default().distance(&a.chars().collect::<Vec<_>>(), &b.chars().collect::<Vec<_>>(), max);
            assert!(d.is_some(), "{a}/{b}");
            assert!((letter_mask(a) ^ letter_mask(b)).count_ones() as usize <= 2 * max, "{a}/{b}");
        }
    }
}
