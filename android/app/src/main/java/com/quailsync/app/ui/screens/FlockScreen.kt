@file:Suppress(
    "ASSIGNED_BUT_NEVER_ACCESSED_VARIABLE",
    "UNUSED_VALUE",
    "CanBeVal",
    "UnusedVariable"
)

package com.quailsync.app.ui.screens

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.Log
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ContentCut
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.core.graphics.toColorInt
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.BirdWeight
import com.quailsync.app.data.CullBatchRequest
import com.quailsync.app.data.FlockBreedingStats
import com.quailsync.app.data.Lineage
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File

// =====================================================================
// ViewModel
// =====================================================================

class FlockViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    private val _lineages = MutableStateFlow<List<Lineage>>(emptyList())
    val lineages: StateFlow<List<Lineage>> = _lineages.asStateFlow()

    /** Server breeding-capacity snapshot for the cull-mode guardrail. Null
     *  until first load completes; treat as "no guardrail data yet" — the
     *  Cull Mode UI degrades to selection-only without zone coloring. */
    private val _flockStats = MutableStateFlow<FlockBreedingStats?>(null)
    val flockStats: StateFlow<FlockBreedingStats?> = _flockStats.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    init { loadData() }

    fun refresh() {
        viewModelScope.launch {
            _isRefreshing.value = true
            loadDataSuspend()
            _isRefreshing.value = false
        }
    }

    private fun loadData() { viewModelScope.launch { loadDataSuspend() } }

    private suspend fun loadDataSuspend() {
        try {
            val birdList = api.getBirds()
            Log.d("QuailSync", "Birds loaded: ${birdList.size}")
            _birds.value = birdList
            val lineageList = try { api.getLineages() } catch (e: Exception) { Log.e("QuailSync", "Failed to load lineages", e); emptyList() }
            _lineages.value = lineageList
            _flockStats.value = try { api.getFlockBreedingStats() } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load flock breeding stats", e); null
            }
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load birds", e)
        } finally {
            _isLoading.value = false
        }
    }

    /** Confirms a cull selection from the Flock screen's Cull Mode bottom
     *  bar. Returns the server-reported count of birds actually updated. */
    suspend fun cullBatch(
        birdIds: List<Int>,
        reason: String,
        method: String,
        notes: String?,
        date: String,
    ): Int {
        val resp = api.cullBatch(CullBatchRequest(birdIds, reason, method, notes, date))
        loadDataSuspend()
        return resp.updated
    }

    suspend fun getBirdWeights(birdId: Int): List<BirdWeight> {
        return try { api.getBirdWeights(birdId) } catch (e: Exception) { Log.e("QuailSync", "Failed to load weights for bird $birdId", e); emptyList() }
    }

    @Suppress("unused") fun uploadBirdPhoto(birdId: Int, uri: Uri, context: android.content.Context) {
        viewModelScope.launch {
            // Always save to the standard local path first
            try {
                val dir = File(context.filesDir, "bird_photos").also { it.mkdirs() }
                val dest = File(dir, "bird_${birdId}.jpg")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    dest.outputStream().use { input.copyTo(it) }
                }
                Log.d("QuailSync", "Photo saved locally: ${dest.absolutePath}")
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to save photo locally", e)
            }
            // Try server upload
            try {
                val bytes = context.contentResolver.openInputStream(uri)?.readBytes() ?: return@launch
                val part = okhttp3.MultipartBody.Part.createFormData(
                    "photo", "bird_${birdId}.jpg", bytes.toRequestBody("image/jpeg".toMediaType()),
                )
                api.uploadBirdPhoto(birdId, part)
                Log.d("QuailSync", "Photo uploaded for bird $birdId")
            } catch (e: Exception) {
                Log.e("QuailSync", "Photo upload failed (saved locally)", e)
            }
        }
    }

    fun updateBirdStatus(birdId: Int, status: String, notes: String? = null, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try {
                api.updateBird(birdId, com.quailsync.app.data.UpdateBirdRequest(status = status, notes = notes))
                true
            } catch (e: Exception) { Log.e("QuailSync", "Update bird status failed", e); false }
            if (ok) loadDataSuspend()
            onResult(ok)
        }
    }

    fun deleteBirdById(birdId: Int, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try { api.deleteBird(birdId); true } catch (e: Exception) { Log.e("QuailSync", "Delete bird failed", e); false }
            if (ok) loadDataSuspend()
            onResult(ok)
        }
    }

    fun logWeight(birdId: Int, weightGrams: Double, notes: String?, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try {
                api.createBirdWeight(birdId, com.quailsync.app.data.CreateWeightRequest(
                    weightGrams = weightGrams,
                    date = java.time.LocalDate.now().format(java.time.format.DateTimeFormatter.ISO_LOCAL_DATE),
                    notes = notes,
                ))
                true
            } catch (e: Exception) { Log.e("QuailSync", "Log weight failed", e); false }
            onResult(ok)
        }
    }

    fun deleteWeight(birdId: Int, weightId: Int, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try { api.deleteBirdWeight(birdId, weightId); true } catch (e: Exception) { Log.e("QuailSync", "Delete weight failed", e); false }
            onResult(ok)
        }
    }

    fun updateBird(birdId: Int, request: com.quailsync.app.data.UpdateBirdRequest, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try { api.updateBird(birdId, request); true } catch (e: Exception) { Log.e("QuailSync", "Update bird failed", e); false }
            if (ok) loadDataSuspend()
            onResult(ok)
        }
    }

    fun createBird(request: com.quailsync.app.data.CreateBirdRequest, onResult: (Bird?) -> Unit) {
        viewModelScope.launch {
            val bird = try { api.createBird(request) } catch (e: Exception) { Log.e("QuailSync", "Create bird failed", e); null }
            if (bird != null) loadDataSuspend()
            onResult(bird)
        }
    }

    /** Save a bitmap directly (used from TakePicturePreview). */
    fun saveBirdPhotoBitmap(birdId: Int, bitmap: Bitmap, context: android.content.Context) {
        viewModelScope.launch {
            try {
                val dir = File(context.filesDir, "bird_photos").also { it.mkdirs() }
                val file = File(dir, "bird_${birdId}.jpg")
                file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.JPEG, 90, it) }
                Log.d("QuailSync", "Photo saved: ${file.absolutePath}")
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to save photo", e)
            }
        }
    }
}

