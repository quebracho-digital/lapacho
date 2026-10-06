//! Local prediction engine (dictionary prefix completion) for Lapacho.
//! Strictly private, offline-only, zero network I/O.
//!
//! It completes the word being typed out of a fixed word list, and that is
//! the whole of what it knows: there is no user model, nothing is written
//! anywhere, and two people with the same dictionary get the same
//! suggestions. Learning a word is a separate, explicit act (see
//! `docs/DECISIONS.md`); when it lands it adds entries to this list, it does
//! not turn this into something that watches you type.

/// One dictionary entry, pointing into [`Predictor::text`]: its key, [`fold`]ed
/// so that a prefix typed without accents still finds the word ("cancion" →
/// "canción"), then the word itself — unless the word *is* its key (no accent,
/// no capital, most of them), which is stored once and marked `word_len: 0`.
/// 16 bytes, against 48 plus two allocations when key and word were boxed.
struct Entry {
    start: u32,
    key_len: u8,
    word_len: u8,
    /// `key`'s length in characters, so a correction can skip words of the
    /// wrong length without decoding them.
    chars: u8,
    freq: u32,
    /// Which letters `key` contains, see [`letter_mask`].
    letters: u32,
}

/// Longer words are dropped: lengths are one byte each. No dictionary word
/// comes near it; a 255-byte line is not a word.
const MAX_WORD: usize = u8::MAX as usize;

pub struct Predictor {
    /// Every key and word, back to back, in `words` order — a prefix scan
    /// reads it front to back.
    text: String,
    /// Sorted by key, which is what [`Predictor::suggest`] binary searches.
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
        let mut p = Predictor { text: String::new(), words: Vec::new() };
        for data in dictionaries {
            let parsed: Vec<(&str, u64)> = data.lines().filter_map(parse_line).collect();
            let total = parsed.iter().map(|(_, f)| f).sum::<u64>().max(1) as f64;
            for (word, freq) in parsed {
                p.push(word, ((freq as f64 / total * SCALE) as u32).max(1));
            }
        }
        for w in learned.iter().map(|w| w.trim()).filter(|w| !w.is_empty()) {
            p.push(w, u32::MAX);
        }
        // Highest frequency first within the same word, so dedup keeps it.
        let Predictor { text, mut words } = p;
        // Bytes, not `&str`: the same order, without the char-boundary checks.
        let bytes = |e: &Entry, from: usize, len: u8| &text.as_bytes()[e.start as usize + from..][..len as usize];
        let word_bytes = |e: &Entry| match e.word_len {
            0 => bytes(e, 0, e.key_len),
            n => bytes(e, e.key_len as usize, n),
        };
        words.sort_unstable_by(|a, b| {
            bytes(a, 0, a.key_len)
                .cmp(bytes(b, 0, b.key_len))
                .then_with(|| word_bytes(a).cmp(word_bytes(b)))
                .then(b.freq.cmp(&a.freq))
        });
        words.dedup_by(|later, first| word(&text, later) == word(&text, first));

        // Rewritten in sorted order: drops what dedup left behind, and a
        // prefix's words end up next to each other.
        let mut sorted = String::with_capacity(words.iter().map(|e| e.key_len as usize + e.word_len as usize).sum());
        for e in &mut words {
            let start = sorted.len() as u32;
            sorted.push_str(&text[e.start as usize..][..e.key_len as usize + e.word_len as usize]);
            e.start = start;
        }
        words.shrink_to_fit();
        Predictor { text: sorted, words }
    }

    fn push(&mut self, w: &str, freq: u32) {
        let k = fold(w);
        if w.len() > MAX_WORD || k.len() > MAX_WORD {
            return;
        }
        let start = self.text.len() as u32;
        self.text.push_str(&k);
        let word_len = if k == w { 0 } else { self.text.push_str(w); w.len() as u8 };
        self.words.push(Entry {
            start,
            key_len: k.len() as u8,
            word_len,
            chars: k.chars().count() as u8,
            freq,
            letters: letter_mask(&k),
        });
    }

    fn key(&self, e: &Entry) -> &str {
        key(&self.text, e)
    }

    fn word(&self, e: &Entry) -> &str {
        word(&self.text, e)
    }

    /// Where the keys starting at or after `key` begin.
    fn find(&self, key: &str) -> usize {
        self.words.partition_point(|e| self.key(e) < key)
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
        self.words.get(self.find(&key)).is_some_and(|e| self.key(e) == key)
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
        let start = self.find(&key);

        let mut best: Vec<&Entry> = Vec::with_capacity(limit + 1);
        for e in self.words[start..].iter().take_while(|e| self.key(e).starts_with(&key)) {
            if self.word(e) == typed {
                continue;
            }
            let at = best.partition_point(|b| b.freq > e.freq);
            if at < limit {
                best.insert(at, e);
                best.truncate(limit);
            }
        }
        best.into_iter().map(|e| apply_case(prefix, self.word(e))).collect()
    }
}

