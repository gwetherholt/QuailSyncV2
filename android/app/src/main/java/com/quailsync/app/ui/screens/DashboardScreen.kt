package com.quailsync.app.ui.screens

import android.app.Application
import android.util.Log
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Science
import androidx.compose.material.icons.filled.Sensors
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.Bloodline
import com.quailsync.app.data.BrooderAlert
import com.quailsync.app.data.BrooderReading
import com.quailsync.app.data.ChickGroupDto
import com.quailsync.app.data.Clutch
import com.quailsync.app.data.LiveReading
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.TargetTempResponse
import com.quailsync.app.data.WebSocketManager
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
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

// =====================================================================
// Data
// =====================================================================

data class BrooderState(
    val brooder: Brooder,
    val readings: List<BrooderReading> = emptyList(),
    val alerts: List<BrooderAlert> = emptyList(),
    val targetTemp: TargetTempResponse? = null,
)

const val STALE_THRESHOLD_MS = 60_000L       // 1 minute
const val OFFLINE_THRESHOLD_MS = 2 * 60_000L // 2 minutes
private const val INCUBATION_DAYS = 17L

enum class SensorStatus { LIVE, STALE, OFFLINE, UNKNOWN }

// =====================================================================
// ViewModel
// =====================================================================

class DashboardViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))
    val webSocketService = WebSocketManager.get(application)

    private val _brooders = MutableStateFlow<List<BrooderState>>(emptyList())
    val brooders: StateFlow<List<BrooderState>> = _brooders.asStateFlow()

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    private val _clutches = MutableStateFlow<List<Clutch>>(emptyList())
    val clutches: StateFlow<List<Clutch>> = _clutches.asStateFlow()

    private val _chickGroups = MutableStateFlow<List<ChickGroupDto>>(emptyList())
    val chickGroups: StateFlow<List<ChickGroupDto>> = _chickGroups.asStateFlow()

    private val _bloodlines = MutableStateFlow<List<Bloodline>>(emptyList())
    val bloodlines: StateFlow<List<Bloodline>> = _bloodlines.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    init {
        loadData()
    }

    fun refresh() {
        viewModelScope.launch {
            _isRefreshing.value = true
            loadDataSuspend()
            if (!webSocketService.isConnected.value) {
                webSocketService.reconnect()
            }
            _isRefreshing.value = false
        }
    }

    private fun loadData() { viewModelScope.launch { loadDataSuspend() } }

    private suspend fun loadDataSuspend() {
        try {
            val brooderList = api.getBrooders().distinctBy { it.id }
            val states = brooderList.map { brooder ->
                val readings = try { api.getBrooderReadings(brooder.id) } catch (_: Exception) { emptyList() }
                val alerts = try { api.getBrooderAlerts(brooder.id) } catch (_: Exception) { emptyList() }
                val targetTemp = try { api.getBrooderTargetTemp(brooder.id) } catch (_: Exception) { null }
                BrooderState(brooder, readings, alerts, targetTemp)
            }
            _brooders.value = states

            _birds.value = try { api.getBirds() } catch (_: Exception) { emptyList() }
            _clutches.value = try { api.getClutches() } catch (_: Exception) { emptyList() }
            _chickGroups.value = try { api.getChickGroups() } catch (_: Exception) { emptyList() }
            _bloodlines.value = try { api.getBloodlines() } catch (_: Exception) { emptyList() }

            Log.d("QuailSync", "Dashboard loaded: ${states.size} brooders, ${_birds.value.size} birds, ${_clutches.value.size} clutches, ${_chickGroups.value.size} chick groups")
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load dashboard data", e)
        } finally {
            _isLoading.value = false
        }
    }

}

// =====================================================================
// Dashboard Screen
// =====================================================================

