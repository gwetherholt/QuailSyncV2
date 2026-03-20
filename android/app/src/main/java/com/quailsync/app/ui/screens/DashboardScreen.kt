package com.quailsync.app.ui.screens

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
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.BrooderAlert
import com.quailsync.app.data.BrooderReading
import com.quailsync.app.data.LiveReading
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.TargetTempResponse
import com.quailsync.app.data.WebSocketService
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

// =====================================================================
// Data
// =====================================================================

data class BrooderState(
    val brooder: Brooder,
    val readings: List<BrooderReading> = emptyList(),
    val alerts: List<BrooderAlert> = emptyList(),
    val targetTemp: TargetTempResponse? = null,
)

private const val STALE_THRESHOLD_MS = 60_000L       // 1 minute
private const val OFFLINE_THRESHOLD_MS = 2 * 60_000L // 2 minutes

enum class SensorStatus { LIVE, STALE, OFFLINE, UNKNOWN }

// =====================================================================
// ViewModel
// =====================================================================

class DashboardViewModel : ViewModel() {
    private val api = QuailSyncApi.create()
    val webSocketService = WebSocketService()

    private val _brooders = MutableStateFlow<List<BrooderState>>(emptyList())
    val brooders: StateFlow<List<BrooderState>> = _brooders.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    init {
        loadData()
        webSocketService.connect()
    }

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
            val brooderList = api.getBrooders()
            Log.d("QuailSync", "Brooders loaded: ${brooderList.size} (IDs: ${brooderList.map { it.id }})")
            // Deduplicate by ID in case the server returns duplicates
            val uniqueBrooders = brooderList.distinctBy { it.id }
            if (uniqueBrooders.size != brooderList.size) {
                Log.w("QuailSync", "Removed ${brooderList.size - uniqueBrooders.size} duplicate brooders")
            }
            val states = uniqueBrooders.map { brooder ->
                val readings = try {
                    api.getBrooderReadings(brooder.id)
                } catch (e: Exception) {
                    Log.e("QuailSync", "Failed to load readings for brooder ${brooder.id}", e)
                    emptyList()
                }
                val alerts = try {
                    api.getBrooderAlerts(brooder.id)
                } catch (e: Exception) {
                    Log.e("QuailSync", "Failed to load alerts for brooder ${brooder.id}", e)
                    emptyList()
                }
                val targetTemp = try {
                    api.getBrooderTargetTemp(brooder.id)
                } catch (_: Exception) { null }
                BrooderState(brooder, readings, alerts, targetTemp)
            }
            _brooders.value = states
            Log.d("QuailSync", "BrooderStates set: ${states.size} cards")
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load brooders", e)
        } finally {
            _isLoading.value = false
        }
    }

    override fun onCleared() {
        super.onCleared()
        webSocketService.disconnect()
    }
}

// =====================================================================
// Dashboard Screen
// =====================================================================

@Composable
fun DashboardScreen(viewModel: DashboardViewModel = viewModel(), onBrooderClick: (Int) -> Unit = {}) {
    val brooders by viewModel.brooders.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    val liveReadings by viewModel.webSocketService.readings.collectAsState()

    // Refresh data whenever this screen becomes visible (e.g. navigating back)
    val lifecycleOwner = androidx.compose.ui.platform.LocalContext.current as LifecycleOwner
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                viewModel.refresh()
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    Column(modifier = Modifier.fillMaxSize()) {
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

        Log.d("QuailSync", "DashboardScreen recompose: ${brooders.size} brooders, ${liveReadings.size} live readings")

        if (isLoading && brooders.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = SageGreen)
            }
        } else {
            // Only show brooders from the REST API — one card per brooder.
            // WebSocket live readings are overlaid by matching brooder ID.
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(brooders, key = { it.brooder.id }) { state ->
                    BrooderCard(
                        state = state,
                        liveReading = liveReadings[state.brooder.id],
                        onClick = { onBrooderClick(state.brooder.id) },
                    )
                }
                item { Spacer(Modifier.height(8.dp)) }
            }
        }
    }
}

// =====================================================================
// Brooder Card
// =====================================================================

