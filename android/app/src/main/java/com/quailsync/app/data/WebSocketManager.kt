package com.quailsync.app.data

import android.content.Context

/**
 * Singleton holder for the shared WebSocketService instance.
 * Both DashboardViewModel and TelemetryViewModel use this so live
 * readings are consistent and only one connection exists.
 */
object WebSocketManager {
    @Volatile
    private var instance: WebSocketService? = null

    fun get(context: Context): WebSocketService {
        return instance ?: synchronized(this) {
            instance ?: WebSocketService(ServerConfig.getServerUrl(context)).also {
                instance = it
                it.connect()
            }
        }
    }

    /**
     * Call when server URL changes (e.g. from Settings) to recreate the connection.
     */
    fun reset(context: Context) {
        synchronized(this) {
            instance?.disconnect()
            instance = WebSocketService(ServerConfig.getServerUrl(context)).also {
                it.connect()
            }
        }
    }
}
