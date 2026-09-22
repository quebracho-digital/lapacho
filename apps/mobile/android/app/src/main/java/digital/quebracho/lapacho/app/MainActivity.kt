package digital.quebracho.lapacho.app

import android.content.ClipData
import android.content.ClipboardManager
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
import android.view.View
import android.view.WindowManager
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.ListView
import android.widget.TextView
import android.util.TypedValue
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.widget.doAfterTextChanged
import digital.quebracho.lapacho.EXTRA_IS_SENSITIVE
import digital.quebracho.lapacho.classify
import digital.quebracho.lapacho.isMasked
import digital.quebracho.lapacho.loadPredictor
import digital.quebracho.lapacho.matchesQuery
import digital.quebracho.lapacho.storage.ClipboardItem
import digital.quebracho.lapacho.storage.HISTORY_MAX
import digital.quebracho.lapacho.storage.HistoryRepo
import digital.quebracho.lapacho.storage.PersistLevel
import digital.quebracho.lapacho.storage.contentId

/**
 * Companion app — P0 spike only. Its whole job here is to prove the storage
 * model: write an item, then let [digital.quebracho.lapacho.ime.LapachoIme]
 * read it back from a *different* process, even after this process (or the
 * IME's) has been force-stopped. See docs/ARQUITECTURA_MOBILE_ANDROID.md §9
 * P0 exit criteria.
 *
 * Items are classified by `lapacho-core` (through the uniffi bridge) and
 * credentials/secrets are shown masked, but everything saved here is
 * [PersistLevel.ALL]: no persistence levels or TTL settings UI yet — those
 * arrive with the rest of the P1 migration.
 */
class MainActivity : AppCompatActivity() {

