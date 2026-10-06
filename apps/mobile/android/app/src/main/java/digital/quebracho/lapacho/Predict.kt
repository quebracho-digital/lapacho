package digital.quebracho.lapacho

import android.content.Context
import android.net.Uri
import androidx.annotation.StringRes
import digital.quebracho.lapacho.app.R
import uniffi.lapacho_mobile_bridge.WordPredictor
import java.io.File
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.security.MessageDigest

/**
 * The dictionaries shipped inside the APK, as `dict/<lang>.txt` assets: both
 * in use until the user removes one. Every other one is *imported* from a
 * file the user picked, so that adding a language never costs the keyboard a
 * network permission (see `docs/DECISIONS.md`, format in
 * `docs/DICTIONARIES.md`). An imported file of a bundled language replaces
 * the bundled one.
 */
val BUNDLED_LANGUAGES = listOf("es", "en")
/**
 * Where imported dictionaries live: app-private, shared by the IME's process.
 * A removed bundled language leaves a `<lang>.off` file here, so the IME's
 * [dictionarySignature] sees the change like any other.
 */
private const val DICT_DIR = "dict"
/** First line of every dictionary: what tells one from any other text file. */
const val DICT_MAGIC = "#lapacho-dict 1"
/**
 * How many dictionaries mix, bundled or not, all loaded in the keyboard's
 * process: ~1.2 MB of heap per 50 000 words since the flat word buffer (it was
 * ~8.5 MB). Six now costs less than two did; past that, what grows is the wait
 * for the first suggestion, which reads every list.
 */
const val MAX_LANGUAGES = 6
/** Ten times the bundled Spanish list: room for a big language, not for a corpus. */
const val MAX_DICT_BYTES = 8 * 1024 * 1024

/**
 * A dictionary's header: the `#` lines at the top of the file.
 * [alternates] maps a key to the characters its long press offers.
 * [rows] are the letter keys, top row first; null is the keyboard's own
 * QWERTY, to whose middle row [keys] adds letters of the language's own (ñ).
 */
data class DictHeader(
    val lang: String,
    val name: String,
    val alternates: Map<String, String>,
    val rows: List<String>? = null,
    val keys: String = "",
)

/**
 * Dictionaries we publish, by SHA-256. Their files live in
 * `apps/mobile/android/dictionaries/`, and a test fails if a hash here and
 * the committed file drift apart. A dictionary published after this APK is
 * not in the list: it imports as a custom one, with a warning, until the
 * next release.
 */
val OFFICIAL_DICTIONARIES = mapOf(
    "f324ba733e869d8a7b541cf3892d6833fccb0783a7a06813a3f9d8d2a9b39d16" to "en",
    "522d00fd9c12172f6b7960a2c660c6273c6ad3f41c514e8cfb57eb104a3d4207" to "he",
    "23e27d78a7d8b5eb8b40c2553a3ce1368469ca3c9eba02dfa411253464ac76df" to "it",
    "044c637af77b91f99d1724d3365272f620ac5c927d64b3d8108e4105f2ebb084" to "pt-br",
    "b16bb1e20cf8327d8dbf5bd78cebdf5599141db367dbec2ca3e16fb25e99fa55" to "fr",
    "4e2bf1f79b94d43fd44e888623a0f4a843b7de05fc53111b81f75e86c32c8e71" to "de",
    "960d72edb51dd17251d0a45a104e37a6936d21227a00093fcda974b2defec24d" to "ru",
)

/** One dictionary the keyboard is using. [file] is null for the bundled one. */
class InstalledDict(val header: DictHeader, val file: File?, val sha256: String?) {
    val bundled: Boolean get() = file == null
    val official: Boolean get() = file == null || sha256 in OFFICIAL_DICTIONARIES
}

/** A picked file that passed validation and is waiting to be installed. */
class PickedDict(val header: DictHeader, val sha256: String, internal val bytes: ByteArray) {
    val official: Boolean get() = sha256 in OFFICIAL_DICTIONARIES
}

/**
 * Reads a dictionary's header, or throws with a message meant for the user.
 * The format is small on purpose; everything outside it is refused rather
 * than guessed at — this parses a file someone picked, not one we wrote.
 */
