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
 * The dictionary shipped inside the APK. Every other one is *imported* from a
 * file the user picked, so that adding a language never costs the keyboard a
 * network permission (see `docs/DECISIONS.md`, format in
 * `docs/DICTIONARIES.md`).
 */
private const val DICT_ASSET = "dict/es.txt"
private const val BUNDLED_LANG = "es"
/** Where imported dictionaries live: app-private, shared by the IME's process. */
private const val DICT_DIR = "dict"
/** First line of every dictionary: what tells one from any other text file. */
const val DICT_MAGIC = "#lapacho-dict 1"
/**
 * ponytail: ~8.5 MB of native heap per 50 000-word list, all of them loaded
 * in the keyboard's process — hence a cap on how many mix. Raising it is
 * safe once the predictor stores words in one flat buffer.
 */
const val MAX_IMPORTED = 2
/** Ten times the bundled Spanish list: room for a big language, not for a corpus. */
const val MAX_DICT_BYTES = 8 * 1024 * 1024

/**
 * A dictionary's header: the `#` lines at the top of the file.
 * [alternates] maps a key to the characters its long press offers.
 */
data class DictHeader(val lang: String, val name: String, val alternates: Map<String, String>)

/**
 * Dictionaries we publish, by SHA-256. Their files live in
 * `apps/mobile/android/dictionaries/`, and a test fails if a hash here and
 * the committed file drift apart. A dictionary published after this APK is
 * not in the list: it imports as a custom one, with a warning, until the
 * next release.
 */
val OFFICIAL_DICTIONARIES = mapOf(
    "f324ba733e869d8a7b541cf3892d6833fccb0783a7a06813a3f9d8d2a9b39d16" to "en",
)

/** One dictionary the keyboard is using. [file] is null for the bundled one. */
class InstalledDict(val header: DictHeader, val file: File?, val sha256: String?) {
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
            key.length == 1 && key[0] in 'a'..'z' && chars.length in 1..MAX_ALTERNATES,
            R.string.err_alternates_rule, pair, MAX_ALTERNATES,
        )
        key to chars
    }
    return DictHeader(lang, fields["name"] ?: lang, alternates)
}

private val LANG = Regex("[a-z0-9-]{1,32}")
private const val MAX_ALTERNATES = 8

private fun importedDir(context: Context) = File(context.filesDir, DICT_DIR)

private fun importedFiles(context: Context): List<File> =
    importedDir(context).listFiles { f -> f.name.endsWith(".txt") }?.sortedBy { it.name }.orEmpty()

private fun bundledText(context: Context) = context.assets.open(DICT_ASSET).bufferedReader().use { it.readText() }

/** The bundled dictionary first, then the imported ones. */
fun installedDictionaries(context: Context): List<InstalledDict> =
    listOf(InstalledDict(parseHeader(bundledText(context)), null, null)) +
        importedFiles(context).mapNotNull { f ->
            val bytes = f.readBytes()
            // A file that no longer parses (edited by hand, half written) is
            // skipped rather than taking the keyboard down with it.
            runCatching { InstalledDict(parseHeader(String(bytes)), f, sha256(bytes)) }.getOrNull()
        }

/**
 * Changes whenever a dictionary is added, replaced or removed: the IME
 * compares it on every show to know when to reload, and a directory listing
 * is cheaper than rebuilding blindly.
 */
fun dictionarySignature(context: Context): String =
    importedFiles(context).joinToString { "${it.name}@${it.lastModified()}" }

/**
 * All active dictionaries, mixed, plus the words the user taught the keyboard.
 * The engine normalizes each list to its own corpus, so a bigger language
 * does not bury a smaller one.
 */
fun loadPredictor(context: Context, learned: List<String> = emptyList()): WordPredictor =
    WordPredictor(listOf(bundledText(context)) + importedFiles(context).map { it.readText() }, learned)

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
 * header, not the bundled language, some words. Throws with a message for
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
    refuseUnless(header.lang != BUNDLED_LANG, R.string.err_bundled_lang, BUNDLED_LANG)
    refuseUnless(text.lineSequence().any { it.isNotBlank() && !it.trimStart().startsWith("#") }, R.string.err_no_words)
    return PickedDict(header, sha256(bytes), bytes)
}

/**
 * Copies a checked file into the imported dictionaries. Importing a language
 * that is already there replaces it. Throws if [MAX_IMPORTED] are in use.
 */
fun installDictionary(context: Context, picked: PickedDict) {
    val lang = picked.header.lang
    val dir = importedDir(context).apply { mkdirs() }
    val target = File(dir, "$lang.txt")
    refuseUnless(target.exists() || importedFiles(context).size < MAX_IMPORTED, R.string.too_many_imported, MAX_IMPORTED)
    // Written aside and renamed, so the keyboard never reads half a file.
    File(dir, "$lang.tmp").apply { writeBytes(picked.bytes); renameTo(target) }
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
