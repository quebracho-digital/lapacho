package digital.quebracho.lapacho

import android.content.Context
import uniffi.lapacho_mobile_bridge.WordPredictor

/**
 * The dictionary shipped inside the APK. One language for now; the rest are
 * meant to be *loaded* rather than compiled in, so that adding a language
 * never costs the keyboard a network permission (see `docs/DECISIONS.md`).
 */
private const val DICT_ASSET = "dict/es.txt"

/** Reads the bundled dictionary into `lapacho-predict`. Costs ~600 KB of asset. */
fun loadPredictor(context: Context): WordPredictor =
    WordPredictor(context.assets.open(DICT_ASSET).bufferedReader().use { it.readText() })

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