    private lateinit var repo: HistoryRepo
    private lateinit var adapter: ArrayAdapter<String>
    private lateinit var search: EditText
    private var all: List<ClipboardItem> = emptyList()
    private var shown: List<ClipboardItem> = emptyList()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // The history is on screen here: keep it out of screenshots, screen
        // recording and the recents thumbnail.
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        setContentView(R.layout.activity_main)
        // Shows which build is installed: APKs are sideloaded from a URL a CDN may cache.
        findViewById<TextView>(R.id.header).text =
            "Lapacho ${packageManager.getPackageInfo(packageName, 0).versionName} — companion (P0 spike)"
        // Targeting API 35 draws edge-to-edge: keep the content clear of the
        // status bar, the navigation bar and the keyboard.
        val root = findViewById<View>(R.id.root)
        val pad = root.paddingTop
        ViewCompat.setOnApplyWindowInsetsListener(root) { v, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.ime())
            v.setPadding(pad + bars.left, pad + bars.top, pad + bars.right, pad + bars.bottom)
            insets
        }

        repo = HistoryRepo(applicationContext)

        val input = findViewById<EditText>(R.id.input)
        val historyList = findViewById<ListView>(R.id.history_list)
        adapter = ArrayAdapter(this, android.R.layout.simple_list_item_1, mutableListOf())
        historyList.adapter = adapter
        historyList.setOnItemClickListener { _, _, position, _ -> copy(shown[position]) }

        findViewById<Button>(R.id.clear_button).setOnClickListener { confirmClear() }
        findViewById<Button>(R.id.words_button).setOnClickListener { showLexicon() }

        search = findViewById(R.id.search)
        search.doAfterTextChanged { applyFilter() }

        findViewById<Button>(R.id.save_button).setOnClickListener {
            val text = input.text.toString()
            if (text.isNotBlank()) {
                save(text)
                input.text.clear()
                refresh()
            }
        }
    }

    // The keyboard writes to the same DB from its own process, so the list
    // must be re-read every time the app comes back, not only when created.
    override fun onResume() {
        super.onResume()
        refresh()
    }

    private fun save(text: String) {
        val item = ClipboardItem(
            id = contentId(text),
            rawContent = text,
            displayContent = text,
            contentType = "text",
            sensitivity = classify(text),
            detectedType = "Text",
            timestamp = System.currentTimeMillis() / 1000,
        )
        repo.save(item, PersistLevel.ALL)
        repo.cleanup(sensitiveTtlSecs = null, maxItems = HISTORY_MAX)
    }

    private fun refresh() {
        all = repo.loadTopN(HISTORY_MAX)
        applyFilter()
    }

    // Masked items stay listed with no query, but never match one: typing
    // part of an old password must not reveal that it is stored.
    private fun applyFilter() {
        val query = search.text.toString()
        shown = if (query.isBlank()) all else all.filter { !it.isMasked() && matchesQuery(it.displayContent, query) }
        adapter.clear()
        adapter.addAll(shown.map(::label))
    }

    /**
     * Wipes the history and empties the clipboard too: otherwise the clip
     * still on it would be captured again the next time the keyboard opens,
     * and the "cleared" history would come back with it.
     */
    private fun confirmClear() {
        AlertDialog.Builder(this)
            .setTitle("Borrar historial")
            .setMessage("Se borran todos los items guardados y se vacía el portapapeles. No se puede deshacer.")
            .setPositiveButton("Borrar") { _, _ ->
                repo.clear()
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).clearPrimaryClip()
                }
                refresh()
            }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    /**
     * Every word the keyboard was taught, in full, with a way to take each one
     * back. This screen is the point of learning words one at a time: a
     * personal lexicon someone can read from end to end is a promise that can
     * be checked, which is not true of a language model.
     */
    private fun showLexicon() {
        val words = repo.lexicon()
        if (words.isEmpty()) {
            AlertDialog.Builder(this)
                .setTitle("Palabras aprendidas")
                .setMessage(
                    "Ninguna todavía.\n\n${dictionaryStatus(words)}\n\nEn el teclado, escribí una palabra " +
                        "que el diccionario no conozca, mantené apretada la palabra en la tira de arriba " +
                        "y tocá «aprender».",
                )
                .setPositiveButton("Entendido", null)
                .show()
            return
        }
        AlertDialog.Builder(this)
            .setCustomTitle(dialogHeader("Palabras aprendidas (${words.size})\n\n${dictionaryStatus(words)}"))
            .setItems(words.toTypedArray()) { _, position -> confirmForget(words[position]) }
            .setNeutralButton("Olvidar todas") { _, _ -> confirmForgetAll(words.size) }
            .setNegativeButton("Cerrar", null)
            .show()
    }

    private fun dialogHeader(text: String): TextView = TextView(this).apply {
        this.text = text
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
        val pad = (16 * resources.displayMetrics.density).toInt()
        setPadding(pad + pad / 2, pad, pad, pad / 2)
    }

    /**
     * What the keyboard would have to work with, asked here because the
     * keyboard cannot be asked: it runs in another process and has no screen
     * of its own to report on. Loads the dictionary the same way the IME
     * does and completes a real prefix with it, so "learned but never
     * suggested" can be told from "the dictionary never loaded".
     */
    private fun dictionaryStatus(words: List<String>): String = try {
        val predictor = loadPredictor(this, words)
        val bundled = predictor.size().toInt() - words.size
        val probe = words.firstOrNull()
        val test = probe?.let {
            val prefix = it.take(maxOf(2, it.length - 3))
            val hits = predictor.suggest(prefix, 3u)
            "Prueba: «$prefix» → ${if (hits.isEmpty()) "(nada)" else hits.joinToString(", ")}"
        }
        listOfNotNull("Diccionario: $bundled palabras", test).joinToString("\n")
    } catch (e: Exception) {
        "Diccionario: NO CARGA (${e.javaClass.simpleName}: ${e.message})"
    }

    private fun confirmForget(word: String) {
        AlertDialog.Builder(this)
            .setTitle("Olvidar «$word»")
            .setMessage("El teclado deja de sugerirla. Podés volver a enseñársela cuando quieras.")
            .setPositiveButton("Olvidar") { _, _ -> repo.forget(word); showLexicon() }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    private fun confirmForgetAll(count: Int) {
        AlertDialog.Builder(this)
            .setTitle("Olvidar todas")
            .setMessage("Se borran las $count palabras aprendidas. El diccionario que vino con la app no se toca.")
            .setPositiveButton("Olvidar todas") { _, _ -> repo.forgetAll() }
            .setNegativeButton("Cancelar", null)
            .show()
    }

    // Masked items all look alike, so they carry a short id to tell them
    // apart; for the rest the content itself does that.
    private fun label(item: ClipboardItem): String =
        if (item.isMasked()) "🔑 ••••••  ·  id=${item.id.take(8)}…" else item.displayContent

    /**
     * Puts the item back on the clipboard, raw. A masked one goes out flagged
     * as sensitive, so the keyboard treats it as a secret and Android hides
     * its preview. Single entry point for acting on an item: plugins will hang
     * off here.
     */
    private fun copy(item: ClipboardItem) {
        val clip = ClipData.newPlainText("lapacho", item.rawContent)
        if (item.isMasked()) {
            clip.description.extras = PersistableBundle().apply { putBoolean(EXTRA_IS_SENSITIVE, true) }
        }
        (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
        // Android 13+ shows its own confirmation; a toast there would say it twice.
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            Toast.makeText(this, "Copiado", Toast.LENGTH_SHORT).show()
        }
    }
}
