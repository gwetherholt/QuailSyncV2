package com.quailsync.app.ui.screens

import android.app.Application
import android.util.Log
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.SystemAlertDto
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.time.Duration
import java.time.Instant
import java.time.format.DateTimeParseException

class AlertsViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

    private val _active = MutableStateFlow<List<SystemAlertDto>>(emptyList())
    val active: StateFlow<List<SystemAlertDto>> = _active.asStateFlow()

    private val _recent = MutableStateFlow<List<SystemAlertDto>>(emptyList())
    val recent: StateFlow<List<SystemAlertDto>> = _recent.asStateFlow()

    private val _refreshingActive = MutableStateFlow(false)
    val refreshingActive: StateFlow<Boolean> = _refreshingActive.asStateFlow()

    private val _refreshingRecent = MutableStateFlow(false)
    val refreshingRecent: StateFlow<Boolean> = _refreshingRecent.asStateFlow()

    init {
        refreshActive()
        refreshRecent()
    }

    fun refreshActive() {
        viewModelScope.launch {
            _refreshingActive.value = true
            try {
                _active.value = api.getActiveAlerts()
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load active alerts", e)
            } finally {
                _refreshingActive.value = false
            }
        }
    }

    fun refreshRecent() {
        viewModelScope.launch {
            _refreshingRecent.value = true
            try {
                _recent.value = api.getRecentAlerts(limit = 50)
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load recent alerts", e)
            } finally {
                _refreshingRecent.value = false
            }
        }
    }

    fun dismiss(alertId: Long) {
        viewModelScope.launch {
            try {
                api.dismissAlert(alertId)
                refreshActive()
                refreshRecent()
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to dismiss alert $alertId", e)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AlertsScreen(
    onBack: () -> Unit,
    viewModel: AlertsViewModel = viewModel(),
) {
    val active by viewModel.active.collectAsState()
    val recent by viewModel.recent.collectAsState()
    val refreshingActive by viewModel.refreshingActive.collectAsState()
    val refreshingRecent by viewModel.refreshingRecent.collectAsState()

    var selectedTab by rememberSaveable { mutableIntStateOf(0) }
    val tabs = listOf("Active" to active.size, "History" to recent.size)

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
            }
            Text(
                "System Alerts",
                style = MaterialTheme.typography.headlineMedium,
                modifier = Modifier.padding(start = 4.dp),
            )
        }

        TabRow(selectedTabIndex = selectedTab) {
            tabs.forEachIndexed { index, (label, count) ->
                Tab(
                    selected = selectedTab == index,
                    onClick = { selectedTab = index },
                    text = {
                        if (index == 0 && count > 0) {
                            Text("$label ($count)", fontWeight = FontWeight.SemiBold)
                        } else {
                            Text(label)
                        }
                    },
                )
            }
        }

        Box(Modifier.fillMaxSize()) {
            when (selectedTab) {
                0 -> AlertList(
                    alerts = active,
                    emptyMessage = "No active alerts — the farm is happy.",
                    onDismiss = { viewModel.dismiss(it.id) },
                )
                1 -> AlertList(
                    alerts = recent,
                    emptyMessage = "No alerts yet.",
                    onDismiss = null,
                )
            }
            val refreshing = if (selectedTab == 0) refreshingActive else refreshingRecent
            Row(
                Modifier
                    .align(Alignment.TopEnd)
                    .padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (refreshing) {
                    CircularProgressIndicator(
                        Modifier.size(20.dp),
                        strokeWidth = 2.dp,
                        color = SageGreen,
                    )
                } else {
                    IconButton(onClick = {
                        if (selectedTab == 0) viewModel.refreshActive() else viewModel.refreshRecent()
                    }) {
                        Icon(Icons.Default.Refresh, contentDescription = "Refresh")
                    }
                }
            }
        }
    }
}

@Composable
private fun AlertList(
    alerts: List<SystemAlertDto>,
    emptyMessage: String,
    onDismiss: ((SystemAlertDto) -> Unit)?,
) {
    if (alerts.isEmpty()) {
        Box(
            Modifier.fillMaxSize().padding(32.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                emptyMessage,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyLarge,
            )
        }
        return
    }

    LazyColumn(
        contentPadding = PaddingValues(12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(alerts, key = { it.id }) { alert ->
            AlertCard(alert = alert, onDismiss = onDismiss)
        }
    }
}

@Composable
private fun AlertCard(
    alert: SystemAlertDto,
    onDismiss: ((SystemAlertDto) -> Unit)?,
) {
    val severityColor = severityColor(alert.severity)
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .size(10.dp)
                        .background(severityColor, shape = androidx.compose.foundation.shape.CircleShape),
                )
                Spacer(Modifier.size(8.dp))
                Text(
                    alert.title,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.weight(1f),
                )
                StatusGlyph(alert)
            }
            Spacer(Modifier.height(4.dp))
            Text(
                relativeTime(alert.createdAt),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                alert.message,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(4.dp))
            Text(
                "source: ${alert.source}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (alert.isActive && onDismiss != null) {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End,
                ) {
                    TextButton(onClick = { onDismiss(alert) }) {
                        Text("Dismiss", color = SageGreen)
                    }
                }
            }
        }
    }
}

@Composable
private fun StatusGlyph(alert: SystemAlertDto) {
    when {
        alert.resolvedAt != null -> Icon(
            Icons.Default.CheckCircle,
            contentDescription = "Resolved",
            tint = AlertGreen,
            modifier = Modifier.size(20.dp),
        )
        alert.dismissedAt != null -> Icon(
            Icons.Default.Cancel,
            contentDescription = "Dismissed",
            tint = Color.Gray,
            modifier = Modifier.size(20.dp),
        )
        else -> {}
    }
}

private fun severityColor(severity: String): Color = when (severity.lowercase()) {
    "critical" -> AlertRed
    "warning" -> AlertYellow
    else -> Color.Gray
}

private fun relativeTime(iso: String): String {
    return try {
        val then = Instant.parse(iso)
        val now = Instant.now()
        val seconds = Duration.between(then, now).seconds
        when {
            seconds < 60 -> "just now"
            seconds < 3600 -> "${seconds / 60} min ago"
            seconds < 86_400 -> "${seconds / 3600} hour${if (seconds / 3600 == 1L) "" else "s"} ago"
            seconds < 604_800 -> "${seconds / 86_400} day${if (seconds / 86_400 == 1L) "" else "s"} ago"
            else -> iso.substringBefore('T')
        }
    } catch (_: DateTimeParseException) {
        iso
    } catch (_: Exception) {
        iso
    }
}
