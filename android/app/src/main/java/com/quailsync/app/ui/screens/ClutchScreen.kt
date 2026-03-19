package com.quailsync.app.ui.screens

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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bloodline
import com.quailsync.app.data.Clutch
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.ui.theme.AlertGreen
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

private const val INCUBATION_DAYS = 17L

class ClutchViewModel : ViewModel() {
    private val api = QuailSyncApi.create()

    private val _clutches = MutableStateFlow<List<Clutch>>(emptyList())
    val clutches: StateFlow<List<Clutch>> = _clutches.asStateFlow()

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
            val clutchList = api.getClutches()
            Log.d("QuailSync", "Clutches loaded: ${clutchList.size}")
            _clutches.value = clutchList

            val bloodlineList = try {
                api.getBloodlines()
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load bloodlines", e)
                emptyList()
            }
            _bloodlines.value = bloodlineList
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load clutches", e)
        } finally {
            _isLoading.value = false
        }
    }
}

@Composable
fun ClutchScreen(viewModel: ClutchViewModel = viewModel()) {
    val clutches by viewModel.clutches.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()

    val bloodlineMap = remember(bloodlines) { bloodlines.associateBy { it.id } }

    val sortedClutches = remember(clutches) {
        clutches.sortedWith(compareBy<Clutch> { clutch ->
            val status = clutch.status?.lowercase()
            when {
                status == "incubating" || status == "active" || status == "set" -> 0
                status == "hatching" -> 1
                else -> 2
            }
        }.thenByDescending { it.setDate })
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
                text = "Clutches",
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

        when {
            isLoading && clutches.isEmpty() -> {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator(color = SageGreen)
                }
            }
            clutches.isEmpty() -> {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(
                            imageVector = Icons.Default.Egg,
                            contentDescription = null,
                            modifier = Modifier.size(64.dp),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(modifier = Modifier.height(16.dp))
                        Text(
                            text = "No clutches tracked yet.\nAdd clutches from the web dashboard or CLI.",
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                        )
                    }
                }
            }
            else -> {
                LazyColumn(
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    items(sortedClutches, key = { it.id }) { clutch ->
                        ClutchCard(
                            clutch = clutch,
                            bloodlineName = clutch.bloodlineName
                                ?: bloodlineMap[clutch.bloodlineId]?.name,
                        )
                    }
                    item { Spacer(modifier = Modifier.height(8.dp)) }
                }
            }
        }
    }
}

