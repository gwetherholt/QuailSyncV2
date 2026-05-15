@file:Suppress("ASSIGNED_BUT_NEVER_ACCESSED_VARIABLE", "UNUSED_VALUE")

package com.quailsync.app.ui.screens

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.net.Uri
import android.util.Log
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.quailsync.app.data.Bird
import com.quailsync.app.data.Lineage
import com.quailsync.app.data.Clutch
import com.quailsync.app.data.CreateBirdRequest
import com.quailsync.app.data.CreateWeightRequest
import com.quailsync.app.data.NfcScanResult
import com.quailsync.app.data.NfcService
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.TagConflict
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.DustyRose
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File
import java.time.LocalDate
import java.time.format.DateTimeFormatter

// =====================================================================
// Data classes
// =====================================================================

data class GraduatedBird(
    val index: Int,
    val bird: Bird,
    val tagId: String,
    val photoUri: Uri? = null,
)

/**
 * A tag whose payload references a bird id that no longer exists in the DB
 * (deleted, escaped, etc). The user must explicitly confirm reuse before we
 * overwrite the tag — never automatic.
 */
data class OrphanTag(
    val tagId: String,
    val orphanBirdId: Int,
)

sealed class BatchState {
    data object Idle : BatchState()
    data object Setup : BatchState()

    /** User is filling per-bird details before tapping the tag.
     *
     *  Bug 3 fix: when a submit fails (e.g. server 500), the per-bird form
     *  used to reset to defaults because PerBirdEntryScreen leaves
     *  composition during the CreatingBird transition and `remember` starts
     *  fresh on return. Persisting unsaved input here keeps the user's
     *  entry across that failure round-trip. Drafts are reset by the
     *  success path when the next PerBirdEntry is constructed (default
     *  null values). */
    data class PerBirdEntry(
        val currentIndex: Int,
        val totalCount: Int,
        val lineageId: Int,
        val graduated: List<GraduatedBird>,
        val lastMaleBandColor: String = "",
        val lastFemaleBandColor: String = "",
        val draftSex: String? = null,
        val draftBandColor: String? = null,
        val draftNotes: String? = null,
        val draftWeightText: String? = null,
        val draftPhotoBitmap: Bitmap? = null,
        /** Source chick group when the batch was started via "Band Group"
         *  on the Hatchery card. When non-null, the chick group's status
         *  is flipped to 'Graduated' on batch completion (Bug A). Null
         *  when the batch was started ad-hoc from the NFC Setup screen. */
        val chickGroupId: Int? = null,
    ) : BatchState()

    /** API call in progress to create the bird. */
    data class CreatingBird(
        val currentIndex: Int,
        val totalCount: Int,
        val lineageId: Int,
        val graduated: List<GraduatedBird>,
        val sex: String,
        val bandColor: String,
        val notes: String,
        val lastMaleBandColor: String,
        val lastFemaleBandColor: String,
        val chickGroupId: Int? = null,
    ) : BatchState()

    /** Bird created, write mode active, waiting for tag tap. */
    data class AwaitingTagWrite(
        val currentIndex: Int,
        val totalCount: Int,
        val lineageId: Int,
        val graduated: List<GraduatedBird>,
        val pendingBird: Bird,
        val lastMaleBandColor: String,
        val lastFemaleBandColor: String,
        val chickGroupId: Int? = null,
    ) : BatchState()

    /** Tag written — confirmation screen with photo/weight options. */
    data class PostTagConfirm(
        val currentIndex: Int,
        val totalCount: Int,
        val lineageId: Int,
        val graduated: List<GraduatedBird>,
        val justTaggedBird: Bird,
        val justTaggedTagId: String,
        val lastMaleBandColor: String,
        val lastFemaleBandColor: String,
        val photoSaved: Boolean = false,
        val weightLogged: Boolean = false,
        val chickGroupId: Int? = null,
    ) : BatchState()

    data class Complete(
        val graduated: List<GraduatedBird>,
        val lineageName: String?,
    ) : BatchState()
}

// =====================================================================
// ViewModel
// =====================================================================

/**
 * Pull a user-safe error message out of a Retrofit HttpException.
 *
 * The server now returns `{ "error": "...", "message": "..." }` JSON for
 * every 500-class error, with the SQL detail logged server-side only.
 * This helper extracts the `message` field; if the body isn't JSON (older
 * servers, plain-text 400 validation errors), it falls back to the raw body
 * for 4xx (those are intentionally human-readable) and a generic apology
 * for 5xx (whose bodies may contain implementation detail we don't want to
 * surface).
 */
private fun friendlyErrorMessage(e: retrofit2.HttpException): String {
    val rawBody = try { e.response()?.errorBody()?.string() } catch (_: Exception) { null }
    if (rawBody != null) {
        try {
            val obj = com.google.gson.JsonParser().parse(rawBody).asJsonObject
            val msg = obj.get("message")?.asString
            if (!msg.isNullOrBlank()) return msg
        } catch (_: Exception) { /* not JSON — fall through */ }
    }
    return when (e.code()) {
        in 400..499 -> rawBody?.takeIf { it.isNotBlank() } ?: "Request rejected (${e.code()})"
        in 500..599 -> "Something went wrong on our end. Please try again."
        else -> "Request failed (${e.code()})"
    }
}

class NfcViewModel(val nfcService: NfcService, serverUrl: String) : ViewModel() {
    private val api = QuailSyncApi.create(serverUrl)

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    private val _lineages = MutableStateFlow<List<Lineage>>(emptyList())
    val lineages: StateFlow<List<Lineage>> = _lineages.asStateFlow()

    private val _clutches = MutableStateFlow<List<Clutch>>(emptyList())
    @Suppress("unused") val clutches: StateFlow<List<Clutch>> = _clutches.asStateFlow()

    private val _batchState = MutableStateFlow<BatchState>(BatchState.Idle)
    val batchState: StateFlow<BatchState> = _batchState.asStateFlow()

    private val _conflictBird = MutableStateFlow<Bird?>(null)
    val conflictBird: StateFlow<Bird?> = _conflictBird.asStateFlow()

    /** Set when a scanned tag references a bird that no longer exists in the DB. */
    private val _orphanTag = MutableStateFlow<OrphanTag?>(null)
    val orphanTag: StateFlow<OrphanTag?> = _orphanTag.asStateFlow()

    private val _batchPausedForConflict = MutableStateFlow(false)

    init { loadData() }

    private fun loadData() {
        viewModelScope.launch {
            try { _birds.value = api.getBirds() } catch (e: Exception) { Log.e("QuailSync", "Failed to load birds", e) }
            try { _lineages.value = api.getLineages() } catch (e: Exception) { Log.e("QuailSync", "Failed to load lineages", e) }
            try { _clutches.value = api.getClutches() } catch (e: Exception) { Log.e("QuailSync", "Failed to load clutches", e) }
        }
    }

    // --- Normal NFC scan ---

