package digital.quebracho.lapacho.ime

import android.content.ClipboardManager
import android.content.Context
import android.inputmethodservice.InputMethodService
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
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
 * characters") — no shift/symbols/autocorrect. Full typing (or dropping the
 * key rows entirely) is a P2+ decision once adoption data exists.
 *
 * Cáscara Kotlin fina: no classification, no encryption logic here — both
 * live in [digital.quebracho.lapacho.storage] today and move into
 * `lapacho-core` behind uniffi at P1 (docs §"IME: no existe Rust IME —
 * cáscara Kotlin fina").
 */
class LapachoIme : InputMethodService() {

    private lateinit var repo: HistoryRepo
    private lateinit var pasteStrip: LinearLayout
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
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }

        pasteStrip = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        root.addView(
            HorizontalScrollView(this).apply { addView(pasteStrip) },
        )

        root.addView(buildKeyRow("qwertyuiop"))
        root.addView(buildKeyRow("asdfghjkl"))
        root.addView(buildKeyRow("zxcvbnm"))
        root.addView(buildActionRow())

        val coldStartMs = (System.nanoTime() - createdAtNanos) / 1_000_000
        Log.i(TAG, "cold start to input view: ${coldStartMs}ms")
        return root
    }

    override fun onStartInputView(info: android.view.inputmethod.EditorInfo?, restarting: Boolean) {
        super.onStartInputView(info, restarting)
        // Re-read top-N every time the keyboard becomes visible (docs §4.4:
        // "load from storage when the IME/app becomes active", not on every
        // keystroke). This is also the read half of the P0 exit criterion:
        // after a Force Stop of either process, this must still show the
        // last items the companion saved.
        captureClipboard()
        refreshPasteStrip()
    }

    /**
     * Stores whatever is on the system clipboard, if it is new.
     *
     * This is the only place Android allows it: since Android 10 the clipboard
     * is off limits to background apps, and the active IME is the one
     * sanctioned reader while it holds focus — the same reason KeePassDX's
     * Magikeyboard captures from here. So on mobile, capture is tied to
     * showing the keyboard; there is no always-on monitor like the desktop's.
     *
     * ponytail: stored as [Sensitivity.NONE] / [PersistLevel.ALL] because this
     * side has no classifier — nothing is masked, nothing expires by TTL, and a
     * copied password is kept like ordinary text. That arrives with the
     * lapacho-core bridge (docs/MIGRACION_MOBILE_RUST.md), which is also where
     * the TTL and the persistence levels come from.
     */
    private fun captureClipboard() {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
        val raw = clipboard.primaryClip
            ?.takeIf { it.itemCount > 0 }
            ?.getItemAt(0)
            ?.coerceToText(this)
            ?.toString()
            .orEmpty()
        if (raw.isBlank()) return

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

    private fun refreshPasteStrip() {
        val t0 = System.nanoTime()
        val items = repo.loadTopN(TOP_N)
        Log.i(TAG, "loadTopN(${TOP_N}) took ${(System.nanoTime() - t0) / 1_000_000}ms, ${items.size} items")

        pasteStrip.removeAllViews()
        if (items.isEmpty()) {
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
        currentInputConnection?.commitText(item.rawContent, 1)
    }

    private fun previewLabel(item: ClipboardItem): String {
        val oneLine = item.displayContent.replace('\n', ' ').trim()
        return if (oneLine.length > LABEL_MAX) oneLine.take(LABEL_MAX - 1) + "…" else oneLine.ifEmpty { "(empty)" }
    }

    private fun pasteButton(label: String, onClick: () -> Unit): Button =
        Button(this).apply {
            text = label
            isAllCaps = false
            setOnClickListener { onClick() }
        }

    private fun buildKeyRow(letters: String): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            for (c in letters) {
                addView(
                    Button(this@LapachoIme).apply {
                        text = c.toString()
                        layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                        setOnClickListener { currentInputConnection?.commitText(c.toString(), 1) }
                    },
                )
            }
        }

    private fun buildActionRow(): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            addView(
                Button(this@LapachoIme).apply {
                    text = "⌫"
                    layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                    setOnClickListener { currentInputConnection?.deleteSurroundingText(1, 0) }
                },
            )
            addView(
                Button(this@LapachoIme).apply {
                    text = "espacio"
                    layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 3f)
                    setOnClickListener { currentInputConnection?.commitText(" ", 1) }
                },
            )
            addView(
                Button(this@LapachoIme).apply {
                    text = "↵"
                    layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                    setOnClickListener { currentInputConnection?.commitText("\n", 1) }
                },
            )
        }

    companion object {
        private const val TAG = "LapachoIme"
        private const val TOP_N = 20
        private const val LABEL_MAX = 24
    }
}