// =====================================================================
// Helpers
// =====================================================================

sealed class FlockFilter {
    data object Active : FlockFilter()
    data object All : FlockFilter()
    data object Records : FlockFilter()
    data object Males : FlockFilter()
    data object Females : FlockFilter()
    data class ByLineage(val lineageId: Int, val name: String) : FlockFilter()
}

private fun formatSex(sex: String?): String {
    return when (sex?.lowercase()) {
        "male", "m" -> "Male"
        "female", "f" -> "Female"
        else -> "Unknown"
    }
}

internal fun parseBandColor(color: String?): Color {
    if (color == null) return Color(0xFF9E9E9E)
    return try {
        val hex = color.removePrefix("#")
        Color("#$hex".toColorInt())
    } catch (_: Exception) {
        when (color.lowercase()) {
            "red" -> Color(0xFFCC4444)
            "blue" -> Color(0xFF4477CC)
            "green" -> Color(0xFF6A8B5E)
            "yellow" -> Color(0xFFCCA844)
            "orange" -> Color(0xFFCC8844)
            "purple" -> Color(0xFF8855AA)
            "pink" -> Color(0xFFD4A0A0)
            "white" -> Color(0xFFCCCCCC)
            "black" -> Color(0xFF333333)
            else -> Color(0xFF9E9E9E)
        }
    }
}

/** Load a bird's photo bitmap from local storage. Returns null if not found. */
@Composable
fun rememberBirdPhoto(birdId: Int, refreshKey: Int = 0): Bitmap? {
    val context = LocalContext.current
    return remember(birdId, refreshKey) {
        val file = File(context.filesDir, "bird_photos/bird_${birdId}.jpg")
        if (file.exists()) {
            try { BitmapFactory.decodeFile(file.absolutePath) } catch (_: Exception) { null }
        } else null
    }
}

// =====================================================================
// Flock Screen
// =====================================================================

