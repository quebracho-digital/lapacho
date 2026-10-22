package digital.quebracho.lapacho

import android.content.Context
import android.net.Uri
import uniffi.lapacho_mobile_bridge.MobileException
import uniffi.lapacho_mobile_bridge.SkinInfo
import uniffi.lapacho_mobile_bridge.parseSkin
import java.io.File

/**
 * The one skin in use, imported like a dictionary: the companion writes it,
 * the keyboard reads it. Parsing and validation live in lapacho-core.
 */
private fun skinFile(context: Context) = File(context.filesDir, "skin.toml")

/** The skin in use, or null for none. A file that no longer parses is none. */
fun loadSkin(context: Context): SkinInfo? =
    skinFile(context).takeIf { it.exists() }?.let { runCatching { parseSkin(it.readText()) }.getOrNull() }

/** Checked before it replaces the old one: a bad file says why and changes nothing. */
fun installSkin(context: Context, uri: Uri) {
    val text = context.contentResolver.openInputStream(uri)!!.use { it.readBytes().decodeToString() }
    try {
        parseSkin(text)
    } catch (e: MobileException.Skin) {
        throw IllegalArgumentException(e.reason)
    }
    skinFile(context).writeText(text)
}

fun removeSkin(context: Context) = skinFile(context).delete()

fun hasSkin(context: Context) = skinFile(context).exists()

/** Changes when the skin is imported or removed; the keyboard compares it on every show. */
fun skinSignature(context: Context): Long = skinFile(context).lastModified()
