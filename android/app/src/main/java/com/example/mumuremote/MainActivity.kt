package com.example.mumuremote

import android.app.AlertDialog
import android.content.Intent
import android.os.Bundle
import android.os.Build
import android.provider.Settings
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import org.json.JSONObject
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetSocketAddress
import java.net.SocketTimeoutException
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class MainActivity : AppCompatActivity() {
    private lateinit var btnStartPair: Button
    private lateinit var txtPairStatus: TextView

    private val pairingRunning = AtomicBoolean(false)
    private var pairingSocket: DatagramSocket? = null
    private var pairingThread: Thread? = null
    private val sendExecutor = Executors.newSingleThreadExecutor()

    @Volatile
    private var pendingScanRequestId: String? = null

    data class IncomingPcPairRequest(
        val requestId: String,
        val deviceName: String,
        val senderIp: String,
        val senderPort: Int,
        val videoPort: Int,
        val controlPort: Int,
    )

    private val scanLauncher = registerForActivityResult(ScanContract()) { result ->
        val content = result.contents ?: return@registerForActivityResult
        startScanPairing(content)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        btnStartPair = findViewById(R.id.btn_start_pair)
        txtPairStatus = findViewById(R.id.txt_pair_status)

        btnStartPair.setOnClickListener {
            val options = ScanOptions()
                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt("扫描电脑端二维码进行配对")
                .setBeepEnabled(false)
                .setOrientationLocked(false)
            scanLauncher.launch(options)
        }

        startPairingListener()
        txtPairStatus.text = "等待配对（可被电脑自动发现）"
    }

    private fun startPairingListener() {
        if (pairingRunning.get()) {
            return
        }

        pairingRunning.set(true)
        pairingThread = Thread {
            try {
                val socket = DatagramSocket(PAIR_PORT)
                socket.soTimeout = 500
                pairingSocket = socket

                val buffer = ByteArray(4096)
                while (pairingRunning.get()) {
                    try {
                        val packet = DatagramPacket(buffer, buffer.size)
                        socket.receive(packet)
                        val payload = String(packet.data, 0, packet.length, Charsets.UTF_8)
                        handlePairingMessage(payload, packet.address.hostAddress ?: "", packet.port)
                    } catch (_: SocketTimeoutException) {
                    }
                }
            } catch (e: Exception) {
                runOnUiThread {
                    txtPairStatus.text = "配对监听失败: ${e.message}"
                }
            } finally {
                pairingSocket?.close()
                pairingSocket = null
            }
        }
        pairingThread?.name = "pairing-listener"
        pairingThread?.start()
    }

    private fun handlePairingMessage(raw: String, senderIp: String, senderPort: Int) {
        val json = try {
            JSONObject(raw)
        } catch (_: Exception) {
            return
        }

        when (json.optString("type")) {
            "discover_probe" -> {
                sendDiscoverResponse(senderIp, senderPort)
            }
            "pair_request" -> {
                val from = json.optString("from")
                if (from == "pc") {
                    val request = IncomingPcPairRequest(
                        requestId = json.optString("request_id"),
                        deviceName = json.optString("device_name", "电脑"),
                        senderIp = senderIp,
                        senderPort = senderPort,
                        videoPort = json.optInt("video_port", DEFAULT_VIDEO_PORT),
                        controlPort = json.optInt("control_port", DEFAULT_CONTROL_PORT),
                    )
                    runOnUiThread {
                        showIncomingPcPairDialog(request)
                    }
                }
            }
            "pair_response" -> {
                val requestId = json.optString("request_id")
                val pendingId = pendingScanRequestId
                if (pendingId != null && pendingId == requestId) {
                    val accepted = json.optBoolean("accepted", false)
                    if (accepted) {
                        val videoPort = json.optInt("video_port", DEFAULT_VIDEO_PORT)
                        val controlPort = json.optInt("control_port", DEFAULT_CONTROL_PORT)
                        runOnUiThread {
                            onPairingAccepted(senderIp, videoPort, controlPort)
                        }
                    } else {
                        runOnUiThread {
                            txtPairStatus.text = "电脑端拒绝了配对请求"
                        }
                    }
                    pendingScanRequestId = null
                }
            }
        }
    }

    private fun sendDiscoverResponse(targetIp: String, targetPort: Int) {
        val json = JSONObject().apply {
            put("type", "discover_response")
            put("from", "phone")
            put("device_id", localDeviceId())
            put("device_name", localDeviceName())
            put("video_port", DEFAULT_VIDEO_PORT)
            put("control_port", DEFAULT_CONTROL_PORT)
        }
        sendPacket(json, targetIp, targetPort)
    }

    private fun showIncomingPcPairDialog(request: IncomingPcPairRequest) {
        if (isFinishing) {
            return
        }

        AlertDialog.Builder(this)
            .setTitle("配对确认")
            .setMessage("${request.deviceName}(${request.senderIp}) 请求与你配对，是否允许？")
            .setNegativeButton("拒绝") { _, _ ->
                sendPairResponse(
                    requestId = request.requestId,
                    accepted = false,
                    targetIp = request.senderIp,
                    targetPort = request.senderPort,
                    videoPort = request.videoPort,
                    controlPort = request.controlPort,
                )
                txtPairStatus.text = "已拒绝电脑端配对请求"
            }
            .setPositiveButton("允许") { _, _ ->
                sendPairResponse(
                    requestId = request.requestId,
                    accepted = true,
                    targetIp = request.senderIp,
                    targetPort = request.senderPort,
                    videoPort = request.videoPort,
                    controlPort = request.controlPort,
                )
                onPairingAccepted(request.senderIp, request.videoPort, request.controlPort)
            }
            .show()
    }

    private fun startScanPairing(raw: String) {
        val json = try {
            JSONObject(raw)
        } catch (_: Exception) {
            Toast.makeText(this, "二维码解析失败", Toast.LENGTH_SHORT).show()
            return
        }

        val pcIp = json.optString("ip")
        if (pcIp.isBlank()) {
            Toast.makeText(this, "二维码缺少 IP", Toast.LENGTH_SHORT).show()
            return
        }

        val videoPort = json.optInt("port", DEFAULT_VIDEO_PORT)
        val controlPort = json.optInt("control_port", DEFAULT_CONTROL_PORT)
        val pairPort = json.optInt("pair_port", PAIR_PORT)
        val token = json.optString("token", "")
        val requestId = UUID.randomUUID().toString()
        pendingScanRequestId = requestId

        val payload = JSONObject().apply {
            put("type", "pair_request")
            put("request_id", requestId)
            put("from", "phone_scan")
            put("device_id", localDeviceId())
            put("device_name", localDeviceName())
            put("token", token)
            put("video_port", videoPort)
            put("control_port", controlPort)
        }

        txtPairStatus.text = "已发送扫码配对请求，等待电脑确认..."
        sendPacket(payload, pcIp, pairPort)
    }

    private fun sendPairResponse(
        requestId: String,
        accepted: Boolean,
        targetIp: String,
        targetPort: Int,
        videoPort: Int,
        controlPort: Int,
    ) {
        val payload = JSONObject().apply {
            put("type", "pair_response")
            put("request_id", requestId)
            put("accepted", accepted)
            put("device_id", localDeviceId())
            put("device_name", localDeviceName())
            put("video_port", videoPort)
            put("control_port", controlPort)
        }
        sendPacket(payload, targetIp, targetPort)
    }

    private fun sendPacket(payload: JSONObject, targetIp: String, targetPort: Int) {
        val payloadText = payload.toString()
        sendExecutor.execute {
            val socket = pairingSocket ?: return@execute
            try {
                val data = payloadText.toByteArray(Charsets.UTF_8)
                val packet = DatagramPacket(data, data.size, InetSocketAddress(targetIp, targetPort))
                socket.send(packet)
            } catch (_: Exception) {
            }
        }
    }

    private fun onPairingAccepted(host: String, videoPort: Int, controlPort: Int) {
        val prefs = getSharedPreferences("mumu_remote", MODE_PRIVATE)
        prefs.edit()
            .putString("host", host)
            .putInt("videoPort", videoPort)
            .putInt("controlPort", controlPort)
            .apply()

        txtPairStatus.text = "配对成功: $host"
        Toast.makeText(this, "配对成功，正在连接", Toast.LENGTH_SHORT).show()

        val intent = Intent(this, VideoActivity::class.java)
        intent.putExtra("host", host)
        intent.putExtra("videoPort", videoPort)
        intent.putExtra("controlPort", controlPort)
        startActivity(intent)
    }

    private fun localDeviceId(): String {
        val androidId = Settings.Secure.getString(contentResolver, Settings.Secure.ANDROID_ID)
        return if (androidId.isNullOrBlank()) {
            "android-${Build.MODEL}-${Build.VERSION.SDK_INT}"
        } else {
            androidId
        }
    }

    private fun localDeviceName(): String {
        return Build.MODEL ?: "Android"
    }

    override fun onDestroy() {
        pairingRunning.set(false)
        pairingSocket?.close()
        try {
            pairingThread?.join(300)
        } catch (_: InterruptedException) {
        }
        pairingThread = null
        sendExecutor.shutdownNow()
        super.onDestroy()
    }

    companion object {
        private const val PAIR_PORT = 56000
        private const val DEFAULT_VIDEO_PORT = 5000
        private const val DEFAULT_CONTROL_PORT = 5001
    }
}
