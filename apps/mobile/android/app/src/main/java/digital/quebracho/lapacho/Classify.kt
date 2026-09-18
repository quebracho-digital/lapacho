package digital.quebracho.lapacho

import digital.quebracho.lapacho.storage.ClipboardItem
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

/** ClipDescription.EXTRA_IS_SENSITIVE (API 33) as a plain key, readable and writable on any version. */
const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