    fun lookupBirdByNfc(tagId: String, payload: String?) {
        viewModelScope.launch {
            val lookupId = if (payload?.startsWith("BIRD-") == true) payload else tagId
            try {
                val bird = api.getBirdByNfcTag(lookupId)
                nfcService.updateScanWithBird(tagId, bird)
            } catch (_: Exception) {
                if (payload?.startsWith("BIRD-") == true) {
                    val birdId = payload.removePrefix("BIRD-").toIntOrNull()
                    val bird = birdId?.let { id -> _birds.value.find { it.id == id } }
                    if (bird != null) { nfcService.updateScanWithBird(tagId, bird); return@launch }
                    // Orphan: tag's payload claims BIRD-{id} but no such bird
                    // exists server-side or in local cache. Surface this so the
                    // user can explicitly choose to reuse the tag for a new
                    // bird — never automatic.
                    if (birdId != null) {
                        _orphanTag.value = OrphanTag(tagId = tagId, orphanBirdId = birdId)
                        return@launch
                    }
                }
                Log.d("QuailSync", "NFC lookup: no bird found for $lookupId")
            }
        }
    }

    /** User dismissed the orphan-tag prompt without reusing. */
    fun dismissOrphanTag() {
        _orphanTag.value = null
    }

    /**
     * User confirmed they want to reuse the orphan tag for a new bird.
     * Drops the orphan state and routes the user into the standard create-bird
     * flow. When they create a bird and tap-to-write, the existing
     * `lookupConflictBird` path auto-overwrites because the orphan bird id
     * doesn't exist server-side, so the conflict dialog won't fire spuriously.
     */
    fun reuseOrphanTag() {
        _orphanTag.value = null
        _batchState.value = BatchState.Setup
    }

    fun startWriteMode(birdId: Int) { nfcService.enterWriteMode("BIRD-$birdId") }
    fun cancelWriteMode() { nfcService.cancelWriteMode() }

    // --- Tag conflict handling ---

    fun lookupConflictBird(conflict: TagConflict) {
        _conflictBird.value = null
        viewModelScope.launch {
            try {
                val bird = api.getBirds().find { it.id == conflict.existingBirdId }
                _conflictBird.value = bird
                if (bird == null) { confirmOverwrite() }
            } catch (_: Exception) { confirmOverwrite() }
        }
    }

    fun confirmOverwrite() {
        val wasBatchPaused = _batchPausedForConflict.value
        val conflict = nfcService.pendingConflict.value
        val success = nfcService.confirmOverwrite()
        _conflictBird.value = null
        _batchPausedForConflict.value = false
        if (wasBatchPaused && conflict != null) { onBatchTagWritten(conflict.tagId, success) }
    }

    fun cancelOverwrite() {
        nfcService.cancelOverwrite()
        _conflictBird.value = null
        if (_batchPausedForConflict.value) {
            _batchPausedForConflict.value = false
            val state = _batchState.value
            if (state is BatchState.AwaitingTagWrite) {
                nfcService.enterWriteMode("BIRD-${state.pendingBird.id}")
            }
        }
    }

    fun setBatchPausedForConflict(paused: Boolean) { _batchPausedForConflict.value = paused }

    // --- Batch graduation ---
    //
    // New flow per bird:
    //   PerBirdEntry (user fills sex/band/notes)
    //     → user taps "Create & Tag" button
    //     → CreatingBird (API POST)
    //     → AwaitingTagWrite (write mode active, user taps tag)
    //     → tag written → next PerBirdEntry
    //
    // Sex, band color, notes are set individually per bird.
    // Band color auto-fills from the last bird of the same sex.

    fun openBatchSetup() { _batchState.value = BatchState.Setup }

    fun cancelBatch() {
        nfcService.cancelWriteMode()
        _batchState.value = BatchState.Idle
    }

    /** Begin a per-bird tagging batch.
     *
     *  When `chickGroupId` is non-null, the source group's status will be
     *  flipped to 'Graduated' once the batch completes — closes the loop
     *  on Hatchery cards (Bug A). */
    fun startBatchTagging(count: Int, lineageId: Int, chickGroupId: Int? = null) {
        _batchState.value = BatchState.PerBirdEntry(
            currentIndex = 0,
            totalCount = count,
            lineageId = lineageId,
            graduated = emptyList(),
            chickGroupId = chickGroupId,
        )
    }

