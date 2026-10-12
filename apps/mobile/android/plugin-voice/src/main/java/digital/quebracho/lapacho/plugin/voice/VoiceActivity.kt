package digital.quebracho.lapacho.plugin.voice

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.util.Log
import android.util.TypedValue
import android.view.MotionEvent
import android.view.View
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import java.io.ByteArrayOutputStream
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.Future
import uniffi.lapacho_voice_bridge.VoiceException
import uniffi.lapacho_voice_bridge.VoiceModel
import uniffi.lapacho_voice_bridge.importModel
import uniffi.lapacho_voice_bridge.modelFiles

/**
 * Hold to talk, let go: the text goes to the clipboard and this window
 * closes. Lapacho's keyboard comes back, reads the clipboard as it does on
 * every show, and offers the text in its strip; nothing is typed without a
 * tap. The audio is kept in memory only and zeroed once recognized.
 *
 * The model (`lapacho-voice`, the desktop's) is loaded while you speak and
 * dropped with the window: it takes ~1 GB, too much to keep for later.
 */
class VoiceActivity : Activity() {
    private val modelDir by lazy { File(filesDir, "voice") }
    private val work = Executors.newSingleThreadExecutor()
    private lateinit var status: TextView
    private lateinit var talk: Button
    private lateinit var importButton: Button
    private lateinit var downloadButton: Button
    private val ui = Handler(Looper.getMainLooper())

