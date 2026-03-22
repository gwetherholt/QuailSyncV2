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
import androidx.compose.material.icons.filled.ArrowBack
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.LiveReading
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
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
// ViewModel
// =====================================================================

class TelemetryViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))
    val webSocketService = WebSocketService(ServerConfig.getServerUrl(application))

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
            val brooderList = api.getBrooders().distinctBy { it.id }
            val states = brooderList.map { brooder ->
                val readings = try { api.getBrooderReadings(brooder.id) } catch (_: Exception) { emptyList() }
                val alerts = try { api.getBrooderAlerts(brooder.id) } catch (_: Exception) { emptyList() }
                val targetTemp = try { api.getBrooderTargetTemp(brooder.id) } catch (_: Exception) { null }
                BrooderState(brooder, readings, alerts, targetTemp)
            }
            _brooders.value = states
        } catch (e: Exception) {
            Log.e("QuailSync", "Telemetry: failed to load brooders", e)
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
// Telemetry Screen
// =====================================================================

@Composable
fun TelemetryScreen(
    viewModel: TelemetryViewModel = viewModel(),
    onBrooderClick: (Int) -> Unit = {},
    onBack: () -> Unit = {},
) {
    val brooders by viewModel.brooders.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    val liveReadings by viewModel.webSocketService.readings.collectAsState()

    val lifecycleOwner = androidx.compose.ui.platform.LocalContext.current as LifecycleOwner
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) viewModel.refresh()
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 8.dp),
            Arrangement.Start, Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Default.ArrowBack, "Back") }
            Text("Telemetry", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
            if (isRefreshing) {
                CircularProgressIndicator(Modifier.size(24.dp).padding(end = 16.dp), strokeWidth = 2.dp, color = SageGreen)
            } else {
                IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
            }
        }

        if (isLoading && brooders.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = SageGreen)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(brooders, key = { it.brooder.id }) { state ->
                    DetailedBrooderCard(
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
// Detailed Brooder Card (moved from old DashboardScreen)
// =====================================================================

@Composable
fun DetailedBrooderCard(state: BrooderState, liveReading: LiveReading?, onClick: () -> Unit = {}) {
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

    Card(
        Modifier.fillMaxWidth().clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text(state.brooder.name, style = MaterialTheme.typography.titleLarge)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (sensorStatus == SensorStatus.OFFLINE || sensorStatus == SensorStatus.STALE) {
                        Text(
                            if (sensorStatus == SensorStatus.OFFLINE) "Offline" else "Stale",
                            style = MaterialTheme.typography.labelMedium, color = statusDotColor,
                        )
                        Spacer(Modifier.width(6.dp))
                    }
                    Box(Modifier.size(12.dp).clip(CircleShape).background(statusDotColor))
                }
            }

            Spacer(Modifier.height(12.dp))

            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
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
                    Box(Modifier.size(8.dp).clip(CircleShape).background(tempColor))
                }
            }

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