    /** Called when user fills per-bird details and taps "Create & Tag".
     *
     *  Takes `weightText` rather than the parsed Double so that, on submit
     *  failure, we can replay the exact text the user typed back into the
     *  form (e.g. "180." mid-entry). The Double is parsed locally for the
     *  weight-record write. */
    fun createAndTagBird(
        sex: String,
        bandColor: String,
        notes: String,
        weightText: String = "",
        photoBitmap: Bitmap? = null,
        context: Context? = null,
    ) {
        val state = _batchState.value as? BatchState.PerBirdEntry ?: return
        val weightGrams = weightText.toDoubleOrNull()

        _batchState.value = BatchState.CreatingBird(
            currentIndex = state.currentIndex,
            totalCount = state.totalCount,
            lineageId = state.lineageId,
            graduated = state.graduated,
            sex = sex,
            bandColor = bandColor,
            notes = notes,
            chickGroupId = state.chickGroupId,
            lastMaleBandColor = state.lastMaleBandColor,
            lastFemaleBandColor = state.lastFemaleBandColor,
        )

        viewModelScope.launch {
            try {
                val sexValue = when (sex.lowercase()) {
                    "male", "m" -> "Male"
                    "female", "f" -> "Female"
                    else -> "Unknown"
                }
                val request = CreateBirdRequest(
                    lineageIds = listOf(state.lineageId.toLong()),
                    sex = sexValue,
                    status = "Active",
                    hatchDate = LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE),
                    generation = 1,
                    bandColor = bandColor.ifBlank { null },
                    notes = notes.ifBlank { null },
                    // Issue #14: stamp the back-link when the batch was
                    // launched from a chick group. Null when the batch was
                    // started ad-hoc from the NFC Setup screen.
                    chickGroupId = state.chickGroupId,
                )
                Log.d("QuailSync", "Batch: creating bird: sex=$sexValue, lineage=${state.lineageId}, band=$bandColor")
                val bird = api.createBird(request)
                Log.d("QuailSync", "Batch: created bird ${bird.id}, entering write mode")

                // Persist optional intake fields one-at-a-time as soon as the bird id is known,
                // so partial data isn't lost if the user closes the app mid-batch.
                if (weightGrams != null && weightGrams > 0) {
                    try {
                        api.createBirdWeight(
                            bird.id,
                            CreateWeightRequest(
                                weightGrams = weightGrams,
                                date = LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE),
                                notes = null,
                            ),
                        )
                    } catch (e: Exception) {
                        Log.e("QuailSync", "Batch: weight log failed for bird ${bird.id}", e)
                    }
                }
                if (photoBitmap != null && context != null) {
                    saveBirdPhotoLocally(bird.id, photoBitmap, context)
                }

                nfcService.enterWriteMode("BIRD-${bird.id}")

                val updatedMale = if (sex.lowercase() == "male" && bandColor.isNotBlank()) bandColor else state.lastMaleBandColor
                val updatedFemale = if (sex.lowercase() == "female" && bandColor.isNotBlank()) bandColor else state.lastFemaleBandColor

                _batchState.value = BatchState.AwaitingTagWrite(
                    currentIndex = state.currentIndex,
                    totalCount = state.totalCount,
                    lineageId = state.lineageId,
                    graduated = state.graduated,
                    pendingBird = bird,
                    lastMaleBandColor = updatedMale,
                    lastFemaleBandColor = updatedFemale,
                    chickGroupId = state.chickGroupId,
                )
            } catch (e: retrofit2.HttpException) {
                Log.e("QuailSync", "Batch: HTTP ${e.code()} creating bird (raw body logged separately)", e)
                nfcService.setWriteResult(NfcService.WriteResult(false, friendlyErrorMessage(e)))
                // Go back to PerBirdEntry — preserving every field the user
                // entered, so they don't have to re-pick sex / re-type weight /
                // re-take the photo just because our server failed.
                _batchState.value = state.copy(
                    draftSex = sex,
                    draftBandColor = bandColor,
                    draftNotes = notes,
                    draftWeightText = weightText,
                    draftPhotoBitmap = photoBitmap,
                )
            } catch (e: Exception) {
                Log.e("QuailSync", "Batch: failed to create bird", e)
                nfcService.setWriteResult(NfcService.WriteResult(false, "Network error — please try again"))
                _batchState.value = state.copy(
                    draftSex = sex,
                    draftBandColor = bandColor,
                    draftNotes = notes,
                    draftWeightText = weightText,
                    draftPhotoBitmap = photoBitmap,
                )
            }
        }
    }

    fun onBatchTagWritten(tagId: String, success: Boolean) {
        val state = _batchState.value
        if (state !is BatchState.AwaitingTagWrite) return

        if (!success) {
            nfcService.enterWriteMode("BIRD-${state.pendingBird.id}")
            return
        }

        Log.d("QuailSync", "Batch: tag $tagId written with BIRD-${state.pendingBird.id}")
        val graduated = GraduatedBird(state.currentIndex, state.pendingBird, tagId)
        val newGraduated = state.graduated + graduated

        // Go to post-tag confirmation for photo/weight
        _batchState.value = BatchState.PostTagConfirm(
            currentIndex = state.currentIndex + 1,
            totalCount = state.totalCount,
            lineageId = state.lineageId,
            graduated = newGraduated,
            justTaggedBird = state.pendingBird,
            justTaggedTagId = tagId,
            lastMaleBandColor = state.lastMaleBandColor,
            lastFemaleBandColor = state.lastFemaleBandColor,
            chickGroupId = state.chickGroupId,
        )
    }

    fun onPostTagPhotoSaved() {
        val state = _batchState.value as? BatchState.PostTagConfirm ?: return
        _batchState.value = state.copy(photoSaved = true)
    }

    fun onPostTagWeightLogged() {
        val state = _batchState.value as? BatchState.PostTagConfirm ?: return
        _batchState.value = state.copy(weightLogged = true)
    }

    private val _weightError = MutableStateFlow<String?>(null)
    val weightError: StateFlow<String?> = _weightError.asStateFlow()

    fun logBirdWeight(birdId: Int, weightGrams: Double, notes: String?) {
        _weightError.value = null
        val dateStr = LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE)
        Log.d("QuailSync", "logBirdWeight: birdId=$birdId, weightGrams=$weightGrams, date=$dateStr, notes=$notes")
        viewModelScope.launch {
            try {
                val request = CreateWeightRequest(
                    weightGrams = weightGrams,
                    date = dateStr,
                    notes = notes?.ifBlank { null },
                )
                Log.d("QuailSync", "POST /api/birds/$birdId/weight body: weight_grams=$weightGrams, date=$dateStr, notes=${request.notes}")
                val result = api.createBirdWeight(birdId, request)
                Log.d("QuailSync", "Weight logged OK for bird $birdId: ${result.weightGrams}g, id=${result.id}")
                onPostTagWeightLogged()
            } catch (e: retrofit2.HttpException) {
                Log.e("QuailSync", "Weight HTTP ${e.code()} for bird $birdId", e)
                _weightError.value = friendlyErrorMessage(e)
            } catch (e: Exception) {
                Log.e("QuailSync", "Weight failed for bird $birdId", e)
                _weightError.value = "Network error — please try again"
            }
        }
    }

    fun saveBirdPhotoLocally(birdId: Int, bitmap: Bitmap, context: Context) {
        viewModelScope.launch {
            try {
                val dir = File(context.filesDir, "bird_photos").also { it.mkdirs() }
                val file = File(dir, "bird_${birdId}.jpg")
                file.outputStream().use { out ->
                    bitmap.compress(Bitmap.CompressFormat.JPEG, 90, out)
                }
                Log.d("QuailSync", "Photo saved locally: ${file.absolutePath}")
                onPostTagPhotoSaved()
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to save photo", e)
            }
        }
    }

    /** Advance from PostTagConfirm to the next bird or completion. */
    fun advanceFromConfirm() {
        val state = _batchState.value as? BatchState.PostTagConfirm ?: return

        if (state.currentIndex >= state.totalCount) {
            val lineageName = _lineages.value.find { it.id == state.lineageId }?.name
            _batchState.value = BatchState.Complete(state.graduated, lineageName)
            viewModelScope.launch {
                // Bug A: when the batch was started from a chick group's
                // "Band Group" button, flip the source group's status to
                // 'Graduated' so it disappears from the Hatchery active
                // list and can't be double-banded. Best-effort — failure
                // logs but doesn't block the success summary.
                val groupId = state.chickGroupId
                if (groupId != null) {
                    try {
                        val resp = api.updateChickGroup(groupId, mapOf("status" to "Graduated"))
                        if (!resp.isSuccessful) {
                            Log.w("QuailSync", "Could not mark chick group $groupId graduated: HTTP ${resp.code()}")
                        }
                    } catch (e: Exception) {
                        Log.e("QuailSync", "Failed to mark chick group $groupId graduated", e)
                    }
                }
                try { _birds.value = api.getBirds() } catch (_: Exception) {}
                // ClutchViewModel owns the chick-groups cache; it refreshes on
                // ON_RESUME when the user navigates back to Hatchery, so the
                // now-graduated group filters out of the Active section then.
            }
        } else {
            _batchState.value = BatchState.PerBirdEntry(
                currentIndex = state.currentIndex,
                totalCount = state.totalCount,
                lineageId = state.lineageId,
                graduated = state.graduated,
                lastMaleBandColor = state.lastMaleBandColor,
                lastFemaleBandColor = state.lastFemaleBandColor,
                chickGroupId = state.chickGroupId,
            )
        }
    }

    fun dismissBatchSummary() { _batchState.value = BatchState.Idle }

    // --- Photo upload ---

    @Suppress("unused") fun uploadPhoto(birdId: Int, uri: Uri, context: Context) {
        viewModelScope.launch {
            try {
                val bytes = context.contentResolver.openInputStream(uri)?.readBytes() ?: return@launch
                val part = okhttp3.MultipartBody.Part.createFormData(
                    "photo", "bird_${birdId}.jpg", bytes.toRequestBody("image/jpeg".toMediaType()),
                )
                api.uploadBirdPhoto(birdId, part)
            } catch (e: Exception) {
                Log.e("QuailSync", "Photo upload failed, saving locally", e)
                try {
                    val dir = File(context.filesDir, "bird_photos").also { it.mkdirs() }
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        File(dir, "bird_${birdId}.jpg").outputStream().use { input.copyTo(it) }
                    }
                } catch (_: Exception) {}
            }
        }
    }
}