fn key<'a>(text: &'a str, e: &Entry) -> &'a str {
    &text[e.start as usize..][..e.key_len as usize]
}

fn word<'a>(text: &'a str, e: &Entry) -> &'a str {
    match e.word_len {
        0 => key(text, e),
        n => &text[e.start as usize + e.key_len as usize..][..n as usize],
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
        let start = self.find(first_str);
        let range = self.words[start..].iter().take_while(|e| self.key(e).starts_with(first));

        let max = if known.is_some() { 1 } else { 2 };
        let mut hits: Vec<(usize, &Entry)> = Vec::new();
        let mut cand: Vec<char> = Vec::new();
        let mut rows = Rows::default();
        for e in range {
            if self.key(e) == key_str
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
            cand.extend(self.key(e).chars());
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
                let fresh = !seen.contains(&self.key(e));
                seen.push(self.key(e));
                fresh
            })
            .take(limit)
            .map(|(_, e)| apply_case(word, self.word(e)))
            .collect()
    }

    /// The highest frequency among the spellings of a folded key, if any.
    fn freq_of(&self, key: &str) -> Option<u32> {
        self.words[self.find(key)..].iter().take_while(|e| self.key(e) == key).map(|e| e.freq).max()
    }
}

/// Points a swipe and each word's template are resampled to before they are
/// compared, evenly spaced along each line.
const SWIPE_POINTS: usize = 32;

/// How much farther than the nearest key, in key widths, a swipe may start or
/// end from a key for that key to be the word's first or last letter. A
/// finger lands on the edge of a key about as often as on its middle.
const SWIPE_ENDS: f32 = 0.6;

/// The spread of a finger around the line through a word's keys, in key
/// widths. It sets how much a line that drifts away from a word costs,
/// against how much the word's frequency counts: smaller trusts the finger,
/// larger the dictionary. Tuned on synthetic swipes over the 500 commonest
/// Spanish words (`tests/swipe.rs`): 0.1–0.3 all land within a few points;
/// real fingers may want it retuned — it is the one knob here.
const SWIPE_SIGMA: f32 = 0.2;

/// A key under a swipe: its letter and its centre, in the same units as the
/// swipe's points (pixels, usually).
pub type SwipeKey = (char, f32, f32);

