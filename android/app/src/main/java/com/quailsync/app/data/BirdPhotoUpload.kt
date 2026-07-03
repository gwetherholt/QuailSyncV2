package com.quailsync.app.data

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File

/**
 * Uploads an already-saved bird photo to the server.
 *
 * Shared by FlockScreen and NfcScreen so the multipart/upload logic lives in
 * one place instead of being duplicated per-screen. The caller MUST have saved
 * the photo to disk first — this is a follow-on to the local save, never a
 * precondition for it. The local file is never read-then-deleted here; we only
 * read it.
 *
 * Uses the caller's already-configured [QuailSyncApi] (built from
 * `ServerConfig.getServerUrl`), so the server address is whatever the app is
 * configured to use — nothing is hardcoded.
 *
 * File I/O and the network call run on [Dispatchers.IO], so this is safe to
 * call from a main-thread coroutine without blocking the UI.
 *
 * @return true on a 2xx response, false on any failure (offline, non-2xx,
 *   unreadable file). Never throws.
 */
suspend fun uploadBirdPhotoFile(api: QuailSyncApi, birdId: Int, file: File): Boolean =
    withContext(Dispatchers.IO) {
        try {
            val bytes = file.readBytes()
            val part = MultipartBody.Part.createFormData(
                "photo",
                "bird_${birdId}.jpg",
                bytes.toRequestBody("image/jpeg".toMediaType()),
            )
            api.uploadBirdPhoto(birdId, part)
            Log.d("QuailSync", "Photo uploaded for bird $birdId")
            true
        } catch (e: Exception) {
            // Local copy is already on disk — log and report failure so the
            // caller can surface it; we never touch the local file.
            Log.e("QuailSync", "Photo upload failed for bird $birdId (local copy kept)", e)
            false
        }
    }
