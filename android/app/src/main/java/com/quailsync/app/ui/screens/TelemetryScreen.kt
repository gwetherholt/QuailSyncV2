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
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import com.quailsync.app.data.AssignSensorRequest
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.GoveeSensorDto
import com.quailsync.app.data.LiveReading
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.WebSocketManager
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

// =====================================================================
// ViewModel
// =====================================================================

class TelemetryViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))
    val webSocketService = WebSocketManager.get(application)

    private val _brooders = MutableStateFlow<List<BrooderState>>(emptyList())
    val brooders: StateFlow<List<BrooderState>> = _brooders.asStateFlow()

    private val _chickGroups = MutableStateFlow<List<com.quailsync.app.data.ChickGroupDto>>(emptyList())
    val chickGroups: StateFlow<List<com.quailsync.app.data.ChickGroupDto>> = _chickGroups.asStateFlow()

    // Govee temp/humidity sensors (auto-registered by the poller).
    private val _sensors = MutableStateFlow<List<GoveeSensorDto>>(emptyList())
    val sensors: StateFlow<List<GoveeSensorDto>> = _sensors.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    init { loadData() }

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
            // List every housing unit so all types (brooder, incubator, hutch)
            // can be managed — including deleted — from here. Hutches have no
            // environmental sensors, so we skip the per-id readings/alerts/
            // target-temp round trips for them (they'd just be empty) and show
            // them with no telemetry, but still render the card + delete action.
            val brooderList = api.getBrooders().distinctBy { it.id }
            val states = brooderList.map { brooder ->
                val isHutch = (brooder.housingType ?: "brooder").lowercase() == "hutch"
                if (isHutch) {
                    BrooderState(brooder, emptyList(), emptyList(), null)
                } else {
                    val readings = try { api.getBrooderReadings(brooder.id) } catch (_: Exception) { emptyList() }
                    val alerts = try { api.getBrooderAlerts(brooder.id) } catch (_: Exception) { emptyList() }
                    val targetTemp = try { api.getBrooderTargetTemp(brooder.id) } catch (_: Exception) { null }
                    BrooderState(brooder, readings, alerts, targetTemp)
                }
            }
            _brooders.value = states
            _chickGroups.value = try { api.getChickGroups() } catch (_: Exception) { emptyList() }
            _sensors.value = try { api.getGoveeSensors() } catch (_: Exception) { emptyList() }
        } catch (e: Exception) {
            Log.e("QuailSync", "Telemetry: failed to load brooders", e)
        } finally {
            _isLoading.value = false
        }
    }

    /** Lightweight DB-only sensor refresh (no Govee API hit) for the 60s loop.
     *  Keeps the existing list on failure rather than blanking the UI. */
    fun refreshSensors() {
        viewModelScope.launch {
            try {
                _sensors.value = api.getGoveeSensors()
            } catch (e: Exception) {
                Log.e("QuailSync", "Telemetry: failed to refresh sensors", e)
            }
        }
    }

    fun assignSensor(sensorId: Int, brooderId: Int) {
        viewModelScope.launch {
            try {
                api.assignGoveeSensor(sensorId, AssignSensorRequest(brooderId))
                loadDataSuspend()
                toast("Sensor assigned")
            } catch (e: retrofit2.HttpException) {
                val body = e.response()?.errorBody()?.string()
                Log.e("QuailSync", "Assign sensor $sensorId failed: $body", e)
                toast(body?.takeIf { it.isNotBlank() } ?: "Assign failed: ${e.message}")
            } catch (e: Exception) {
                Log.e("QuailSync", "Assign sensor $sensorId failed", e)
                toast("Assign failed: ${e.message}")
            }
        }
    }

    fun unassignSensor(sensorId: Int) {
        viewModelScope.launch {
            try {
                api.unassignGoveeSensor(sensorId)
                loadDataSuspend()
                toast("Sensor unassigned")
            } catch (e: retrofit2.HttpException) {
                val body = e.response()?.errorBody()?.string()
                Log.e("QuailSync", "Unassign sensor $sensorId failed: $body", e)
                toast(body?.takeIf { it.isNotBlank() } ?: "Unassign failed: ${e.message}")
            } catch (e: Exception) {
                Log.e("QuailSync", "Unassign sensor $sensorId failed", e)
                toast("Unassign failed: ${e.message}")
            }
        }
    }

    private fun toast(msg: String) {
        android.widget.Toast.makeText(getApplication(), msg, android.widget.Toast.LENGTH_SHORT).show()
    }

    fun deleteBrooderAsync(id: Int) {
        Log.d("QuailSync", "deleteBrooderAsync called for brooder $id")
        viewModelScope.launch {
            try {
                Log.d("QuailSync", "Calling DELETE API for brooder $id")
                val resp = api.deleteBrooder(id)
                Log.d("QuailSync", "Delete brooder response: ${resp.code()}")
                loadDataSuspend()
                android.widget.Toast.makeText(
                    getApplication(), "Brooder deleted", android.widget.Toast.LENGTH_SHORT
                ).show()
            } catch (e: Exception) {
                Log.e("QuailSync", "Delete brooder $id failed", e)
                android.widget.Toast.makeText(
                    getApplication(), "Delete failed: ${e.message}", android.widget.Toast.LENGTH_SHORT
                ).show()
            }
        }
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
    val chickGroups by viewModel.chickGroups.collectAsState()
    val sensors by viewModel.sensors.collectAsState()

    // Govee readings come from our DB (the poller writes them), not the live
    // WebSocket — so poll them every 60s while this screen is on top.
    LaunchedEffect(Unit) {
        while (true) {
            delay(60_000)
            viewModel.refreshSensors()
        }
    }

    // Explicit MutableState — see BreedingScreen for rationale: with the `by`
    // delegate, lambda writes (`deleteTargetId = null`) get flagged by the
    // Kotlin flow-analyser as `UNUSED_VALUE` because it can't see Compose's
    // recomposition-time reads.
    val deleteTargetId = remember { mutableStateOf<Int?>(null) }

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
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
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
                    val live = liveReadings[state.brooder.id]
                    val hasActiveGroup = chickGroups.any { it.brooderId == state.brooder.id && it.status == "Active" }
                    val sensorSt = computeSensorStatus(live, state.readings.isNotEmpty())
                    val canDelete = !hasActiveGroup && (sensorSt == SensorStatus.OFFLINE || sensorSt == SensorStatus.UNKNOWN)
                    DetailedBrooderCard(
                        state = state,
                        liveReading = live,
                        assignedSensors = sensors.filter { it.assignment?.brooderId == state.brooder.id },
                        onClick = { onBrooderClick(state.brooder.id) },
                        onDelete = if (canDelete) ({
                            Log.d("QuailSync", "Delete icon tapped for brooder ${state.brooder.id}")
                            deleteTargetId.value = state.brooder.id
                        }) else null,
                    )
                }

                // --- Govee sensors overview + assignment ---
                item {
                    Spacer(Modifier.height(4.dp))
                    Text(
                        "🌡️ Govee Sensors",
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.padding(top = 8.dp, bottom = 2.dp),
                    )
                }
                if (sensors.isEmpty()) {
                    item {
                        Text(
                            "No Govee sensors registered yet. They appear here automatically once the poller reports one.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(vertical = 4.dp),
                        )
                    }
                } else {
                    items(sensors, key = { "sensor-${it.id}" }) { sensor ->
                        SensorCard(
                            sensor = sensor,
                            brooders = brooders.map { it.brooder },
                            onAssign = { brooderId -> viewModel.assignSensor(sensor.id, brooderId) },
                            onUnassign = { viewModel.unassignSensor(sensor.id) },
                        )
                    }
                }
                item { Spacer(Modifier.height(8.dp)) }
            }
        }
    }

    if (deleteTargetId.value != null) {
        val idToDelete = deleteTargetId.value!!
        AlertDialog(
            onDismissRequest = { deleteTargetId.value = null },
            title = { Text("Delete Brooder?") },
            text = { Text("This will remove brooder #$idToDelete and all its sensor readings. This cannot be undone.") },
            confirmButton = {
                Button(
                    onClick = {
                        val id = deleteTargetId.value!!
                        Log.d("QuailSync", "Delete confirmed for brooder $id")
                        deleteTargetId.value = null
                        viewModel.deleteBrooderAsync(id)
                    },
                    colors = ButtonDefaults.buttonColors(containerColor = AlertRed),
                ) { Text("Delete") }
            },
            dismissButton = {
                OutlinedButton(onClick = { deleteTargetId.value = null }) { Text("Cancel") }
            },
        )
    }
}

