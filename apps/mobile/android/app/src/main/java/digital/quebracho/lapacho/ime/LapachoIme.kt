package digital.quebracho.lapacho.ime

import android.content.ClipboardManager
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.inputmethodservice.InputMethodService
import android.text.InputType
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import digital.quebracho.lapacho.EXTRA_IS_SENSITIVE
import digital.quebracho.lapacho.classify
import digital.quebracho.lapacho.isMasked
import digital.quebracho.lapacho.isSecret
import digital.quebracho.lapacho.matchesQuery
import digital.quebracho.lapacho.purgeUnclassifiedSecrets
import digital.quebracho.lapacho.storage.ClipboardItem
import digital.quebracho.lapacho.storage.HISTORY_MAX
import digital.quebracho.lapacho.storage.HistoryRepo
import digital.quebracho.lapacho.storage.PersistLevel
import digital.quebracho.lapacho.storage.Sensitivity
import digital.quebracho.lapacho.storage.contentId

/**
 * P0 spike IME. Deliberately NOT a full Gboard replacement — per the
 * "paste keyboard" positioning decided in docs/DEBATE_ARQUITECTURA_MOBILE.md
 * (§"El riesgo que este debate no menciona: nadie cambia de teclado"): the
 * primary feature is the **paste strip** (tap a history item, it commits),
 * modeled on KeePassDX's Magikeyboard. The row of letter keys below exists
 * only to satisfy the P0 spike's literal requirement ("empty IME that types
 * characters"), plus a numbers/symbols layer and shift/caps lock — no
 * autocorrect. Full typing (or dropping the
 * key rows entirely) is a P2+ decision once adoption data exists.
 *
 * Thin Kotlin shell: no classification, no encryption logic here — both
 * live in [digital.quebracho.lapacho.storage] today and move into
 * `lapacho-core` behind uniffi at P1 (docs §"IME: no existe Rust IME —
 * cáscara Kotlin fina").
 */
class LapachoIme : InputMethodService() {

    private lateinit var repo: HistoryRepo
    private lateinit var pasteStrip: LinearLayout
    private lateinit var keyRows: LinearLayout
    private var symbols = false
    private var shift = Shift.OFF
    private var lastShiftTapMs = 0L
    private var accentPending = false
    private var privateField = false
    private var secretOnClipboard = false
    // Search mode: non-null while searching. The keys edit [query] instead of
    // the field, and the strip shows what in this pool matches it.
    private var searchPool: List<ClipboardItem>? = null
    private var query = ""
    private var createdAtNanos: Long = 0
    private var lastCapturedId: String? = null

    override fun onCreate() {
        super.onCreate()
        createdAtNanos = System.nanoTime()
        // Same DB the companion writes to (app-private storage, shared by
        // both processes under this app's UID) — no IPC needed to read it.
        repo = HistoryRepo(applicationContext)
        repo.purgeUnclassifiedSecrets()
    }

    override fun onCreateInputView(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(KEYBOARD_BG)
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }

        pasteStrip = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        root.addView(
            HorizontalScrollView(this).apply { addView(pasteStrip) },
        )

