package com.quailsync.app

import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import java.util.concurrent.TimeUnit

/**
 * Shared helpers for the androidTest suite.
 *
 * The tests assume DEV_MODE=true on the server so that /api/dev/seed and
 * /api/dev/restore are wired up. Without those routes the test suite cannot
 * give itself a deterministic dataset and most assertions will not hold.
 *
 * SERVER_URL points at the Android emulator's loopback alias by default
 * (10.0.2.2 forwards to the host machine's localhost). When running the
 * instrumented tests on a physical device, override SERVER_URL to the Pi's
 * LAN address (e.g. "http://192.168.1.42:3000") or to the Tailscale URL
 * before invoking the test runner — the simplest approach is to edit the
 * constant temporarily, since AndroidJUnitRunner doesn't expose env vars.
 */
const val SERVER_URL: String = "http://100.109.222.48:3000"

private val httpClient: OkHttpClient by lazy {
    OkHttpClient.Builder()
        // Seed wipes + re-inserts the full fixture set, including the
        // production backup VACUUM INTO. Give it plenty of headroom — 30s
        // covers a slow Pi without making genuinely-stuck calls hang forever.
        .callTimeout(30, TimeUnit.SECONDS)
        .build()
}

private val EMPTY_JSON_BODY = "{}".toRequestBody()

/**
 * Wipes and reseeds the dev DB. Must succeed before each test; an HTTP 200
 * indicates the server has installed the basic fixture set (5 lineages,
 * 5 housing units, 4 chick groups, 15 birds — see routes/dev.rs).
 *
 * Fails the test immediately on non-200 so we never run an assertion against
 * data the test didn't establish.
 */
fun seedTestData(serverUrl: String = SERVER_URL) {
    val request = Request.Builder()
        .url("$serverUrl/api/dev/seed")
        .post(EMPTY_JSON_BODY)
        .build()
    httpClient.newCall(request).execute().use { resp ->
        assertEquals(
            "POST /api/dev/seed must return 200 (got ${resp.code}: ${resp.body?.string()})",
            200,
            resp.code,
        )
    }
}

/**
 * Restores the production DB snapshot taken by the most recent seed run.
 * 404 is treated as a non-fatal soft-failure: it means no backup exists
 * (e.g. seed was skipped or already restored), which leaves prod-equivalent
 * data in place — fine for test teardown.
 */
fun restoreTestData(serverUrl: String = SERVER_URL) {
    val request = Request.Builder()
        .url("$serverUrl/api/dev/restore")
        .post(EMPTY_JSON_BODY)
        .build()
    httpClient.newCall(request).execute().use { resp ->
        assertTrue(
            "POST /api/dev/restore returned unexpected status ${resp.code}",
            resp.code == 200 || resp.code == 404,
        )
    }
}
