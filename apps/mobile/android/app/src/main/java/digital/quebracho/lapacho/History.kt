package digital.quebracho.lapacho

import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import java.io.File
import java.io.RandomAccessFile
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.lapacho_mobile_bridge.MobileCore
import uniffi.lapacho_mobile_bridge.MobileException
import uniffi.lapacho_mobile_bridge.MobileItem
import uniffi.lapacho_mobile_bridge.generateMasterKey

/**
 * How many items the history keeps: everything that search can reach. Trimmed
 * after every write, so nothing sits on the phone out of reach.
 */
const val HISTORY_MAX = 100

/** lapacho-core's `Sensitivity`, as the bridge spells it. */
enum class Sensitivity { NONE, PERSONAL, CREDENTIAL, SECRET }

/** An item as the lists show it. Its raw content stays in the store until pasted ([History.raw]). */
data class ClipboardItem(
    val id: String,
    val displayContent: String,
    val sensitivity: Sensitivity,
    val timestamp: Long,
)

/**
 * The history, the learned words and the keyboard's preferences: lapacho-core's
 * store (Rust, the same code desktop runs), shared by the companion and the
 * IME. Both are processes of this app's UID, so they open the same file in
 * `filesDir`; SQLite's WAL lets them.
 *
 * What is stored goes in as `All` with no TTL: the phone never stores
 * credentials or secrets in the first place (see LapachoIme.capture).
 */
class History(context: Context) {
    private val core: MobileCore

    init {
        val app = context.applicationContext
        // One process sets up the key and imports the old store; the other waits.
        RandomAccessFile(File(app.filesDir, "history.lock"), "rw").channel.use { lock ->
            lock.lock().use {
                core = MobileCore(File(app.filesDir, DB_NAME).path, MasterKey.get(app))
                importKotlinStore(app)
            }
        }
    }

    fun save(text: String) {
        core.ingestText(text, "All", null)
        core.trim(HISTORY_MAX.toUInt())
    }

    fun recent(limit: Int): List<ClipboardItem> = core.getRecentItems(limit.toUInt()).map { it.toItem() }

    /** What to paste or hand to a plugin; null when the item is gone meanwhile. */
    fun raw(item: ClipboardItem): String? =
        try {
            core.getRawContent(item.id)
        } catch (_: MobileException.NotFound) {
            null
        }

    fun contentId(text: String): String = core.contentId(text)

    fun clear() = core.clear()

    fun learn(word: String) = core.learn(word)

    fun lexicon(): List<String> = core.lexicon()

    fun forget(word: String) = core.forget(word)

    fun forgetAll() = core.forgetAll()

    fun getPreference(key: String): String? = core.getPreference(key)

    fun setPreference(key: String, value: String) = core.setPreference(key, value)

    /**
     * Up to 0.1.42 the history lived in a store written in Kotlin, with each
     * row encrypted by the Keystore key itself. Moves its items (with their
     * times), learned words and preferences here once, then deletes it.
     */
    private fun importKotlinStore(context: Context) {
        val old = context.getDatabasePath(OLD_DB_NAME)
        if (!old.exists()) return
        try {
            SQLiteDatabase.openDatabase(old.path, null, SQLiteDatabase.OPEN_READONLY).use { db ->
                db.rawQuery("SELECT raw_content, timestamp FROM history ORDER BY timestamp", null).use { c ->
                    while (c.moveToNext()) {
                        val raw = runCatching { MasterKey.unwrap(c.getString(0)) }.getOrNull() ?: continue
                        core.ingestText(raw, "All", c.getLong(1).toULong())
                    }
                }
                runCatching {
                    db.rawQuery("SELECT word FROM lexicon ORDER BY added", null).use { c ->
                        while (c.moveToNext()) {
                            runCatching { MasterKey.unwrap(c.getString(0)) }.getOrNull()?.let { core.learn(it) }
                        }
                    }
                }
                db.rawQuery("SELECT key, value FROM settings", null).use { c ->
                    while (c.moveToNext()) core.setPreference(c.getString(0), c.getString(1))
                }
            }
            core.trim(HISTORY_MAX.toUInt())
            Log.i(TAG, "imported the Kotlin store")
        } catch (e: Exception) {
            // Better a fresh history than a keyboard that can't open.
            Log.w(TAG, "could not import the Kotlin store", e)
        }
        context.deleteDatabase(OLD_DB_NAME)
    }

    private fun MobileItem.toItem() = ClipboardItem(
        id = id,
        displayContent = displayContent,
        sensitivity = Sensitivity.valueOf(sensitivity.uppercase()),
        timestamp = timestamp.toLong(),
    )

    private companion object {
        const val TAG = "LapachoHistory"
        const val DB_NAME = "history.db"
        const val OLD_DB_NAME = "lapacho_history.db"
    }
}

/**
 * The history's master key, wrapped by an AES-GCM key in the Android Keystore
 * (hardware-backed when there is one, never exportable). lapacho-core needs the
 * key's bytes, which a Keystore key never gives out, so the Keystore key guards
 * them instead: without it the wrapped file can't be opened, and neither can
 * the history.
 */
private object MasterKey {
    private const val ALIAS = "lapacho_history_key"
    private const val FILE = "master.key"
    private const val IV_LEN = 12
    private const val TAG_BITS = 128

    /** The key, made and wrapped the first time. Callers hold History's lock. */
    fun get(context: Context): String {
        val file = File(context.filesDir, FILE)
        if (file.exists()) return unwrap(file.readText())
        val key = generateMasterKey()
        val tmp = File(context.filesDir, "$FILE.tmp")
        tmp.writeText(wrap(key))
        check(tmp.renameTo(file)) { "could not save the master key" }
        return key
    }

    private fun keystoreKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    /** `base64(iv || ciphertext || tag)`, the format the Kotlin store used for its rows too. */
    private fun wrap(plaintext: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, keystoreKey())
        return Base64.encodeToString(cipher.iv + cipher.doFinal(plaintext.toByteArray()), Base64.NO_WRAP)
    }

    fun unwrap(blob: String): String {
        val bytes = Base64.decode(blob, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(TAG_BITS, bytes, 0, IV_LEN))
        return String(cipher.doFinal(bytes, IV_LEN, bytes.size - IV_LEN))
    }
}
