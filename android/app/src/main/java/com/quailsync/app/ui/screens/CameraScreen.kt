@file:Suppress("ASSIGNED_BUT_NEVER_ACCESSED_VARIABLE", "UNUSED_VALUE")

package com.quailsync.app.ui.screens

import android.content.Intent
import androidx.core.net.toUri
import android.util.Log
import androidx.compose.foundation.Image
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.List
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material3.AlertDialog
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.TabRowDefaults.SecondaryIndicator
import androidx.compose.material3.TabRowDefaults.tabIndicatorOffset
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.Camera
import com.quailsync.app.data.CreateCameraRequest
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.UpdateBrooderRequest
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ScrollableTabRow
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.painter.ColorPainter
import coil.compose.AsyncImage
import com.quailsync.app.data.TrailcamCamera
import com.quailsync.app.data.TrailcamLatest
import com.quailsync.app.data.AssignIndoorCameraRequest
import com.quailsync.app.data.IndoorCamera
import com.quailsync.app.data.IndoorcamLatest
import java.time.LocalDateTime
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import kotlin.math.roundToInt

// =====================================================================
// ViewModel
// =====================================================================

class CameraViewModel(application: Application) : AndroidViewModel(application) {
    private val serverUrl = ServerConfig.getServerUrl(application)
    private val api = QuailSyncApi.create(serverUrl)

    // Outdoor (SPYPOINT) cameras, discovered from the server's observations log.
    private val _outdoorCameras = MutableStateFlow<List<TrailcamCamera>>(emptyList())
    val outdoorCameras: StateFlow<List<TrailcamCamera>> = _outdoorCameras.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    init { loadAll() }

    fun refresh() {
        viewModelScope.launch {
            _isRefreshing.value = true
            loadAllSuspend()
            _isRefreshing.value = false
        }
    }

    private fun loadAll() { viewModelScope.launch { loadAllSuspend() } }

    private suspend fun loadAllSuspend() {
        // Outdoor cameras come from the server's trail-cam observations log.
        // Best-effort: on failure or empty, no outdoor cameras are shown.
        _outdoorCameras.value = try {
            api.getTrailcamCameras()
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load outdoor cameras", e)
            emptyList()
        }

        _isLoading.value = false
    }
}

// =====================================================================
// Camera Screen
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CameraScreen(viewModel: CameraViewModel = viewModel()) {
    val isLoading by viewModel.isLoading.collectAsState()
    val outdoorCameras by viewModel.outdoorCameras.collectAsState()
    var selectedTab by remember { mutableIntStateOf(0) }

    // Two tabs: outdoor (SPYPOINT trail) cameras and the indoor RTSP
    // chick-counter camera(s). The retired Brooder-1 hutch MJPEG stream tab was
    // removed when that physical camera was decommissioned.
    val tabTitles = listOf("Outdoor Cams", "Indoor Cams")

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            Arrangement.SpaceBetween, Alignment.CenterVertically,
        ) {
            Text("Cameras", style = MaterialTheme.typography.headlineMedium)
        }

        ScrollableTabRow(
            selectedTabIndex = selectedTab,
            containerColor = MaterialTheme.colorScheme.surface,
            edgePadding = 12.dp,
            indicator = { tabPositions ->
                if (selectedTab < tabPositions.size) {
                    SecondaryIndicator(Modifier.tabIndicatorOffset(tabPositions[selectedTab]), color = SageGreen)
                }
            },
        ) {
            tabTitles.forEachIndexed { i, title ->
                Tab(selectedTab == i, { selectedTab = i }) { Text(title, Modifier.padding(12.dp)) }
            }
        }

        when (selectedTab) {
            0 -> {
                // --- Outdoor Cams: vertical scrollable list, one card per camera ---
                OutdoorCamsList(
                    cameras = outdoorCameras,
                    isLoadingCameras = isLoading,
                    onRefresh = { viewModel.refresh() },
                )
            }
            else -> {
                // --- Indoor Cams: chick count + latest saved image + assignment ---
                IndoorCamsList(onRefresh = { viewModel.refresh() })
            }
        }
    }
}

// =====================================================================
// Outdoor Cams — scrollable list of every outdoor camera, one card each
// =====================================================================

