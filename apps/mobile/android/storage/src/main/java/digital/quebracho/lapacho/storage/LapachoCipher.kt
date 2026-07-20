package digital.quebracho.lapacho.storage

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * AES-256-GCM at rest, key held in the Android Keystore (hardware-backed
 * when available, never exportable) — the mobile equivalent of desktop's
 * `crypto::Cipher` (crates/lapacho-core/src/crypto.rs), which wraps a
 * `SecretKey` from the OS keyring the same way.
 *
 * One key alias per app install; both the companion and IME process open the
 * *same* Keystore entry (shared by app UID), same as they share the SQLite
 * file — see docs/ARQUITECTURA_MOBILE_ANDROID.md §4.3.
 */
class LapachoCipher(private val alias: String = "lapacho_history_key") {

    private val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    private fun keyOrCreate(): SecretKey {
        (keyStore.getKey(alias, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    /** Encrypts to `base64(iv || ciphertext || tag)`. */
    fun encrypt(plaintext: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, keyOrCreate())
        val iv = cipher.iv
        val ct = cipher.doFinal(plaintext.toByteArray(Charsets.UTF_8))
        return b64Encode(iv + ct)
    }

    fun decrypt(blob: String): String {
        val bytes = b64Decode(blob)
        val iv = bytes.copyOfRange(0, IV_LEN)
        val ct = bytes.copyOfRange(IV_LEN, bytes.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, keyOrCreate(), GCMParameterSpec(TAG_BITS, iv))
        return String(cipher.doFinal(ct), Charsets.UTF_8)
    }

    /**
     * Deletes the Keystore key. The "wipe" gesture for mobile: rotate the key
     * so every ciphertext row becomes unreadable garbage, instead of DELETEing
     * rows one by one (same rationale as the debate doc's verdict on wipe =
     * key rotation, not row deletion).
     */
    fun wipeKey() {
        if (keyStore.containsAlias(alias)) keyStore.deleteEntry(alias)
    }

    companion object {
        private const val IV_LEN = 12
        private const val TAG_BITS = 128

        private fun b64Encode(bytes: ByteArray): String =
            android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP)

        private fun b64Decode(s: String): ByteArray =
            android.util.Base64.decode(s, android.util.Base64.NO_WRAP)
    }
}

/**
 * Content-identity hash for dedup ("re-copying moves to top instead of
 * duplicating" — same rule as desktop's `crypto::content_id`).
 *
 * ponytail: plain SHA-256, not the keyed hash desktop uses. Good enough for
 * the P0 spike (single device, no cross-device dedup yet); becomes a real gap
 * once P1 wires the uniffi bridge to `lapacho-core`, at which point this
 * function should be *replaced* by calling into Rust so mobile and desktop
 * compute the exact same id for the same content (required for sync dedup in
 * P4 — see debate doc's "same ID scheme for everything" verdict).
 */
fun contentId(raw: String): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(raw.toByteArray(Charsets.UTF_8))
    return digest.joinToString("") { "%02x".format(it) }
}
