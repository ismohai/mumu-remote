package com.example.mumuremote

import android.app.AlertDialog
import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.widget.Button
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AppCompatActivity
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetSocketAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.max
import kotlin.math.min

class VideoActivity : AppCompatActivity() {
    private lateinit var imageView: ImageView
    private lateinit var statusText: TextView
    private lateinit var sideNav: LinearLayout
    private lateinit var edgeHandle: View

    private var host: String = "192.168.1.10"
    private var videoPort: Int = 5000
    private var controlPort: Int = 5001

    private val running = AtomicBoolean(false)
    private var videoSocket: DatagramSocket? = null
    private var videoThread: Thread? = null
    private var controlSocket: DatagramSocket? = null
    private val controlExecutor = Executors.newSingleThreadExecutor()
    private var edgeDownX = 0f
    private val uiHandler = Handler(Looper.getMainLooper())
    private val hideStatusRunnable = Runnable {
        statusText.visibility = View.GONE
    }

    data class VideoHeader(
        val sessionId: Int,
        val frameIndex: Int,
        val chunkIndex: Int,
        val chunkCount: Int,
        val timestampMicros: Long,
    )

    data class FrameAssembly(
        var chunkCount: Int,
        val chunks: MutableMap<Int, ByteArray>,
        var createdAtMs: Long,
    )

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_video)

        imageView = findViewById(R.id.video_surface)
        statusText = findViewById(R.id.status_text)
        sideNav = findViewById(R.id.side_nav)
        edgeHandle = findViewById(R.id.edge_handle)

        host = intent.getStringExtra("host") ?: host
        videoPort = intent.getIntExtra("videoPort", videoPort)
        controlPort = intent.getIntExtra("controlPort", controlPort)

        controlSocket = DatagramSocket()
        enterImmersiveMode()
        statusText.visibility = View.GONE

        setupSideNav()
        setupTouchForwarding()
        setupBackIntercept()
        startVideoReceiver()
    }

    private fun setupSideNav() {
        val btnBack: Button = findViewById(R.id.btn_back)
        val btnHome: Button = findViewById(R.id.btn_home)
        val btnRecent: Button = findViewById(R.id.btn_recent)
        val btnSettings: Button = findViewById(R.id.btn_settings)
        val btnDisconnect: Button = findViewById(R.id.btn_disconnect)

        btnBack.setOnClickListener {
            sendKeyEvent("back")
            sideNav.visibility = View.GONE
        }
        btnHome.setOnClickListener {
            sendKeyEvent("home")
            sideNav.visibility = View.GONE
        }
        btnRecent.setOnClickListener {
            sendKeyEvent("recent")
            sideNav.visibility = View.GONE
        }
        btnSettings.setOnClickListener { showSettingsDialog() }
        btnDisconnect.setOnClickListener {
            finish()
        }

        edgeHandle.setOnTouchListener { _, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    edgeDownX = event.rawX
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    val delta = edgeDownX - event.rawX
                    if (delta > 20f) {
                        sideNav.visibility = View.VISIBLE
                    }
                    true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> true
                else -> false
            }
        }
    }

    private fun showSettingsDialog() {
        val options = arrayOf("默认", "跟随模拟器", "720p@60", "1080p@90", "2K@90")
        AlertDialog.Builder(this)
            .setTitle("全局设置")
            .setItems(options) { _, which ->
                when (which) {
                    0 -> sendSetting("default", 60)
                    1 -> sendSetting("follow", 90)
                    2 -> sendSetting("720p", 60)
                    3 -> sendSetting("1080p", 90)
                    4 -> sendSetting("2k", 90)
                }
            }
            .setNegativeButton("取消", null)
            .show()
    }

    private fun enterImmersiveMode() {
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
            window.insetsController?.let { controller ->
                controller.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                controller.systemBarsBehavior =
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = (
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_FULLSCREEN
                    or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                )
        }
    }

    private fun showTransientStatus(message: String) {
        statusText.text = message
        statusText.visibility = View.VISIBLE
        uiHandler.removeCallbacks(hideStatusRunnable)
        uiHandler.postDelayed(hideStatusRunnable, 1500)
    }

    private fun setupBackIntercept() {
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                sendKeyEvent("back")
            }
        })
    }

    private fun setupTouchForwarding() {
        imageView.setOnTouchListener { _, event ->
            val w = max(1, imageView.width)
            val h = max(1, imageView.height)
            val xNorm = clamp(event.x / w)
            val yNorm = clamp(event.y / h)

            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    if (sideNav.visibility == View.VISIBLE) {
                        sideNav.visibility = View.GONE
                    }
                    sendTouchEvent("down", xNorm, yNorm)
                }
                MotionEvent.ACTION_MOVE -> sendTouchEvent("move", xNorm, yNorm)
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> sendTouchEvent("up", xNorm, yNorm)
            }
            true
        }
    }

    private fun startVideoReceiver() {
        running.set(true)
        videoThread = Thread {
            val frameMap = HashMap<Int, FrameAssembly>()
            val packetBuffer = ByteArray(65535)

            try {
                val socket = DatagramSocket(videoPort)
                socket.soTimeout = 400
                videoSocket = socket

                while (running.get()) {
                    try {
                        val packet = DatagramPacket(packetBuffer, packetBuffer.size)
                        socket.receive(packet)

                        val header = parseHeader(packet.data, packet.length) ?: continue
                        if (packet.length <= HEADER_SIZE) continue

                        val payload = packet.data.copyOfRange(HEADER_SIZE, packet.length)
                        val frame = frameMap.getOrPut(header.frameIndex) {
                            FrameAssembly(header.chunkCount, HashMap(), System.currentTimeMillis())
                        }
                        frame.chunkCount = header.chunkCount
                        frame.createdAtMs = System.currentTimeMillis()
                        frame.chunks[header.chunkIndex] = payload

                        if (frame.chunkCount > 0 && frame.chunks.size >= frame.chunkCount) {
                            val fullFrame = mergeFrame(frame)
                            if (fullFrame != null) {
                                val bitmap = BitmapFactory.decodeByteArray(fullFrame, 0, fullFrame.size)
                                if (bitmap != null) {
                                    runOnUiThread {
                                        imageView.setImageBitmap(bitmap)
                                        statusText.visibility = View.GONE
                                    }
                                }
                            }
                            frameMap.remove(header.frameIndex)
                        }

                        pruneExpiredFrames(frameMap)
                    } catch (_: java.net.SocketTimeoutException) {
                    }
                }
            } catch (e: Exception) {
                runOnUiThread {
                    showTransientStatus("视频接收异常: ${e.message}")
                }
            } finally {
                videoSocket?.close()
                videoSocket = null
            }
        }
        videoThread?.name = "video-recv"
        videoThread?.start()
    }

    private fun parseHeader(data: ByteArray, length: Int): VideoHeader? {
        if (length < HEADER_SIZE) return null
        val bb = ByteBuffer.wrap(data, 0, HEADER_SIZE).order(ByteOrder.BIG_ENDIAN)
        return VideoHeader(
            sessionId = bb.int,
            frameIndex = bb.int,
            chunkIndex = bb.short.toInt() and 0xFFFF,
            chunkCount = bb.short.toInt() and 0xFFFF,
            timestampMicros = bb.long,
        )
    }

    private fun mergeFrame(frame: FrameAssembly): ByteArray? {
        val chunks = ArrayList<ByteArray>(frame.chunkCount)
        var total = 0
        for (i in 0 until frame.chunkCount) {
            val chunk = frame.chunks[i] ?: return null
            chunks.add(chunk)
            total += chunk.size
        }

        val output = ByteArray(total)
        var offset = 0
        for (chunk in chunks) {
            System.arraycopy(chunk, 0, output, offset, chunk.size)
            offset += chunk.size
        }
        return output
    }

    private fun pruneExpiredFrames(frameMap: MutableMap<Int, FrameAssembly>) {
        val now = System.currentTimeMillis()
        val iterator = frameMap.entries.iterator()
        while (iterator.hasNext()) {
            val entry = iterator.next()
            if (now - entry.value.createdAtMs > 1500) {
                iterator.remove()
            }
        }
    }

    private fun sendTouchEvent(phase: String, x: Float, y: Float) {
        val payload =
            "{\"type\":\"touch\",\"phase\":\"$phase\",\"x\":${formatFloat(x)},\"y\":${formatFloat(y)}}"
        sendControlPacket(payload)
    }

    private fun sendKeyEvent(key: String) {
        val payload = "{\"type\":\"key\",\"key\":\"$key\"}"
        sendControlPacket(payload)
    }

    private fun sendSetting(resolution: String, fps: Int) {
        val payload = "{\"type\":\"setting\",\"resolution\":\"$resolution\",\"fps\":$fps}"
        sendControlPacket(payload)
        showTransientStatus("设置已发送: $resolution @ $fps")
    }

    private fun sendControlPacket(payload: String) {
        controlExecutor.execute {
            try {
                val socket = controlSocket ?: return@execute
                val data = payload.toByteArray(Charsets.UTF_8)
                val packet = DatagramPacket(data, data.size, InetSocketAddress(host, controlPort))
                socket.send(packet)
            } catch (_: Exception) {
            }
        }
    }

    private fun clamp(value: Float): Float {
        return min(1f, max(0f, value))
    }

    private fun formatFloat(value: Float): String {
        return String.format(java.util.Locale.US, "%.4f", value)
    }

    override fun onDestroy() {
        running.set(false)
        videoSocket?.close()
        try {
            videoThread?.join(200)
        } catch (_: InterruptedException) {
        }
        controlSocket?.close()
        controlExecutor.shutdownNow()
        uiHandler.removeCallbacks(hideStatusRunnable)
        super.onDestroy()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) {
            enterImmersiveMode()
        }
    }

    companion object {
        private const val HEADER_SIZE = 20
    }
}