@Composable
fun FlockScreen(
    viewModel: FlockViewModel = viewModel(),
    onBreedingClick: () -> Unit = {},
) {
    val birds by viewModel.birds.collectAsState()
    val lineages by viewModel.lineages.collectAsState()
    val flockStats by viewModel.flockStats.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    var selectedFilter by remember { mutableStateOf<FlockFilter>(FlockFilter.Active) }
    var selectedBird by remember { mutableStateOf<Bird?>(null) }
    var showAddBird by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    // Cull Mode: toggled by the scissors icon. While active, bird rows show
    // checkboxes and a sticky bottom-bar surfaces the guardrail. Selection
    // is held as a stable SnapshotStateList so checkbox toggles recompose
    // only the affected row.
    var cullMode by remember { mutableStateOf(false) }
    val selectedToCull = remember { mutableStateListOf<Int>() }
    var showCullDialog by remember { mutableStateOf(false) }

    val filteredBirds = remember(birds, selectedFilter) {
        when (selectedFilter) {
            FlockFilter.Active -> birds.filter { it.status?.lowercase() == "active" }
            FlockFilter.All -> birds
            FlockFilter.Records -> birds.filter { it.status?.lowercase() in listOf("culled", "deceased", "sold") }
            FlockFilter.Males -> birds.filter { it.sex?.lowercase() == "male" && it.status?.lowercase() == "active" }
            FlockFilter.Females -> birds.filter { it.sex?.lowercase() == "female" && it.status?.lowercase() == "active" }
            is FlockFilter.ByLineage -> {
                // Match any of the bird's lineages — under many-to-many a bird
                // with [Fernbank, NWQuail] should appear under both filters.
                val targetId = (selectedFilter as FlockFilter.ByLineage).lineageId
                birds.filter { b -> b.lineages.any { it.id == targetId } }
            }
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Flock", style = MaterialTheme.typography.headlineMedium)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("${filteredBirds.size} bird${if (filteredBirds.size != 1) "s" else ""}", style = MaterialTheme.typography.bodyMedium)
                Spacer(Modifier.width(8.dp))
                if (isRefreshing) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = SageGreen)
                } else {
                    IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
                }
                IconButton(
                    onClick = onBreedingClick,
                    modifier = Modifier.testTag("flock_breeding"),
                ) { Icon(Icons.Default.Groups, "Breeding groups", tint = SageGreen) }
                // Scissors toggles Cull Mode. Tinted differently when active
                // so the user can see at a glance that selection mode is on.
                IconButton(
                    onClick = {
                        cullMode = !cullMode
                        if (!cullMode) selectedToCull.clear()
                    },
                    modifier = Modifier.testTag("flock_cull"),
                ) {
                    Icon(
                        Icons.Default.ContentCut,
                        contentDescription = if (cullMode) "Exit cull mode" else "Cull mode",
                        tint = if (cullMode) AlertRed else SageGreen,
                    )
                }
                IconButton(
                    onClick = { showAddBird = true },
                    modifier = Modifier.testTag("flock_add_bird"),
                ) { Icon(Icons.Default.Add, "Add Bird", tint = SageGreen) }
            }
        }

        if (!isLoading || birds.isNotEmpty()) {
            FlockFilterChips(lineages, selectedFilter) { selectedFilter = it }
        }

        when {
            isLoading && birds.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = SageGreen) }
            }
            birds.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("\uD83D\uDC25", fontSize = 48.sp)
                        Spacer(Modifier.height(16.dp))
                        Text("No birds registered yet.\nAdd birds from the web dashboard or CLI.", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    }
                }
            }
            else -> {
                // Pre-compute the per-female "last safe mate" map so each row
                // can render its own warning without scanning the whole stats
                // payload. Maps female_id -> list of unselected males still
                // safe for her; selecting a male whose id appears in any
                // length-1 list will orphan that female.
                val safeMalesForFemale: Map<Int, List<Int>> = remember(flockStats, selectedToCull.toList()) {
                    val out = HashMap<Int, MutableList<Int>>()
                    flockStats?.perMaleSafePairings?.forEach { entry ->
                        if (entry.birdId in selectedToCull) return@forEach
                        entry.safeFemaleIds.forEach { fid ->
                            out.getOrPut(fid) { mutableListOf() }.add(entry.birdId)
                        }
                    }
                    out
                }

                Column(Modifier.fillMaxSize()) {
                    LazyColumn(
                        modifier = Modifier.testTag("flock_bird_list").weight(1f),
                        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                        verticalArrangement = Arrangement.spacedBy(10.dp),
                    ) {
                        items(filteredBirds, key = { it.id }) { bird ->
                            // Per-row "last safe mate" warning: only meaningful
                            // for active males. Lists females who'd be left
                            // with zero safe mates if this male were culled
                            // alongside the current selection.
                            val orphanedFemaleIds: List<Int> = if (
                                cullMode &&
                                bird.sex?.lowercase() == "male" &&
                                bird.id !in selectedToCull
                            ) {
                                val mine = flockStats?.perMaleSafePairings
                                    ?.firstOrNull { it.birdId == bird.id }
                                    ?.safeFemaleIds.orEmpty()
                                mine.filter { fid ->
                                    val safe = safeMalesForFemale[fid].orEmpty()
                                    safe.size == 1 && safe.first() == bird.id
                                }
                            } else emptyList()

                            CullableBirdRow(
                                bird = bird,
                                lineageLabel = com.quailsync.app.data.formatLineages(bird.lineages, emptyText = "").ifEmpty { null },
                                cullMode = cullMode,
                                selected = bird.id in selectedToCull,
                                orphanedFemaleIds = orphanedFemaleIds,
                                onToggleSelect = {
                                    if (bird.id in selectedToCull) selectedToCull.remove(bird.id)
                                    else selectedToCull.add(bird.id)
                                },
                                onOpen = { selectedBird = bird },
                            )
                        }
                        item { Spacer(Modifier.height(8.dp)) }
                    }
                    if (cullMode) {
                        CullModeBottomBar(
                            stats = flockStats,
                            allBirds = birds,
                            selectedIds = selectedToCull.toList(),
                            onClear = { selectedToCull.clear() },
                            onConfirm = { showCullDialog = true },
                        )
                    }
                }
            }
        }
    }

    if (showCullDialog) {
        FlockCullConfirmDialog(
            count = selectedToCull.size,
            onConfirm = { method, notes ->
                showCullDialog = false
                val ids = selectedToCull.toList()
                scope.launch {
                    try {
                        val updated = viewModel.cullBatch(
                            ids,
                            // The new flow doesn't track per-bird reasons —
                            // the user owns the selection, so we just record a
                            // generic "manual_cull". The cull-batch endpoint
                            // still requires a reason string for audit.
                            reason = "manual_cull",
                            method = method,
                            notes = notes.ifBlank { null },
                            date = java.time.LocalDate.now().toString(),
                        )
                        Toast.makeText(
                            context,
                            "Culled $updated bird${if (updated != 1) "s" else ""}",
                            Toast.LENGTH_SHORT,
                        ).show()
                        selectedToCull.clear()
                        cullMode = false
                    } catch (e: Exception) {
                        Toast.makeText(context, "Cull failed: ${e.message}", Toast.LENGTH_SHORT).show()
                    }
                }
            },
            onDismiss = { showCullDialog = false },
        )
    }

    if (selectedBird != null) {
        BirdDetailDialog(
            selectedBird!!,
            com.quailsync.app.data.formatLineages(selectedBird!!.lineages, emptyText = "").ifEmpty { null },
            viewModel,
            onDismiss = { selectedBird = null },
            onStatusChanged = { selectedBird = null; viewModel.refresh() },
            onDeleted = { selectedBird = null; viewModel.refresh() },
        )
    }

    if (showAddBird) {
        AddBirdDialog(lineages, viewModel, onDismiss = { showAddBird = false }, onSuccess = { bird ->
            showAddBird = false
            Toast.makeText(context, "Bird #${bird.id} created!", Toast.LENGTH_SHORT).show()
        })
    }
}

// =====================================================================
// Cull Mode UI
// =====================================================================

/**
 * Wraps a regular bird row with a leading checkbox when Cull Mode is on,
 * plus an inline warning when culling this bird (a male) would leave any
 * female with zero safe mates. Tapping the body opens the bird detail
 * dialog as usual; tapping the checkbox toggles selection.
 */
