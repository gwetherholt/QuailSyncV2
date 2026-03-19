package com.quailsync.app.data

import com.google.gson.Gson
import com.google.gson.JsonParser
import com.quailsync.app.BuildConfig
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
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
)

class WebSocketService(
    private val baseUrl: String = BuildConfig.BASE_URL,
) {
    private val gson = Gson()
    private val client = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .pingInterval(30, TimeUnit.SECONDS)
        .build()

    private var webSocket: WebSocket? = null

    private val _readings = MutableStateFlow<Map<Int, LiveReading>>(emptyMap())
    val readings: StateFlow<Map<Int, LiveReading>> = _readings.asStateFlow()

    private val _isConnected = MutableStateFlow(false)
    val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

    fun connect() {
        if (webSocket != null) return

        val wsUrl = baseUrl
            .replace("http://", "ws://")
            .replace("https://", "wss://")
            .trimEnd('/')

        val request = Request.Builder()
            .url("$wsUrl/ws/live")
            .build()

        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                _isConnected.value = true
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                parseMessage(text)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                webSocket.close(1000, null)
                _isConnected.value = false
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                _isConnected.value = false
                this@WebSocketService.webSocket = null
            }
        })
    }

    fun disconnect() {
        webSocket?.close(1000, "App closing")
        webSocket = null
        _isConnected.value = false
    }

    private fun parseMessage(text: String) {
        try {
            val json = JsonParser().parse(text).asJsonObject

            val brooderId = when {
                json.has("brooder_id") -> json.get("brooder_id").asInt
                json.has("brooderId") -> json.get("brooderId").asInt
                else -> return
            }

            val temperature = when {
                json.has("temperature") && !json.get("temperature").isJsonNull ->
                    json.get("temperature").asDouble
                else -> null
            }

            val humidity = when {
                json.has("humidity") && !json.get("humidity").isJsonNull ->
                    json.get("humidity").asDouble
                else -> null
            }

            val timestamp = when {
                json.has("recorded_at") -> json.get("recorded_at").asString
                json.has("timestamp") -> json.get("timestamp").asString
                else -> null
            }

            val reading = LiveReading(
                brooderId = brooderId,
                temperature = temperature,
                humidity = humidity,
                timestamp = timestamp,
            )

            _readings.value = _readings.value.toMutableMap().apply {
                put(brooderId, reading)
            }
        } catch (_: Exception) {
            // Ignore malformed messages
        }
    }
}