@Composable
fun ClutchCard(clutch: Clutch, bloodlineName: String?) {
    val today = remember { LocalDate.now() }
    val setDate = remember(clutch.setDate) { parseDate(clutch.setDate) }
    val daysElapsed = remember(setDate, today) {
        setDate?.let { ChronoUnit.DAYS.between(it, today).toInt() }
    }
    val expectedHatchDate = remember(setDate) {
        setDate?.plusDays(INCUBATION_DAYS)
    }
    val daysUntilHatch = remember(expectedHatchDate, today) {
        expectedHatchDate?.let { ChronoUnit.DAYS.between(today, it).toInt() }
    }
    val progress = remember(daysElapsed) {
        if (daysElapsed == null) 0f
        else (daysElapsed.toFloat() / INCUBATION_DAYS).coerceIn(0f, 1f)
    }

    val isComplete = clutch.status?.lowercase() in listOf("hatched", "completed", "complete")
    val isHatching = daysElapsed != null && daysElapsed >= INCUBATION_DAYS && !isComplete

    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            // Header row
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text(
                        text = if (bloodlineName != null) bloodlineName
                            else "Clutch #${clutch.id}",
                        style = MaterialTheme.typography.titleLarge,
                    )
                    if (bloodlineName != null) {
                        Text(
                            text = "Clutch #${clutch.id}",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
                ClutchStatusBadge(clutch.status, isHatching)
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Egg counts row
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
            ) {
                if (clutch.eggCount != null) {
                    ClutchStat(value = clutch.eggCount.toString(), label = "Eggs")
                }
                if (clutch.fertileCount != null) {
                    ClutchStat(
                        value = clutch.fertileCount.toString(),
                        label = "Fertile",
                        subtitle = clutch.eggCount?.let { "of $it" },
                    )
                }
                if (clutch.hatchCount != null) {
                    ClutchStat(
                        value = clutch.hatchCount.toString(),
                        label = "Hatched",
                        subtitle = (clutch.fertileCount ?: clutch.eggCount)?.let { "of $it" },
                    )
                }
            }

            if (setDate != null) {
                Spacer(modifier = Modifier.height(14.dp))

                // Progress bar
                IncubationProgressBar(progress = progress, daysElapsed = daysElapsed ?: 0)

                Spacer(modifier = Modifier.height(6.dp))

                // Milestone markers
                MilestoneMarkers()

                Spacer(modifier = Modifier.height(10.dp))

                // Countdown / status text
                Text(
                    text = when {
                        isComplete -> "Hatched"
                        isHatching -> "Hatch day!"
                        daysUntilHatch != null && daysUntilHatch == 1 -> "1 day until hatch"
                        daysUntilHatch != null && daysUntilHatch > 0 ->
                            "$daysUntilHatch days until hatch"
                        daysElapsed != null -> "Day $daysElapsed of $INCUBATION_DAYS"
                        else -> ""
                    },
                    style = MaterialTheme.typography.titleMedium,
                    color = when {
                        isHatching -> AlertYellow
                        isComplete -> AlertGreen
                        else -> SageGreen
                    },
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.fillMaxWidth(),
                    textAlign = TextAlign.Center,
                )
            }

            // Dates row
            if (clutch.setDate != null || expectedHatchDate != null) {
                Spacer(modifier = Modifier.height(8.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    if (clutch.setDate != null) {
                        Text(
                            text = "Set: ${clutch.setDate}",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                    if (expectedHatchDate != null) {
                        Text(
                            text = "Due: ${expectedHatchDate.format(DateTimeFormatter.ISO_LOCAL_DATE)}",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun ClutchStatusBadge(status: String?, isHatching: Boolean) {
    val displayStatus = when {
        isHatching -> "Hatching!"
        status != null -> status.replaceFirstChar { it.uppercase() }
        else -> "Unknown"
    }
    val bgColor = when {
        isHatching -> AlertYellow
        status?.lowercase() in listOf("hatched", "completed", "complete") -> AlertGreen
        status?.lowercase() in listOf("incubating", "active", "set") -> SageGreenLight
        else -> MaterialTheme.colorScheme.surfaceVariant
    }
    val textColor = when {
        isHatching -> Color(0xFF4A3D00)
        status?.lowercase() in listOf("hatched", "completed", "complete") -> Color(0xFF1B3A14)
        status?.lowercase() in listOf("incubating", "active", "set") -> Color(0xFF2D4A1E)
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    Text(
        text = displayStatus,
        style = MaterialTheme.typography.labelLarge,
        color = textColor,
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .background(bgColor)
            .padding(horizontal = 8.dp, vertical = 3.dp),
    )
}

@Composable
fun ClutchStat(value: String, label: String, subtitle: String? = null) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(
            text = value,
            fontSize = 22.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
        )
        if (subtitle != null) {
            Text(
                text = subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
fun IncubationProgressBar(progress: Float, daysElapsed: Int) {
    Column {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "Day $daysElapsed",
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Medium,
            )
            Text(
                text = "of $INCUBATION_DAYS",
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        Spacer(modifier = Modifier.height(4.dp))
        LinearProgressIndicator(
            progress = { progress },
            modifier = Modifier
                .fillMaxWidth()
                .height(10.dp)
                .clip(RoundedCornerShape(5.dp)),
            color = when {
                progress >= 1f -> AlertYellow
                progress >= 0.82f -> DustyRose // Day 14+ lockdown
                else -> SageGreen
            },
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
        )
    }
}

@Composable
fun MilestoneMarkers() {
    // Milestones: Day 1, 7, 10, 14, 17
    data class Milestone(val day: Int, val label: String)
    val milestones = listOf(
        Milestone(1, "Set"),
        Milestone(7, "Candle"),
        Milestone(10, "Candle"),
        Milestone(14, "Lockdown"),
        Milestone(17, "Hatch"),
    )

    Box(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            milestones.forEach { milestone ->
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    modifier = Modifier.width(48.dp),
                ) {
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .clip(CircleShape)
                            .background(SageGreen),
                    )
                    Spacer(modifier = Modifier.height(2.dp))
                    Text(
                        text = "D${milestone.day}",
                        fontSize = 10.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    Text(
                        text = milestone.label,
                        fontSize = 9.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
}

private fun parseDate(dateStr: String?): LocalDate? {
    if (dateStr == null) return null
    return try {
        LocalDate.parse(dateStr, DateTimeFormatter.ISO_LOCAL_DATE)
    } catch (_: Exception) {
        try {
            LocalDate.parse(dateStr.take(10), DateTimeFormatter.ISO_LOCAL_DATE)
        } catch (_: Exception) {
            null
        }
    }
}