// =====================================================================
// Top-level screen dispatcher
// =====================================================================

@Composable
fun NfcScreen(nfcService: NfcService, viewModel: NfcViewModel) {
    val batchState by viewModel.batchState.collectAsState()
    val pendingConflict by nfcService.pendingConflict.collectAsState()
    val conflictBird by viewModel.conflictBird.collectAsState()
    val orphanTag by viewModel.orphanTag.collectAsState()

    when (val state = batchState) {
        is BatchState.Setup -> BatchSetupScreen(viewModel)
        is BatchState.PerBirdEntry -> PerBirdEntryScreen(state, viewModel)
        is BatchState.CreatingBird -> BatchCreatingScreen(state)
        is BatchState.AwaitingTagWrite -> BatchAwaitingWriteScreen(state, viewModel)
        is BatchState.PostTagConfirm -> PostTagConfirmScreen(state, viewModel)
        is BatchState.Complete -> BatchCompleteScreen(state, viewModel)
        is BatchState.Idle -> NfcMainScreen(nfcService, viewModel)
    }

    if (pendingConflict != null && conflictBird != null) {
        TagConflictDialog(pendingConflict!!, conflictBird!!, { viewModel.confirmOverwrite() }, { viewModel.cancelOverwrite() })
    }


    orphanTag?.let { orphan ->
        OrphanTagDialog(
            orphan = orphan,
            onReuse = { viewModel.reuseOrphanTag() },
            onCancel = { viewModel.dismissOrphanTag() },
        )
    }
}

// =====================================================================
// NFC Main Screen (idle — scan/write single tags)
// =====================================================================

@Composable
fun NfcMainScreen(nfcService: NfcService, viewModel: NfcViewModel) {
    val lastScan by nfcService.lastScan.collectAsState()
    val scanHistory by nfcService.scanHistory.collectAsState()
    val writeMode by nfcService.writeMode.collectAsState()
    val pendingWriteData by nfcService.pendingWriteData.collectAsState()
    val writeResult by nfcService.writeResult.collectAsState()
    val isAvailable by nfcService.isAvailable.collectAsState()
    val birds by viewModel.birds.collectAsState()

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text("NFC Scanner", style = MaterialTheme.typography.headlineMedium)
                Button(onClick = { viewModel.openBatchSetup() }, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) {
                    Icon(Icons.Default.Group, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Graduate Batch")
                }
            }
        }
        if (!isAvailable) {
            item {
                Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF3E0))) {
                    Text("NFC is not available or not enabled. Enable NFC in system settings.", Modifier.padding(16.dp), color = Color(0xFF6D4C00), style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
        item { NfcScanArea(writeMode, pendingWriteData) }
        if (writeResult != null) {
            item {
                Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = if (writeResult!!.success) Color(0xFFE8F5E9) else Color(0xFFFFEBEE))) {
                    Row(Modifier.fillMaxWidth().padding(12.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                        Text(writeResult!!.message, Modifier.weight(1f), color = if (writeResult!!.success) Color(0xFF2E7D32) else Color(0xFFC62828), style = MaterialTheme.typography.bodyMedium)
                        IconButton(onClick = { nfcService.clearWriteResult() }) { Icon(Icons.Default.Close, "Dismiss", Modifier.size(18.dp)) }
                    }
                }
            }
        }
        if (lastScan != null) {
            item { Text("Last Scan", style = MaterialTheme.typography.titleMedium) }
            item { NfcResultCard(lastScan!!) }
        }
        item {
            HorizontalDivider(); Spacer(Modifier.height(4.dp))
            WriteTagSection(birds, writeMode, { viewModel.startWriteMode(it) }, { viewModel.cancelWriteMode() })
        }
        if (scanHistory.size > 1) {
            item { HorizontalDivider(); Spacer(Modifier.height(4.dp)); Text("Scan History", style = MaterialTheme.typography.titleMedium) }
            items(scanHistory.drop(1)) { NfcHistoryItem(it) }
        }
        item { Spacer(Modifier.height(8.dp)) }
    }
}

// =====================================================================
// Batch Setup — lineage + count only
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BatchSetupScreen(viewModel: NfcViewModel) {
    val lineages by viewModel.lineages.collectAsState()
    var selectedLineageId by remember { mutableStateOf<Int?>(null) }
    var birdCount by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }

    Column(Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Text("Graduate Batch", style = MaterialTheme.typography.headlineMedium)
            IconButton(onClick = { viewModel.cancelBatch() }) { Icon(Icons.Default.Close, "Cancel") }
        }
        Text("Band and register a batch of chicks with NFC tags. You'll set sex and band color for each bird individually.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)

        ExposedDropdownMenuBox(expanded, { expanded = it }) {
            OutlinedTextField(
                value = selectedLineageId?.let { id -> lineages.find { it.id == id }?.name ?: "Lineage #$id" } ?: "",
                onValueChange = {}, readOnly = true, label = { Text("Lineage") },
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                modifier = Modifier.menuAnchor().fillMaxWidth(),
            )
            ExposedDropdownMenu(expanded, { expanded = false }) {
                lineages.forEach { bl ->
                    DropdownMenuItem(text = { Text(bl.name) }, onClick = { selectedLineageId = bl.id; expanded = false })
                }
            }
        }

        OutlinedTextField(
            value = birdCount,
            onValueChange = { birdCount = it.filter { ch -> ch.isDigit() } },
            label = { Text("Number of birds to band") },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
        )

        val count = birdCount.toIntOrNull() ?: 0
        Button(
            onClick = { selectedLineageId?.let { viewModel.startBatchTagging(count, it) } },
            enabled = count > 0 && selectedLineageId != null,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) {
            Icon(Icons.Default.Nfc, null, Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
            Text("Start Tagging $count Bird${if (count != 1) "s" else ""}", fontSize = 16.sp)
        }
    }
}

// =====================================================================
// Per-Bird Entry — sex, band color, notes per bird + NFC prompt
// =====================================================================