// =====================================================================
// Detailed Brooder Card (moved from old DashboardScreen)
// =====================================================================

@Composable
fun DetailedBrooderCard(
    state: BrooderState,
    liveReading: LiveReading?,
    assignedSensors: List<GoveeSensorDto> = emptyList(),
    onClick: () -> Unit = {},
    onDelete: (() -> Unit)? = null,
) {
    // An assigned Govee sensor (replaces the DIY ESP32) is the source of truth
    // for this unit's headline temp/humidity when present — most recent reading
    // across its assigned sensors. Makes hutch cards (no Pi sensor) show a value.
    val goveeReading = assignedSensors.asSequence()
        .mapNotNull { it.latestReading }
        .maxByOrNull { it.recordedAt ?: "" }
    val currentTemp = goveeReading?.temperatureF
        ?: liveReading?.temperature
        ?: state.readings.firstOrNull()?.temperature
        ?: state.brooder.latestTemperature
        ?: state.brooder.latestTemperatureF
    val currentHumidity = goveeReading?.humidity
        ?: liveReading?.humidity
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
            // Camera status indicator
            if (!state.brooder.cameraUrl.isNullOrBlank()) {
                Text(
                    "\uD83D\uDCF9 Camera connected",
                    style = MaterialTheme.typography.labelMedium,
                    color = AlertGreen,
                )
                Spacer(Modifier.height(4.dp))
            }

            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text(state.brooder.name, style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (sensorStatus == SensorStatus.OFFLINE || sensorStatus == SensorStatus.STALE) {
                        Text(
                            if (sensorStatus == SensorStatus.OFFLINE) "Offline" else "Stale",
                            style = MaterialTheme.typography.labelMedium, color = statusDotColor,
                        )
                        Spacer(Modifier.width(6.dp))
                    }
                    Box(Modifier.size(12.dp).clip(CircleShape).background(statusDotColor))
                    if (onDelete != null) {
                        Spacer(Modifier.width(12.dp))
                        IconButton(onClick = onDelete, modifier = Modifier.size(36.dp)) {
                            Icon(Icons.Default.Delete, "Delete brooder", Modifier.size(22.dp), tint = AlertRed.copy(alpha = 0.7f))
                        }
                    }
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
                            color = if (sensorStatus == SensorStatus.OFFLINE && goveeReading == null) MaterialTheme.colorScheme.onSurfaceVariant
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
                            color = if (sensorStatus == SensorStatus.OFFLINE && goveeReading == null) MaterialTheme.colorScheme.onSurfaceVariant
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
                val tempInRange = currentTemp != null && currentTemp in tt.minTempF..tt.maxTempF
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

            // Assigned Govee sensors — glanceable temp/humidity readings.
            if (assignedSensors.isNotEmpty()) {
                Spacer(Modifier.height(10.dp))
                assignedSensors.forEach { s ->
                    val r = s.latestReading
                    val reading = if (r != null) "%.1f°F · %.0f%%".format(r.temperatureF, r.humidity) else "no data"
                    val stale = sensorIsStale(s.lastSeen)
                    Row(
                        Modifier.fillMaxWidth().padding(vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("🌡️ ", style = MaterialTheme.typography.bodyMedium)
                        Text(
                            (s.name ?: s.goveeDeviceId) + ": ",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Text(
                            reading,
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.SemiBold,
                            color = if (stale) AlertYellow else MaterialTheme.colorScheme.onSurface,
                        )
                        if (stale) {
                            Spacer(Modifier.width(4.dp))
                            Text("⚠", style = MaterialTheme.typography.bodyMedium, color = AlertYellow)
                        }
                    }
                }
            }
        }
    }
}

// =====================================================================
// Govee Sensor Card (overview + assignment)
// =====================================================================

@Composable
fun SensorCard(
    sensor: GoveeSensorDto,
    brooders: List<Brooder>,
    onAssign: (brooderId: Int) -> Unit,
    onUnassign: () -> Unit,
) {
    val stale = sensorIsStale(sensor.lastSeen)
    val r = sensor.latestReading

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(
                        sensor.name ?: sensor.goveeDeviceId,
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        sensor.model ?: "—",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                // Online indicator: green dot when fresh, amber + ">10m" when stale.
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(10.dp).clip(CircleShape).background(if (stale) AlertYellow else AlertGreen))
                    Spacer(Modifier.width(5.dp))
                    Text(
                        if (stale) "No data >10m" else "Online",
                        style = MaterialTheme.typography.labelMedium,
                        color = if (stale) AlertYellow else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            Spacer(Modifier.height(10.dp))

            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Temperature", style = MaterialTheme.typography.bodySmall)
                    Text(
                        r?.let { "%.1f°F".format(it.temperatureF) } ?: "--",
                        fontSize = 22.sp, fontWeight = FontWeight.Bold,
                    )
                }
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Humidity", style = MaterialTheme.typography.bodySmall)
                    Text(
                        r?.let { "%.0f%%".format(it.humidity) } ?: "--",
                        fontSize = 22.sp, fontWeight = FontWeight.Bold,
                    )
                }
            }

            Spacer(Modifier.height(12.dp))

            // Assignment row: brooder name + Unassign, or a brooder picker + Assign.
            val assignment = sensor.assignment
            if (assignment != null) {
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Text(
                        "Assigned to ${assignment.brooderName}",
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Medium,
                        color = SageGreen,
                    )
                    OutlinedButton(onClick = onUnassign) { Text("Unassign") }
                }
            } else if (brooders.isNotEmpty()) {
                val expanded = remember { mutableStateOf(false) }
                val selected = remember(sensor.id, brooders) { mutableStateOf(brooders.first()) }
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Box {
                        OutlinedButton(onClick = { expanded.value = true }) {
                            Text(selected.value.name)
                            Icon(Icons.Default.ArrowDropDown, "Pick brooder")
                        }
                        DropdownMenu(expanded.value, onDismissRequest = { expanded.value = false }) {
                            brooders.forEach { b ->
                                DropdownMenuItem(
                                    text = { Text(b.name) },
                                    onClick = { selected.value = b; expanded.value = false },
                                )
                            }
                        }
                    }
                    Button(
                        onClick = { onAssign(selected.value.id) },
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) { Text("Assign") }
                }
            } else {
                Text(
                    "No brooders to assign",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            Spacer(Modifier.height(8.dp))
            Text(
                "Last seen ${sensorRelativeTime(sensor.lastSeen)}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// =====================================================================
// Sensor freshness helpers (last_seen is UTC; >10 min ⇒ stale warning)
// =====================================================================

private const val SENSOR_STALE_MINUTES = 10L

/** Parse a server timestamp as an Instant. Handles both
 *  "YYYY-MM-DD HH:MM:SS" (UTC, no zone) and ISO-with-offset/Z forms. */
private fun parseServerInstant(s: String?): java.time.Instant? {
    if (s.isNullOrBlank()) return null
    return try {
        val iso = if (s.contains('T')) s else s.replace(' ', 'T')
        if (Regex("([zZ])$|[+-]\\d\\d:?\\d\\d$").containsMatchIn(iso)) {
            java.time.OffsetDateTime.parse(iso).toInstant()
        } else {
            java.time.LocalDateTime.parse(iso).toInstant(java.time.ZoneOffset.UTC)
        }
    } catch (_: Exception) {
        null
    }
}

private fun sensorMinutesSince(s: String?): Long? {
    val inst = parseServerInstant(s) ?: return null
    return java.time.Duration.between(inst, java.time.Instant.now()).toMinutes()
}

/** Stale (warning) when we haven't heard from the sensor in >10 min, or when
 *  the timestamp is missing/unparseable. */
private fun sensorIsStale(lastSeen: String?): Boolean {
    val m = sensorMinutesSince(lastSeen) ?: return true
    return m > SENSOR_STALE_MINUTES
}

private fun sensorRelativeTime(s: String?): String {
    val m = sensorMinutesSince(s) ?: return "unknown"
    return when {
        m < 1 -> "just now"
        m < 60 -> "${m}m ago"
        m < 1440 -> "${m / 60}h ago"
        else -> "${m / 1440}d ago"
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

/** Compute sensor status from a LiveReading without requiring a composable context. */
fun computeSensorStatus(liveReading: LiveReading?, hasRestData: Boolean): SensorStatus {
    val lastReadingMs = liveReading?.receivedAt
    return if (lastReadingMs != null) {
        val age = System.currentTimeMillis() - lastReadingMs
        when {
            age < STALE_THRESHOLD_MS -> SensorStatus.LIVE
            age < OFFLINE_THRESHOLD_MS -> SensorStatus.STALE
            else -> SensorStatus.OFFLINE
        }
    } else if (hasRestData) {
        SensorStatus.UNKNOWN
    } else {
        SensorStatus.OFFLINE
    }
}
