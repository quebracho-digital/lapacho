package digital.quebracho.lapacho.storage

import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper

/**
 * SQLite-backed history store shared by the companion app and the IME —
 * mobile counterpart of `lapacho_core::storage::SqliteRepo`. Same schema
 * shape, same rules: content (`raw`/`display`) encrypted at rest, metadata in
 * clear so it can be ordered without decrypting everything.
 *
 * Multi-process: the companion and the IME are two processes of the same app
 * UID, so they share `context.filesDir` and this DB file. Per
 * docs/ARQUITECTURA_MOBILE_ANDROID.md §4.3: WAL + short-lived connections,
 * no ContentProvider yet (only added if contention numbers demand it).
 */
class HistoryRepo(context: Context, private val cipher: LapachoCipher = LapachoCipher()) :
    SQLiteOpenHelper(context.applicationContext, DB_NAME, null, DB_VERSION) {

    init {
        setWriteAheadLoggingEnabled(true)
    }

    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """
            CREATE TABLE history (
                id TEXT PRIMARY KEY,
                raw_content TEXT NOT NULL,
                display_content TEXT NOT NULL,
                content_type TEXT NOT NULL,
                sensitivity TEXT NOT NULL,
                detected_type TEXT NOT NULL DEFAULT 'Text',
                timestamp INTEGER NOT NULL,
                thumbnail TEXT,
                size INTEGER,
                sync_eligible INTEGER NOT NULL DEFAULT 0,
                sync_state TEXT
            )
            """.trimIndent(),
        )
        db.execSQL(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        )
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        // P0 spike: no migrations to carry yet.
    }

    /**
     * Persists [item] if [level] allows it — same predicate as desktop's
     * `HistoryRepo::save`. Re-copying identical content (same `item.id`,
     * which callers must set via [contentId]) upserts the timestamp instead
     * of inserting a duplicate row.
     */
    fun save(item: ClipboardItem, level: PersistLevel) {
        val shouldSave = when (level) {
            PersistLevel.NONE -> item.sensitivity == Sensitivity.NONE
            PersistLevel.SENSITIVE -> item.sensitivity != Sensitivity.SECRET
            PersistLevel.ALL -> true
        }
        if (!shouldSave) return

        val values = ContentValues().apply {
            put("id", item.id)
            put("raw_content", cipher.encrypt(item.rawContent))
            put("display_content", cipher.encrypt(item.displayContent))
            put("content_type", item.contentType)
            put("sensitivity", item.sensitivity.name)
            put("detected_type", item.detectedType)
            put("timestamp", item.timestamp)
            put("thumbnail", item.thumbnail)
            put("size", item.size)
            put("sync_eligible", if (item.syncEligible) 1 else 0)
            put("sync_state", item.syncState)
        }
        writableDatabase.insertWithOnConflict(
            "history",
            null,
            values,
            SQLiteDatabase.CONFLICT_REPLACE,
        )
    }

    /** Newest-first history, decrypting only the rows returned. */
    fun load(limit: Int = 100): List<ClipboardItem> {
        val items = mutableListOf<ClipboardItem>()
        readableDatabase.query(
            "history",
            null,
            null,
            null,
            null,
            null,
            "timestamp DESC",
            limit.toString(),
        ).use { cursor ->
            while (cursor.moveToNext()) {
                fromCursor(cursor)?.let { items.add(it) }
            }
        }
        return items
    }

    /**
     * Cold-start hot path (docs §4.4): load only the top [n] rows, not the
     * whole table. This is what the IME calls from `onCreate`/first
     * `onStartInput` — see P0 exit criterion "measure cold-start time to show
     * top-10".
     */
    fun loadTopN(n: Int = 20): List<ClipboardItem> = load(limit = n)

    fun delete(id: String) {
        writableDatabase.delete("history", "id = ?", arrayOf(id))
    }

    fun clear() {
        writableDatabase.delete("history", null, null)
    }

    /** Purges expired sensitive items + enforces the size cap, mirroring
     * desktop's `RetentionPolicy`/`cleanup`. */
    fun cleanup(sensitiveTtlSecs: Long?, maxItems: Int) {
        val db = writableDatabase
        if (sensitiveTtlSecs != null) {
            val cutoff = System.currentTimeMillis() / 1000 - sensitiveTtlSecs
            db.delete(
                "history",
                "sensitivity IN ('CREDENTIAL', 'SECRET') AND timestamp < ?",
                arrayOf(cutoff.toString()),
            )
        }
        db.execSQL(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY timestamp DESC LIMIT ?)",
            arrayOf(maxItems),
        )
    }

    fun setPreference(key: String, value: String) {
        val values = ContentValues().apply {
            put("key", key)
            put("value", value)
        }
        writableDatabase.insertWithOnConflict("settings", null, values, SQLiteDatabase.CONFLICT_REPLACE)
    }

    fun getPreference(key: String): String? {
        readableDatabase.query(
            "settings", arrayOf("value"), "key = ?", arrayOf(key), null, null, null,
        ).use { cursor ->
            return if (cursor.moveToFirst()) cursor.getString(0) else null
        }
    }

    private fun fromCursor(cursor: Cursor): ClipboardItem? {
        return try {
            ClipboardItem(
                id = cursor.getString(cursor.getColumnIndexOrThrow("id")),
                rawContent = cipher.decrypt(cursor.getString(cursor.getColumnIndexOrThrow("raw_content"))),
                displayContent = cipher.decrypt(cursor.getString(cursor.getColumnIndexOrThrow("display_content"))),
                contentType = cursor.getString(cursor.getColumnIndexOrThrow("content_type")),
                sensitivity = Sensitivity.valueOf(cursor.getString(cursor.getColumnIndexOrThrow("sensitivity"))),
                detectedType = cursor.getString(cursor.getColumnIndexOrThrow("detected_type")),
                timestamp = cursor.getLong(cursor.getColumnIndexOrThrow("timestamp")),
                thumbnail = cursor.getString(cursor.getColumnIndexOrThrow("thumbnail")),
                size = if (cursor.isNull(cursor.getColumnIndexOrThrow("size"))) null else cursor.getLong(cursor.getColumnIndexOrThrow("size")),
                syncEligible = cursor.getInt(cursor.getColumnIndexOrThrow("sync_eligible")) != 0,
                syncState = cursor.getString(cursor.getColumnIndexOrThrow("sync_state")),
            )
        } catch (_: Exception) {
            // A row that fails to decrypt (wrong/rotated key) is skipped, not
            // fatal — same policy as desktop's `SqliteRepo::load`.
            null
        }
    }

    companion object {
        private const val DB_NAME = "lapacho_history.db"
        private const val DB_VERSION = 1
    }
}
