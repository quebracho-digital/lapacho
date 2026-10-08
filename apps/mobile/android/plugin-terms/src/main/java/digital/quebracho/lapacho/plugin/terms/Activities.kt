package digital.quebracho.lapacho.plugin.terms

import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.text.InputType
import android.util.TypedValue
import android.view.Gravity
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import android.widget.Toast
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/**
 * Where to send the terms. ponytail: plain app-private preferences, the token
 * included; only this app can read them. Android's Keystore is the upgrade if
 * the token ever guards more than a test server.
 */
private class Settings(context: Context) {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)
    var endpoint: String
        get() = prefs.getString("endpoint", "").orEmpty()
        set(v) = prefs.edit().putString("endpoint", v.trim()).apply()
    var model: String
        get() = prefs.getString("model", "").orEmpty()
        set(v) = prefs.edit().putString("model", v.trim()).apply()
    var token: String
        get() = prefs.getString("token", "").orEmpty()
        set(v) = prefs.edit().putString("token", v.trim()).apply()
    val configured get() = endpoint.startsWith("http://") || endpoint.startsWith("https://")
}

private fun Activity.dp(v: Float) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, resources.displayMetrics).toInt()

private fun Activity.column(): LinearLayout = LinearLayout(this).apply {
    orientation = LinearLayout.VERTICAL
    val pad = dp(20f)
    setPadding(pad, pad * 2, pad, pad)
}

/** The launcher screen: where the terms go, and how to authenticate there. */
class SettingsActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val s = Settings(this)
        val root = column()
        fun field(hint: Int, value: String, secret: Boolean = false) = EditText(this).apply {
            setHint(hint)
            setText(value)
            isSingleLine = true
            if (secret) inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            root.addView(this)
        }
        root.addView(TextView(this).apply { setText(R.string.settings_intro) })
        val endpoint = field(R.string.hint_endpoint, s.endpoint)
        val model = field(R.string.hint_model, s.model)
        root.addView(Button(this).apply {
            setText(R.string.choose_model)
            setOnClickListener { chooseModel(endpoint.text.toString().trim(), token.text.toString().trim(), model) }
        })
        val token = field(R.string.hint_token, s.token, secret = true)
        root.addView(Button(this).apply {
            setText(R.string.save)
            setOnClickListener {
                val e = endpoint.text.toString().trim()
                if (!e.startsWith("http://") && !e.startsWith("https://")) {
                    Toast.makeText(this@SettingsActivity, R.string.err_endpoint, Toast.LENGTH_LONG).show()
                    return@setOnClickListener
                }
                s.endpoint = e
                s.model = model.text.toString()
                s.token = token.text.toString()
                Toast.makeText(this@SettingsActivity, R.string.saved, Toast.LENGTH_SHORT).show()
            }
        })
        setContentView(root)
    }

    /** Asks the server which models it serves and puts the one picked in [into]. */
    private fun chooseModel(endpoint: String, token: String, into: EditText) {
        if (!endpoint.startsWith("http://") && !endpoint.startsWith("https://")) {
            return Toast.makeText(this, R.string.err_endpoint, Toast.LENGTH_LONG).show()
        }
        if (Terms.isDrupal(endpoint)) return Toast.makeText(this, R.string.err_drupal_models, Toast.LENGTH_LONG).show()
        Toast.makeText(this, R.string.loading_models, Toast.LENGTH_SHORT).show()
        Thread {
            val result = runCatching {
                Terms.modelIds(http(Terms.modelsUrl(endpoint), token, timeoutMs = 15_000))
                    ?: throw IOException(getString(R.string.err_answer, endpoint))
            }
            runOnUiThread {
                if (isFinishing) return@runOnUiThread
                result.fold(
                    onSuccess = { ids ->
                        if (ids.isEmpty()) return@fold Toast.makeText(this, R.string.err_no_models, Toast.LENGTH_LONG).show()
                        AlertDialog.Builder(this)
                            .setTitle(R.string.choose_model)
                            .setItems(ids.toTypedArray()) { _, i -> into.setText(ids[i]) }
                            .show()
                    },
                    onFailure = { e -> Toast.makeText(this, e.message ?: e.javaClass.simpleName, Toast.LENGTH_LONG).show() },
                )
            }
        }.start()
    }
}