fun parseHeader(text: String): DictHeader {
    val lines = text.lineSequence().map(String::trim)
    refuseUnless(lines.firstOrNull() == DICT_MAGIC, R.string.err_not_dict, DICT_MAGIC)
    val fields = lines.drop(1).takeWhile { it.startsWith("#") }
        .mapNotNull { it.removePrefix("#").split(Regex("\\s+"), limit = 2).takeIf { f -> f.size == 2 } }
        .associate { (k, v) -> k to v.trim() }
    val lang = fields["lang"].orEmpty()
    // It becomes a file name: nothing that can climb out of the directory.
    refuseUnless(LANG.matches(lang), R.string.err_lang)
    val alternates = fields["alternates"].orEmpty().split(Regex("\\s+")).filter(String::isNotEmpty).associate { pair ->
        val (key, chars) = pair.split(":", limit = 2).takeIf { it.size == 2 }
            ?: throw UserError(R.string.err_alternates_format, pair)
        refuseUnless(
            key.length == 1 && key[0].isLetter() && key[0].lowercaseChar() == key[0] && chars.length in 1..MAX_ALTERNATES,
            R.string.err_alternates_rule, pair, MAX_ALTERNATES,
        )
        key to chars
    }
    val rows = fields["rows"]?.split(Regex("\\s+"))?.filter(String::isNotEmpty)?.also { rows ->
        val keys = rows.joinToString("")
        refuseUnless(
            rows.size in 2..MAX_ROWS && rows.all { it.length <= MAX_ROW_KEYS } &&
                keys.all { it.isLetter() && it.lowercaseChar() == it } && keys.toSet().size == keys.length,
            R.string.err_rows, MAX_ROWS, MAX_ROW_KEYS,
        )
    }
    val keys = fields["keys"].orEmpty()
    refuseUnless(
        keys.length <= MAX_EXTRA_KEYS && keys.all { it.isLetter() && it.lowercaseChar() == it } && keys.toSet().size == keys.length,
        R.string.err_keys, MAX_EXTRA_KEYS,
    )
    return DictHeader(lang, fields["name"] ?: lang, alternates, rows, keys)
}

/** Letters a language may add to the built-in middle row: it holds 12 at most. */
private const val MAX_EXTRA_KEYS = 3

/**
 * The keyboard's own QWERTY, for dictionaries without `#rows`: the middle
 * row ends in what their `#keys` add, each once — ñ after l, as every Spanish
 * keyboard has it, and only while a dictionary that wants it is active.
 */
fun builtInRows(dicts: List<DictHeader>): List<String> =
    listOf("qwertyuiop", "asdfghjkl" + dicts.joinToString("") { it.keys }.toList().distinct().joinToString(""), "zxcvbnm")

private val LANG = Regex("[a-z0-9-]{1,32}")
private const val MAX_ALTERNATES = 8
/** Letter rows a layout may have, and keys per row: what fits a phone's width. */
private const val MAX_ROWS = 4
private const val MAX_ROW_KEYS = 12

/**
 * The active dictionaries grouped by the letter rows they type on, in the
 * order they come (so the first dictionary's layout is the first): Spanish,
 * English and Portuguese share QWERTY and stay mixed, a French AZERTY is a
 * second layout. What the keyboard switches between is these, not languages.
 */
fun <T> layouts(dicts: List<T>, header: (T) -> DictHeader): List<Pair<List<String>?, List<T>>> =
    dicts.groupBy { header(it).rows }.toList()

private fun importedDir(context: Context) = File(context.filesDir, DICT_DIR)

private fun importedFiles(context: Context): List<File> =
    importedDir(context).listFiles { f -> f.name.endsWith(".txt") }?.sortedBy { it.name }.orEmpty()

private fun removedMark(context: Context, lang: String) = File(importedDir(context), "$lang.off")

private fun bundledText(context: Context, lang: String) =
    context.assets.open("dict/$lang.txt").bufferedReader().use { it.readText() }

/** Just the `#` lines: the header, without reading 600 KB of words for it. */
private fun bundledHeader(context: Context, lang: String): DictHeader =
    context.assets.open("dict/$lang.txt").bufferedReader().useLines { lines ->
        parseHeader(lines.takeWhile { it.startsWith("#") }.joinToString("\n"))
    }

/** Bundled languages in use: not removed, and not replaced by an imported file. */
fun activeBundled(bundled: List<String>, removed: Set<String>, imported: Set<String>): List<String> =
    bundled.filter { it !in removed && it !in imported }

/** The bundled dictionaries in use first, then the imported ones. */
fun installedDictionaries(context: Context): List<InstalledDict> {
    val imported = importedFiles(context).mapNotNull { f ->
        val bytes = f.readBytes()
        // A file that no longer parses (edited by hand, half written) is
        // skipped rather than taking the keyboard down with it.
        runCatching { InstalledDict(parseHeader(String(bytes)), f, sha256(bytes)) }.getOrNull()
    }
    val removed = BUNDLED_LANGUAGES.filter { removedMark(context, it).exists() }.toSet()
    return activeBundled(BUNDLED_LANGUAGES, removed, imported.map { it.header.lang }.toSet())
        .map { InstalledDict(bundledHeader(context, it), null, null) } + imported
}

/** Bundled languages the user removed, which come back without a file. */
fun removedBundled(context: Context): List<DictHeader> =
    BUNDLED_LANGUAGES.filter { removedMark(context, it).exists() }.map { bundledHeader(context, it) }

/**
 * Stops using [dict]. A bundled language is marked removed too, so that
 * removing an imported copy of it does not quietly bring the bundled one
 * back. The last language stays: with none, the strip would never suggest.
 */