@Composable
private fun CullableBirdRow(
    bird: Bird,
    lineageLabel: String?,
    cullMode: Boolean,
    selected: Boolean,
    orphanedFemaleIds: List<Int>,
    onToggleSelect: () -> Unit,
    onOpen: () -> Unit,
) {
    if (!cullMode) {
        BirdCard(
            bird,
            lineageLabel,
            modifier = Modifier.testTag("flock_bird_row_${bird.id}"),
            onClick = onOpen,
        )
        return
    }
    Column(Modifier.testTag("flock_bird_row_${bird.id}")) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(
                checked = selected,
                onCheckedChange = { onToggleSelect() },
                colors = CheckboxDefaults.colors(checkedColor = AlertRed),
                modifier = Modifier.testTag("flock_cull_check_${bird.id}"),
            )
            Box(Modifier.weight(1f)) {
                BirdCard(bird, lineageLabel, onClick = onOpen)
            }
        }
        if (orphanedFemaleIds.isNotEmpty()) {
            Text(
                "⚠️ Last safe mate for ${orphanedFemaleIds.size} female${if (orphanedFemaleIds.size != 1) "s" else ""} (${orphanedFemaleIds.joinToString(", ") { "#$it" }})",
                style = MaterialTheme.typography.labelSmall,
                color = AlertRed,
                modifier = Modifier.padding(start = 48.dp, end = 8.dp, bottom = 4.dp),
            )
        }
    }
}

/**
 * Sticky bottom bar shown while Cull Mode is active. Surfaces the
 * guardrail:
 *  - "X selected · Y males will remain · Z females in flock"
 *  - Color zone: green (above min), yellow (at min), red (below min)
 *  - "Cull Selected" is disabled in the red zone — the user can still
 *    deselect to recover. The line is recomputed locally from the stats
 *    settings so deselecting a female lowers the requirement live.
 */
@Composable
private fun CullModeBottomBar(
    stats: FlockBreedingStats?,
    allBirds: List<Bird>,
    selectedIds: List<Int>,
    onClear: () -> Unit,
    onConfirm: () -> Unit,
) {
    val activeBirdMap = remember(allBirds) {
        allBirds.filter { it.status?.lowercase() == "active" }.associateBy { it.id }
    }
    val selectedMales = selectedIds.count { activeBirdMap[it]?.sex?.lowercase() == "male" }
    val selectedFemales = selectedIds.count { activeBirdMap[it]?.sex?.lowercase() == "female" }

    val totalMales = stats?.totalMales ?: 0
    val totalFemales = stats?.totalFemales ?: 0
    val malesAfter = (totalMales - selectedMales).coerceAtLeast(0)
    val femalesAfter = (totalFemales - selectedFemales).coerceAtLeast(0)
    // Recompute live from the settings echoed back in stats. Falls back to
    // the server's pre-selection figure if stats hasn't loaded yet.
    val minAfter: Int = if (stats == null) 0 else if (femalesAfter == 0) 0 else {
        val maxPer = stats.maxFemalesPerMale.coerceAtLeast(1)
        val groups = (femalesAfter + maxPer - 1) / maxPer
        groups * stats.desiredMalesPerGroup
    }

    val zone = when {
        stats == null || selectedIds.isEmpty() -> Zone.NEUTRAL
        malesAfter < minAfter -> Zone.RED
        malesAfter == minAfter -> Zone.YELLOW
        else -> Zone.GREEN
    }
    val zoneColor = when (zone) {
        Zone.GREEN -> AlertGreen
        Zone.YELLOW -> AlertYellow
        Zone.RED -> AlertRed
        Zone.NEUTRAL -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    val zoneIcon = when (zone) {
        Zone.GREEN -> "✅"   // ✅
        Zone.YELLOW -> "⚠️" // ⚠️
        Zone.RED -> "🛑" // 🛑
        Zone.NEUTRAL -> ""
    }

    Card(
        Modifier.fillMaxWidth().padding(12.dp).testTag("flock_cull_bottom_bar"),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(6.dp),
    ) {
        Column(Modifier.padding(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (zoneIcon.isNotEmpty()) {
                    Text(zoneIcon, fontSize = 18.sp)
                    Spacer(Modifier.width(6.dp))
                }
                Text(
                    "${selectedIds.size} selected",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = zoneColor,
                )
            }
            Spacer(Modifier.height(2.dp))
            Text(
                "$malesAfter male${if (malesAfter != 1) "s" else ""} will remain · $femalesAfter female${if (femalesAfter != 1) "s" else ""} in flock",
                style = MaterialTheme.typography.bodyMedium,
            )
            if (stats != null) {
                Text(
                    "Minimum males needed: $minAfter",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(8.dp))
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedButton(
                    onClick = onClear,
                    modifier = Modifier.weight(1f),
                ) { Text("Clear") }
                Button(
                    onClick = onConfirm,
                    enabled = selectedIds.isNotEmpty() && zone != Zone.RED,
                    colors = ButtonDefaults.buttonColors(containerColor = AlertRed),
                    modifier = Modifier.weight(1f).testTag("flock_cull_confirm"),
                ) {
                    Icon(Icons.Default.Delete, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(4.dp))
                    Text("Cull Selected")
                }
            }
            if (zone == Zone.RED) {
                Spacer(Modifier.height(4.dp))
                Text(
                    "Below the minimum-males line — deselect to enable.",
                    style = MaterialTheme.typography.labelSmall,
                    color = AlertRed,
                )
            }
        }
    }
}

private enum class Zone { NEUTRAL, GREEN, YELLOW, RED }

@Composable
private fun FlockCullConfirmDialog(
    count: Int,
    onConfirm: (method: String, notes: String) -> Unit,
    onDismiss: () -> Unit,
) {
    var method by remember { mutableStateOf("Butchered") }
    var notes by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Cull $count bird${if (count != 1) "s" else ""}?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    "This will update their status. This action cannot be easily undone.",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf("Butchered", "Culled").forEach { m ->
                        OutlinedButton(
                            onClick = { method = m },
                            colors = if (method == m)
                                ButtonDefaults.outlinedButtonColors(
                                    containerColor = SageGreen.copy(alpha = 0.12f),
                                    contentColor = SageGreen,
                                )
                            else ButtonDefaults.outlinedButtonColors(),
                        ) { Text(m) }
                    }
                }
                OutlinedTextField(
                    value = notes,
                    onValueChange = { notes = it },
                    label = { Text("Notes (optional)") },
                    modifier = Modifier.fillMaxWidth(),
                    maxLines = 2,
                )
            }
        },
        confirmButton = {
            Button(
                onClick = { onConfirm(method, notes) },
                colors = ButtonDefaults.buttonColors(containerColor = AlertRed),
            ) { Text("Confirm Cull") }
        },
        dismissButton = { OutlinedButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Filter Chips
// =====================================================================

@Composable
fun FlockFilterChips(lineages: List<Lineage>, selectedFilter: FlockFilter, onFilterSelected: (FlockFilter) -> Unit) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val chipColors = FilterChipDefaults.filterChipColors(selectedContainerColor = SageGreen, selectedLabelColor = Color.White)
        FilterChip(selectedFilter is FlockFilter.Active, { onFilterSelected(FlockFilter.Active) }, { Text("Active") }, colors = chipColors, modifier = Modifier.testTag("flock_filter_active"))
        FilterChip(selectedFilter is FlockFilter.All, { onFilterSelected(FlockFilter.All) }, { Text("All") }, colors = chipColors, modifier = Modifier.testTag("flock_filter_all"))
        FilterChip(selectedFilter is FlockFilter.Records, { onFilterSelected(FlockFilter.Records) }, { Text("Records") }, colors = chipColors)
        FilterChip(selectedFilter is FlockFilter.Males, { onFilterSelected(FlockFilter.Males) }, { Text("Males") }, colors = chipColors, modifier = Modifier.testTag("flock_filter_males"))
        FilterChip(selectedFilter is FlockFilter.Females, { onFilterSelected(FlockFilter.Females) }, { Text("Females") }, colors = chipColors, modifier = Modifier.testTag("flock_filter_females"))
        lineages.forEach { bl ->
            FilterChip(
                selectedFilter is FlockFilter.ByLineage && selectedFilter.lineageId == bl.id,
                { onFilterSelected(FlockFilter.ByLineage(bl.id, bl.name)) },
                { Text(bl.name) }, colors = chipColors,
            )
        }
    }
}

