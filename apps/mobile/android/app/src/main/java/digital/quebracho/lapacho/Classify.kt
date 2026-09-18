package digital.quebracho.lapacho

import android.util.Log
import digital.quebracho.lapacho.storage.ClipboardItem
import digital.quebracho.lapacho.storage.HistoryRepo
import digital.quebracho.lapacho.storage.Sensitivity
import uniffi.lapacho_mobile_bridge.classifySensitivity

/** lapacho-core's classifier (Rust, the same one desktop runs) on the Kotlin enum. */
fun classify(text: String): Sensitivity = Sensitivity.valueOf(classifySensitivity(text).uppercase())

/** Credentials and secrets: the IME never stores them, and nothing shows them. */
fun Sensitivity.isSecret(): Boolean = this == Sensitivity.CREDENTIAL || this == Sensitivity.SECRET

/**
 * Shown as 🔑 •••••• and left out of search. Re-classifying covers rows stored
 * before the classifier existed.
 */
fun ClipboardItem.isMasked(): Boolean = sensitivity.isSecret() || classify(displayContent).isSecret()

/**
 * One-time purge of secrets captured before the classifier existed (up to
 * 0.1.6): they were stored as [Sensitivity.NONE] and newer builds only masked
 * them on display, so a copied password stayed on the phone. Rows saved as
 * secrets on purpose (the app's GUARDAR) carry their sensitivity and are kept.
 * Scans the whole table: history had no size cap before 0.1.8.
 */
fun HistoryRepo.purgeUnclassifiedSecrets() {
    if (getPreference(PURGED_PREF) != null) return
    val stale = load(limit = Int.MAX_VALUE)
        .filter { it.sensitivity == Sensitivity.NONE && classify(it.displayContent).isSecret() }
    stale.forEach { delete(it.id) }
    setPreference(PURGED_PREF, "1")
    Log.i("Lapacho", "purged ${stale.size} secret(s) stored before the classifier")
}

private const val PURGED_PREF = "unclassified_secrets_purged"

/** ClipDescription.EXTRA_IS_SENSITIVE (API 33) as a plain key, readable and writable on any version. */
const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
