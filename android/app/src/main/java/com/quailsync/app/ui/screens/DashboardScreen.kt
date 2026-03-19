package com.quailsync.app.ui.screens

import androidx.compose.foundation.background
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
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material.icons.filled.Refresh
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
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.BrooderAlert
import com.quailsync.app.data.BrooderReading
import com.quailsync.app.data.LiveReading
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.WebSocketService
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

data class BrooderState(
    val brooder: Brooder,
    val readings: List<BrooderReading> = emptyList(),
    val alerts: List<BrooderAlert> = emptyList(),
)

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

    private fun loadData() {
        viewModelScope.launch {
            loadDataSuspend()
        }
    }

    private suspend fun loadDataSuspend() {
        try {
            val brooderList = api.getBrooders()
            val states = brooderList.map { brooder ->
                val readings = try {
                    api.getBrooderReadings(brooder.id)
                } catch (_: Exception) {
                    emptyList()
                }
                val alerts = try {
                    api.getBrooderAlerts(brooder.id)
                } catch (_: Exception) {
                    emptyList()
                }
                BrooderState(brooder, readings, alerts)
            }
            _brooders.value = states
        } catch (_: Exception) {
            // Keep existing data on error
        } finally {
            _isLoading.value = false
        }
    }

    override fun onCleared() {
        super.onCleared()
        webSocketService.disconnect()
    }
}

@Composable
fun DashboardScreen(viewModel: DashboardViewModel = viewModel()) {
    val brooders by viewModel.brooders.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    val liveReadings by viewModel.webSocketService.readings.collectAsState()

    DisposableEffect(Unit) {
        onDispose { }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Dashboard",
                style = MaterialTheme.typography.headlineMedium,
            )
            if (isRefreshing) {
                CircularProgressIndicator(
                    modifier = Modifier.size(24.dp),
                    strokeWidth = 2.dp,
                    color = SageGreen,
                )
            } else {
                IconButton(onClick = { viewModel.refresh() }) {
                    Icon(
                        imageVector = Icons.Default.Refresh,
                        contentDescription = "Refresh",
                    )
                }
            }
        }

        if (isLoading && brooders.isEmpty()) {
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator(color = SageGreen)
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(brooders) { state ->
                    BrooderCard(
                        state = state,
                        liveReading = liveReadings[state.brooder.id],
                    )
                }
                item { Spacer(modifier = Modifier.height(8.dp)) }
            }
        }
    }
}

@Composable
fun BrooderCard(state: BrooderState, liveReading: LiveReading?) {
    val currentTemp = liveReading?.temperature
        ?: state.readings.firstOrNull()?.temperature
    val currentHumidity = liveReading?.humidity
        ?: state.readings.firstOrNull()?.humidity

    val previousTemp = if (state.readings.size >= 2) state.readings[1].temperature else null
    val previousHumidity = if (state.readings.size >= 2) state.readings[1].humidity else null

    val hasActiveAlerts = state.alerts.any { it.acknowledged != true }
    val hasCriticalAlert = state.alerts.any {
        it.acknowledged != true && it.severity?.lowercase() == "critical"
    }

    val alertColor = when {
        hasCriticalAlert -> AlertRed
        hasActiveAlerts -> AlertYellow
        else -> AlertGreen
    }

    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = state.brooder.name,
                    style = MaterialTheme.typography.titleLarge,
                )
                Box(
                    modifier = Modifier
                        .size(12.dp)
                        .clip(CircleShape)
                        .background(alertColor),
                )
            }

            Spacer(modifier = Modifier.height(12.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                // Temperature
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = "Temperature",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            text = currentTemp?.let { "%.1f°F".format(it) } ?: "--",
                            fontSize = 24.sp,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onSurface,
                        )
                        if (currentTemp != null && previousTemp != null) {
                            Spacer(modifier = Modifier.width(4.dp))
                            TrendIcon(current = currentTemp, previous = previousTemp)
                        }
                    }
                }

                // Humidity
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = "Humidity",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            text = currentHumidity?.let { "%.0f%%".format(it) } ?: "--",
                            fontSize = 24.sp,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onSurface,
                        )
                        if (currentHumidity != null && previousHumidity != null) {
                            Spacer(modifier = Modifier.width(4.dp))
                            TrendIcon(current = currentHumidity, previous = previousHumidity)
                        }
                    }
                }
            }

            if (hasActiveAlerts) {
                Spacer(modifier = Modifier.height(8.dp))
                val alertCount = state.alerts.count { it.acknowledged != true }
                Text(
                    text = "$alertCount active alert${if (alertCount != 1) "s" else ""}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = alertColor,
                )
            }
        }
    }
}

@Composable
fun TrendIcon(current: Double, previous: Double) {
    val diff = current - previous
    val (icon, tint) = when {
        diff > 0.5 -> Icons.Default.ArrowUpward to AlertRed
        diff < -0.5 -> Icons.Default.ArrowDownward to MaterialTheme.colorScheme.primary
        else -> Icons.Default.Remove to MaterialTheme.colorScheme.onSurfaceVariant
    }
    Icon(
        imageVector = icon,
        contentDescription = "Trend",
        modifier = Modifier.size(18.dp),
        tint = tint,
    )
}