// =====================================================================
// Bird Card — with photo thumbnail
// =====================================================================

@Composable
fun BirdCard(bird: Bird, lineageName: String?, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val photo = rememberBirdPhoto(bird.id)

    Card(
        modifier = modifier.fillMaxWidth().clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            // Photo or band color circle
            Box(Modifier.size(42.dp).clip(CircleShape), contentAlignment = Alignment.Center) {
                if (photo != null) {
                    Image(
                        bitmap = photo.asImageBitmap(),
                        contentDescription = "Bird photo",
                        modifier = Modifier.fillMaxSize().clip(CircleShape),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Box(
                        Modifier.fillMaxSize().background(parseBandColor(bird.bandColor)),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(bird.id.toString(), style = MaterialTheme.typography.labelLarge, color = Color.White, fontWeight = FontWeight.Bold)
                    }
                }
            }

            Spacer(Modifier.width(12.dp))

            Column(Modifier.weight(1f)) {
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                    Text(bird.bandId ?: "Bird #${bird.id}", style = MaterialTheme.typography.titleMedium)
                    StatusBadge(bird.status)
                }
                Spacer(Modifier.height(4.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(formatSex(bird.sex), style = MaterialTheme.typography.bodyMedium)
                    if (lineageName != null) Text(lineageName, style = MaterialTheme.typography.bodyMedium, color = SageGreen)
                }
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                    if (bird.hatchDate != null) Text("Hatched ${bird.hatchDate}", style = MaterialTheme.typography.bodyMedium)
                    if (bird.latestWeight != null) Text("%.0fg".format(bird.latestWeight), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, color = SageGreen)
                }
            }
        }
    }
}

@Composable
fun StatusBadge(status: String?) {
    val displayStatus = status?.replaceFirstChar { it.uppercase() } ?: "Unknown"
    val bgColor = when (status?.lowercase()) { "active" -> SageGreenLight; "culled", "deceased" -> Color(0xFFE0B0B0); else -> MaterialTheme.colorScheme.surfaceVariant }
    val textColor = when (status?.lowercase()) { "active" -> Color(0xFF2D4A1E); "culled", "deceased" -> Color(0xFF6B2D2D); else -> MaterialTheme.colorScheme.onSurfaceVariant }
    Text(displayStatus, style = MaterialTheme.typography.labelLarge, color = textColor, modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(bgColor).padding(horizontal = 8.dp, vertical = 2.dp))
}

// =====================================================================
// Bird Detail Dialog — with profile photo and Take Photo button
// =====================================================================

