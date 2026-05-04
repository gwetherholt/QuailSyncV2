package com.quailsync.app.data

import android.content.Context

object ServerConfig {
    private const val PREFS_NAME = "quailsync"
    private const val KEY_SERVER_URL = "server_url"
    const val DEFAULT_URL = "https://quailsync.tail01d133.ts.net"

    fun getServerUrl(context: Context): String {
        return context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .getString(KEY_SERVER_URL, DEFAULT_URL) ?: DEFAULT_URL
    }

    fun setServerUrl(context: Context, url: String) {
        context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            .edit().putString(KEY_SERVER_URL, url.trimEnd('/')).apply()
    }
}