impl Predictor {
    /// Words a swipe spelled, best first: `path` is where the finger went,
    /// `keys` where each letter is, `key_width` the distance between two
    /// neighbouring keys' centres — everything is measured in key widths, so
    /// the screen's size and density do not matter.
    ///
    /// SHARK2 (Kristensson & Zhai, 2004), location channel only: a word's
    /// template is the line through its keys' centres, the swipe and the
    /// template are resampled to [`SWIPE_POINTS`], and the word scores by the
    /// mean distance between the two, turned into a likelihood and weighed
    /// against the word's frequency. Candidates are the words whose first and
    /// last letters are near where the swipe started and ended. A doubled
    /// letter is one point: `calle` and `cale` draw the same line, and the
    /// frequency picks.
    ///
    /// ponytail: no shape channel (the line compared after scaling away its
    /// position), which SHARK2 adds for swipes drawn small or off to one side.
    /// Add it if real swipes that pass through the right keys lose to others.
    pub fn swipe(&self, path: &[(f32, f32)], keys: &[SwipeKey], key_width: f32, limit: usize) -> Vec<String> {
        if path.len() < 2 || keys.is_empty() || key_width <= 0.0 || limit == 0 {
            return Vec::new();
        }
        let scale = |x: f32, y: f32| (x / key_width, y / key_width);
        // Each key as typed and folded: a word's letters find their key by
        // the first (ם and מ are two keys) and fall back to the second (ó is
        // drawn through o).
        let keys: Vec<(char, (f32, f32), char)> = keys
            .iter()
            .filter_map(|&(c, x, y)| {
                let raw = c.to_lowercase().next()?;
                Some((fold(&c.to_string()).chars().next()?, scale(x, y), raw))
            })
            .collect();
        let key_of = |c: char| keys.iter().find(|k| k.2 == c).or_else(|| keys.iter().find(|k| k.0 == fold_char(c)));
        let drawn = resample(&path.iter().map(|&(x, y)| scale(x, y)).collect::<Vec<_>>());
        let near = |p: (f32, f32)| -> Vec<char> {
            let nearest = keys.iter().map(|k| dist(k.1, p)).fold(f32::MAX, f32::min);
            let mut near: Vec<char> = keys.iter().filter(|k| dist(k.1, p) <= nearest + SWIPE_ENDS).map(|k| k.0).collect();
            near.dedup();
            near
        };
        let (firsts, lasts) = (near(drawn[0]), near(drawn[SWIPE_POINTS - 1]));

        let mut hits: Vec<(f32, &Entry)> = Vec::new();
        let mut line: Vec<(f32, f32)> = Vec::new();
        for first in firsts {
            let mut buf = [0u8; 4];
            let first_str = first.encode_utf8(&mut buf);
            for e in self.words[self.find(first_str)..].iter().take_while(|e| self.key(e).starts_with(first)) {
                if !self.key(e).chars().last().is_some_and(|c| lasts.contains(&c)) {
                    continue;
                }
                line.clear();
                let mut prev = None;
                let spelled = self.word(e).chars().flat_map(char::to_lowercase).all(|c| {
                    if prev == Some(c) {
                        return true;
                    }
                    prev = Some(c);
                    key_of(c).map(|k| line.push(k.1)).is_some()
                });
                // A word with a letter that is not on the keys, or one a tap types.
                if !spelled || line.len() < 2 {
                    continue;
                }
                let template = resample(&line);
                let d = drawn.iter().zip(&template).map(|(a, b)| dist(*a, *b)).sum::<f32>() / SWIPE_POINTS as f32;
                hits.push((d * d / (2.0 * SWIPE_SIGMA * SWIPE_SIGMA) - (e.freq as f32).ln(), e));
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut seen: Vec<&str> = Vec::new();
        hits.into_iter()
            .filter(|(_, e)| {
                let fresh = !seen.contains(&self.key(e));
                seen.push(self.key(e));
                fresh
            })
            .take(limit)
            .map(|(_, e)| self.word(e).to_string())
            .collect()
    }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// `points` as [`SWIPE_POINTS`] points evenly spaced along the line through
/// them, so that a slow stretch of a swipe weighs no more than a fast one.
fn resample(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let total: f32 = points.windows(2).map(|w| dist(w[0], w[1])).sum();
    let step = total / (SWIPE_POINTS - 1) as f32;
    let mut out = Vec::with_capacity(SWIPE_POINTS);
    out.push(points[0]);
    let (mut walked, mut next) = (0.0, step);
    for w in points.windows(2) {
        let len = dist(w[0], w[1]);
        while step > 0.0 && out.len() < SWIPE_POINTS - 1 && next <= walked + len {
            let t = (next - walked) / len;
            out.push((w[0].0 + t * (w[1].0 - w[0].0), w[0].1 + t * (w[1].1 - w[0].1)));
            next += step;
        }
        walked += len;
    }
    // Rounding can leave the loop one short; the rest is the last point.
    out.resize(SWIPE_POINTS, points[points.len() - 1]);
    out
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
/// Hebrew's five final letters fold to their regular forms (ם → מ): the same
/// letter at the end of a word, and typing the wrong one is a slip like a
/// missing accent.
///
/// ponytail: a table, not Unicode NFD — it covers Spanish, English,
/// Portuguese, French, German, Italian, Russian and Hebrew without niqqud. A language
/// with other marks (Polish, Czech, Arabic harakat) needs
/// `unicode-normalization` here, not more rows.
pub fn fold(s: &str) -> String {
    // Lowercase first, so the table needs only the small letters: Á → á → a.
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ÿ' => 'y',
            'ç' => 'c',
            'ñ' => 'n',
            // Russian writes ё as е more often than not.
            'ё' => 'е',
            'ם' => 'מ',
            'ן' => 'נ',
            'ץ' => 'צ',
            'ף' => 'פ',
            'ך' => 'כ',
            other => other,
        })
        .collect()
}

fn fold_char(c: char) -> char {
    let mut buf = [0u8; 4];
    fold(c.encode_utf8(&mut buf)).chars().next().unwrap_or(c)
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
        assert_eq!(fold("שלום"), "שלומ", "a final letter is its regular form");
        assert_eq!(fold("ÁRBOL"), "arbol", "a capital accent folds too");
        assert_eq!(fold("ЁЛКА"), "елка");
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
    fn a_swipe_over_three_keys_spells_the_word_they_draw() {
        let keys = [('o', 0.0, 0.0), ('s', 1.0, -1.0), ('a', 2.0, 0.0), ('ñ', 1.0, 1.0)];
        let p = Predictor::new(&["osa 5\noa 90\nsa 7\nño 50"], &[]);
        assert_eq!(p.swipe(&[(0.0, 0.0), (1.0, -1.0), (2.0, 0.0)], &keys, 1.0, 1), vec!["osa"]);
        // Straight from o to a: the more frequent word the same ends allow.
        assert_eq!(p.swipe(&[(0.0, 0.0), (2.0, 0.0)], &keys, 1.0, 1), vec!["oa"]);
    }

    #[test]
    fn a_swipe_needs_a_line_and_keys() {
        let p = one(DICT);
        let keys = [('q', 0.0, 0.0), ('e', 1.0, 0.0)];
        assert!(p.swipe(&[(0.0, 0.0)], &keys, 1.0, 3).is_empty(), "a tap, not a swipe");
        assert!(p.swipe(&[(0.0, 0.0), (1.0, 0.0)], &[], 1.0, 3).is_empty());
        assert!(p.swipe(&[(0.0, 0.0), (1.0, 0.0)], &keys, 0.0, 3).is_empty());
        // "que" needs a u, and there is none on these keys.
        assert!(p.swipe(&[(0.0, 0.0), (1.0, 0.0)], &keys, 1.0, 3).is_empty());
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
