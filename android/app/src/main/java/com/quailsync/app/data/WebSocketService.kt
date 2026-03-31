package com.quailsync.app.data

import android.util.Log
import com.google.gson.Gson
import com.google.gson.JsonParser
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.util.concurrent.TimeUnit

data class LiveReading(
    val brooderId: Int,
    val temperature: Double?,
    val humidity: Double?,
    val timestamp: String? = null,
    val receivedAt: Long = System.currentTimeMillis(),
)

class WebSocketService(
    private val baseUrl: String,
) {
    private val gson = Gson()
    private val client = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .pingInterval(30, TimeUnit.SECONDS)
        .build()

    private var webSocket: WebSocket? = null
    private val scope = CoroutineScope(Dispatchers.IO)
    private var backoffMs = INITIAL_BACKOFF_MS
    private var shouldReconnect = true

    private val _readings = MutableStateFlow<Map<Int, LiveReading>>(emptyMap())
    val readings: StateFlow<Map<Int, LiveReading>> = _readings.asStateFlow()

    private val _isConnected = MutableStateFlow(false)
    val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

    fun connect() {
        if (webSocket != null) return
        shouldReconnect = true

        val wsUrl = baseUrl
            .replace("http://", "ws://")
            .replace("https://", "wss://")
            .trimEnd('/')

        val request = Request.Builder()
            .url("$wsUrl/ws/live")
            .build()

        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                Log.d("QuailSync", "WebSocket connected to $wsUrl/ws/live")
                _isConnected.value = true
                backoffMs = INITIAL_BACKOFF_MS
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                parseMessage(text)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                Log.d("QuailSync", "WebSocket closing: code=$code reason=$reason")
                webSocket.close(1000, null)
                _isConnected.value = false
                this@WebSocketService.webSocket = null
                scheduleReconnect()
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                Log.e("QuailSync", "WebSocket failure: ${t.message}")
                _isConnected.value = false
                this@WebSocketService.webSocket = null
                scheduleReconnect()
            }
        })
    }

    fun disconnect() {
        shouldReconnect = false
        webSocket?.close(1000, "App closing")
        webSocket = null
        _isConnected.value = false
    }

    fun reconnect() {
        disconnect()
        shouldReconnect = true
        backoffMs = INITIAL_BACKOFF_MS
        connect()
    }

    private fun scheduleReconnect() {
        if (!shouldReconnect) return
        val delayMs = backoffMs
        backoffMs = (backoffMs * 2).coerceAtMost(MAX_BACKOFF_MS)
        Log.d("QuailSync", "WebSocket reconnecting in ${delayMs}ms")
        scope.launch {
            delay(delayMs)
            if (shouldReconnect && webSocket == null) {
                connect()
            }
        }
    }

    private fun parseMessage(text: String) {
        try {
            val root = JsonParser().parse(text).asJsonObject

            // Unwrap the outer key: {"Brooder": {...}} or {"brooder": {...}}
            val json = when {
                root.has("Brooder") -> root.getAsJsonObject("Brooder")
                root.has("brooder") -> root.getAsJsonObject("brooder")
                else -> root // fallback: treat as flat object
            }

            val brooderId = when {
                json.has("brooder_id") -> json.get("brooder_id").asInt
                json.has("brooderId") -> json.get("brooderId").asInt
                else -> return
            }

            val temperature = when {
                json.has("temperature_f") && !json.get("temperature_f").isJsonNull ->
                    json.get("temperature_f").asDouble
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

            val timestamp = when {
                json.has("timestamp") -> json.get("timestamp").asString
                json.has("recorded_at") -> json.get("recorded_at").asString
                else -> null
            }

            val reading = LiveReading(
                brooderId = brooderId,
                temperature = temperature,
                humidity = humidity,
                timestamp = timestamp,
                receivedAt = System.currentTimeMillis(),
            )

            _readings.value = _readings.value.toMutableMap().apply {
                put(brooderId, reading)
            }
        } catch (e: Exception) {
            Log.e("QuailSync", "WebSocket parse error", e)
        }
    }

    companion object {
        private const val INITIAL_BACKOFF_MS = 5_000L
        private const val MAX_BACKOFF_MS = 30_000L
    }
}