/**
 * What Lapacho opens with a clip. A model needs a minute or two for a whole
 * document, so this shows that it is working, runs the request off the main
 * thread, and hands the report back. It survives a rotation (configChanges in
 * the manifest); leaving it cancels the run.
 */
class RunActivity : Activity() {
    @Volatile private var cancelled = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val clip = intent.getStringExtra(Contract.EXTRA_TEXT)
        val s = Settings(this)
        if (clip.isNullOrBlank()) return fail(getString(R.string.err_nothing))
        if (!s.configured) {
            Toast.makeText(this, R.string.err_not_configured, Toast.LENGTH_LONG).show()
            startActivity(Intent(this, SettingsActivity::class.java))
            return fail(getString(R.string.err_not_configured))
        }
        val root = column().apply { gravity = Gravity.CENTER_HORIZONTAL }
        root.addView(ProgressBar(this))
        root.addView(TextView(this).apply {
            text = getString(R.string.working, s.endpoint)
            gravity = Gravity.CENTER
        })
        root.addView(Button(this).apply {
            setText(android.R.string.cancel)
            setOnClickListener { cancelled = true; fail(getString(R.string.err_cancelled)) }
        })
        setContentView(root)

        val template = assets.open("terms_request.txt").bufferedReader().use { it.readText() }
        Thread {
            val result = runCatching { analyse(clip, template, s) }
            runOnUiThread {
                if (cancelled || isFinishing) return@runOnUiThread
                result.fold(
                    onSuccess = { report ->
                        setResult(RESULT_OK, Intent().putExtra(Contract.EXTRA_TEXT, report))
                        finish()
                    },
                    onFailure = { e -> fail(e.message ?: e.javaClass.simpleName) },
                )
            }
        }.start()
    }

    private fun fail(message: String) {
        setResult(RESULT_CANCELED, Intent().putExtra(Contract.EXTRA_ERROR, message))
        finish()
    }

    private fun analyse(text: String, template: String, s: Settings): String =
        if (Terms.isDrupal(s.endpoint)) {
            Terms.drupalReport(http(s.endpoint, s.token, Terms.drupalBody(text)))
                ?: throw IOException(getString(R.string.err_answer, s.endpoint))
        } else {
            val answer = Terms.chatAnswer(http(Terms.chatUrl(s.endpoint), s.token, Terms.chatBody(Terms.request(template, text), s.model)))
                ?: throw IOException(getString(R.string.err_answer, s.endpoint))
            "$answer\n\n(${s.model.ifBlank { "model" }}, ${s.endpoint})"
        }
}

/** A POST of [body] as JSON, or a GET without one; the reply, or an IOException with the server's reason. */
private fun http(url: String, token: String, body: String? = null, timeoutMs: Int = 600_000): String {
    val c = URL(url).openConnection() as HttpURLConnection
    try {
        c.connectTimeout = 15_000
        // A whole document on a home server takes a minute or two.
        c.readTimeout = timeoutMs
        c.setRequestProperty("Accept", "application/json")
        Terms.authHeader(token)?.let { c.setRequestProperty("Authorization", it) }
        if (body != null) {
            c.requestMethod = "POST"
            c.doOutput = true
            c.setRequestProperty("Content-Type", "application/json")
            c.outputStream.use { it.write(body.toByteArray()) }
        }
        val code = c.responseCode
        val stream = if (code in 200..299) c.inputStream else c.errorStream
        val reply = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
        if (code !in 200..299) throw IOException("HTTP $code: ${Terms.serverError(reply)}")
        return reply
    } finally {
        c.disconnect()
    }
}
