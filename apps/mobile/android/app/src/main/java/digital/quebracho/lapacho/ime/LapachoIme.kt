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
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import digital.quebracho.lapacho.storage.ClipboardItem
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
    private var createdAtNanos: Long = 0
    private var lastCapturedId: String? = null

    override fun onCreate() {
        super.onCreate()
        createdAtNanos = System.nanoTime()
        // Same DB the companion writes to (app-private storage, shared by
        // both processes under this app's UID) — no IPC needed to read it.
        repo = HistoryRepo(applicationContext)
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
        if (clip != null && !clip.sensitive && !privateField) capture(clip.text)
        refreshPasteStrip(secretOnClipboard = clip != null && (clip.sensitive || privateField))
    }

    private class Clip(val text: String, val sensitive: Boolean)

    private fun readClip(): Clip? {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return null
        val clip = clipboard.primaryClip?.takeIf { it.itemCount > 0 } ?: return null
        val text = clip.getItemAt(0).coerceToText(this)?.toString().orEmpty()
        if (text.isBlank()) return null
        // ClipDescription.EXTRA_IS_SENSITIVE is API 33; the key is a plain
        // string, so reading it needs no version gate (older apps never set it).
        return Clip(text, clip.description.extras?.getBoolean(EXTRA_IS_SENSITIVE) == true)
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
     * Clips the source app marks as sensitive are never stored — password
     * managers set that flag on what they copy (Android 13+), and a copied
     * password must not outlive the clipboard in our history. Neither is
     * anything read while a private field has focus. Both can still be pasted
     * from the clipboard itself, see [refreshPasteStrip].
     *
     * ponytail: everything else is stored as [Sensitivity.NONE] /
     * [PersistLevel.ALL] because this side has no classifier — nothing is
     * masked, nothing expires by TTL, and a password copied from an app that
     * doesn't set the flag is kept like ordinary text. That arrives with the
     * lapacho-core bridge (docs/MIGRACION_MOBILE_RUST.md), which is also where
     * the TTL and the persistence levels come from.
     */
    private fun capture(raw: String) {
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
                sensitivity = Sensitivity.NONE,
                detectedType = "Text",
                timestamp = System.currentTimeMillis() / 1000,
            ),
            PersistLevel.ALL,
        )
        Log.i(TAG, "captured clipboard item ${id.take(8)}… (${raw.length} chars)")
    }

    /**
     * [secretOnClipboard]: the clipboard holds something we won't store or
     * show (a sensitive clip, or anything while a private field has focus).
     * It gets a masked chip that pastes the clipboard as it is at tap time, so
     * a copied password can still go into a password field without ever
     * entering the history.
     */
    private fun refreshPasteStrip(secretOnClipboard: Boolean) {
        val t0 = System.nanoTime()
        val items = repo.loadTopN(TOP_N)
        Log.i(TAG, "loadTopN(${TOP_N}) took ${(System.nanoTime() - t0) / 1_000_000}ms, ${items.size} items")

        pasteStrip.removeAllViews()
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
        for (item in items) {
            pasteStrip.addView(pasteButton(previewLabel(item)) { commitRaw(item) })
        }
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
        if (item.sensitivity != Sensitivity.NONE) return "🔑 ••••••"
        val oneLine = item.displayContent.replace('\n', ' ').trim()
        return if (oneLine.length > LABEL_MAX) oneLine.take(LABEL_MAX - 1) + "…" else oneLine.ifEmpty { "(empty)" }
    }

    private fun pasteButton(label: String, onClick: () -> Unit): Button =
        Button(this).apply {
            text = label
            isAllCaps = false
            setOnClickListener { onClick() }
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
            setOnClickListener { onClick() }
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
        currentInputConnection?.commitText(if (accentPending) withAcute(key) else key, 1)
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
            addView(keyButton("espacio", 3f) { currentInputConnection?.commitText(" ", 1) })
            addView(keyButton(".", 1f) { type(".") })
            addView(keyButton("⌫", 1.2f) { currentInputConnection?.deleteSurroundingText(1, 0) })
            addView(keyButton("↵", 1.2f) { currentInputConnection?.commitText("\n", 1) })
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

        private const val EXTRA_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
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
