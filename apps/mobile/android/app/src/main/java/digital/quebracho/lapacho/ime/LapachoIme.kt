package digital.quebracho.lapacho.ime

import digital.quebracho.lapacho.app.R
import android.content.ClipboardManager
import android.content.Context
import android.app.LocaleManager
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.content.res.Resources
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.inputmethodservice.InputMethodService
import android.os.Build
import android.text.InputType
import android.text.SpannableString
import android.text.Spanned
import android.text.style.ForegroundColorSpan
import android.text.style.RelativeSizeSpan
import android.text.style.SuperscriptSpan
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.SoundEffectConstants
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.PopupWindow
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import digital.quebracho.lapacho.EXTRA_IS_SENSITIVE
import digital.quebracho.lapacho.classify
import digital.quebracho.lapacho.currentWord
import digital.quebracho.lapacho.dictionarySignature
import digital.quebracho.lapacho.installedDictionaries
import digital.quebracho.lapacho.keyAlternates
import digital.quebracho.lapacho.isMasked
import digital.quebracho.lapacho.isSecret
import digital.quebracho.lapacho.loadPredictor
import uniffi.lapacho_mobile_bridge.WordPredictor
import digital.quebracho.lapacho.matchesQuery
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
    private var layer = Layer.LETTERS
    private lateinit var layerToggle: TextView
    private var shift = Shift.OFF
    private var lastShiftTapMs = 0L
    private var accentPending = false
    private var privateField = false
    private var secretOnClipboard = false
    // Search mode: non-null while searching. The keys edit [query] instead of
    // the field, and the strip shows what in this pool matches it.
    private var searchPool: List<ClipboardItem>? = null
    // The strip's clips, read when the keyboard opens and not per keystroke
    // (docs §4.4): with suggestions, the strip redraws on every key.
    private var clips: List<ClipboardItem> = emptyList()
    // ~600 KB of dictionary parsed on the first word typed, not on the cold
    // start path — the keyboard has to be on screen before that matters.
    // Dropped when a word is learned, so the next lookup picks it up.
    private var loadedPredictor: WordPredictor? = null
    private var lexicon: List<String> = emptyList()
    // Which imported dictionaries the predictor and [longPress] were built
    // from; null until the first show, so that one always builds them.
    private var dictSig: String? = null
    // Long-press alternates: [BASE_LONG_PRESS] plus what every active
    // dictionary's header adds (ñ comes from Spanish's).
    private var longPress: Map<String, String> = BASE_LONG_PRESS
    // Set when a word is learned, shown once, gone on the next keystroke.
    private var justLearned: String? = null
    // True while the space before the cursor is one a suggestion put there,
    // so a closing mark typed next can take its place: "hola ," -> "hola, ".
    private var autoSpace = false
    private val predictor: WordPredictor
        get() = loadedPredictor ?: run {
            val t0 = System.nanoTime()
            loadPredictor(this, lexicon).also {
                loadedPredictor = it
                Log.i(TAG, "dictionary: ${it.size()} words in ${(System.nanoTime() - t0) / 1_000_000}ms")
            }
        }
    private var query = ""
    // The key under a finger that has not lifted yet; see [pressable].
    private var pendingTap: (() -> Unit)? = null
    // Redraws each key of the current layer for the current shift and dead
    // key, in place; see [relabel].
    private val relabels = mutableListOf<() -> Unit>()
    private var createdAtNanos: Long = 0
    private var lastCapturedId: String? = null
    private lateinit var strings: Resources

    override fun onCreate() {
        super.onCreate()
        createdAtNanos = System.nanoTime()
        // Same DB the companion writes to (app-private storage, shared by
        // both processes under this app's UID) — no IPC needed to read it.
        repo = HistoryRepo(applicationContext)
    }

    /**
     * The keyboard's texts in the language picked for Lapacho in Android 13+'s
     * per-app setting. Android applies that choice to the app's activities but
     * not to this service, which kept following the phone's language.
     * ponytail: read when the keys are built, so a change shows the next time
     * Android recreates the keyboard's view, not mid-session.
     */
    private fun appResources(): Resources {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return resources
        val locales = getSystemService(LocaleManager::class.java).applicationLocales
        if (locales.isEmpty) return resources
        val config = Configuration(resources.configuration).apply { setLocales(locales) }
        return createConfigurationContext(config).resources
    }

    override fun onCreateInputView(): View {
        strings = appResources()
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
        autoSpace = false
        // Re-read top-N every time the keyboard becomes visible (docs §4.4:
        // "load from storage when the IME/app becomes active", not on every
        // keystroke). This is also the read half of the P0 exit criterion:
        // after a Force Stop of either process, this must still show the
        // last items the companion saved.
        privateField = info != null && isPrivateField(info.inputType, info.imeOptions)
        // A dictionary imported or removed in the companion since the last
        // show: new words, and possibly new keys to hold. Before the layer
        // is drawn below, so the hints are the new ones.
        val sig = dictionarySignature(this)
        if (sig != dictSig) {
            dictSig = sig
            loadedPredictor = null
            longPress = keyAlternates(installedDictionaries(this).map { it.header }, BASE_LONG_PRESS)
        }
        // Every session starts on the layer the field asks for — numbers for
        // an amount, a PIN, a phone or a date; letters for everything else —
        // with no shift pending. Where the last app left the keyboard is not
        // a preference: opening on the symbols layer in a chat is just wrong,
        // and so is a caps lock the user set somewhere else an hour ago.
        //
        // Only a new session, though. Some apps restart the input on every
        // change to the field (a field that reformats what is typed, a
        // search box that re-queries), and Android reports that as
        // `restarting` on the same field: resetting there threw the user
        // back to the letters after every digit typed on the numbers layer.
        if (!restarting) {
            shift = Shift.OFF
            accentPending = false
            if (::layerToggle.isInitialized) setLayer(initialLayer(info?.inputType ?: 0))
        }
        val clip = readClip()
        val secret = clip != null && clip.sensitivity.isSecret()
        if (clip != null && !secret && !privateField) capture(clip.text, clip.sensitivity)
        secretOnClipboard = clip != null && (secret || privateField)
        searchPool = null
        // The companion can forget a word while the keyboard is not on
        // screen, and the dictionary in memory would not know. Reading a
        // handful of rows is cheaper than rebuilding it blindly.
        val stored = repo.lexicon()
        if (stored != lexicon) {
            lexicon = stored
            loadedPredictor = null
        }
        val t0 = System.nanoTime()
        clips = repo.loadTopN(TOP_N)
        Log.i(TAG, "loadTopN($TOP_N) took ${(System.nanoTime() - t0) / 1_000_000}ms, ${clips.size} items")
        refreshStrip()
    }

    override fun onFinishInputView(finishingInput: Boolean) {
        // Don't keep decrypted items around once the keyboard is gone.
        searchPool = null
        clips = emptyList()
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
    private fun refreshStrip() {
        pasteStrip.removeAllViews()
        // Learning is otherwise invisible: the word stays where it was and the
        // strip goes back to the clips, so the only way to tell it worked was
        // to type the word's first letters again — and the word itself is
        // never suggested back, because there is nothing left to complete.
        justLearned?.let { word ->
            justLearned = null
            pasteStrip.addView(pasteButton(strings.getString(R.string.learned_chip, word)) {})
            return
        }
        searchPool?.let { showSearch(it); return }
        // While a word is being typed the strip belongs to the suggestions;
        // finish the word and the clips come back. One row, three jobs — the
        // alternative is a keyboard one row taller for everyone.
        if (!privateField && showSuggestions()) return
        showClips()
    }

    /**
     * The strip while a word is being typed: corrections if it looks
     * misspelled, what the dictionary can complete, and an offer to learn the
     * word if the dictionary has nothing to say about it. Returns whether it
     * took the strip over.
     *
     * A correction is only ever offered, never applied: it goes first in the
     * strip and the user taps it or types on. An autocorrect that rewrites
     * the word on space is the keyboard deciding for the user.
     *
     * Corrections are looked for only when nothing completes the word. While
     * completions exist the word may simply be unfinished — `gra` is in the
     * corpus and one edit from far commoner words, and offering those would
     * push aside `gracias`. The corpus's own typos (`qeu`) complete to
     * nothing, so they still get here, and the engine corrects a known word
     * only towards a far more common one. It also keeps the scan off most
     * keystrokes.
     *
     * Nothing is looked up until [MIN_PREFIX] letters: a single letter matches
     * most of the dictionary, and hiding the clips on the first keystroke of
     * every word costs more than the suggestion is worth.
     */
    private fun showSuggestions(): Boolean {
        val word = currentWord(currentInputConnection?.getTextBeforeCursor(WORD_LOOKBEHIND, 0))
        if (word.length < MIN_PREFIX) return false
        val predictor = predictor // the first lookup loads it; keep that out of the timing
        val t0 = System.nanoTime()
        val hits = predictor.suggest(word, SUGGESTIONS.toUInt())
        val fixes = if (hits.isEmpty()) predictor.correct(word, SUGGESTIONS.toUInt()) else emptyList()
        // Lengths and counts, never the words themselves: this is a keyboard,
        // and what gets typed is exactly what must not end up in a log.
        Log.i(TAG, "suggest: ${word.length}-letter prefix, ${hits.size} hits, ${fixes.size} fixes in ${(System.nanoTime() - t0) / 1000}µs")
        for (hit in (fixes + hits).distinct().take(SUGGESTIONS)) {
            pasteStrip.addView(pasteButton(hit) { commitSuggestion(hit) })
        }
        if (hits.isEmpty() && isLearnable(word)) {
            pasteStrip.addView(learnableChip(word))
            return true
        }
        return hits.isNotEmpty() || fixes.isNotEmpty()
    }

    /**
     * Whether to offer to learn [word]. Only when the dictionary leads
     * nowhere — while a normal word is being typed there are always
     * completions, so the offer stays out of the way — and never for
     * something the classifier reads as a secret: a learned word comes back
     * as a suggestion, and a password must not.
     */
    private fun isLearnable(word: String): Boolean =
        word.length >= MIN_LEARN && !predictor.knows(word) && !classify(word).isSecret()

    /**
     * The word as typed, with a mark saying there is something under a long
     * press. Tapping it just finishes the word; holding it offers to learn it.
     *
     * The system's 500 ms here, not the keys' 280: adding a word to a
     * permanent list should take a press nobody makes by accident.
     */
    private fun learnableChip(word: String): Button =
        pasteButton(word) {}.apply {
            text = labelWithHint(word, LEARN_HINT)
            holdToOpen(
                this,
                LEARN_PRESS_MS,
                onHold = { offerToLearn(this, word) },
                onTap = { commitSuggestion(word) },
            )
        }

    /** The confirmation: one more deliberate tap, and only then is it stored. */
    private fun offerToLearn(anchor: View, word: String) {
        // Below the chip, over the top key row — inside the keyboard's own
        // window. Above it would hang over the app, which is a place some
        // devices refuse to draw an IME's popup.
        popupNear(anchor, 0) { row, popup ->
            row.addView(
                pasteButton(strings.getString(R.string.learn_chip, word)) {
                    repo.learn(word)
                    // Rebuilt on the next lookup, with the new word in it.
                    lexicon = lexicon + word
                    loadedPredictor = null
                    justLearned = word
                    Log.i(TAG, "learned a ${word.length}-letter word")
                    popup.dismiss()
                    refreshStrip()
                },
            )
        }
    }

    /** Replaces the word being typed with [word], plus the space after it. */
    private fun commitSuggestion(word: String) {
        val typed = currentWord(currentInputConnection?.getTextBeforeCursor(WORD_LOOKBEHIND, 0))
        currentInputConnection?.deleteSurroundingText(typed.length, 0)
        commitText("$word ")
        autoSpace = true
        refreshStrip()
    }

    private fun showClips() {
        if (secretOnClipboard) {
            pasteStrip.addView(pasteButton("🔑 ••••••") { readClip()?.let { commitText(it.text) } })
        }
        // In a password field or an incognito session the history stays
        // hidden: nothing we show there should be visible over a secret.
        if (privateField) {
            pasteStrip.addView(pasteButton(strings.getString(R.string.private_field)) {})
            return
        }
        if (clips.isEmpty()) {
            if (secretOnClipboard) return
            pasteStrip.addView(pasteButton(strings.getString(R.string.no_clips)) {})
            return
        }
        pasteStrip.addView(pasteButton("🔍") { startSearch() })
        for (item in clips) {
            pasteStrip.addView(pasteButton(previewLabel(item)) { commitRaw(item) })
        }
    }

    // Masked items are left out of the pool: matching against them would let
    // typing part of an old password reveal that it is stored.
    private fun startSearch() {
        searchPool = repo.loadTopN(HISTORY_MAX).filterNot { it.isMasked() }
        query = ""
        refreshStrip()
    }

    private fun stopSearch() {
        searchPool = null
        query = ""
        refreshStrip()
    }

    private fun searchHits(pool: List<ClipboardItem>): List<ClipboardItem> =
        pool.filter { matchesQuery(it.displayContent, query) }.take(TOP_N)

    private fun showSearch(pool: List<ClipboardItem>) {
        pasteStrip.addView(pasteButton("✕") { stopSearch() })
        pasteStrip.addView(pasteButton("🔍 $query▏") {})
        val hits = searchHits(pool)
        if (hits.isEmpty()) pasteStrip.addView(pasteButton(strings.getString(R.string.no_results)) {})
        for (item in hits) {
            pasteStrip.addView(pasteButton(previewLabel(item)) { commitRaw(item); stopSearch() })
        }
    }

    /** Where every key's text goes: the search query while searching, else the field. */
    private fun output(text: String) {
        if (searchPool == null) {
            val swap = autoSpace && text in CLOSING_MARKS &&
                currentInputConnection?.getTextBeforeCursor(1, 0) == " "
            if (swap) currentInputConnection?.deleteSurroundingText(1, 0)
            commitText(if (swap) "$text " else text)
            // Kept after a swap, so "?" then "!" still lands as "hola?! ".
            autoSpace = swap
            refreshStrip()
            return
        }
        query += text
        refreshStrip()
    }

    private fun backspace() {
        if (searchPool == null) {
            autoSpace = false
            currentInputConnection?.deleteSurroundingText(1, 0)
            refreshStrip()
            return
        }
        query = query.dropLast(1)
        refreshStrip()
    }

    /** Enter: a new line, or while searching, paste the first match. */
    private fun enter() {
        val pool = searchPool
        if (pool == null) {
            autoSpace = false
            commitText("\n")
            refreshStrip()
            return
        }
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
        return if (oneLine.length > LABEL_MAX) oneLine.take(LABEL_MAX - 1) + "…" else oneLine.ifEmpty { strings.getString(R.string.empty_clip) }
    }

    private fun pasteButton(label: String, onClick: () -> Unit): Button =
        Button(this).apply {
            text = label
            isAllCaps = false
            setOnClickListener { flushPendingTap(); keyFeedback(it); onClick() }
        }

    /**
     * A key that stays readable under any display zoom or font scale. Not a
     * [Button]: each vendor styles those differently (min width, padding,
     * insets, `ellipsize=end`) and on a phone with display zoom the glyph was
     * squeezed out and drawn as "…". A plain TextView has none of that; the
     * label is sized in dp, not sp, so the system font scale doesn't grow it
     * past the key — the same choice Gboard makes.
     */
    private fun keyButton(
        label: String,
        weight: Float,
        alternates: String? = null,
        color: Int = KEY_COLOR,
        onClick: () -> Unit,
    ): TextView =
        TextView(this).apply {
            text = if (alternates == null) label else labelWithHint(label, alternates)
            gravity = Gravity.CENTER
            maxLines = 1
            setTextColor(Color.WHITE)
            setTextSize(TypedValue.COMPLEX_UNIT_DIP, KEY_TEXT_DP)
            background = RippleDrawable(
                ColorStateList.valueOf(Color.GRAY),
                GradientDrawable().apply { setColor(color); cornerRadius = dp(6f) },
                null,
            )
            isClickable = true
            layoutParams = LinearLayout.LayoutParams(0, dp(KEY_HEIGHT_DP).toInt(), weight).apply {
                val m = dp(2f).toInt()
                setMargins(m, m, m, m)
            }
            if (alternates == null) {
                pressable(this, onClick)
                return@apply
            }
            holdToOpen(this, LONG_PRESS_MS, onHold = { showAlternates(this, alternates) }, onTap = onClick)
        }

    /**
     * Types [onTap] when the finger lifts — or as soon as another key goes
     * down, whichever comes first. Fast typing rolls: the next finger lands
     * before the last one lifts, so "down, up, down, up" is really "down,
     * down, up, up", one gesture for a whole word. The key a new finger
     * lands on is never a hold of the previous one, and typing the previous
     * one there keeps the letters in the order they were pressed.
     *
     * Not `setOnClickListener`: a click needs the finger to lift inside the
     * key and knows nothing of the other keys, which is how a rolled key got
     * dropped. The click listener stays for TalkBack and switch access,
     * which never touch.
     */
    private fun pressable(key: View, onTap: () -> Unit, onDown: () -> Unit = {}, onUp: () -> Unit = {}) {
        key.setOnTouchListener { v, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    flushPendingTap()
                    v.drawableHotspotChanged(event.x, event.y)
                    v.isPressed = true
                    keyFeedback(v)
                    v.playSoundEffect(SoundEffectConstants.CLICK)
                    pendingTap = onTap
                    onDown()
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    v.isPressed = false
                    onUp()
                    if (pendingTap === onTap) {
                        if (event.actionMasked == MotionEvent.ACTION_UP) flushPendingTap() else pendingTap = null
                    }
                }
            }
            true
        }
        key.setOnClickListener { onTap() }
    }

    private fun flushPendingTap() {
        val tap = pendingTap ?: return
        pendingTap = null
        tap()
    }

    /**
     * A press held for [delayMs] runs [onHold]; a shorter one runs [onTap].
     *
     * Deliberately not `setOnLongClickListener`. That one waits for the
     * system's touch-and-hold delay, which is 500 ms by default but is a
     * per-device accessibility setting the user can raise — a keyboard whose
     * ñ needs a press as long as the phone was told to wait is a keyboard
     * that does not work on that phone. It also loses the gesture to any
     * scrolling parent the moment a finger drifts, which is what the paste
     * strip is.
     *
     * It vibrates when it fires, so the hold is felt before anything is seen.
     */
    private fun holdToOpen(key: TextView, delayMs: Long, onHold: () -> Unit, onTap: () -> Unit) {
        val open = Runnable {
            // The hold is the press: the release types nothing.
            pendingTap = null
            keyFeedback(key)
            onHold()
        }
        pressable(
            key,
            // Also what another key going down runs: a rolled key is a tap.
            onTap = { key.removeCallbacks(open); onTap() },
            onDown = {
                // The strip scrolls; a hold on one of its chips is not a
                // swipe and the scroll view must keep its hands off it.
                key.parent?.requestDisallowInterceptTouchEvent(true)
                key.postDelayed(open, delayMs)
            },
            onUp = { key.removeCallbacks(open) },
        )
    }

    /**
     * Repeats [action] while the key is held, after a pause — a backspace that
     * deletes one character per tap and nothing on a long press is the thing
     * people notice first about a keyboard that is not finished.
     */
    private fun holdToRepeat(key: TextView, action: () -> Unit) {
        lateinit var again: Runnable
        again = Runnable {
            // Repeating is the press: the release deletes nothing more.
            pendingTap = null
            action()
            key.postDelayed(again, REPEAT_EVERY_MS)
        }
        pressable(
            key,
            onTap = { key.removeCallbacks(again); action() },
            onDown = { key.postDelayed(again, REPEAT_AFTER_MS) },
            onUp = { key.removeCallbacks(again) },
        )
    }

    /**
     * The alternates of a long-pressed key, as a row floating above it —
     * what a phone keyboard does for the characters that don't fit on it.
     * Always a row to choose from, even for a single alternate: a long press
     * that types straight into the field gives no chance to see what it is
     * about to type, or to back out.
     *
     * It closes when one is picked, when a touch lands outside it, or on its
     * own after [ALTERNATES_TIMEOUT_MS] — a popup left open over the keys
     * would swallow the next keystroke.
     *
     * ponytail: one flat row, no repositioning near the screen edge; the keys
     * with alternates today sit mid-row.
     */
    private fun showAlternates(anchor: View, alternates: String) {
        popupNear(anchor, -(anchor.height + dp(KEY_HEIGHT_DP + 8f)).toInt()) { row, popup ->
            for (c in alternates) {
                val key = if (shift != Shift.OFF) c.uppercase() else c.toString()
                row.addView(
                    keyButton(key, 1f) { type(key); popup.dismiss() }.apply {
                        // Weighted widths collapse to 0 inside a WRAP_CONTENT parent.
                        layoutParams = LinearLayout.LayoutParams(dp(44f).toInt(), dp(KEY_HEIGHT_DP).toInt())
                    },
                )
            }
        }
    }

    /**
     * A row floating [yOffset] from the bottom of [anchor], filled by [fill],
     * which gets the row and the window so whatever it puts in there can
     * close it.
     */
    private fun popupNear(anchor: View, yOffset: Int, fill: (LinearLayout, PopupWindow) -> Unit) {
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            setBackgroundColor(KEYBOARD_BG)
        }
        val popup = PopupWindow(row, ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, true)
        fill(row, popup)
        popup.showAsDropDown(anchor, 0, yOffset)
        // Dismissing an already dismissed popup does nothing, so the picked
        // and the outside-touch cases need no cancelling.
        row.postDelayed({ popup.dismiss() }, ALTERNATES_TIMEOUT_MS)
    }

    /**
     * The key's label with its first alternate beside it, small and dim: a
     * long press nobody can see is a long press nobody uses. One character,
     * not all four of the period's — the hint says "hold me", the row that
     * opens says what is in there.
     */
    private fun labelWithHint(label: String, alternates: String): CharSequence {
        val hint = alternates.first().toString()
        val text = SpannableString("$label$hint")
        val from = label.length
        val to = text.length
        text.setSpan(RelativeSizeSpan(0.5f), from, to, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        text.setSpan(SuperscriptSpan(), from, to, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        text.setSpan(ForegroundColorSpan(HINT_COLOR), from, to, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        return text
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

    /**
     * The keys of a row. What shift and the dead key change is redrawn in
     * place by [relabel], never by rebuilding the row: a key rebuilt under a
     * finger that has not lifted yet takes that keystroke with it, and with
     * rolled typing there is nearly always one — the letter after a capital
     * was the one lost.
     */
    private fun buildKeyRow(keys: List<String>, withShift: Boolean = false): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            if (withShift) {
                // Click and hold are both wired below, so the key itself takes none.
                addView(
                    keyButton("", 1.5f) {}.also { key ->
                        holdToOpen(
                            key,
                            LONG_PRESS_MS,
                            // Holding locks it outright: a double tap is a
                            // rhythm the keyboard grades, and it fails the
                            // people who type slowly.
                            onHold = { shift = Shift.LOCKED; relabel() },
                            onTap = { onShift() },
                        )
                        relabels += {
                            key.text = when (shift) { Shift.LOCKED -> "⇪"; Shift.ONCE -> "⬆"; Shift.OFF -> "⇧" }
                            // The glyphs differ by a stroke, and not every font draws ⇪ at
                            // all: the colour is what says, at a glance, that the next
                            // letter is capital or every letter is.
                            val tint = when (shift) {
                                Shift.LOCKED -> SHIFT_LOCKED_COLOR
                                Shift.ONCE -> SHIFT_ONCE_COLOR
                                Shift.OFF -> KEY_COLOR
                            }
                            ((key.background as RippleDrawable).getDrawable(0) as GradientDrawable).setColor(tint)
                        }
                    },
                )
            }
            for (c in keys) {
                if (c == DEAD_ACUTE) {
                    addView(
                        keyButton("", 1f) { accentPending = !accentPending; relabel() }.also { key ->
                            relabels += { key.text = if (accentPending) "[´]" else "´" }
                        },
                    )
                    continue
                }
                val alternates = longPress[c]
                // Shift is read when the key is typed, not when it was drawn.
                addView(
                    keyButton(c, 1f, alternates) { type(shifted(c)) }.also { key ->
                        // Shifted too, so the hint on the key says what the row will
                        // actually give: Ñ over N, not ñ.
                        relabels += {
                            key.text = if (alternates == null) shifted(c) else labelWithHint(shifted(c), shifted(alternates))
                        }
                    },
                )
            }
        }

    private fun shifted(s: String): String = if (shift != Shift.OFF) s.uppercase() else s

    private fun relabel() = relabels.forEach { it() }

    private fun onShift() {
        val now = System.currentTimeMillis()
        shift = nextShift(shift, now - lastShiftTapMs)
        lastShiftTapMs = now
        relabel()
    }

    private fun type(key: String) {
        output(if (accentPending) withAcute(key) else key)
        if (shift == Shift.ONCE || accentPending) {
            shift = if (shift == Shift.ONCE) Shift.OFF else shift
            accentPending = false
            relabel()
        }
    }

    enum class Shift { OFF, ONCE, LOCKED }

    enum class Layer { LETTERS, SYMBOLS, EMOJI }

    /** Switches to [to], or back to the letters if it is already showing. */
    private fun toggleLayer(to: Layer) = setLayer(if (layer == to) Layer.LETTERS else to)

    private fun setLayer(to: Layer) {
        layer = to
        layerToggle.text = if (layer == Layer.LETTERS) "?123" else "abc"
        showLayer()
    }

    private fun showLayer() {
        keyRows.removeAllViews()
        relabels.clear()
        val rows = when (layer) {
            Layer.LETTERS -> LETTER_ROWS
            Layer.SYMBOLS -> SYMBOL_ROWS
            Layer.EMOJI -> EMOJI_ROWS
        }
        val withShift = layer == Layer.LETTERS
        rows.forEachIndexed { i, row -> keyRows.addView(buildKeyRow(row, withShift && i == rows.lastIndex)) }
        relabel()
    }

    private fun buildActionRow(): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            layerToggle = keyButton("?123", 1.4f) { toggleLayer(Layer.SYMBOLS) }
            addView(layerToggle)
            addView(keyButton("☺", 1f) { toggleLayer(Layer.EMOJI) })
            addView(keyButton(",", 1f) { type(",") })
            addView(keyButton(strings.getString(R.string.key_space), 2.5f) { output(" ") })
            addView(keyButton(".", 1f, PUNCT_ALTERNATES) { type(".") })
            addView(keyButton("⌫", 1.2f) { backspace() }.also { holdToRepeat(it) { backspace() } })
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
        private const val DEAD_ACUTE = "´"
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

        /**
         * The layer a field opens on. A field that declares itself numeric —
         * a transfer amount, a PIN, a phone number, a date — opens on the
         * numbers: making the user hunt for ?123 before every amount is the
         * keyboard ignoring what the app already told it.
         */
        fun initialLayer(inputType: Int): Layer = when (inputType and InputType.TYPE_MASK_CLASS) {
            InputType.TYPE_CLASS_NUMBER, InputType.TYPE_CLASS_PHONE, InputType.TYPE_CLASS_DATETIME -> Layer.SYMBOLS
            else -> Layer.LETTERS
        }

        private const val DOUBLE_TAP_MS = 400
        /** Under the system's 500 ms: holding ñ is a daily gesture here. */
        private const val LONG_PRESS_MS = 280L
        /** Longer than a key's: this one writes to a list that outlives the session. */
        private const val LEARN_PRESS_MS = 500L
        private const val REPEAT_AFTER_MS = 400L
        private const val REPEAT_EVERY_MS = 55L
        private const val ALTERNATES_TIMEOUT_MS = 5_000L
        private const val TAG = "LapachoIme"
        private const val TOP_N = 20
        private const val SUGGESTIONS = 3
        private const val MIN_PREFIX = 2
        /** Shorter than this is a fragment of a word, not a word to learn. */
        private const val MIN_LEARN = 4
        private const val LEARN_HINT = "+"

        /** Enough to hold the longest word anyone types before the cursor. */
        private const val WORD_LOOKBEHIND = 48
        private const val LABEL_MAX = 24
        private const val KEY_TEXT_DP = 20f
        private const val KEY_HEIGHT_DP = 46f
        private const val KEY_COLOR = 0xFF3C3C3C.toInt()
        private const val SHIFT_ONCE_COLOR = 0xFF5A5A5A.toInt()
        private const val SHIFT_LOCKED_COLOR = 0xFF2E7D32.toInt()
        private const val HINT_COLOR = 0xFF9E9E9E.toInt()
        private const val KEYBOARD_BG = 0xFF1E1E1E.toInt()
        private fun row(keys: String) = keys.map(Char::toString)
        private val LETTER_ROWS = listOf(row("qwertyuiop"), row("asdfghjkl"), row("zxcvbnm$DEAD_ACUTE"))
        private val SYMBOL_ROWS = listOf(row("1234567890"), row("@#\$%&-+()/"), row("<>[]{}=_|\\"), row("*\"':;!¡?¿"))
        /**
         * Emoji layer: the ones actually used in a chat, not a picker. No
         * search, no recents, no skin tones — that is a keyboard of its own.
         */
        private val EMOJI_ROWS = listOf(
            listOf("😀", "😂", "🥹", "😍", "😎", "🤔", "😅", "😭", "😡", "🙃"),
            listOf("👍", "👎", "🙏", "👏", "💪", "🤝", "✌️", "🫶", "👀", "🤷"),
            listOf("❤️", "🔥", "✨", "🎉", "✅", "❌", "⚠️", "💡", "📌", "🧉"),
        )
        /**
         * Long-press alternates the keyboard offers in any language; the
         * ones a language needs (ñ, ç, ß…) come from its dictionary header.
         * @ on the a: an address is typed in every language, and going to
         * the symbols layer for it breaks the word in two.
         */
        private val BASE_LONG_PRESS = mapOf("a" to "@")
        /** Long press on the period: Spanish needs the opening marks too. */
        private const val PUNCT_ALTERNATES = "¿?¡!"
        /** Marks that close a word: they sit on it, the space goes after. */
        private val CLOSING_MARKS = setOf(",", ".", "?", "!", ":", ";")
    }
}