    @Volatile private var recording = false
    private var recorder: Thread? = null
    private var model: Future<VoiceModel>? = null
    private val pcm = ByteArrayOutputStream()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pad = dp(20f)
        status = TextView(this).apply { setPadding(0, 0, 0, pad) }
        talk = Button(this).apply {
            setText(R.string.hold_to_talk)
            minHeight = dp(96f)
            setOnTouchListener(::onTalkTouch)
        }
        importButton = Button(this).apply {
            setText(R.string.import_model)
            setOnClickListener { pickModel() }
        }
        // The browser downloads it: this app has no internet permission.
        downloadButton = Button(this).apply {
            setText(R.string.download_model)
            setOnClickListener { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(MODEL_URL))) }
        }
        setContentView(LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad)
            addView(status)
            addView(talk)
            addView(downloadButton)
            addView(importButton)
        })
        showState()
    }

    override fun onDestroy() {
        ui.removeCallbacksAndMessages(null)
        recording = false
        work.shutdownNow()
        super.onDestroy()
    }

    private fun hasModel() = modelFiles().all { File(modelDir, it).isFile }

    private fun showState() {
        ui.removeCallbacksAndMessages(null)
        val busy = importer?.isAlive == true
        val ready = !busy && hasModel()
        talk.visibility = if (ready) View.VISIBLE else View.GONE
        importButton.visibility = if (ready) View.GONE else View.VISIBLE
        downloadButton.visibility = importButton.visibility
        importButton.isEnabled = !busy
        downloadButton.isEnabled = !busy
        // Unpacking takes minutes on a phone; with the screen off the CPU
        // sleeps and it stalls, so the screen stays on while it runs.
        if (busy) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        when {
            busy -> {
                status.text = getString(R.string.importing, progress())
                ui.postDelayed({ showState() }, 1000)
            }
            importError != null -> fail(importError!!).also { importError = null }
            else -> status.setText(if (ready) R.string.ready else R.string.no_model)
        }
    }

    /** How much of the model is on disk, in percent: what is written so far
     *  against the four files' known size. 99 at most until it is done. */
    private fun progress(): Int {
        val written = modelDir.listFiles()?.sumOf { it.length() } ?: 0L
        return (written * 100 / MODEL_BYTES).toInt().coerceIn(0, 99)
    }

    @SuppressLint("ClickableViewAccessibility") // the press itself is the action: hold to talk
    private fun onTalkTouch(v: View, e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
                    status.setText(R.string.no_permission)
                    requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 1)
                } else {
                    v.isPressed = true
                    start()
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                v.isPressed = false
                if (recording) stop()
            }
        }
        return true
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, results: IntArray) {
        if (results.firstOrNull() == PackageManager.PERMISSION_GRANTED) showState()
    }

    @SuppressLint("MissingPermission") // checked in onTalkTouch
    private fun start() {
        val dir = modelDir.path
        model = work.submit<VoiceModel> { VoiceModel(dir, THREADS) }
        val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val rec = AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE,
            AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, min * 4)
        pcm.reset()
        recording = true
        status.setText(R.string.listening)
        recorder = Thread {
            val buf = ByteArray(min)
            rec.startRecording()
            while (recording) {
                val n = rec.read(buf, 0, buf.size)
                if (n > 0) pcm.write(buf, 0, n)
            }
            rec.stop()
            rec.release()
            buf.fill(0)
        }.also { it.start() }
    }

    private fun stop() {
        recording = false
        status.setText(R.string.recognizing)
        talk.isEnabled = false
        val pending = model
        Thread {
            recorder?.join()
            // ponytail: toByteArray copies; the stream's own buffer can't be
            // zeroed through its API, so it is dropped with this window.
            val audio = pcm.toByteArray()
            pcm.reset()
            Log.i(TAG, "recorded %.1f s, peak %d".format(audio.size / (RATE * 2.0), peak(audio)))
            val text = try {
                if (audio.size < RATE * 2 * MIN_SECONDS) "" // a tap, not speech
                else pending!!.get().use { it.transcribe(RATE, audio) }
            } catch (e: Exception) {
                runOnUiThread { fail(e) }
                return@Thread
            } finally {
                audio.fill(0)
            }
            runOnUiThread { done(text) }
        }.start()
    }

    private fun done(text: String) {
        talk.isEnabled = true
        if (text.isBlank()) {
            status.setText(R.string.nothing_heard)
            return
        }
        val clipboard = getSystemService(CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText(getString(R.string.app_name), text))
        finish()
    }

    private fun fail(e: Exception) {
        talk.isEnabled = true
        val reason = (e.cause as? VoiceException ?: e).message ?: e.javaClass.simpleName
        status.text = getString(R.string.error, reason)
    }

    /** The system picker, opened on Download (see the app's OpenInDownloads). */
    private fun pickModel() {
        startActivityForResult(
            Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("*/*")
                .putExtra(
                    DocumentsContract.EXTRA_INITIAL_URI,
                    DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:Download"),
                ),
            PICK_MODEL,
        )
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        val uri = data?.data
        if (requestCode != PICK_MODEL || resultCode != RESULT_OK || uri == null) return
        // What an interrupted import left behind would count as progress.
        modelDir.listFiles()?.forEach { it.delete() }
        val dir = modelDir.path
        val fd = try {
            contentResolver.openFileDescriptor(uri, "r")!!.detachFd()
        } catch (e: Exception) {
            fail(e)
            return
        }
        // Kept outside this window: leaving and coming back while it runs
        // finds it here and shows its progress instead of offering it again.
        importer = Thread {
            try {
                importModel(fd, dir)
            } catch (e: Exception) {
                importError = e
            }
        }.also { it.start() }
        showState()
    }

    /** Loudest sample: says whether the microphone heard anything, not what. */
    private fun peak(pcm: ByteArray): Int {
        var max = 0
        for (i in 0 until pcm.size - 1 step 2) {
            max = maxOf(max, kotlin.math.abs((pcm[i].toInt() and 0xff) or (pcm[i + 1].toInt() shl 8)))
        }
        return max
    }

    private fun dp(v: Float) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, resources.displayMetrics).toInt()

    companion object {
        @Volatile private var importer: Thread? = null
        @Volatile private var importError: Exception? = null
        private const val MODEL_URL = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2"
        private const val MODEL_BYTES = 670_478_772L // the four MODEL_FILES, unpacked
        private const val TAG = "LapachoVoice"
        private const val RATE = 16_000
        private const val THREADS = 4
        private const val MIN_SECONDS = 0.3
        private const val PICK_MODEL = 1
    }
}