private sealed interface OutdoorState {
    data object Loading : OutdoorState
    data object Empty : OutdoorState
    data class Error(val message: String) : OutdoorState
    data class Data(val latest: TrailcamLatest) : OutdoorState
}

/** The "Outdoor Cams" tab: a pull-to-refresh vertical list with one
 *  [OutdoorCamCard] per camera. New cameras in [cameras] appear automatically. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OutdoorCamsList(
    cameras: List<TrailcamCamera>,
    isLoadingCameras: Boolean,
    onRefresh: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var isRefreshing by remember { mutableStateOf(false) }
    // Bumped on pull-to-refresh so every card re-fetches its latest capture.
    var refreshKey by remember { mutableIntStateOf(0) }

    PullToRefreshBox(
        isRefreshing = isRefreshing,
        onRefresh = {
            scope.launch {
                isRefreshing = true
                onRefresh()
                refreshKey++
                isRefreshing = false
            }
        },
        modifier = Modifier.fillMaxSize(),
    ) {
        when {
            cameras.isEmpty() && isLoadingCameras -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(color = SageGreen)
                }
            }
            cameras.isEmpty() -> {
                // Empty state — still scrollable so the pull-to-refresh works.
                Column(
                    Modifier
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Spacer(Modifier.height(64.dp))
                    Icon(Icons.Default.PhotoCamera, null, Modifier.size(56.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(12.dp))
                    Text(
                        "No outdoor cameras yet.\nPull down to refresh.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
            }
            else -> {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(16.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    items(cameras, key = { it.cameraId }) { cam ->
                        OutdoorCamCard(cameraId = cam.cameraId, label = cam.label, refreshKey = refreshKey)
                    }
                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }
}

/** One outdoor camera: header label + its latest annotated capture, bird-count
 *  badge, timestamp, and average confidence. Fetches its own latest observation;
 *  re-fetches when [refreshKey] changes. */
