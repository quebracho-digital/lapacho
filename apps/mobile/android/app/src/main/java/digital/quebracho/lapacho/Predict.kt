package digital.quebracho.lapacho

import android.content.Context
import uniffi.lapacho_mobile_bridge.WordPredictor

/**
 * The dictionary shipped inside the APK. One language for now; the rest are
 * meant to be *loaded* rather than compiled in, so that adding a language
 * never costs the keyboard a network permission (see `docs/DECISIONS.md`).
 */
private const val DICT_ASSET = "dict/es.txt"

/**
 * Reads the bundled dictionary into `lapacho-predict`, plus the words the
 * user taught it. A learned word is just another entry with a frequency
 * nothing can outrank: it was asked for by name, so it comes first.
 */
fun loadPredictor(context: Context, learned: List<String> = emptyList()): WordPredictor {
    val dictionary = context.assets.open(DICT_ASSET).bufferedReader().use { it.readText() }
    val lexicon = learned.joinToString("") { "\n$it ${UInt.MAX_VALUE}" }
    return WordPredictor(dictionary + lexicon)
}

/**
 * The word being typed: the run of letters that ends at the cursor. Empty
 * right after a space or a punctuation mark — there is nothing to complete
 * there, and that is what puts the paste strip back on screen.
 *
 * Taken from the field rather than from a buffer of our own: the cursor moves,
 * and the app that owns the field can rewrite it while we hold focus.
 */
fun currentWord(before: CharSequence?): String {
    val s = before ?: return ""
    var i = s.length
    while (i > 0 && s[i - 1].isLetter()) i--
    return s.subSequence(i, s.length).toString()
}
