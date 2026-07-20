package digital.quebracho.lapacho.storage

/** Mirrors `lapacho_core::types::Sensitivity`. Keep names in sync until the
 * uniffi bridge (P1) makes this the same enum on both sides. */
enum class Sensitivity {
    NONE,
    PERSONAL,
    CREDENTIAL,
    SECRET,
}

/** Mirrors `lapacho_core::types::PersistLevel`. */
enum class PersistLevel {
    NONE,
    SENSITIVE,
    ALL,
}

/**
 * Mobile counterpart of `lapacho_core::types::ClipboardItem`. Until the P1
 * uniffi bridge lands, this is a hand-kept mirror — same shape, same field
 * names where they overlap.
 *
 * [syncEligible] / [syncState] are P0 schema-foresight placeholders (per
 * docs/ARQUITECTURA_MOBILE_ANDROID.md §9 P0.5): unused until `lapacho-sync`
 * (P4) exists, but present now so P4 doesn't need a migration.
 */
data class ClipboardItem(
    val id: String,
    val rawContent: String,
    val displayContent: String,
    val contentType: String,
    val sensitivity: Sensitivity,
    val detectedType: String,
    val timestamp: Long,
    val thumbnail: String? = null,
    val size: Long? = null,
    val syncEligible: Boolean = false,
    val syncState: String? = null,
)