        keyRows = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        root.addView(keyRows)
        root.addView(buildActionRow())
        showLayer()
        // Targeting API 35 draws edge-to-edge: without this the navigation
        // bar's buttons land on top of the bottom key row.
        ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
            v.setPadding(0, 0, 0, insets.getInsets(WindowInsetsCompat.Type.navigationBars()).bottom)
            insets
        }

        val coldStartMs = (System.nanoTime() - createdAtNanos) / 1_000_000
        Log.i(TAG, "cold start to input view: ${coldStartMs}ms")
        return root
    }

    override fun onStartInputView(info: EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        // Re-read top-N every time the keyboard becomes visible (docs §4.4:
        // "load from storage when the IME/app becomes active", not on every
        // keystroke). This is also the read half of the P0 exit criterion:
        // after a Force Stop of either process, this must still show the
        // last items the companion saved.
        privateField = info != null && isPrivateField(info.inputType, info.imeOptions)
        val clip = readClip()
        val secret = clip != null && clip.sensitivity.isSecret()
        if (clip != null && !secret && !privateField) capture(clip.text, clip.sensitivity)
        secretOnClipboard = clip != null && (secret || privateField)
        searchPool = null
        refreshPasteStrip()
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        // Don't keep up to HISTORY_MAX decrypted items around once the
        // keyboard is gone.
        searchPool = null
        super.onFinishInputView(finishingInput)
    }

    private class Clip(val text: String, val sensitivity: Sensitivity)

    private fun readClip(): Clip? {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return null
        val clip = clipboard.primaryClip?.takeIf { it.itemCount > 0 } ?: return null
        val text = clip.getItemAt(0).coerceToText(this)?.toString().orEmpty()
        if (text.isBlank()) return null
        // ClipDescription.EXTRA_IS_SENSITIVE is API 33; the key is a plain
        // string, so reading it needs no version gate (older apps never set it).
        // Password managers set it; a password copied from anywhere else is
        // caught by lapacho-core's classifier instead.
        val flagged = clip.description.extras?.getBoolean(EXTRA_IS_SENSITIVE) == true
        return Clip(text, if (flagged) Sensitivity.SECRET else classify(text))
    }

    /**
     * Stores what is on the system clipboard, if it is new.
     *
     * This is the only place Android allows it: since Android 10 the clipboard
     * is off limits to background apps, and the active IME is the one
     * sanctioned reader while it holds focus — the same reason KeePassDX's
     * Magikeyboard captures from here. So on mobile, capture is tied to
     * showing the keyboard; there is no always-on monitor like the desktop's.
     *
     * Credentials and secrets are never stored — whether the source app
     * flagged the clip (password managers do, Android 13+) or lapacho-core's
     * classifier recognized it — and a copied password must not outlive the
     * clipboard in our history. Neither is anything read while a private field
     * has focus. Both can still be pasted from the clipboard itself, see
     * [refreshPasteStrip].
     *
     * ponytail: what is stored goes in as [PersistLevel.ALL] with no TTL; the
     * persistence levels arrive with the rest of the lapacho-core migration
     * (docs/MIGRACION_MOBILE_RUST.md).
     */
    private fun capture(raw: String, sensitivity: Sensitivity) {
        val id = contentId(raw)
        // Seeing the same clip again on every keyboard show is not a re-copy:
        // skipping it keeps the item's timestamp at when it was really copied.
        if (id == lastCapturedId) return
        lastCapturedId = id

        repo.save(
            ClipboardItem(
                id = id,
                rawContent = raw,
                displayContent = raw,
                contentType = "text",
                sensitivity = sensitivity,
                detectedType = "Text",
                timestamp = System.currentTimeMillis() / 1000,
            ),
            PersistLevel.ALL,
        )
        repo.cleanup(sensitiveTtlSecs = null, maxItems = HISTORY_MAX)
        Log.i(TAG, "captured clipboard item ${id.take(8)}… (${raw.length} chars)")
    }

    /**
     * [secretOnClipboard]: the clipboard holds something we won't store or
     * show (a sensitive clip, or anything while a private field has focus).
     * It gets a masked chip that pastes the clipboard as it is at tap time, so
     * a copied password can still go into a password field without ever
     * entering the history.
     */
    private fun refreshPasteStrip() {
        pasteStrip.removeAllViews()
        searchPool?.let { showSearch(it); return }

        val t0 = System.nanoTime()
        val items = repo.loadTopN(TOP_N)
        Log.i(TAG, "loadTopN(${TOP_N}) took ${(System.nanoTime() - t0) / 1_000_000}ms, ${items.size} items")

        if (secretOnClipboard) {
            pasteStrip.addView(pasteButton("🔑 ••••••") { readClip()?.let { commitText(it.text) } })
        }
        // In a password field or an incognito session the history stays
        // hidden: nothing we show there should be visible over a secret.
        if (privateField) {
            pasteStrip.addView(pasteButton("🔒 campo privado: historial oculto") {})
            return
        }
        if (items.isEmpty()) {
            if (secretOnClipboard) return
            pasteStrip.addView(pasteButton("(sin clips)") {})
            return
        }
        pasteStrip.addView(pasteButton("🔍") { startSearch() })
        for (item in items) {
            pasteStrip.addView(pasteButton(previewLabel(item)) { commitRaw(item) })
        }
    }

    // Masked items are left out of the pool: matching against them would let
    // typing part of an old password reveal that it is stored.
    private fun startSearch() {
        searchPool = repo.loadTopN(HISTORY_MAX).filterNot { it.isMasked() }
        query = ""
        refreshPasteStrip()
    }

    private fun stopSearch() {
        searchPool = null
        query = ""
        refreshPasteStrip()
    }

    private fun searchHits(pool: List<ClipboardItem>): List<ClipboardItem> =
        pool.filter { matchesQuery(it.displayContent, query) }.take(TOP_N)

    private fun showSearch(pool: List<ClipboardItem>) {
        pasteStrip.addView(pasteButton("✕") { stopSearch() })
        pasteStrip.addView(pasteButton("🔍 $query▏") {})
        val hits = searchHits(pool)
        if (hits.isEmpty()) pasteStrip.addView(pasteButton("(sin resultados)") {})
        for (item in hits) {
            pasteStrip.addView(pasteButton(previewLabel(item)) { commitRaw(item); stopSearch() })
        }
    }

    /** Where every key's text goes: the search query while searching, else the field. */
    private fun output(text: String) {
        if (searchPool == null) {
            commitText(text)
            return
        }
        query += text
        refreshPasteStrip()
    }

    private fun backspace() {
        if (searchPool == null) {
            currentInputConnection?.deleteSurroundingText(1, 0)
            return
        }
        query = query.dropLast(1)
        refreshPasteStrip()
    }

    /** Enter: a new line, or while searching, paste the first match. */
    private fun enter() {
        val pool = searchPool ?: return commitText("\n")
        searchHits(pool).firstOrNull()?.let { commitRaw(it) }
        stopSearch()
    }

    private fun commitRaw(item: ClipboardItem) {
        // Paste = commit the RAW content, intact — same rule as desktop's
        // copy_item: masking is a display-only concern, never applied to
        // what actually gets typed.
        commitText(item.rawContent)
    }

    private fun commitText(text: String) {
        currentInputConnection?.commitText(text, 1)
    }

    private fun previewLabel(item: ClipboardItem): String {
        if (item.isMasked()) return "🔑 ••••••"
        val oneLine = item.displayContent.replace('\n', ' ').trim()
        return if (oneLine.length > LABEL_MAX) oneLine.take(LABEL_MAX - 1) + "…" else oneLine.ifEmpty { "(empty)" }
    }

    private fun pasteButton(label: String, onClick: () -> Unit): Button =
        Button(this).apply {
            text = label
            isAllCaps = false
            setOnClickListener { keyFeedback(it); onClick() }
        }

    /**
     * A key that stays readable under any display zoom or font scale. Not a
     * [Button]: each vendor styles those differently (min width, padding,
     * insets, `ellipsize=end`) and on a phone with display zoom the glyph was
     * squeezed out and drawn as "…". A plain TextView has none of that; the
     * label is sized in dp, not sp, so the system font scale doesn't grow it
     * past the key — the same choice Gboard makes.
     */
    private fun keyButton(label: String, weight: Float, onClick: () -> Unit): TextView =
        TextView(this).apply {
            text = label
            gravity = Gravity.CENTER
            maxLines = 1
            setTextColor(Color.WHITE)
            setTextSize(TypedValue.COMPLEX_UNIT_DIP, KEY_TEXT_DP)
            background = RippleDrawable(
                ColorStateList.valueOf(Color.GRAY),
                GradientDrawable().apply { setColor(KEY_COLOR); cornerRadius = dp(6f) },
                null,
            )
            isClickable = true
            layoutParams = LinearLayout.LayoutParams(0, dp(KEY_HEIGHT_DP).toInt(), weight).apply {
                val m = dp(2f).toInt()
                setMargins(m, m, m, m)
            }
            setOnClickListener { keyFeedback(it); onClick() }
        }

    /**
     * Vibration on every key and chip, following the system's own touch /
     * keyboard vibration setting (no setting of ours). The click sound needs
     * nothing: performClick() already plays it when the system's "touch
     * sounds" setting is on.
     */
    private fun keyFeedback(view: View) {
        view.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP)
    }

    private fun dp(v: Float): Float = v * resources.displayMetrics.density

    private fun buildKeyRow(letters: String, withShift: Boolean = false): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            if (withShift) {
                val label = when (shift) { Shift.LOCKED -> "⇪"; Shift.ONCE -> "⬆"; Shift.OFF -> "⇧" }
                addView(keyButton(label, 1.5f) { onShift() })
            }
            for (c in letters) {
                if (c == DEAD_ACUTE) {
                    addView(keyButton(if (accentPending) "[´]" else "´", 1f) { accentPending = !accentPending; showLayer() })
                    continue
                }
                val key = if (shift != Shift.OFF) c.uppercase() else c.toString()
                addView(keyButton(key, 1f) { type(key) })
            }
        }

    private fun onShift() {
        val now = System.currentTimeMillis()
        shift = nextShift(shift, now - lastShiftTapMs)
        lastShiftTapMs = now
        showLayer()
    }

    private fun type(key: String) {
        output(if (accentPending) withAcute(key) else key)
        if (shift == Shift.ONCE || accentPending) {
            shift = if (shift == Shift.ONCE) Shift.OFF else shift
            accentPending = false
            showLayer()
        }
    }

    enum class Shift { OFF, ONCE, LOCKED }

    private fun showLayer() {
        keyRows.removeAllViews()
        val rows = if (symbols) SYMBOL_ROWS else LETTER_ROWS
        rows.forEachIndexed { i, row -> keyRows.addView(buildKeyRow(row, withShift = !symbols && i == rows.lastIndex)) }
    }

    private fun buildActionRow(): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            lateinit var toggle: TextView
            toggle = keyButton(if (symbols) "abc" else "?123", 1.4f) {
                symbols = !symbols
                toggle.text = if (symbols) "abc" else "?123"
                showLayer()
            }
            addView(toggle)
            addView(keyButton(",", 1f) { type(",") })
            addView(keyButton("espacio", 3f) { output(" ") })
            addView(keyButton(".", 1f) { type(".") })
            addView(keyButton("⌫", 1.2f) { backspace() })
            addView(keyButton("↵", 1.2f) { enter() })
        }

    companion object {
        /**
         * Dead-key acute accent, as on a Spanish physical keyboard: ´ then a
         * vowel gives the accented vowel; anything else comes out unchanged.
         */
        fun withAcute(key: String): String {
            val i = PLAIN_VOWELS.indexOf(key)
            return if (key.length == 1 && i >= 0) ACUTE_VOWELS[i].toString() else key
        }

        private const val PLAIN_VOWELS = "aeiouAEIOU"
        private const val ACUTE_VOWELS = "áéíóúÁÉÍÓÚ"
        private const val DEAD_ACUTE = '´'
        /** Tap: shift for one letter. Double tap: caps lock. Tap again: off. */
        fun nextShift(current: Shift, msSinceLastTap: Long): Shift = when (current) {
            Shift.OFF -> Shift.ONCE
            Shift.ONCE -> if (msSinceLastTap < DOUBLE_TAP_MS) Shift.LOCKED else Shift.OFF
            Shift.LOCKED -> Shift.OFF
        }

        /**
         * A field whose content must not mix with the history: passwords and
         * PINs, or an app that asked for no learning (incognito tabs, some
         * banking apps).
         */
        fun isPrivateField(inputType: Int, imeOptions: Int): Boolean {
            if (imeOptions and EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING != 0) return true
            val variation = inputType and InputType.TYPE_MASK_VARIATION
            return when (inputType and InputType.TYPE_MASK_CLASS) {
                InputType.TYPE_CLASS_TEXT -> variation in setOf(
                    InputType.TYPE_TEXT_VARIATION_PASSWORD,
                    InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD,
                    InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD,
                )
                InputType.TYPE_CLASS_NUMBER -> variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD
                else -> false
            }
        }

        private const val DOUBLE_TAP_MS = 400
        private const val TAG = "LapachoIme"
        private const val TOP_N = 20
        private const val LABEL_MAX = 24
        private const val KEY_TEXT_DP = 20f
        private const val KEY_HEIGHT_DP = 46f
        private const val KEY_COLOR = 0xFF3C3C3C.toInt()
        private const val KEYBOARD_BG = 0xFF1E1E1E.toInt()
        private val LETTER_ROWS = listOf("qwertyuiop", "asdfghjklñ", "zxcvbnm$DEAD_ACUTE")
        private val SYMBOL_ROWS = listOf("1234567890", "@#\$%&-+()/", "<>[]{}=_|\\", "*\"':;!¡?¿")
    }
}