@Composable
fun PerBirdEntryScreen(state: BatchState.PerBirdEntry, viewModel: NfcViewModel) {
    val context = LocalContext.current
    // Form state. On a clean entry to bird N, initial values default to
    // empty / "Unknown". On re-entry after a failed submit (Bug 3),
    // `state.draftXxx` carries the user's prior input back through, so they
    // don't have to re-type / re-pick / re-take a photo.
    var sex by remember(state.currentIndex) { mutableStateOf(state.draftSex ?: "Unknown") }
    var bandColor by remember(state.currentIndex) {
        // Auto-fill from last bird of the same sex unless we have a draft.
        mutableStateOf(state.draftBandColor ?: "")
    }
    var notes by remember(state.currentIndex) { mutableStateOf(state.draftNotes ?: "") }
    var weightText by remember(state.currentIndex) { mutableStateOf(state.draftWeightText ?: "") }
    var photoBitmap by remember(state.currentIndex) { mutableStateOf<Bitmap?>(state.draftPhotoBitmap) }

    val photoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.TakePicturePreview(),
    ) { bitmap -> if (bitmap != null) photoBitmap = bitmap }

    // Bug A fix: TakePicturePreview requires the runtime CAMERA permission
    // when the calling app declares CAMERA in its manifest (which we do).
    // Without this gate, launching the camera on Android 12+ throws
    // SecurityException and crashes the activity. Request → grant → launch.
    val cameraPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) photoLauncher.launch(null)
        else Toast.makeText(context, "Camera permission denied", Toast.LENGTH_SHORT).show()
    }
    val launchCamera: () -> Unit = launchCamera@{
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        if (granted) photoLauncher.launch(null)
        else cameraPermissionLauncher.launch(Manifest.permission.CAMERA)
    }

    val writeResult by viewModel.nfcService.writeResult.collectAsState()

    // Tally
    val maleCount = state.graduated.count { it.bird.sex?.lowercase() == "male" }
    val femaleCount = state.graduated.count { it.bird.sex?.lowercase() == "female" }
    val unknownCount = state.graduated.count { it.bird.sex?.lowercase() !in listOf("male", "female") }

    Column(
        Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState()),
    ) {
        // Header
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Text("Graduate Batch", style = MaterialTheme.typography.headlineMedium)
            OutlinedButton(onClick = { viewModel.cancelBatch() }, colors = ButtonDefaults.outlinedButtonColors(contentColor = AlertRed)) { Text("Cancel") }
        }

        Spacer(Modifier.height(8.dp))

        // Progress
        LinearProgressIndicator(
            progress = { state.graduated.size.toFloat() / state.totalCount },
            modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
            color = SageGreen, trackColor = SageGreenLight.copy(alpha = 0.3f),
        )
        Spacer(Modifier.height(4.dp))

        // Tally
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
            Text("Bird ${state.currentIndex + 1} of ${state.totalCount}", style = MaterialTheme.typography.titleMedium)
            Text("M:$maleCount  F:$femaleCount  ?:$unknownCount", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }

        Spacer(Modifier.height(16.dp))

        // Sex selector
        Text("Sex", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf("Male", "Female", "Unknown").forEach { option ->
                val isSelected = sex == option
                OutlinedButton(
                    onClick = {
                        sex = option
                        // Auto-fill band color from last bird of this sex
                        val autoFill = when (option) {
                            "Male" -> state.lastMaleBandColor
                            "Female" -> state.lastFemaleBandColor
                            else -> ""
                        }
                        if (bandColor.isBlank() && autoFill.isNotBlank()) bandColor = autoFill
                    },
                    colors = if (isSelected) ButtonDefaults.outlinedButtonColors(containerColor = SageGreen, contentColor = Color.White)
                        else ButtonDefaults.outlinedButtonColors(),
                ) { Text(option) }
            }
        }

        Spacer(Modifier.height(12.dp))

        // Band color — preset dropdown with swatches, "Other" reveals a
        // free-text field for any color outside the preset palette.
        com.quailsync.app.ui.components.BandColorPicker(
            value = bandColor,
            onValueChange = { bandColor = it },
            label = "Band color (optional)",
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(8.dp))

        // Weight (optional)
        OutlinedTextField(
            value = weightText,
            onValueChange = { weightText = it.filter { ch -> ch.isDigit() || ch == '.' } },
            label = { Text("Weight (g) — optional") },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            supportingText = {
                val grams = weightText.toDoubleOrNull()
                if (grams != null && (grams < 50 || grams > 500)) {
                    Text("Outside typical 50–500g range", color = AlertYellow)
                }
            },
        )

        Spacer(Modifier.height(8.dp))

        // Photo (optional) — TakePicturePreview returns a Bitmap; FileProvider is
        // configured but the existing FlockScreen capture pattern uses preview, so
        // we follow that for consistency.
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (photoBitmap != null) {
                Image(
                    bitmap = photoBitmap!!.asImageBitmap(),
                    contentDescription = "Bird photo preview",
                    modifier = Modifier.size(64.dp).clip(RoundedCornerShape(8.dp)),
                )
                Spacer(Modifier.width(12.dp))
                OutlinedButton(onClick = launchCamera) {
                    Icon(Icons.Default.CameraAlt, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Retake")
                }
            } else {
                OutlinedButton(
                    onClick = launchCamera,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Default.CameraAlt, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(8.dp))
                    Text("Take Photo (optional)")
                }
            }
        }

        Spacer(Modifier.height(8.dp))

        // Notes
        OutlinedTextField(
            value = notes,
            onValueChange = { notes = it },
            label = { Text("Notes (optional)") },
            modifier = Modifier.fillMaxWidth(),
            minLines = 2,
        )

        Spacer(Modifier.height(16.dp))

        // Error feedback — shown ABOVE the Create button so the user sees
        // why the prior attempt failed before tapping again. Form state is
        // preserved (Bug 3) so retrying after a fix is one tap.
        if (writeResult != null && !writeResult!!.success) {
            Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(8.dp), colors = CardDefaults.cardColors(containerColor = Color(0xFFFFEBEE))) {
                Text(writeResult!!.message, Modifier.padding(12.dp), color = Color(0xFFC62828), style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(8.dp))
        }

        // Create & Tag button — always enabled, optional fields can be skipped.
        Button(
            onClick = {
                viewModel.createAndTagBird(
                    sex = sex,
                    bandColor = bandColor,
                    notes = notes,
                    weightText = weightText,
                    photoBitmap = photoBitmap,
                    context = context,
                )
            },
            modifier = Modifier.fillMaxWidth().height(52.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) {
            Icon(Icons.Default.Nfc, null, Modifier.size(20.dp))
            Spacer(Modifier.width(8.dp))
            Text("Create & Tag Bird", fontSize = 16.sp)
        }

        // Recent graduates
        if (state.graduated.isNotEmpty()) {
            Spacer(Modifier.height(12.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))
            Text("Tagged so far", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            state.graduated.reversed().take(5).forEach { g ->
                GraduatedRow(g)
            }
            if (state.graduated.size > 5) {
                Text("…and ${state.graduated.size - 5} more", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

// =====================================================================
// Batch Creating — spinner while API call runs
// =====================================================================

@Composable
fun BatchCreatingScreen(state: BatchState.CreatingBird) {
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text("Graduate Batch", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(8.dp))
        LinearProgressIndicator(
            progress = { state.graduated.size.toFloat() / state.totalCount },
            modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
            color = SageGreen, trackColor = SageGreenLight.copy(alpha = 0.3f),
        )
        Spacer(Modifier.height(32.dp))
        CircularProgressIndicator(color = SageGreen, modifier = Modifier.size(48.dp))
        Spacer(Modifier.height(16.dp))
        Text("Creating ${state.sex} bird…", style = MaterialTheme.typography.titleMedium)
    }
}

// =====================================================================
// Batch Awaiting Tag Write — write mode active, pulsing NFC prompt
// =====================================================================

@Composable
fun BatchAwaitingWriteScreen(state: BatchState.AwaitingTagWrite, viewModel: NfcViewModel) {
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Text("Graduate Batch", style = MaterialTheme.typography.headlineMedium)
            OutlinedButton(onClick = { viewModel.cancelBatch() }, colors = ButtonDefaults.outlinedButtonColors(contentColor = AlertRed)) { Text("Cancel") }
        }
        Spacer(Modifier.height(8.dp))
        LinearProgressIndicator(
            progress = { state.graduated.size.toFloat() / state.totalCount },
            modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
            color = SageGreen, trackColor = SageGreenLight.copy(alpha = 0.3f),
        )
        Spacer(Modifier.height(4.dp))
        Text("${state.graduated.size} of ${state.totalCount} tagged", style = MaterialTheme.typography.bodyMedium)

        Spacer(Modifier.height(24.dp))

        // Bird created confirmation
        Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = AlertGreen.copy(alpha = 0.1f))) {
            Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Default.CheckCircle, null, tint = AlertGreen, modifier = Modifier.size(24.dp))
                Spacer(Modifier.width(8.dp))
                Text("Bird #${state.pendingBird.id} created", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            }
        }

        Spacer(Modifier.height(16.dp))

        // NFC scan prompt
        NfcScanArea(writeMode = true, pendingWriteData = "Tap tag to write BIRD-${state.pendingBird.id}")
    }
}