fun removeDictionary(context: Context, dict: InstalledDict) {
    refuseUnless(installedDictionaries(context).size > 1, R.string.keep_one_language)
    dict.file?.delete()
    if (dict.header.lang in BUNDLED_LANGUAGES) {
        importedDir(context).mkdirs()
        removedMark(context, dict.header.lang).createNewFile()
    }
}

/** Brings back a bundled language the user removed. */
fun restoreBundled(context: Context, lang: String) {
    refuseUnless(installedDictionaries(context).size < MAX_LANGUAGES, R.string.too_many_languages, MAX_LANGUAGES)
    removedMark(context, lang).delete()
}

/**
 * Changes whenever a dictionary is added, replaced or removed: the IME
 * compares it on every show to know when to reload, and a directory listing
 * is cheaper than rebuilding blindly.
 */
fun dictionarySignature(context: Context): String =
    importedDir(context).listFiles()?.sortedBy { it.name }?.joinToString { "${it.name}@${it.lastModified()}" }.orEmpty()

/**
 * All active dictionaries, mixed, plus the words the user taught the keyboard.
 * The engine normalizes each list to its own corpus, so a bigger language
 * does not bury a smaller one.
 */
fun loadPredictor(
    context: Context,
    learned: List<String> = emptyList(),
    dicts: List<InstalledDict> = installedDictionaries(context),
): WordPredictor =
    WordPredictor(
        dicts.map { d -> d.file?.readText() ?: bundledText(context, d.header.lang) },
        learned,
    )

/**
 * Long-press alternates from every active dictionary's header, on top of
 * [base] (what the keyboard offers in any language). A key's characters are
 * merged in order, each once: `n` gets ñ from Spanish however many lists
 * name it.
 */
fun keyAlternates(dicts: List<DictHeader>, base: Map<String, String>): Map<String, String> {
    val merged = base.toMutableMap()
    for (d in dicts) for ((key, chars) in d.alternates) {
        merged[key] = ((merged[key] ?: "") + chars).toList().distinct().joinToString("")
    }
    return merged
}

/**
 * Reads and checks a picked file: UTF-8, under [MAX_DICT_BYTES], a valid
 * header, some words. A bundled language is accepted: it replaces the
 * bundled list (a newer official one, or a custom variant). Throws with a message for
 * the user. Nothing is written — the caller decides whether a file that is
 * not [PickedDict.official] goes in, see [installDictionary].
 *
 * The format check applies to every file, official or not: it is what keeps
 * the parser to building a word list. The hash only says who made the list.
 */
fun readDictionary(context: Context, uri: Uri): PickedDict {
    // Capped read: a picked file can be anything, including gigabytes.
    // (`readNBytes` would do this, but it is API 33.)
    val bytes = context.contentResolver.openInputStream(uri)?.use { input ->
        val out = java.io.ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        while (out.size() <= MAX_DICT_BYTES) {
            val n = input.read(buf)
            if (n < 0) break
            out.write(buf, 0, n)
        }
        out.toByteArray()
    } ?: throw UserError(R.string.err_cant_read)
    refuseUnless(bytes.size <= MAX_DICT_BYTES, R.string.err_too_big, MAX_DICT_BYTES / 1024 / 1024)
    val text = try {
        Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes)).toString()
    } catch (e: java.nio.charset.CharacterCodingException) {
        throw UserError(R.string.err_not_utf8)
    }
    val header = parseHeader(text)
    refuseUnless(text.lineSequence().any { it.isNotBlank() && !it.trimStart().startsWith("#") }, R.string.err_no_words)
    return PickedDict(header, sha256(bytes), bytes)
}

/**
 * Copies a checked file into the imported dictionaries. Importing a language
 * that is already there replaces it, and brings back a bundled one that was
 * removed. Throws if [MAX_LANGUAGES] are in use.
 */
fun installDictionary(context: Context, picked: PickedDict) {
    val lang = picked.header.lang
    val dir = importedDir(context).apply { mkdirs() }
    val target = File(dir, "$lang.txt")
    val active = installedDictionaries(context).map { it.header.lang }
    refuseUnless(lang in active || active.size < MAX_LANGUAGES, R.string.too_many_languages, MAX_LANGUAGES)
    // Written aside and renamed, so the keyboard never reads half a file.
    File(dir, "$lang.tmp").apply { writeBytes(picked.bytes); renameTo(target) }
    removedMark(context, lang).delete()
}

/**
 * A refusal meant for the user, carried as a string resource so the screen
 * that shows it picks the language: the parser has no Context to ask.
 */
class UserError(@StringRes val id: Int, vararg val args: Any) : IllegalArgumentException()

private fun refuseUnless(ok: Boolean, @StringRes id: Int, vararg args: Any) {
    if (!ok) throw UserError(id, *args)
}

private fun sha256(bytes: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

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