@Composable
fun BirdDetailDialog(
    bird: Bird, lineageName: String?, viewModel: FlockViewModel,
    onDismiss: () -> Unit, onStatusChanged: () -> Unit = {}, onDeleted: () -> Unit = {},
) {
    val context = LocalContext.current
    var weights by remember { mutableStateOf<List<BirdWeight>>(emptyList()) }
    var weightsLoaded by remember { mutableStateOf(false) }
    var photoRefreshKey by remember { mutableIntStateOf(0) }
    val photo = rememberBirdPhoto(bird.id, photoRefreshKey)
    var showStatusConfirm by remember { mutableStateOf<String?>(null) }
    var showDeleteConfirm by remember { mutableStateOf(false) }
    var statusNotes by remember { mutableStateOf("") }
    var showLogWeight by remember { mutableStateOf(false) }
    var showEditBird by remember { mutableStateOf(false) }
    var weightRefreshKey by remember { mutableStateOf(0) }

    val photoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.TakePicturePreview(),
    ) { bitmap ->
        if (bitmap != null) {
            viewModel.saveBirdPhotoBitmap(bird.id, bitmap, context)
            photoRefreshKey++ // trigger recomposition to show new photo
        }
    }

    androidx.compose.runtime.LaunchedEffect(bird.id, weightRefreshKey) {
        weights = viewModel.getBirdWeights(bird.id)
        weightsLoaded = true
    }

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Card(
            Modifier.fillMaxWidth().padding(16.dp),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            LazyColumn(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                // Photo + header
                item {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                        // Action bar
                        Row(Modifier.fillMaxWidth(), Arrangement.End) {
                            IconButton(onClick = { showEditBird = true }) { Icon(Icons.Default.Edit, "Edit", tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                            IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, "Close") }
                        }

                        // Profile photo or placeholder
                        Box(
                            Modifier.size(100.dp).clip(CircleShape),
                            contentAlignment = Alignment.Center,
                        ) {
                            if (photo != null) {
                                Image(
                                    bitmap = photo.asImageBitmap(),
                                    contentDescription = "Bird photo",
                                    modifier = Modifier.fillMaxSize().clip(CircleShape),
                                    contentScale = ContentScale.Crop,
                                )
                            } else {
                                Box(
                                    Modifier.fillMaxSize().background(parseBandColor(bird.bandColor)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Text("\uD83D\uDC25", fontSize = 32.sp)
                                }
                            }
                        }

                        Spacer(Modifier.height(8.dp))
                        Text(bird.bandId ?: "Bird #${bird.id}", style = MaterialTheme.typography.headlineMedium)
                        if (lineageName != null) Text(lineageName, style = MaterialTheme.typography.titleMedium, color = SageGreen)
                        Spacer(Modifier.height(4.dp))
                        StatusBadge(bird.status)

                        Spacer(Modifier.height(8.dp))
                        Button(
                            onClick = { photoLauncher.launch(null) },
                            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                        ) {
                            Icon(Icons.Default.CameraAlt, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(if (photo != null) "Update Photo" else "Take Photo")
                        }
                    }
                }

                item { HorizontalDivider() }

                // Details
                item { DetailRow("Sex", formatSex(bird.sex)) }
                if (bird.species != null) { item { DetailRow("Species", bird.species) } }
                if (bird.hatchDate != null) { item { DetailRow("Hatch Date", bird.hatchDate) } }
                if (bird.bandColor != null) { item { DetailRow("Band Color", bird.bandColor) } }
                if (bird.sireId != null) { item { DetailRow("Sire", "Bird #${bird.sireId}") } }
                if (bird.damId != null) { item { DetailRow("Dam", "Bird #${bird.damId}") } }
                if (bird.brooderId != null) { item { DetailRow("Brooder", "#${bird.brooderId}") } }

                if (bird.notes != null) {
                    item {
                        Spacer(Modifier.height(4.dp))
                        Text("Notes", style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(4.dp))
                        Text(bird.notes, style = MaterialTheme.typography.bodyMedium)
                    }
                }

                // Weight section
                item {
                    Spacer(Modifier.height(8.dp))
                    HorizontalDivider()
                    Spacer(Modifier.height(8.dp))
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                        Text("Weight History", style = MaterialTheme.typography.titleMedium)
                        Button(onClick = { showLogWeight = true }, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) {
                            Text("Log Weight")
                        }
                    }
                }
                if (!weightsLoaded) {
                    item { Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = SageGreen, modifier = Modifier.size(24.dp), strokeWidth = 2.dp) } }
                } else if (weights.isEmpty()) {
                    item { Text("No weight records yet", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                } else {
                    // Weight chart (2+ entries)
                    if (weights.size >= 2) {
                        item {
                            val sageGreenColor = SageGreen
                            Canvas(Modifier.fillMaxWidth().height(120.dp).clip(RoundedCornerShape(8.dp)).background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.3f))) {
                                val sorted = weights.sortedBy { it.date }
                                val minW = sorted.minOf { it.weightGrams }.toFloat()
                                val maxW = sorted.maxOf { it.weightGrams }.toFloat()
                                val rangeW = (maxW - minW).coerceAtLeast(1f)
                                val padding = 16f
                                val w = size.width - padding * 2
                                val h = size.height - padding * 2

                                // Draw lines
                                for (i in 0 until sorted.size - 1) {
                                    val x1 = padding + (i.toFloat() / (sorted.size - 1)) * w
                                    val y1 = padding + h - ((sorted[i].weightGrams.toFloat() - minW) / rangeW) * h
                                    val x2 = padding + ((i + 1).toFloat() / (sorted.size - 1)) * w
                                    val y2 = padding + h - ((sorted[i + 1].weightGrams.toFloat() - minW) / rangeW) * h
                                    drawLine(sageGreenColor, Offset(x1, y1), Offset(x2, y2), strokeWidth = 3f, cap = StrokeCap.Round)
                                }
                                // Draw dots
                                sorted.forEachIndexed { i, entry ->
                                    val x = padding + (i.toFloat() / (sorted.size - 1)) * w
                                    val y = padding + h - ((entry.weightGrams.toFloat() - minW) / rangeW) * h
                                    drawCircle(sageGreenColor, radius = 5f, center = Offset(x, y))
                                }
                            }
                            Spacer(Modifier.height(4.dp))
                            // Stats
                            val sorted = weights.sortedBy { it.date }
                            val latest = sorted.last().weightGrams
                            val first = sorted.first().weightGrams
                            val days = sorted.size.coerceAtLeast(2) - 1
                            val adg = if (days > 0) (latest - first) / days else 0.0
                            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                    Text("%.0fg".format(latest), fontWeight = FontWeight.Bold); Text("Current", style = MaterialTheme.typography.labelSmall)
                                }
                                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                    Text("%.1fg/d".format(adg), fontWeight = FontWeight.Bold); Text("Avg gain", style = MaterialTheme.typography.labelSmall)
                                }
                                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                                    Text("${weights.size}", fontWeight = FontWeight.Bold); Text("Records", style = MaterialTheme.typography.labelSmall)
                                }
                            }
                        }
                    } else {
                        item { Text("Log more weights to see growth chart", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                    }
                    // Weight list with delete
                    items(weights.sortedByDescending { it.date }) { w ->
                        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Text(w.date ?: "—", style = MaterialTheme.typography.bodyMedium)
                                    Text("%.1f g".format(w.weightGrams), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                                }
                                if (w.notes != null) Text(w.notes, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            w.id?.let { wid ->
                                IconButton(onClick = {
                                    viewModel.deleteWeight(bird.id, wid) { ok ->
                                        if (ok) { weightRefreshKey++; Toast.makeText(context, "Weight deleted", Toast.LENGTH_SHORT).show() }
                                    }
                                }, modifier = Modifier.size(28.dp)) {
                                    Icon(Icons.Default.Delete, "Delete", Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                        }
                    }
                }

                // --- Status Actions ---
                item {
                    Spacer(Modifier.height(8.dp))
                    HorizontalDivider()
                    Spacer(Modifier.height(8.dp))
                    Text("Actions", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(8.dp))
                }

                if (bird.status?.lowercase() == "active") {
                    item {
                        Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = { showStatusConfirm = "Culled" }, Modifier.weight(1f)) { Text("Culled") }
                            OutlinedButton(onClick = { showStatusConfirm = "Deceased" }, Modifier.weight(1f)) { Text("Deceased") }
                        }
                    }
                    item {
                        Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = { showStatusConfirm = "Sold" }, Modifier.weight(1f)) { Text("Sold") }
                            OutlinedButton(onClick = { showDeleteConfirm = true }, Modifier.weight(1f),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = Color(0xFFCC4444))) {
                                Icon(Icons.Default.Delete, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Delete")
                            }
                        }
                    }
                } else {
                    // Non-active bird: show reactivate + delete
                    item {
                        Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                            Button(onClick = { showStatusConfirm = "Active" }, Modifier.weight(1f),
                                colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text("Reactivate") }
                            OutlinedButton(onClick = { showDeleteConfirm = true }, Modifier.weight(1f),
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = Color(0xFFCC4444))) {
                                Icon(Icons.Default.Delete, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Delete")
                            }
                        }
                    }
                }
            }
        }
    }

    // Status change confirmation
    if (showStatusConfirm != null) {
        val newStatus = showStatusConfirm!!
        AlertDialog(
            onDismissRequest = { showStatusConfirm = null },
            title = { Text("Mark as $newStatus?") },
            text = {
                Column {
                    Text("Change Bird #${bird.id} status to $newStatus?")
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(value = statusNotes, onValueChange = { statusNotes = it }, label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth())
                }
            },
            confirmButton = {
                Button(onClick = {
                    val s = newStatus; val n = statusNotes.ifBlank { null }
                    showStatusConfirm = null; statusNotes = ""
                    viewModel.updateBirdStatus(bird.id, s, n) { ok ->
                        if (ok) { Toast.makeText(context, "Bird #${bird.id} marked as $s", Toast.LENGTH_SHORT).show(); onStatusChanged() }
                        else Toast.makeText(context, "Failed to update status", Toast.LENGTH_SHORT).show()
                    }
                }, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text("Confirm") }
            },
            dismissButton = { TextButton(onClick = { showStatusConfirm = null }) { Text("Cancel") } },
        )
    }

    // Delete confirmation
    if (showDeleteConfirm) {
        AlertDialog(
            onDismissRequest = { showDeleteConfirm = false },
            title = { Text("Delete Bird #${bird.id}?") },
            text = { Text("Permanently delete this bird? This cannot be undone. Use status changes (Culled/Deceased/Sold) to keep records instead.") },
            confirmButton = {
                Button(onClick = {
                    showDeleteConfirm = false
                    viewModel.deleteBirdById(bird.id) { ok ->
                        if (ok) { Toast.makeText(context, "Bird deleted", Toast.LENGTH_SHORT).show(); onDeleted() }
                        else Toast.makeText(context, "Delete failed", Toast.LENGTH_SHORT).show()
                    }
                }, colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFCC4444))) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { showDeleteConfirm = false }) { Text("Cancel") } },
        )
    }

    // Log Weight dialog
    if (showLogWeight) {
        var weightText by remember { mutableStateOf("") }
        var weightNotes by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { showLogWeight = false },
            title = { Text("Log Weight") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("Bird #${bird.id}", style = MaterialTheme.typography.bodyMedium)
                    OutlinedTextField(value = weightText, onValueChange = { weightText = it.filter { c -> c.isDigit() || c == '.' } },
                        label = { Text("Weight (grams)") }, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal), modifier = Modifier.fillMaxWidth())
                    OutlinedTextField(value = weightNotes, onValueChange = { weightNotes = it }, label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth())
                }
            },
            confirmButton = {
                Button(onClick = {
                    val grams = weightText.toDoubleOrNull() ?: return@Button
                    showLogWeight = false
                    viewModel.logWeight(bird.id, grams, weightNotes.ifBlank { null }) { ok ->
                        if (ok) { weightRefreshKey++; Toast.makeText(context, "Weight logged: ${weightText}g", Toast.LENGTH_SHORT).show() }
                        else Toast.makeText(context, "Failed to log weight", Toast.LENGTH_SHORT).show()
                    }
                }, enabled = (weightText.toDoubleOrNull() ?: 0.0) > 0, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text("Save") }
            },
            dismissButton = { TextButton(onClick = { showLogWeight = false }) { Text("Cancel") } },
        )
    }

    // Edit Bird dialog
    if (showEditBird) {
        EditBirdDialog(bird, viewModel, onDismiss = { showEditBird = false }, onSuccess = {
            showEditBird = false
            Toast.makeText(context, "Bird updated", Toast.LENGTH_SHORT).show()
            onStatusChanged() // triggers refresh
        })
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditBirdDialog(bird: Bird, viewModel: FlockViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    var sex by remember { mutableStateOf(bird.sex?.replaceFirstChar { it.uppercase() } ?: "Unknown") }
    var bandColor by remember { mutableStateOf(bird.bandColor ?: "") }
    var hatchDate by remember { mutableStateOf(bird.hatchDate ?: "") }
    var notes by remember { mutableStateOf(bird.notes ?: "") }
    var status by remember { mutableStateOf(bird.status?.replaceFirstChar { it.uppercase() } ?: "Active") }
    var sexExpanded by remember { mutableStateOf(false) }
    var statusExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Edit Bird #${bird.id}") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                ExposedDropdownMenuBox(sexExpanded, { sexExpanded = it }) {
                    OutlinedTextField(value = sex, onValueChange = {}, readOnly = true, label = { Text("Sex") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(sexExpanded) }, modifier = Modifier.menuAnchor().fillMaxWidth())
                    ExposedDropdownMenu(sexExpanded, { sexExpanded = false }) {
                        listOf("Male", "Female", "Unknown").forEach { s -> DropdownMenuItem(text = { Text(s) }, onClick = { sex = s; sexExpanded = false }) }
                    }
                }
                com.quailsync.app.ui.components.BandColorPicker(
                    value = bandColor,
                    onValueChange = { bandColor = it },
                    label = "Band color",
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(value = hatchDate, onValueChange = { hatchDate = it }, label = { Text("Hatch date (YYYY-MM-DD)") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
                ExposedDropdownMenuBox(statusExpanded, { statusExpanded = it }) {
                    OutlinedTextField(value = status, onValueChange = {}, readOnly = true, label = { Text("Status") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(statusExpanded) }, modifier = Modifier.menuAnchor().fillMaxWidth())
                    ExposedDropdownMenu(statusExpanded, { statusExpanded = false }) {
                        listOf("Active", "Culled", "Deceased", "Sold").forEach { s -> DropdownMenuItem(text = { Text(s) }, onClick = { status = s; statusExpanded = false }) }
                    }
                }
                OutlinedTextField(value = notes, onValueChange = { notes = it }, label = { Text("Notes") }, modifier = Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            Button(onClick = {
                saving = true
                viewModel.updateBird(bird.id, com.quailsync.app.data.UpdateBirdRequest(
                    status = status,
                    notes = notes.ifBlank { null },
                    bandColor = bandColor.ifBlank { null },
                    sex = sex.ifBlank { null },
                    hatchDate = hatchDate.ifBlank { null },
                )) { ok -> saving = false; if (ok) onSuccess() }
            }, enabled = !saving, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text(if (saving) "Saving..." else "Save") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
fun DetailRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
    }
}

// =====================================================================
// Add Bird Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddBirdDialog(lineages: List<Lineage>, viewModel: FlockViewModel, onDismiss: () -> Unit, onSuccess: (Bird) -> Unit) {
    var selectedLineageId by remember { mutableStateOf<Int?>(null) }
    var sex by remember { mutableStateOf("Unknown") }
    var bandColor by remember { mutableStateOf("") }
    var notes by remember { mutableStateOf("") }
    var blExpanded by remember { mutableStateOf(false) }
    var sexExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Bird") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                ExposedDropdownMenuBox(blExpanded, { blExpanded = it }) {
                    OutlinedTextField(
                        value = selectedLineageId?.let { id -> lineages.find { it.id == id }?.name ?: "" } ?: "",
                        onValueChange = {}, readOnly = true, label = { Text("Lineage") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(blExpanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(blExpanded, { blExpanded = false }) {
                        lineages.forEach { bl -> DropdownMenuItem(text = { Text(bl.name) }, onClick = { selectedLineageId = bl.id; blExpanded = false }) }
                    }
                }
                ExposedDropdownMenuBox(sexExpanded, { sexExpanded = it }) {
                    OutlinedTextField(
                        value = sex, onValueChange = {}, readOnly = true, label = { Text("Sex") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(sexExpanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(sexExpanded, { sexExpanded = false }) {
                        listOf("Male", "Female", "Unknown").forEach { s ->
                            DropdownMenuItem(text = { Text(s) }, onClick = { sex = s; sexExpanded = false })
                        }
                    }
                }
                com.quailsync.app.ui.components.BandColorPicker(
                    value = bandColor,
                    onValueChange = { bandColor = it },
                    label = "Band color (optional)",
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(value = notes, onValueChange = { notes = it }, label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    val blId = selectedLineageId ?: return@Button
                    saving = true
                    viewModel.createBird(
                        com.quailsync.app.data.CreateBirdRequest(
                            lineageIds = listOf(blId.toLong()), sex = sex, status = "Active",
                            hatchDate = java.time.LocalDate.now().format(java.time.format.DateTimeFormatter.ISO_LOCAL_DATE),
                            generation = 1,
                            bandColor = bandColor.ifBlank { null },
                            notes = notes.ifBlank { null },
                        )
                    ) { bird ->
                        saving = false
                        if (bird != null) onSuccess(bird)
                    }
                },
                enabled = selectedLineageId != null && !saving,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Creating..." else "Create Bird") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