// =====================================================================
// Post-Tag Confirm — photo, weight, skip
// =====================================================================

@Composable
fun PostTagConfirmScreen(state: BatchState.PostTagConfirm, viewModel: NfcViewModel) {
    val context = LocalContext.current
    var photoBitmap by remember { mutableStateOf<Bitmap?>(null) }
    var showWeightEntry by remember { mutableStateOf(false) }
    var weightText by remember { mutableStateOf("") }
    var weightNotes by remember { mutableStateOf("") }
    val weightError by viewModel.weightError.collectAsState()

    val photoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.TakePicturePreview(),
    ) { bitmap ->
        if (bitmap != null) {
            photoBitmap = bitmap
            viewModel.saveBirdPhotoLocally(state.justTaggedBird.id, bitmap, context)
        }
    }

    // Bug A fix: gate camera launch on runtime CAMERA permission.
    val cameraPermissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) photoLauncher.launch(null)
        else Toast.makeText(context, "Camera permission denied", Toast.LENGTH_SHORT).show()
    }
    val launchCamera: () -> Unit = launchCamera@{
        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        if (granted) photoLauncher.launch(null)
        else cameraPermissionLauncher.launch(Manifest.permission.CAMERA)
    }

    val maleCount = state.graduated.count { it.bird.sex?.lowercase() == "male" }
    val femaleCount = state.graduated.count { it.bird.sex?.lowercase() == "female" }
    val unknownCount = state.graduated.size - maleCount - femaleCount

    Column(
        Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState()),
    ) {
        // Header
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Text("Graduate Batch", style = MaterialTheme.typography.headlineMedium)
            OutlinedButton(onClick = { viewModel.cancelBatch() }, colors = ButtonDefaults.outlinedButtonColors(contentColor = AlertRed)) { Text("Cancel") }
        }

        Spacer(Modifier.height(8.dp))

        LinearProgressIndicator(
            progress = { state.graduated.size.toFloat() / state.totalCount },
            modifier = Modifier.fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)),
            color = SageGreen, trackColor = SageGreenLight.copy(alpha = 0.3f),
        )
        Spacer(Modifier.height(4.dp))
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
            Text("${state.graduated.size} of ${state.totalCount} tagged", style = MaterialTheme.typography.bodyMedium)
            Text("M:$maleCount  F:$femaleCount  ?:$unknownCount", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }

        Spacer(Modifier.height(16.dp))

        // Success card
        Card(
            Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = AlertGreen.copy(alpha = 0.1f)),
        ) {
            Column(Modifier.fillMaxWidth().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(Icons.Default.CheckCircle, null, tint = AlertGreen, modifier = Modifier.size(40.dp))
                Spacer(Modifier.height(8.dp))
                Text(
                    "Bird #${state.justTaggedBird.id} created & tagged!",
                    style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold,
                )
                val details = listOfNotNull(
                    state.justTaggedBird.sex?.replaceFirstChar { it.uppercase() },
                    state.justTaggedBird.bandColor?.let { "band: $it" },
                ).joinToString(" · ")
                if (details.isNotEmpty()) {
                    Text(details, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Text("Tag: ${state.justTaggedTagId}", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }

        Spacer(Modifier.height(16.dp))

        // Photo section
        Card(
            Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(1.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                if (state.photoSaved && photoBitmap != null) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Default.CheckCircle, null, tint = AlertGreen, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Photo saved", style = MaterialTheme.typography.bodyMedium, color = AlertGreen, fontWeight = FontWeight.Medium)
                    }
                } else {
                    Button(
                        onClick = launchCamera,
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) {
                        Icon(Icons.Default.CameraAlt, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Take Photo")
                    }
                }
            }
        }

        Spacer(Modifier.height(8.dp))

        // Weight section
        Card(
            Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(1.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                if (state.weightLogged) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Default.CheckCircle, null, tint = AlertGreen, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Weight logged", style = MaterialTheme.typography.bodyMedium, color = AlertGreen, fontWeight = FontWeight.Medium)
                    }
                } else if (showWeightEntry) {
                    Text("Log Weight", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(
                        value = weightText,
                        onValueChange = { weightText = it.filter { ch -> ch.isDigit() || ch == '.' } },
                        label = { Text("Weight (grams)") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Spacer(Modifier.height(4.dp))
                    OutlinedTextField(
                        value = weightNotes,
                        onValueChange = { weightNotes = it },
                        label = { Text("Notes (optional)") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )
                    Spacer(Modifier.height(8.dp))
                    var localWeightError by remember { mutableStateOf<String?>(null) }
                    Button(
                        onClick = {
                            Log.d("QuailSync", "Save Weight tapped: weightText='$weightText' for bird ${state.justTaggedBird.id}")
                            val grams = weightText.toDoubleOrNull()
                            if (grams == null) {
                                localWeightError = "Enter a valid number (got: '$weightText')"
                                Log.e("QuailSync", "Weight parse failed: '$weightText'")
                            } else if (grams <= 0) {
                                localWeightError = "Weight must be greater than 0"
                                Log.e("QuailSync", "Weight <= 0: $grams")
                            } else {
                                localWeightError = null
                                Log.d("QuailSync", "Calling logBirdWeight(${state.justTaggedBird.id}, $grams, '$weightNotes')")
                                viewModel.logBirdWeight(state.justTaggedBird.id, grams, weightNotes)
                            }
                        },
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) { Text("Save Weight") }
                    val errorToShow = localWeightError ?: weightError
                    if (errorToShow != null) {
                        Spacer(Modifier.height(4.dp))
                        Text(errorToShow, style = MaterialTheme.typography.bodyMedium, color = AlertRed)
                    }
                } else {
                    OutlinedButton(
                        onClick = { showWeightEntry = true },
                        Modifier.fillMaxWidth(),
                    ) {
                        Icon(Icons.Default.Edit, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Log Weight")
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // Next / Skip button
        Button(
            onClick = { viewModel.advanceFromConfirm() },
            Modifier.fillMaxWidth().height(48.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) {
            val isLast = state.currentIndex >= state.totalCount
            Text(if (isLast) "Finish" else "Next Bird →", fontSize = 16.sp)
        }
    }
}

// =====================================================================
// Batch Complete — summary
// =====================================================================

@Composable
fun BatchCompleteScreen(state: BatchState.Complete, viewModel: NfcViewModel) {
    val maleCount = state.graduated.count { it.bird.sex?.lowercase() == "male" }
    val femaleCount = state.graduated.count { it.bird.sex?.lowercase() == "female" }
    val unknownCount = state.graduated.size - maleCount - femaleCount

    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Spacer(Modifier.height(24.dp))
        Icon(Icons.Default.CheckCircle, null, tint = AlertGreen, modifier = Modifier.size(80.dp))
        Spacer(Modifier.height(16.dp))
        Text("${state.graduated.size} birds graduated", style = MaterialTheme.typography.headlineMedium, textAlign = TextAlign.Center)
        if (state.lineageName != null) {
            Text("from ${state.lineageName} lineage", style = MaterialTheme.typography.titleMedium, color = SageGreen)
        }
        Spacer(Modifier.height(8.dp))
        Text("Males: $maleCount  Females: $femaleCount  Unknown: $unknownCount", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)

        Spacer(Modifier.height(16.dp))

        LazyColumn(Modifier.weight(1f).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            items(state.graduated, key = { "${it.bird.id}-${it.tagId}" }) { g ->
                Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(8.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
                    Row(Modifier.fillMaxWidth().padding(10.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(28.dp).clip(CircleShape).background(SageGreen), contentAlignment = Alignment.Center) {
                            Text("${g.index + 1}", style = MaterialTheme.typography.labelLarge, color = Color.White, fontWeight = FontWeight.Bold)
                        }
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text("Bird #${g.bird.id}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            val details = listOfNotNull(
                                g.bird.sex?.replaceFirstChar { it.uppercase() },
                                g.bird.bandColor?.let { "band: $it" },
                            ).joinToString(" · ")
                            if (details.isNotEmpty()) Text(details, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        Text(g.tagId, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        Button(onClick = { viewModel.dismissBatchSummary() }, Modifier.fillMaxWidth().height(48.dp), colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) {
            Text("Done")
        }
    }
}

// =====================================================================
// Shared composables
// =====================================================================

@Composable
fun GraduatedRow(g: GraduatedBird) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Default.CheckCircle, null, tint = AlertGreen, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(6.dp))
        Text("Bird #${g.bird.id}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
        Spacer(Modifier.width(6.dp))
        Text(g.bird.sex?.replaceFirstChar { it.uppercase() } ?: "?", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (g.bird.bandColor != null) {
            Spacer(Modifier.width(6.dp))
            Text(g.bird.bandColor, style = MaterialTheme.typography.bodyMedium, color = SageGreen)
        }
        Spacer(Modifier.weight(1f))
        Text(g.tagId, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
fun NfcScanArea(writeMode: Boolean, pendingWriteData: String?) {
    Card(
        Modifier.fillMaxWidth(), shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = if (writeMode) DustyRose.copy(alpha = 0.15f) else SageGreenLight.copy(alpha = 0.15f)),
    ) {
        Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            PulsingNfcIcon(writeMode)
            Spacer(Modifier.height(16.dp))
            Text(
                if (writeMode) "Hold phone near tag to write" else "Hold phone near NFC tag to scan",
                style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center,
            )
            if (writeMode && pendingWriteData != null) {
                Spacer(Modifier.height(8.dp))
                Text(pendingWriteData, style = MaterialTheme.typography.bodyMedium, color = DustyRose, fontWeight = FontWeight.Medium)
            }
        }
    }
}

@Composable
fun PulsingNfcIcon(writeMode: Boolean) {
    val transition = rememberInfiniteTransition(label = "nfc_pulse")
    val scale by transition.animateFloat(1f, 1.15f, infiniteRepeatable(tween(1000, easing = LinearEasing), RepeatMode.Reverse), label = "s")
    val ringAlpha by transition.animateFloat(0.4f, 0f, infiniteRepeatable(tween(1500, easing = LinearEasing), RepeatMode.Restart), label = "a")
    val color = if (writeMode) DustyRose else SageGreen
    Box(Modifier.size(120.dp), contentAlignment = Alignment.Center) {
        Box(Modifier.size(120.dp).scale(scale).alpha(ringAlpha).border(3.dp, color, CircleShape))
        Box(Modifier.size(80.dp).clip(CircleShape).background(color.copy(alpha = 0.12f)), contentAlignment = Alignment.Center) {
            Icon(if (writeMode) Icons.Default.Edit else Icons.Default.Nfc, null, Modifier.size(40.dp), tint = color)
        }
    }
}

@Composable
fun NfcResultCard(scan: NfcScanResult) {
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface), elevation = CardDefaults.cardElevation(2.dp)) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text("Tag: ${scan.tagId}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                Text(scan.timestamp.format(DateTimeFormatter.ofPattern("HH:mm:ss")), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (scan.payload != null) { Spacer(Modifier.height(4.dp)); Text("Data: ${scan.payload}", style = MaterialTheme.typography.bodyMedium) }
            if (scan.bird != null) { Spacer(Modifier.height(12.dp)); NfcBirdInfo(scan.bird) }
            else if (scan.payload?.startsWith("BIRD-") == true) { Spacer(Modifier.height(8.dp)); Text("Looking up bird…", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

@Composable
fun NfcBirdInfo(bird: Bird) {
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(8.dp), colors = CardDefaults.cardColors(containerColor = AlertGreen.copy(alpha = 0.1f))) {
        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(36.dp).clip(CircleShape).background(parseBandColor(bird.bandColor)), contentAlignment = Alignment.Center) {
                Text("\uD83D\uDC25", fontSize = 16.sp)
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(bird.bandId ?: "Bird #${bird.id}", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    bird.sex?.let { Text(it.replaceFirstChar { c -> c.uppercase() }, style = MaterialTheme.typography.bodyMedium) }
                    if (bird.lineages.isNotEmpty()) {
                        Text(com.quailsync.app.data.formatLineages(bird.lineages), style = MaterialTheme.typography.bodyMedium, color = SageGreen)
                    }
                    bird.status?.let { Text(it.replaceFirstChar { c -> c.uppercase() }, style = MaterialTheme.typography.bodyMedium) }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WriteTagSection(birds: List<Bird>, writeMode: Boolean, onStartWrite: (Int) -> Unit, onCancel: () -> Unit) {
    var selectedBirdId by remember { mutableStateOf<Int?>(null) }
    var expanded by remember { mutableStateOf(false) }
    Column {
        Text("Write Tag", style = MaterialTheme.typography.titleMedium); Spacer(Modifier.height(8.dp))
        if (writeMode) {
            OutlinedButton(onClick = onCancel, Modifier.fillMaxWidth(), colors = ButtonDefaults.outlinedButtonColors(contentColor = AlertRed)) {
                Icon(Icons.Default.Close, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Cancel Write Mode")
            }
        } else {
            ExposedDropdownMenuBox(expanded, { expanded = it }) {
                OutlinedTextField(
                    value = selectedBirdId?.let { id -> birds.find { it.id == id }?.let { b -> "${b.bandId ?: "Bird #${b.id}"} — ${com.quailsync.app.data.formatLineages(b.lineages, emptyText = "").ifEmpty { b.sex ?: "" }}" } ?: "Bird #$id" } ?: "",
                    onValueChange = {}, readOnly = true, label = { Text("Select bird to write") },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                    modifier = Modifier.menuAnchor().fillMaxWidth(),
                )
                ExposedDropdownMenu(expanded, { expanded = false }) {
                    birds.forEach { bird ->
                        DropdownMenuItem(text = { Text("${bird.bandId ?: "Bird #${bird.id}"} — ${com.quailsync.app.data.formatLineages(bird.lineages, emptyText = "")}") }, onClick = { selectedBirdId = bird.id; expanded = false })
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            Button(onClick = { selectedBirdId?.let { onStartWrite(it) } }, enabled = selectedBirdId != null, modifier = Modifier.fillMaxWidth(), colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) {
                Icon(Icons.Default.Edit, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Write BIRD-${selectedBirdId ?: "?"} to Tag")
            }
        }
    }
}

@Composable
fun NfcHistoryItem(scan: NfcScanResult) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(scan.tagId, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                scan.payload?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = SageGreen) }
            }
            scan.bird?.let { b -> Text("${b.bandId ?: "Bird #${b.id}"} — ${com.quailsync.app.data.formatLineages(b.lineages, emptyText = "")}", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        Text(scan.timestamp.format(DateTimeFormatter.ofPattern("HH:mm")), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
@Suppress("UNUSED_PARAMETER")
fun TagConflictDialog(conflict: TagConflict, existingBird: Bird, onConfirm: () -> Unit, onCancel: () -> Unit) {
    val isActive = existingBird.status?.lowercase() in listOf("active", "alive")
    Dialog(onDismissRequest = onCancel) {
        Card(shape = RoundedCornerShape(16.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
            Column(Modifier.padding(20.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(if (isActive) Icons.Default.Warning else Icons.Default.Info, null, tint = if (isActive) AlertRed else AlertYellow, modifier = Modifier.size(28.dp))
                    Spacer(Modifier.width(12.dp))
                    Text(if (isActive) "Tag Already Assigned" else "Tag Previously Used", style = MaterialTheme.typography.titleLarge)
                }
                Spacer(Modifier.height(16.dp))
                Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(8.dp), colors = CardDefaults.cardColors(containerColor = if (isActive) AlertRed.copy(alpha = 0.08f) else MaterialTheme.colorScheme.surfaceVariant)) {
                    Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(36.dp).clip(CircleShape).background(parseBandColor(existingBird.bandColor)), contentAlignment = Alignment.Center) {
                            Text("\uD83D\uDC25", fontSize = 16.sp)
                        }
                        Spacer(Modifier.width(12.dp))
                        Column {
                            Text(existingBird.bandId ?: "Bird #${existingBird.id}", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                            val lineageStr = com.quailsync.app.data.formatLineages(existingBird.lineages, emptyText = "").ifEmpty { null }
                            val details = listOfNotNull(existingBird.sex?.replaceFirstChar { it.uppercase() }, lineageStr, existingBird.status?.uppercase()).joinToString(" · ")
                            if (details.isNotEmpty()) Text(details, style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }
                Spacer(Modifier.height(16.dp))
                Text(
                    if (isActive) "This tag is assigned to an ACTIVE bird. Reassign?"
                    else "This tag was previously assigned to Bird #${existingBird.id} (${existingBird.status}). Reassign?",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(20.dp))
                Row(Modifier.fillMaxWidth(), Arrangement.End) {
                    OutlinedButton(onClick = onCancel) { Text("Cancel") }
                    Spacer(Modifier.width(8.dp))
                    Button(onClick = onConfirm, colors = ButtonDefaults.buttonColors(containerColor = if (isActive) AlertRed else SageGreen)) {
                        Text(if (isActive) "Reassign Tag" else "Reassign")
                    }
                }
            }
        }
    }
}

// =====================================================================
// Orphan-tag dialog (Bug C): tag references a bird that no longer exists.
// Always requires explicit user confirmation before the tag can be reused.
// =====================================================================

@Composable
fun OrphanTagDialog(orphan: OrphanTag, onReuse: () -> Unit, onCancel: () -> Unit) {
    Dialog(onDismissRequest = onCancel) {
        Card(
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            Column(Modifier.padding(20.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Default.Warning, null, tint = AlertYellow, modifier = Modifier.size(28.dp))
                    Spacer(Modifier.width(10.dp))
                    Text("Orphan tag", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                }
                Spacer(Modifier.height(12.dp))
                Text(
                    "This tag is registered to bird #${orphan.orphanBirdId} but that bird no longer exists in the database. Reuse this tag for a new bird?",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(Modifier.height(6.dp))
                Text(
                    "Tag id: ${orphan.tagId}",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(20.dp))
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    OutlinedButton(onClick = onCancel) { Text("Cancel") }
                    Spacer(Modifier.width(8.dp))
                    Button(
                        onClick = onReuse,
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) { Text("Reuse Tag") }
                }
            }
        }
    }
}
