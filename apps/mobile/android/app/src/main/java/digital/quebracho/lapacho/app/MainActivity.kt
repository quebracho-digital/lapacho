package digital.quebracho.lapacho.app

import android.os.Bundle
import android.view.View
import android.view.WindowManager
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.ListView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import digital.quebracho.lapacho.storage.ClipboardItem
import digital.quebracho.lapacho.storage.HistoryRepo
import digital.quebracho.lapacho.storage.PersistLevel
import digital.quebracho.lapacho.storage.Sensitivity
import digital.quebracho.lapacho.storage.contentId

/**
 * Companion app — P0 spike only. Its whole job here is to prove the storage
 * model: write an item, then let [digital.quebracho.lapacho.ime.LapachoIme]
 * read it back from a *different* process, even after this process (or the
 * IME's) has been force-stopped. See docs/ARQUITECTURA_MOBILE_ANDROID.md §9
 * P0 exit criteria.
 *
 * No classification, no PersistLevel/TTL settings UI yet — those arrive with
 * the P1 uniffi bridge to `lapacho-core`. Everything here is
 * [Sensitivity.NONE] / [PersistLevel.ALL] so nothing is filtered out while
 * proving multi-process read/write.
 */
class MainActivity : AppCompatActivity() {

    private lateinit var repo: HistoryRepo
    private lateinit var adapter: ArrayAdapter<String>

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

        findViewById<Button>(R.id.save_button).setOnClickListener {
            val text = input.text.toString()
            if (text.isNotBlank()) {
                save(text)
                input.text.clear()
                refresh()
            }
        }

        refresh()
    }

    private fun save(text: String) {
        val item = ClipboardItem(
            id = contentId(text),
            rawContent = text,
            displayContent = text,
            contentType = "text",
            sensitivity = Sensitivity.NONE,
            detectedType = "Text",
            timestamp = System.currentTimeMillis() / 1000,
        )
        repo.save(item, PersistLevel.ALL)
    }

    private fun refresh() {
        val items = repo.loadTopN(20)
        adapter.clear()
        adapter.addAll(items.map { "${it.displayContent}  ·  id=${it.id.take(8)}…" })
    }
}