@Composable
fun BrooderCard(state: BrooderState, liveReading: LiveReading?, onClick: () -> Unit = {}) {
    // Use live WebSocket data if available, otherwise fall back to REST data
    val currentTemp = liveReading?.temperature
        ?: state.readings.firstOrNull()?.temperature
        ?: state.brooder.latestTemperature
        ?: state.brooder.latestTemperatureCelsius
    val currentHumidity = liveReading?.humidity
        ?: state.readings.firstOrNull()?.humidity
        ?: state.brooder.latestHumidity
        ?: state.brooder.latestHumidityPercent

    val previousTemp = if (state.readings.size >= 2) state.readings[1].temperature else null
    val previousHumidity = if (state.readings.size >= 2) state.readings[1].humidity else null

    // Sensor status based on last reading time
    // Not cached in remember — recalculates on every recomposition so age stays fresh
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
            // Have REST data but no live WebSocket data yet
            SensorStatus.UNKNOWN
        } else {
            SensorStatus.OFFLINE
        }
    }

    // Alert status
    val hasActiveAlerts = state.alerts.any { it.acknowledged != true }
    val hasCriticalAlert = state.alerts.any { it.acknowledged != true && it.severity?.lowercase() == "critical" }

    // Status dot: sensor status takes priority for color if offline/stale
    val statusDotColor = when {
        sensorStatus == SensorStatus.OFFLINE -> AlertRed
        sensorStatus == SensorStatus.STALE -> AlertYellow
        hasCriticalAlert -> AlertRed
        hasActiveAlerts -> AlertYellow
        else -> AlertGreen
    }

    Card(
        Modifier.fillMaxWidth().clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            // Header: name + status dot
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text(state.brooder.name, style = MaterialTheme.typography.titleLarge)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (sensorStatus == SensorStatus.OFFLINE || sensorStatus == SensorStatus.STALE) {
                        Text(
                            if (sensorStatus == SensorStatus.OFFLINE) "Offline" else "Stale",
                            style = MaterialTheme.typography.labelMedium,
                            color = statusDotColor,
                        )
                        Spacer(Modifier.width(6.dp))
                    }
                    Box(Modifier.size(12.dp).clip(CircleShape).background(statusDotColor))
                }
            }

            Spacer(Modifier.height(12.dp))

            // Readings
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                // Temperature
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Temperature", style = MaterialTheme.typography.bodyMedium)
                    Spacer(Modifier.height(4.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            currentTemp?.let { "%.1f°F".format(it) } ?: "--",
                            fontSize = 24.sp, fontWeight = FontWeight.Bold,
                            color = if (sensorStatus == SensorStatus.OFFLINE) MaterialTheme.colorScheme.onSurfaceVariant
                                else MaterialTheme.colorScheme.onSurface,
                        )
                        if (currentTemp != null && previousTemp != null) {
                            Spacer(Modifier.width(4.dp))
                            TrendIcon(currentTemp, previousTemp)
                        }
                    }
                    if (sensorStatus == SensorStatus.OFFLINE && currentTemp == null) {
                        Text("No data", style = MaterialTheme.typography.labelSmall, color = AlertRed)
                    }
                }

                // Humidity
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Humidity", style = MaterialTheme.typography.bodyMedium)
                    Spacer(Modifier.height(4.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            currentHumidity?.let { "%.0f%%".format(it) } ?: "--",
                            fontSize = 24.sp, fontWeight = FontWeight.Bold,
                            color = if (sensorStatus == SensorStatus.OFFLINE) MaterialTheme.colorScheme.onSurfaceVariant
                                else MaterialTheme.colorScheme.onSurface,
                        )
                        if (currentHumidity != null && previousHumidity != null) {
                            Spacer(Modifier.width(4.dp))
                            TrendIcon(currentHumidity, previousHumidity)
                        }
                    }
                    if (sensorStatus == SensorStatus.OFFLINE && currentHumidity == null) {
                        Text("No data", style = MaterialTheme.typography.labelSmall, color = AlertRed)
                    }
                }
            }

            // Target temp + chick age
            if (state.targetTemp != null) {
                Spacer(Modifier.height(10.dp))
                val tt = state.targetTemp
                val tempInRange = currentTemp != null && currentTemp >= tt.minTempF && currentTemp <= tt.maxTempF
                val tempColor = when {
                    currentTemp == null -> MaterialTheme.colorScheme.onSurfaceVariant
                    tempInRange -> AlertGreen
                    else -> AlertRed
                }

                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(6.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                        .padding(horizontal = 10.dp, vertical = 6.dp),
                    Arrangement.SpaceBetween, Alignment.CenterVertically,
                ) {
                    Column {
                        Text(
                            "Target: %.0f°F".format(tt.targetTempF),
                            style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, color = tempColor,
                        )
                        if (tt.ageDays != null) {
                            Text(
                                "Day ${tt.ageDays} — ${tt.scheduleLabel}",
                                style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        } else {
                            Text("Unassigned", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    // Range indicator dot
                    Box(Modifier.size(8.dp).clip(CircleShape).background(tempColor))
                }
            }

            // Alerts
            if (hasActiveAlerts) {
                Spacer(Modifier.height(8.dp))
                val alertCount = state.alerts.count { it.acknowledged != true }
                Text(
                    "$alertCount active alert${if (alertCount != 1) "s" else ""}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (hasCriticalAlert) AlertRed else AlertYellow,
                )
            }
        }
    }
}

// =====================================================================
// Trend Icon
// =====================================================================

@Composable
fun TrendIcon(current: Double, previous: Double) {
    val diff = current - previous
    val (icon, tint) = when {
        diff > 0.5 -> Icons.Default.ArrowUpward to AlertRed
        diff < -0.5 -> Icons.Default.ArrowDownward to MaterialTheme.colorScheme.primary
        else -> Icons.Default.Remove to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Icon(icon, "Trend", Modifier.size(18.dp), tint = tint)
}