@Composable
fun DashboardScreen(
    viewModel: DashboardViewModel = viewModel(),
    onBrooderClick: (Int) -> Unit = {},
    onTelemetryClick: () -> Unit = {},
    onBreedingClick: () -> Unit = {},
) {
    val brooders by viewModel.brooders.collectAsState()
    val birds by viewModel.birds.collectAsState()
    val clutches by viewModel.clutches.collectAsState()
    val chickGroups by viewModel.chickGroups.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    val liveReadings by viewModel.webSocketService.readings.collectAsState()
    val wsConnected by viewModel.webSocketService.isConnected.collectAsState()

    val lifecycleOwner = androidx.compose.ui.platform.LocalContext.current as LifecycleOwner
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) viewModel.refresh()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    // Derived data
    val today = LocalDate.now()
    val bloodlineMap = bloodlines.associateBy { it.id }
    val incubatingClutches = clutches.filter { it.status?.lowercase() in listOf("incubating", "active", "set") }
    val hatchCountdowns = incubatingClutches.mapNotNull { clutch ->
        val setDate = clutch.setDate?.let { parseDate(it) } ?: return@mapNotNull null
        val expectedHatch = setDate.plusDays(INCUBATION_DAYS)
        val daysUntil = ChronoUnit.DAYS.between(today, expectedHatch).toInt()
        Triple(clutch, daysUntil, expectedHatch)
    }.sortedBy { it.second }

    val nearestHatch = hatchCountdowns.firstOrNull()
    val urgentHatches = hatchCountdowns.filter { it.second in 0..3 }

    val activeBirds = birds.count { it.status?.lowercase() == "active" }
    val eggsIncubating = incubatingClutches.sumOf { it.totalEggs ?: 0 }
    val activeGroups = chickGroups.count { it.status == "Active" }

    val allAlerts = brooders.flatMap { bs ->
        bs.alerts.filter { it.acknowledged != true }.map { alert -> alert to bs.brooder.name }
    }.sortedByDescending { it.first.createdAt }.take(10)

    Column(modifier = Modifier.fillMaxSize()) {
        // Header
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            Arrangement.SpaceBetween, Alignment.CenterVertically,
        ) {
            Text("Dashboard", style = MaterialTheme.typography.headlineMedium)
            if (isRefreshing) {
                CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = SageGreen)
            } else {
                IconButton(onClick = { viewModel.refresh() }) {
                    Icon(Icons.Default.Refresh, "Refresh")
                }
            }
        }

        if (isLoading && brooders.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = SageGreen)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 4.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                // === 1. Hatch Countdown Banner ===
                if (urgentHatches.isNotEmpty()) {
                    item(key = "hatch-banner") {
                        HatchCountdownBanner(urgentHatches, bloodlineMap)
                    }
                }

                // === 2. Quick Stats Row ===
                item(key = "quick-stats") {
                    QuickStatsRow(
                        activeBirds = activeBirds,
                        eggsIncubating = eggsIncubating,
                        activeGroups = activeGroups,
                        daysToNextHatch = nearestHatch?.second,
                    )
                }

                // === 3. Compact Brooder Cards ===
                item(key = "brooders-header") {
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                        Text("Brooders", style = MaterialTheme.typography.titleMedium)
                        TextButton(onClick = onTelemetryClick) {
                            Icon(Icons.Default.Sensors, null, Modifier.size(16.dp), tint = SageGreen)
                            Spacer(Modifier.width(4.dp))
                            Text("Telemetry", color = SageGreen)
                        }
                    }
                }

                items(brooders, key = { "brooder-${it.brooder.id}" }) { state ->
                    CompactBrooderCard(
                        state = state,
                        liveReading = liveReadings[state.brooder.id],
                        chickGroup = chickGroups.find { it.brooderId == state.brooder.id && it.status == "Active" },
                        onClick = { onBrooderClick(state.brooder.id) },
                    )
                }

                // === Breeding & Culling Card ===
                item(key = "breeding-card") {
                    Card(
                        Modifier.fillMaxWidth().clickable(onClick = onBreedingClick),
                        shape = RoundedCornerShape(10.dp),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                        elevation = CardDefaults.cardElevation(1.dp),
                    ) {
                        Row(
                            Modifier.padding(14.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(Icons.Default.Science, null, Modifier.size(24.dp), tint = SageGreen)
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text("Breeding & Culling", style = MaterialTheme.typography.titleMedium)
                                Text("Manage groups, pair checks, cull list", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            Text("\u203A", fontSize = 20.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }

                // === 4. Recent Alerts Feed ===
                item(key = "alerts-header") {
                    Spacer(Modifier.height(4.dp))
                    Text("Recent Alerts", style = MaterialTheme.typography.titleMedium)
                }

                if (allAlerts.isEmpty()) {
                    item(key = "alerts-empty") {
                        Text(
                            "No active alerts",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(vertical = 4.dp),
                        )
                    }
                } else {
                    items(allAlerts, key = { "alert-${it.first.id ?: it.hashCode()}" }) { (alert, brooderName) ->
                        AlertRow(alert, brooderName)
                    }
                }

                // === 5. System Health Footer ===
                item(key = "system-health") {
                    Spacer(Modifier.height(4.dp))
                    SystemHealthFooter(wsConnected = wsConnected, brooderCount = brooders.size)
                }

                item { Spacer(Modifier.height(16.dp)) }
            }
        }
    }
}

// =====================================================================
// Hatch Countdown Banner
// =====================================================================

@Composable
private fun HatchCountdownBanner(urgentHatches: List<Triple<Clutch, Int, LocalDate>>, bloodlineMap: Map<Int, Bloodline>) {
    val nearest = urgentHatches.first()
    val clutch = nearest.first
    val daysUntil = nearest.second
    val clutchLabel = clutch.bloodlineName
        ?: clutch.bloodlineId?.let { bloodlineMap[it]?.name }
        ?: "Clutch #${clutch.id}"
    val eggCount = clutch.totalEggs ?: 0

    val message = when {
        daysUntil <= 0 -> "Hatch Day! $clutchLabel ($eggCount eggs) due today!"
        daysUntil == 1 -> "Hatch tomorrow: $clutchLabel ($eggCount eggs)"
        else -> "Hatch in $daysUntil days: $clutchLabel ($eggCount eggs)"
    }

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (daysUntil <= 0) AlertYellow.copy(alpha = 0.15f) else SageGreen.copy(alpha = 0.12f),
        ),
    ) {
        Row(
            Modifier.padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("\uD83D\uDC23", fontSize = 28.sp) // 🐣
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    message,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                if (urgentHatches.size > 1) {
                    Text(
                        "+${urgentHatches.size - 1} more within 3 days",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

// =====================================================================
// Quick Stats Row
// =====================================================================

@Composable
private fun QuickStatsRow(activeBirds: Int, eggsIncubating: Int, activeGroups: Int, daysToNextHatch: Int?) {
    Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
        StatPill(Modifier.weight(1f), activeBirds.toString(), "Birds", SageGreen)
        StatPill(Modifier.weight(1f), eggsIncubating.toString(), "Eggs", DustyRose)
        StatPill(Modifier.weight(1f), activeGroups.toString(), "Groups", SageGreen)
        StatPill(Modifier.weight(1f), daysToNextHatch?.toString() ?: "--", "Next Hatch", AlertYellow)
    }
}

@Composable
private fun StatPill(modifier: Modifier, value: String, label: String, accentColor: Color) {
    Card(
        modifier,
        shape = RoundedCornerShape(10.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(1.dp),
    ) {
        Column(
            Modifier.padding(horizontal = 8.dp, vertical = 10.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                value,
                fontSize = 22.sp,
                fontWeight = FontWeight.Bold,
                color = accentColor,
            )
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                maxLines = 1,
            )
        }
    }
}

// =====================================================================
// Compact Brooder Card
// =====================================================================

@Composable
private fun CompactBrooderCard(
    state: BrooderState,
    liveReading: LiveReading?,
    chickGroup: ChickGroupDto?,
    onClick: () -> Unit,
) {
    val currentTemp = liveReading?.temperature
        ?: state.readings.firstOrNull()?.temperature
        ?: state.brooder.latestTemperature
        ?: state.brooder.latestTemperatureF
    val currentHumidity = liveReading?.humidity
        ?: state.readings.firstOrNull()?.humidity
        ?: state.brooder.latestHumidity
        ?: state.brooder.latestHumidityPercent

    val sensorStatus = run {
        val lastReadingMs = liveReading?.receivedAt
        if (lastReadingMs != null) {
            val age = System.currentTimeMillis() - lastReadingMs
            when {
                age < STALE_THRESHOLD_MS -> SensorStatus.LIVE
                age < OFFLINE_THRESHOLD_MS -> SensorStatus.STALE
                else -> SensorStatus.OFFLINE
            }
        } else if (state.readings.isNotEmpty()) {
            SensorStatus.UNKNOWN
        } else {
            SensorStatus.OFFLINE
        }
    }

    val hasActiveAlerts = state.alerts.any { it.acknowledged != true }
    val hasCriticalAlert = state.alerts.any { it.acknowledged != true && it.severity?.lowercase() == "critical" }

    val statusDotColor = when {
        sensorStatus == SensorStatus.OFFLINE -> AlertRed
        sensorStatus == SensorStatus.STALE -> AlertYellow
        hasCriticalAlert -> AlertRed
        hasActiveAlerts -> AlertYellow
        else -> AlertGreen
    }

    // Chick age
    val chickAge = chickGroup?.hatchDate?.let { hd ->
        parseDate(hd)?.let { ChronoUnit.DAYS.between(it, LocalDate.now()).toInt() }
    }

    Card(
        Modifier.fillMaxWidth().clickable(onClick = onClick),
        shape = RoundedCornerShape(10.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(1.dp),
    ) {
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Status dot
            Box(Modifier.size(10.dp).clip(CircleShape).background(statusDotColor))
            Spacer(Modifier.width(10.dp))

            // Brooder name
            Text(
                state.brooder.name,
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )

            // Temp + Humidity
            Column(horizontalAlignment = Alignment.End) {
                Text(
                    currentTemp?.let { "%.1f°F".format(it) } ?: "--",
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = if (sensorStatus == SensorStatus.OFFLINE) MaterialTheme.colorScheme.onSurfaceVariant
                    else MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    currentHumidity?.let { "%.0f%%".format(it) } ?: "--",
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            // Chick info
            if (chickGroup != null) {
                Spacer(Modifier.width(12.dp))
                Column(horizontalAlignment = Alignment.End) {
                    Text(
                        "${chickGroup.currentCount} chicks",
                        style = MaterialTheme.typography.labelMedium,
                        color = SageGreen,
                        maxLines = 1,
                    )
                    if (chickAge != null && chickAge >= 0) {
                        Text(
                            "Day $chickAge",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }
}

// =====================================================================
// Alert Row
// =====================================================================

@Composable
private fun AlertRow(alert: BrooderAlert, brooderName: String) {
    val severityColor = when (alert.severity?.lowercase()) {
        "critical" -> AlertRed
        "warning" -> AlertYellow
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }

    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Box(
            Modifier.padding(top = 5.dp).size(8.dp).clip(CircleShape).background(severityColor),
        )
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Row {
                Text(
                    brooderName,
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                alert.createdAt?.let { ts ->
                    Spacer(Modifier.width(8.dp))
                    Text(
                        formatRelativeTime(ts),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Text(
                alert.message ?: alert.alertType,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

// =====================================================================
// System Health Footer
// =====================================================================

@Composable
private fun SystemHealthFooter(wsConnected: Boolean, brooderCount: Int) {
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(10.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)),
        elevation = CardDefaults.cardElevation(0.dp),
    ) {
        Row(
            Modifier.padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.size(8.dp).clip(CircleShape)
                    .background(if (wsConnected) AlertGreen else AlertRed),
            )
            Spacer(Modifier.width(8.dp))
            Text(
                if (wsConnected) "Pi Agent Connected" else "Pi Agent Disconnected",
                style = MaterialTheme.typography.labelMedium,
                color = if (wsConnected) AlertGreen else AlertRed,
            )
            Spacer(Modifier.weight(1f))
            Text(
                "$brooderCount brooder${if (brooderCount != 1) "s" else ""}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// =====================================================================
// Utilities
// =====================================================================

private fun parseDate(dateStr: String): LocalDate? {
    return try {
        LocalDate.parse(dateStr, DateTimeFormatter.ISO_LOCAL_DATE)
    } catch (_: Exception) {
        try {
            LocalDate.parse(dateStr.take(10), DateTimeFormatter.ISO_LOCAL_DATE)
        } catch (_: Exception) { null }
    }
}

private fun formatRelativeTime(timestamp: String): String {
    return try {
        val date = LocalDate.parse(timestamp.take(10), DateTimeFormatter.ISO_LOCAL_DATE)
        val days = ChronoUnit.DAYS.between(date, LocalDate.now()).toInt()
        when {
            days == 0 -> "today"
            days == 1 -> "yesterday"
            days < 7 -> "${days}d ago"
            else -> timestamp.take(10)
        }
    } catch (_: Exception) {
        timestamp.take(16)
    }
}