@Composable
fun OutdoorCamCard(cameraId: String, label: String, refreshKey: Int) {
    val context = LocalContext.current
    val baseUrl = remember { ServerConfig.getServerUrl(context).trimEnd('/') }
    val api = remember { QuailSyncApi.create(ServerConfig.getServerUrl(context)) }

    var state by remember(cameraId) { mutableStateOf<OutdoorState>(OutdoorState.Loading) }
    var showHistory by remember(cameraId) { mutableStateOf(false) }

    LaunchedEffect(cameraId, refreshKey) {
        state = OutdoorState.Loading
        state = try {
            val latest = withContext(Dispatchers.IO) { api.getTrailcamLatest(cameraId) }
            OutdoorState.Data(latest)
        } catch (e: retrofit2.HttpException) {
            // 404 = nothing captured for this camera yet (vs. a real error).
            if (e.code() == 404) OutdoorState.Empty else OutdoorState.Error("Server error (HTTP ${e.code()})")
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load outdoor cam $cameraId", e)
            OutdoorState.Error(e.message ?: "Couldn't reach the server")
        }
    }

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(label, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(12.dp))
            when (val s = state) {
                is OutdoorState.Loading -> OutdoorCardMessage {
                    CircularProgressIndicator(color = SageGreen, modifier = Modifier.size(32.dp), strokeWidth = 2.dp)
                }
                is OutdoorState.Empty -> OutdoorCardMessage {
                    Icon(Icons.Default.PhotoCamera, null, Modifier.size(48.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "No captures yet.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
                is OutdoorState.Error -> OutdoorCardMessage {
                    Icon(Icons.Default.VideocamOff, null, Modifier.size(48.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        s.message,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
                is OutdoorState.Data -> OutdoorCamCardContent(s.latest, baseUrl, label)
            }

            Spacer(Modifier.height(12.dp))
            OutlinedButton(
                onClick = { showHistory = true },
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = SageGreen),
            ) {
                Icon(Icons.Default.List, null, Modifier.size(18.dp))
                Spacer(Modifier.size(6.dp))
                Text("View History")
            }
        }
    }

    if (showHistory) {
        OutdoorCamHistoryDialog(cameraId = cameraId, label = label) { showHistory = false }
    }
}

/** Centered single-message column (loading / empty / error) inside a card. */
@Composable
private fun OutdoorCardMessage(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxWidth().padding(vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        content = content,
    )
}

@Composable
private fun OutdoorCamCardContent(latest: TrailcamLatest, baseUrl: String, label: String) {
    fun absolute(url: String?): String? = url?.let { if (it.startsWith("http")) it else "$baseUrl$it" }
    // Prefer the annotated image (bounding boxes); fall back to the raw capture.
    val imageUrl = absolute(latest.annotatedImageUrl) ?: absolute(latest.imageUrl)
    val count = latest.birdCount ?: 0
    val placeholder = ColorPainter(MaterialTheme.colorScheme.surfaceVariant)

    Column(Modifier.fillMaxWidth()) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(4f / 3f)
                .clip(RoundedCornerShape(12.dp))
                .background(Color.Black),
        ) {
            if (imageUrl != null) {
                AsyncImage(
                    model = imageUrl,
                    contentDescription = "$label latest capture",
                    modifier = Modifier.fillMaxSize(),
                    contentScale = ContentScale.Crop,
                    placeholder = placeholder,
                    error = placeholder,
                )
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Icon(Icons.Default.PhotoCamera, null, Modifier.size(48.dp), tint = Color.White.copy(alpha = 0.6f))
                }
            }

            // Bird-count badge, top-left.
            Box(
                Modifier
                    .align(Alignment.TopStart)
                    .padding(10.dp)
                    .background(SageGreen.copy(alpha = 0.92f), RoundedCornerShape(10.dp))
                    .padding(horizontal = 10.dp, vertical = 5.dp),
            ) {
                Text(
                    "$count bird${if (count == 1) "" else "s"} detected",
                    style = MaterialTheme.typography.labelLarge,
                    color = Color.White,
                    fontWeight = FontWeight.SemiBold,
                )
            }
        }

        Spacer(Modifier.height(10.dp))

        // Freshness: how long ago the latest capture was, colored to warn when
        // the pipeline is falling behind (orange >1h, red >4h).
        val freshness = freshnessFor(latest.timestamp)
        if (freshness != null) {
            Text(
                freshness.text,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
                color = freshness.color ?: MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(4.dp))
        }

        Row(
            Modifier.fillMaxWidth(),
            Arrangement.SpaceBetween,
            Alignment.CenterVertically,
        ) {
            Text(
                formatCaptureTime(latest.timestamp),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "Avg confidence: ${formatConfidence(latest.confidenceAvg)}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// =====================================================================
// Freshness indicator — "how long ago" with a staleness warning color
// =====================================================================

/** Orange once an observation is older than this (pipeline lagging). */
private val FRESHNESS_WARN = java.time.Duration.ofHours(1)
/** Red once older than this (pipeline likely stalled). */
private val FRESHNESS_STALE = java.time.Duration.ofHours(4)
private val FreshnessOrange = Color(0xFFE08A2E)
private val FreshnessRed = Color(0xFFCC4444)

/** A relative "x ago" label plus an optional warning color (null = use the
 *  default muted color). */
private data class Freshness(val text: String, val color: Color?)

/** Build a [Freshness] from an ISO-8601 capture timestamp: a relative age
 *  ("2 min ago", "3 hours ago", "2 days ago") plus a color that turns orange
 *  past 1 hour and red past 4 hours. Returns null for an unparseable/blank
 *  timestamp. Timestamps without an offset are treated as UTC (as the API
 *  emits), matching [formatCaptureTime]. */
private fun freshnessFor(iso: String?): Freshness? {
    if (iso.isNullOrBlank()) return null
    val instant = runCatching { OffsetDateTime.parse(iso).toInstant() }
        .recoverCatching { LocalDateTime.parse(iso).toInstant(ZoneOffset.UTC) }
        .getOrNull() ?: return null

    val age = java.time.Duration.between(instant, java.time.Instant.now())
    // Future timestamps (clock skew) read as "just now" rather than negatives.
    val minutes = age.toMinutes().coerceAtLeast(0)
    val text = when {
        minutes < 1 -> "just now"
        minutes < 60 -> "$minutes min ago"
        minutes < 24 * 60 -> {
            val h = minutes / 60
            "$h hour${if (h == 1L) "" else "s"} ago"
        }
        else -> {
            val d = minutes / (24 * 60)
            "$d day${if (d == 1L) "" else "s"} ago"
        }
    }
    val color = when {
        age >= FRESHNESS_STALE -> FreshnessRed
        age >= FRESHNESS_WARN -> FreshnessOrange
        else -> null
    }
    return Freshness(text, color)
}

// =====================================================================
// Photo history browser — last 7 days of observations for one camera
// =====================================================================

private sealed interface HistoryState {
    data object Loading : HistoryState
    data object Empty : HistoryState
    data class Error(val message: String) : HistoryState
    data class Data(val items: List<TrailcamLatest>) : HistoryState
}

/** Full-screen modal listing every observation for [cameraId] over the last
 *  7 days, newest first. Each row lazy-loads its thumbnail with Coil; tapping a
 *  photo opens it full-screen. */
@Composable
fun OutdoorCamHistoryDialog(cameraId: String, label: String, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val baseUrl = remember { ServerConfig.getServerUrl(context).trimEnd('/') }
    val api = remember { QuailSyncApi.create(ServerConfig.getServerUrl(context)) }

    var state by remember(cameraId) { mutableStateOf<HistoryState>(HistoryState.Loading) }
    // The image currently shown full-screen (null = none).
    var fullScreenUrl by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(cameraId) {
        state = try {
            // History comes back oldest-first; reverse for newest-first display.
            val items = withContext(Dispatchers.IO) { api.getTrailcamHistory(cameraId, 168) }
            if (items.isEmpty()) HistoryState.Empty else HistoryState.Data(items.reversed())
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load history for $cameraId", e)
            HistoryState.Error(e.message ?: "Couldn't reach the server")
        }
    }

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            Column(Modifier.fillMaxSize()) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
                    Arrangement.SpaceBetween,
                    Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f).padding(start = 8.dp)) {
                        Text(label, style = MaterialTheme.typography.titleLarge)
                        Text(
                            "Last 7 days",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, "Close") }
                }
                HorizontalDivider()

                when (val s = state) {
                    is HistoryState.Loading -> Box(Modifier.fillMaxSize(), Alignment.Center) {
                        CircularProgressIndicator(color = SageGreen)
                    }
                    is HistoryState.Empty -> Box(Modifier.fillMaxSize(), Alignment.Center) {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Icon(Icons.Default.PhotoCamera, null, Modifier.size(56.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.height(12.dp))
                            Text(
                                "No observations in the last 7 days.",
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                        }
                    }
                    is HistoryState.Error -> Box(Modifier.fillMaxSize(), Alignment.Center) {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Icon(Icons.Default.VideocamOff, null, Modifier.size(56.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.height(12.dp))
                            Text(
                                s.message,
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                        }
                    }
                    is HistoryState.Data -> LazyColumn(
                        Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(16.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        items(s.items) { obs ->
                            HistoryRow(obs, baseUrl) { url -> fullScreenUrl = url }
                        }
                    }
                }
            }
        }
    }

    fullScreenUrl?.let { url ->
        FullScreenImageDialog(url) { fullScreenUrl = null }
    }
}

/** One observation row: lazy-loaded thumbnail (annotated if available, else
 *  raw), a bird-count badge, timestamp, and average confidence. Tapping the
 *  thumbnail invokes [onOpenPhoto] with the chosen image URL. */
@Composable
private fun HistoryRow(obs: TrailcamLatest, baseUrl: String, onOpenPhoto: (String) -> Unit) {
    fun absolute(url: String?): String? = url?.let { if (it.startsWith("http")) it else "$baseUrl$it" }
    val imageUrl = absolute(obs.annotatedImageUrl) ?: absolute(obs.imageUrl)
    val count = obs.birdCount ?: 0
    val placeholder = ColorPainter(MaterialTheme.colorScheme.surfaceVariant)

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(10.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(96.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(Color.Black)
                    .then(if (imageUrl != null) Modifier.clickable { onOpenPhoto(imageUrl) } else Modifier),
            ) {
                if (imageUrl != null) {
                    AsyncImage(
                        model = imageUrl,
                        contentDescription = "Observation thumbnail",
                        modifier = Modifier.fillMaxSize(),
                        contentScale = ContentScale.Crop,
                        placeholder = placeholder,
                        error = placeholder,
                    )
                } else {
                    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                        Icon(Icons.Default.PhotoCamera, null, Modifier.size(28.dp), tint = Color.White.copy(alpha = 0.6f))
                    }
                }
                // Bird-count badge, top-left.
                Box(
                    Modifier
                        .align(Alignment.TopStart)
                        .padding(6.dp)
                        .background(SageGreen.copy(alpha = 0.92f), RoundedCornerShape(8.dp))
                        .padding(horizontal = 6.dp, vertical = 3.dp),
                ) {
                    Text(
                        "$count",
                        style = MaterialTheme.typography.labelMedium,
                        color = Color.White,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }

            Spacer(Modifier.size(12.dp))

            Column(Modifier.weight(1f)) {
                Text(
                    formatCaptureTime(obs.timestamp),
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.Medium,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    "$count bird${if (count == 1) "" else "s"} detected",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    "Avg confidence: ${formatConfidence(obs.confidenceAvg)}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/** Tap-to-dismiss full-screen view of a single observation photo. */
@Composable
private fun FullScreenImageDialog(imageUrl: String, onDismiss: () -> Unit) {
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black)
                .clickable(onClick = onDismiss),
            contentAlignment = Alignment.Center,
        ) {
            AsyncImage(
                model = imageUrl,
                contentDescription = "Observation photo",
                modifier = Modifier.fillMaxWidth(),
                contentScale = ContentScale.Fit,
            )
            IconButton(
                onClick = onDismiss,
                modifier = Modifier.align(Alignment.TopEnd).padding(8.dp),
            ) {
                Icon(Icons.Default.Close, "Close", tint = Color.White)
            }
        }
    }
}

/** Format an ISO-8601 capture timestamp into a friendly "MMM d, h:mm a" in the
 *  device's local timezone. The API returns UTC, so a naive timestamp (no
 *  offset) is treated as UTC before converting. Falls back to the raw string. */
private fun formatCaptureTime(iso: String?): String {
    if (iso.isNullOrBlank()) return "Unknown time"
    val fmt = DateTimeFormatter.ofPattern("MMM d, h:mm a")
    val zone = ZoneId.systemDefault()
    return runCatching {
        // Has an explicit offset / "Z" -> convert that instant to local time.
        OffsetDateTime.parse(iso).atZoneSameInstant(zone).format(fmt)
    }.recoverCatching {
        // Naive timestamp from the API is UTC -> treat as UTC, then convert.
        LocalDateTime.parse(iso).atZone(ZoneOffset.UTC).withZoneSameInstant(zone).format(fmt)
    }.getOrDefault(iso)
}

private fun formatConfidence(value: Double?): String =
    if (value == null) "—" else "${(value * 100).roundToInt()}%"

// =====================================================================
// Indoor Cams — RTSP chick-counter: live count + saved image + assignment
// =====================================================================

private sealed interface IndoorObsState {
    data object Loading : IndoorObsState
    data object Empty : IndoorObsState
    data class Error(val message: String) : IndoorObsState
    data class Data(val latest: IndoorcamLatest) : IndoorObsState
}

/** The "Indoor Cams" tab: a pull-to-refresh list with one [IndoorCamCard] per
 *  registered indoor camera. Loads the registry (for assignment) + the housing
 *  units it may attach to (brooders/incubators only — never hutches). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IndoorCamsList(onRefresh: () -> Unit) {
    val context = LocalContext.current
    val api = remember { QuailSyncApi.create(ServerConfig.getServerUrl(context)) }
    val scope = rememberCoroutineScope()

    var cameras by remember { mutableStateOf<List<IndoorCamera>>(emptyList()) }
    var assignableUnits by remember { mutableStateOf<List<Brooder>>(emptyList()) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var isRefreshing by remember { mutableStateOf(false) }
    // Bumped after any (re)load so each card re-fetches its latest observation.
    var refreshKey by remember { mutableIntStateOf(0) }

    suspend fun load() {
        try {
            val cams = withContext(Dispatchers.IO) { api.getIndoorCameras() }
            val units = withContext(Dispatchers.IO) { api.getBrooders() }
            cameras = cams
            // Indoor cameras only attach to brooders/incubators, never hutches.
            assignableUnits = units.filter { it.housingType == "brooder" || it.housingType == "incubator" }
            error = null
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load indoor cameras", e)
            error = e.message ?: "Couldn't reach the server"
        } finally {
            loading = false
        }
    }

    LaunchedEffect(Unit) { load() }

    PullToRefreshBox(
        isRefreshing = isRefreshing,
        onRefresh = {
            scope.launch {
                isRefreshing = true
                onRefresh()
                load()
                refreshKey++
                isRefreshing = false
            }
        },
        modifier = Modifier.fillMaxSize(),
    ) {
        when {
            loading && cameras.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(color = SageGreen)
                }
            }
            cameras.isEmpty() -> {
                Column(
                    Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Spacer(Modifier.height(64.dp))
                    Icon(Icons.Default.PhotoCamera, null, Modifier.size(56.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(12.dp))
                    Text(
                        error ?: "No indoor camera yet.\nIt appears automatically once the pipeline posts an observation.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
            }
            else -> {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(16.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    items(cameras, key = { it.id }) { cam ->
                        IndoorCamCard(
                            camera = cam,
                            assignableUnits = assignableUnits,
                            refreshKey = refreshKey,
                            onAssign = { brooderId ->
                                scope.launch {
                                    try {
                                        withContext(Dispatchers.IO) {
                                            api.assignIndoorCamera(cam.id, AssignIndoorCameraRequest(brooderId))
                                        }
                                        load(); refreshKey++
                                    } catch (e: Exception) {
                                        Log.e("QuailSync", "Assign indoor cam failed", e)
                                    }
                                }
                            },
                            onUnassign = {
                                scope.launch {
                                    try {
                                        withContext(Dispatchers.IO) { api.unassignIndoorCamera(cam.id) }
                                        load(); refreshKey++
                                    } catch (e: Exception) {
                                        Log.e("QuailSync", "Unassign indoor cam failed", e)
                                    }
                                }
                            },
                        )
                    }
                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }
}

/** One indoor camera: header + live chick count, a saved frame if the
 *  observation kept one, and the brooder/incubator assignment control. Fetches
 *  its own latest observation; re-fetches when [refreshKey] changes. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun IndoorCamCard(
    camera: IndoorCamera,
    assignableUnits: List<Brooder>,
    refreshKey: Int,
    onAssign: (Int) -> Unit,
    onUnassign: () -> Unit,
) {
    val context = LocalContext.current
    val baseUrl = remember { ServerConfig.getServerUrl(context).trimEnd('/') }
    val api = remember { QuailSyncApi.create(ServerConfig.getServerUrl(context)) }

    var state by remember(camera.cameraId) { mutableStateOf<IndoorObsState>(IndoorObsState.Loading) }
    LaunchedEffect(camera.cameraId, refreshKey) {
        state = IndoorObsState.Loading
        state = try {
            val latest = withContext(Dispatchers.IO) { api.getIndoorcamLatest(camera.cameraId) }
            IndoorObsState.Data(latest)
        } catch (e: retrofit2.HttpException) {
            // 404 = no observations yet for this camera (vs. a real error).
            if (e.code() == 404) IndoorObsState.Empty else IndoorObsState.Error("Server error (HTTP ${e.code()})")
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load indoor cam ${camera.cameraId}", e)
            IndoorObsState.Error(e.message ?: "Couldn't reach the server")
        }
    }

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(camera.name ?: camera.cameraId, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(10.dp))
            when (val s = state) {
                is IndoorObsState.Loading -> CircularProgressIndicator(
                    color = SageGreen, modifier = Modifier.size(28.dp), strokeWidth = 2.dp,
                )
                is IndoorObsState.Empty -> Text(
                    "No detections yet.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                is IndoorObsState.Error -> Text(
                    s.message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                is IndoorObsState.Data -> IndoorObsContent(s.latest, baseUrl)
            }

            Spacer(Modifier.height(12.dp))
            HorizontalDivider()
            Spacer(Modifier.height(12.dp))
            IndoorAssignmentSection(camera, assignableUnits, onAssign, onUnassign)
        }
    }
}

/** Live chick count + freshness, plus the saved frame if the observation kept
 *  one (most won't — only "notable" frames are saved, and they may be cleared
 *  after a Roboflow upload, so the image is hidden if it 404s). */
@Composable
private fun IndoorObsContent(latest: IndoorcamLatest, baseUrl: String) {
    fun absolute(url: String?): String? = url?.let { if (it.startsWith("http")) it else "$baseUrl$it" }
    val count = latest.detectionCount ?: 0
    Text(
        "$count chick${if (count == 1) "" else "s"} detected",
        style = MaterialTheme.typography.headlineSmall,
        fontWeight = FontWeight.Bold,
        color = SageGreen,
    )
    val freshness = freshnessFor(latest.timestamp)
    Text(
        freshness?.text ?: formatCaptureTime(latest.timestamp),
        style = MaterialTheme.typography.bodyMedium,
        color = freshness?.color ?: MaterialTheme.colorScheme.onSurfaceVariant,
    )

    // Prefer the annotated frame (only present when on disk); fall back to raw.
    val imageUrl = absolute(latest.annotatedImageUrl) ?: absolute(latest.imageUrl)
    if (imageUrl != null) {
        // Cache-bust: the rolling latest.jpg reuses one URL but changes each
        // cycle, so key Coil by the observation timestamp to force a reload.
        val model = latest.timestamp?.let { "$imageUrl?v=${java.net.URLEncoder.encode(it, "UTF-8")}" } ?: imageUrl
        Spacer(Modifier.height(10.dp))
        AsyncImage(
            model = model,
            contentDescription = "Latest indoor frame",
            modifier = Modifier
                .fillMaxWidth()
                .aspectRatio(4f / 3f)
                .clip(RoundedCornerShape(12.dp))
                .background(Color.Black),
            contentScale = ContentScale.Crop,
        )
    }
}

/** Assignment row: shows the watched brooder/incubator with an Unassign button,
 *  or an "Assign to" dropdown (brooders + incubators only) when unassigned. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun IndoorAssignmentSection(
    camera: IndoorCamera,
    assignableUnits: List<Brooder>,
    onAssign: (Int) -> Unit,
    onUnassign: () -> Unit,
) {
    val assignment = camera.assignment
    when {
        assignment != null -> {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("Watching", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    val kind = assignment.housingType?.takeIf { it.isNotBlank() }?.let { " ($it)" } ?: ""
                    Text(
                        "${assignment.brooderName}$kind",
                        style = MaterialTheme.typography.bodyLarge,
                        color = SageGreen,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
                OutlinedButton(onClick = onUnassign, colors = ButtonDefaults.outlinedButtonColors(contentColor = SageGreen)) {
                    Text("Unassign")
                }
            }
        }
        assignableUnits.isEmpty() -> {
            Text(
                "Unassigned · no brooders or incubators to assign",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        else -> {
            var expanded by remember { mutableStateOf(false) }
            var selectedId by remember(camera.id) { mutableStateOf<Int?>(null) }
            val selectedName = assignableUnits.find { it.id == selectedId }?.name

            Text("Unassigned", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(8.dp))
            ExposedDropdownMenuBox(expanded, { expanded = it }) {
                OutlinedTextField(
                    value = selectedName ?: "Select brooder or incubator",
                    onValueChange = {},
                    readOnly = true,
                    label = { Text("Assign to") },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                    modifier = Modifier.menuAnchor().fillMaxWidth(),
                )
                ExposedDropdownMenu(expanded, { expanded = false }) {
                    assignableUnits.forEach { unit ->
                        val kind = if (unit.housingType == "incubator") " (incubator)" else ""
                        DropdownMenuItem(
                            text = { Text("${unit.name}$kind") },
                            onClick = { selectedId = unit.id; expanded = false },
                        )
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { selectedId?.let(onAssign) },
                enabled = selectedId != null,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text("Assign") }
        }
    }
}
