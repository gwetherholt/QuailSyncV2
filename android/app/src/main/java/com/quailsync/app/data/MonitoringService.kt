package com.quailsync.app.data

import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.IBinder
import android.util.Log
import com.google.gson.JsonParser
import com.quailsync.app.BuildConfig
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

class MonitoringService : Service() {

    companion object {
        private const val TAG = "QuailSync-Monitor"

        // Temperature thresholds (°F)
        private const val TEMP_CRITICAL_LOW = 60.0
        private const val TEMP_CRITICAL_HIGH = 75.0
        private const val TEMP_WARNING_LOW = 65.0
        private const val TEMP_WARNING_HIGH = 72.0

        // Humidity thresholds (%)
        private const val HUMIDITY_WARNING_LOW = 40.0
        private const val HUMIDITY_WARNING_HIGH = 80.0

        // Timing
        private const val OFFLINE_TIMEOUT_MS = 2 * 60 * 1000L // 2 minutes
        private const val NOTIFICATION_COOLDOWN_MS = 5 * 60 * 1000L // 5 minutes

        fun start(context: Context) {
            val intent = Intent(context, MonitoringService::class.java)
            context.startForegroundService(intent)
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, MonitoringService::class.java))
        }

        fun isMonitoringEnabled(context: Context): Boolean {
            return context.getSharedPreferences("quailsync", Context.MODE_PRIVATE)
                .getBoolean("monitoring_enabled", true)
        }

        fun setMonitoringEnabled(context: Context, enabled: Boolean) {
            context.getSharedPreferences("quailsync", Context.MODE_PRIVATE)
                .edit().putBoolean("monitoring_enabled", enabled).apply()
            if (enabled) start(context) else stop(context)
        }
    }

    private var webSocket: WebSocket? = null
    private val client = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .pingInterval(30, TimeUnit.SECONDS)
        .build()

    // Tracks the last alert state per brooder to avoid spamming
    private data class BrooderAlertState(
        val severity: String, // "OK", "WARNING", "CRITICAL", "OFFLINE"
        val lastNotifiedAt: Long = 0,
    )

    private val alertStates = ConcurrentHashMap<Int, BrooderAlertState>()
    private val lastReadingTime = ConcurrentHashMap<Int, Long>()
    private val brooderNames = ConcurrentHashMap<Int, String>()
    private val connectedBrooders = ConcurrentHashMap<Int, Boolean>()

    private var offlineCheckThread: Thread? = null
    private var running = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        NotificationHelper.createChannels(this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = NotificationHelper.buildMonitorNotification(this, 0).build()
        startForeground(NotificationHelper.MONITOR_NOTIFICATION_ID, notification)

        running = true
        connectWebSocket()
        startOfflineChecker()

        Log.d(TAG, "Monitoring service started")
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        webSocket?.close(1000, "Service stopping")
        webSocket = null
        offlineCheckThread?.interrupt()
        Log.d(TAG, "Monitoring service stopped")
        super.onDestroy()
    }

    private fun connectWebSocket() {
        val baseUrl = BuildConfig.BASE_URL
        val wsUrl = baseUrl
            .replace("http://", "ws://")
            .replace("https://", "wss://")
            .trimEnd('/')

        val request = Request.Builder()
            .url("$wsUrl/ws/live")
            .build()

        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(ws: WebSocket, response: Response) {
                Log.d(TAG, "WebSocket connected")
            }

            override fun onMessage(ws: WebSocket, text: String) {
                parseAndCheck(text)
            }

            override fun onClosing(ws: WebSocket, code: Int, reason: String) {
                ws.close(1000, null)
                Log.d(TAG, "WebSocket closing: $code $reason")
                scheduleReconnect()
            }

            override fun onFailure(ws: WebSocket, t: Throwable, response: Response?) {
                Log.e(TAG, "WebSocket failure", t)
                scheduleReconnect()
            }
        })
    }

    private fun scheduleReconnect() {
        if (!running) return
        Thread {
            try {
                Thread.sleep(10_000)
                if (running) {
                    Log.d(TAG, "Reconnecting WebSocket...")
                    connectWebSocket()
                }
            } catch (_: InterruptedException) {}
        }.start()
    }

    private fun parseAndCheck(text: String) {
        try {
            val root = JsonParser().parse(text).asJsonObject
            val json = when {
                root.has("Brooder") -> root.getAsJsonObject("Brooder")
                root.has("brooder") -> root.getAsJsonObject("brooder")
                else -> root
            }

            val brooderId = when {
                json.has("brooder_id") -> json.get("brooder_id").asInt
                json.has("brooderId") -> json.get("brooderId").asInt
                else -> return
            }

            // Track the brooder name if available
            if (json.has("brooder_name")) {
                brooderNames[brooderId] = json.get("brooder_name").asString
            }

            val temperature = when {
                json.has("temperature_celsius") && !json.get("temperature_celsius").isJsonNull ->
                    json.get("temperature_celsius").asDouble
                json.has("temperature") && !json.get("temperature").isJsonNull ->
                    json.get("temperature").asDouble
                else -> null
            }

            val humidity = when {
                json.has("humidity_percent") && !json.get("humidity_percent").isJsonNull ->
                    json.get("humidity_percent").asDouble
                json.has("humidity") && !json.get("humidity").isJsonNull ->
                    json.get("humidity").asDouble
                else -> null
            }

            // Record that we got a reading
            lastReadingTime[brooderId] = System.currentTimeMillis()
            connectedBrooders[brooderId] = true
            updateMonitorNotification()

            // Check thresholds
            checkThresholds(brooderId, temperature, humidity)
        } catch (e: Exception) {
            Log.e(TAG, "Parse error", e)
        }
    }

    private fun checkThresholds(brooderId: Int, temperature: Double?, humidity: Double?) {
        val now = System.currentTimeMillis()
        val name = brooderNames[brooderId] ?: "Brooder #$brooderId"
        val prevState = alertStates[brooderId]

        // Determine severity
        var severity = "OK"
        val messages = mutableListOf<String>()

        if (temperature != null) {
            when {
                temperature < TEMP_CRITICAL_LOW -> {
                    severity = "CRITICAL"
                    messages.add("Temp LOW: %.1f°F (below %.0f°F)".format(temperature, TEMP_CRITICAL_LOW))
                }
                temperature > TEMP_CRITICAL_HIGH -> {
                    severity = "CRITICAL"
                    messages.add("Temp HIGH: %.1f°F (above %.0f°F)".format(temperature, TEMP_CRITICAL_HIGH))
                }
                temperature < TEMP_WARNING_LOW -> {
                    if (severity != "CRITICAL") severity = "WARNING"
                    messages.add("Temp low: %.1f°F (below %.0f°F)".format(temperature, TEMP_WARNING_LOW))
                }
                temperature > TEMP_WARNING_HIGH -> {
                    if (severity != "CRITICAL") severity = "WARNING"
                    messages.add("Temp high: %.1f°F (above %.0f°F)".format(temperature, TEMP_WARNING_HIGH))
                }
            }
        }

        if (humidity != null) {
            when {
                humidity < HUMIDITY_WARNING_LOW -> {
                    if (severity != "CRITICAL") severity = "WARNING"
                    messages.add("Humidity low: %.0f%% (below %.0f%%)".format(humidity, HUMIDITY_WARNING_LOW))
                }
                humidity > HUMIDITY_WARNING_HIGH -> {
                    if (severity != "CRITICAL") severity = "WARNING"
                    messages.add("Humidity high: %.0f%% (above %.0f%%)".format(humidity, HUMIDITY_WARNING_HIGH))
                }
            }
        }

        if (severity == "OK") {
            // Clear alert state if it was previously alerting
            if (prevState != null && prevState.severity != "OK") {
                alertStates[brooderId] = BrooderAlertState("OK", now)
            }
            return
        }

        // Check if we should fire a notification
        val stateChanged = prevState == null || prevState.severity != severity
        val cooldownExpired = prevState == null || (now - prevState.lastNotifiedAt) > NOTIFICATION_COOLDOWN_MS

        if (stateChanged || cooldownExpired) {
            val message = messages.joinToString(", ")
            Log.d(TAG, "Alert for $name: $severity — $message")
            NotificationHelper.fireAlertNotification(this, name, message, severity, brooderId)
            alertStates[brooderId] = BrooderAlertState(severity, now)
        }
    }

    private fun startOfflineChecker() {
        val service = this
        offlineCheckThread = Thread {
            while (running) {
                try {
                    Thread.sleep(30_000) // Check every 30 seconds
                    val now = System.currentTimeMillis()
                    for ((brooderId, lastTime) in lastReadingTime) {
                        if (now - lastTime > OFFLINE_TIMEOUT_MS) {
                            val prevState = alertStates[brooderId]
                            if (prevState?.severity != "OFFLINE") {
                                val name = brooderNames[brooderId] ?: "Brooder #$brooderId"
                                Log.d(TAG, "Sensor offline: $name (no data for ${(now - lastTime) / 1000}s)")
                                NotificationHelper.fireAlertNotification(
                                    service, name, "No sensor data received for 2+ minutes", "WARNING", brooderId,
                                )
                                alertStates[brooderId] = BrooderAlertState("OFFLINE", now)
                                connectedBrooders[brooderId] = false
                                updateMonitorNotification()
                            }
                        }
                    }
                } catch (_: InterruptedException) {
                    break
                }
            }
        }.also { it.isDaemon = true; it.start() }
    }

    private fun updateMonitorNotification() {
        val count = connectedBrooders.values.count { it }
        val notification = NotificationHelper.buildMonitorNotification(this, count).build()
        val manager = getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager
        manager.notify(NotificationHelper.MONITOR_NOTIFICATION_ID, notification)
    }
}
