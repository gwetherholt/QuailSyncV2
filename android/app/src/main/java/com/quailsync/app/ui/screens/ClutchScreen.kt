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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.ChickGroupDto
import com.quailsync.app.data.Clutch
import com.quailsync.app.data.QuailSyncApi
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

private const val INCUBATION_DAYS = 17L
private const val BANDING_AGE_DAYS = 28

// =====================================================================
// ViewModel
// =====================================================================

class ClutchViewModel : ViewModel() {
    private val api = QuailSyncApi.create()

    private val _clutches = MutableStateFlow<List<Clutch>>(emptyList())
    val clutches: StateFlow<List<Clutch>> = _clutches.asStateFlow()

    private val _bloodlines = MutableStateFlow<List<Bloodline>>(emptyList())
    val bloodlines: StateFlow<List<Bloodline>> = _bloodlines.asStateFlow()

    private val _chickGroups = MutableStateFlow<List<ChickGroupDto>>(emptyList())
    val chickGroups: StateFlow<List<ChickGroupDto>> = _chickGroups.asStateFlow()

    private val _brooders = MutableStateFlow<List<Brooder>>(emptyList())
    val brooders: StateFlow<List<Brooder>> = _brooders.asStateFlow()

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
            _clutches.value = api.getClutches()
            _bloodlines.value = try { api.getBloodlines() } catch (_: Exception) { emptyList() }
            _chickGroups.value = try { api.getChickGroups() } catch (_: Exception) { emptyList() }
            _brooders.value = try { api.getBrooders() } catch (_: Exception) { emptyList() }
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load hatchery data", e)
        } finally {
            _isLoading.value = false
        }
    }
}

// =====================================================================
// Hatchery Screen (combined Clutches + Chick Groups)
// =====================================================================

@Composable
fun ClutchScreen(viewModel: ClutchViewModel = viewModel()) {
    val clutches by viewModel.clutches.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val chickGroups by viewModel.chickGroups.collectAsState()
    val broodersList by viewModel.brooders.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()

    val bloodlineMap = remember(bloodlines) { bloodlines.associateBy { it.id } }
    val brooderMap = remember(broodersList) { broodersList.associateBy { it.id } }
    val clutchGroupMap = remember(chickGroups) { chickGroups.filter { it.clutchId != null }.associateBy { it.clutchId } }

    val sortedClutches = remember(clutches) {
        clutches.sortedWith(compareBy<Clutch> { clutch ->
            when (clutch.status?.lowercase()) {
                "incubating", "active", "set" -> 0
                "hatching" -> 1
                else -> 2
            }
        }.thenByDescending { it.setDate })
    }

    val activeGroups = remember(chickGroups) {
        chickGroups.filter { it.status == "Active" }.sortedByDescending { it.hatchDate }
    }
    val graduatedGroups = remember(chickGroups) {
        chickGroups.filter { it.status != "Active" }.sortedByDescending { it.hatchDate }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            Arrangement.SpaceBetween, Alignment.CenterVertically,
        ) {
            Text("Hatchery", style = MaterialTheme.typography.headlineMedium)
            if (isRefreshing) {
                CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = SageGreen)
            } else {
                IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
            }
        }

        when {
            isLoading && clutches.isEmpty() && chickGroups.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = SageGreen) }
            }
            clutches.isEmpty() && chickGroups.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(Icons.Default.Egg, null, Modifier.size(64.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.height(16.dp))
                        Text("No clutches or chick groups yet.\nAdd from the web dashboard.", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    }
                }
            }
            else -> {
                LazyColumn(
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    // --- Clutches section ---
                    if (sortedClutches.isNotEmpty()) {
                        item {
                            Text("Clutches", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        items(sortedClutches, key = { "clutch-${it.id}" }) { clutch ->
                            val group = clutchGroupMap[clutch.id]
                            val brooderName = group?.brooderId?.let { brooderMap[it]?.name }
                            ClutchCard(clutch, clutch.bloodlineName ?: bloodlineMap[clutch.bloodlineId]?.name, brooderName)
                        }
                    }

                    // --- Active Chick Groups section ---
                    if (activeGroups.isNotEmpty()) {
                        item {
                            Spacer(Modifier.height(4.dp))
                            HorizontalDivider()
                            Spacer(Modifier.height(4.dp))
                            Text("Chick Groups", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        items(activeGroups, key = { "group-${it.id}" }) { group ->
                            val bloodlineName = bloodlineMap[group.bloodlineId]?.name
                            val brooderName = group.brooderId?.let { brooderMap[it]?.name }
                            ChickGroupCard(group, bloodlineName, brooderName)
                        }
                    }

                    // --- Graduated/completed groups ---
                    if (graduatedGroups.isNotEmpty()) {
                        item {
                            Spacer(Modifier.height(4.dp))
                            HorizontalDivider()
                            Spacer(Modifier.height(4.dp))
                            Text("Completed", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        items(graduatedGroups, key = { "done-${it.id}" }) { group ->
                            val bloodlineName = bloodlineMap[group.bloodlineId]?.name
                            GraduatedGroupCard(group, bloodlineName)
                        }
                    }

                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }
}

// =====================================================================
// Clutch Card (incubation tracking)
// =====================================================================

@Composable
fun ClutchCard(clutch: Clutch, bloodlineName: String?, brooderName: String? = null) {
    val today = remember { LocalDate.now() }
    val setDate = remember(clutch.setDate) { parseDate(clutch.setDate) }
    val daysElapsed = remember(setDate, today) { setDate?.let { ChronoUnit.DAYS.between(it, today).toInt() } }
    val expectedHatchDate = remember(setDate) { setDate?.plusDays(INCUBATION_DAYS) }
    val daysUntilHatch = remember(expectedHatchDate, today) { expectedHatchDate?.let { ChronoUnit.DAYS.between(today, it).toInt() } }
    val progress = remember(daysElapsed) {
        if (daysElapsed == null) 0f else (daysElapsed.toFloat() / INCUBATION_DAYS).coerceIn(0f, 1f)
    }
    val isComplete = clutch.status?.lowercase() in listOf("hatched", "completed", "complete")
    val isHatching = daysElapsed != null && daysElapsed >= INCUBATION_DAYS && !isComplete

    Card(
        Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column {
                    Text(bloodlineName ?: "Clutch #${clutch.id}", style = MaterialTheme.typography.titleLarge)
                    if (bloodlineName != null) Text("Clutch #${clutch.id}", style = MaterialTheme.typography.bodyMedium)
                }
                ClutchStatusBadge(clutch.status, isHatching)
            }

            Spacer(Modifier.height(12.dp))

            // Egg counts
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                clutch.eggCount?.let { ClutchStat(it.toString(), "Eggs") }
                clutch.fertileCount?.let { ClutchStat(it.toString(), "Fertile", clutch.eggCount?.let { e -> "of $e" }) }
                clutch.hatchCount?.let { ClutchStat(it.toString(), "Hatched", (clutch.fertileCount ?: clutch.eggCount)?.let { e -> "of $e" }) }
            }

            if (setDate != null) {
                Spacer(Modifier.height(14.dp))

                // Progress bar
                IncubationProgressBar(progress, daysElapsed ?: 0)
                Spacer(Modifier.height(6.dp))
                MilestoneMarkers()

                Spacer(Modifier.height(10.dp))

                // Countdown
                Text(
                    when {
                        isComplete -> "Hatched"
                        isHatching -> "Hatch day!"
                        daysUntilHatch == 1 -> "1 day until hatch"
                        daysUntilHatch != null && daysUntilHatch > 0 -> "$daysUntilHatch days until hatch"
                        daysElapsed != null -> "Day $daysElapsed of $INCUBATION_DAYS"
                        else -> ""
                    },
                    style = MaterialTheme.typography.titleMedium,
                    color = when { isHatching -> AlertYellow; isComplete -> AlertGreen; else -> SageGreen },
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.fillMaxWidth(),
                    textAlign = TextAlign.Center,
                )
            }

            // Brooder assignment
            if (brooderName != null) {
                Spacer(Modifier.height(6.dp))
                Text("Brooder: $brooderName", style = MaterialTheme.typography.bodyMedium, color = SageGreen)
            } else if (isComplete) {
                Spacer(Modifier.height(6.dp))
                Text("Not assigned to a brooder", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }

            // Dates
            if (clutch.setDate != null || expectedHatchDate != null) {
                Spacer(Modifier.height(8.dp))
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                    clutch.setDate?.let { Text("Set: $it", style = MaterialTheme.typography.bodyMedium) }
                    expectedHatchDate?.let { Text("Due: ${it.format(DateTimeFormatter.ISO_LOCAL_DATE)}", style = MaterialTheme.typography.bodyMedium) }
                }
            }
        }
    }
}

// =====================================================================
// Chick Group Card (active nursery groups)
// =====================================================================

@Composable
fun ChickGroupCard(group: ChickGroupDto, bloodlineName: String?, brooderName: String?) {
    val today = remember { LocalDate.now() }
    val hatchDate = remember(group.hatchDate) { parseDate(group.hatchDate) }
    val ageDays = remember(hatchDate, today) { hatchDate?.let { ChronoUnit.DAYS.between(it, today).toInt() } ?: 0 }
    val mortalityPct = if (group.initialCount > 0) ((group.initialCount - group.currentCount).toFloat() / group.initialCount * 100) else 0f
    val canBand = ageDays >= BANDING_AGE_DAYS

    Card(
        Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(bloodlineName ?: "Group #${group.id}", style = MaterialTheme.typography.titleLarge)
                    Text("Group #${group.id}", style = MaterialTheme.typography.bodyMedium)
                }
                // Age badge
                Box(
                    Modifier.clip(RoundedCornerShape(8.dp)).background(SageGreenLight.copy(alpha = 0.3f)).padding(horizontal = 10.dp, vertical = 4.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("Day $ageDays", style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = SageGreen)
                }
            }

            Spacer(Modifier.height(12.dp))

            // Stats row
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("${group.currentCount}", fontSize = 22.sp, fontWeight = FontWeight.Bold)
                    Text("Alive", style = MaterialTheme.typography.bodyMedium)
                    Text("of ${group.initialCount}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("%.0f%%".format(mortalityPct), fontSize = 22.sp, fontWeight = FontWeight.Bold,
                        color = if (mortalityPct > 20) AlertRed else if (mortalityPct > 10) AlertYellow else AlertGreen)
                    Text("Mortality", style = MaterialTheme.typography.bodyMedium)
                }
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("${ageDays / 7 + 1}", fontSize = 22.sp, fontWeight = FontWeight.Bold)
                    Text("Week", style = MaterialTheme.typography.bodyMedium)
                }
            }

            // Brooder
            if (brooderName != null) {
                Spacer(Modifier.height(8.dp))
                Text("Brooder: $brooderName", style = MaterialTheme.typography.bodyMedium, color = SageGreen)
            }

            Spacer(Modifier.height(12.dp))

            // Action buttons
            Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                OutlinedButton(
                    onClick = { /* TODO: Log mortality dialog */ },
                    Modifier.weight(1f),
                ) {
                    Icon(Icons.Default.Pets, null, Modifier.size(16.dp))
                    Spacer(Modifier.width(4.dp))
                    Text("Log Mortality")
                }

                if (canBand) {
                    Button(
                        onClick = { /* TODO: Navigate to NFC graduation with this group */ },
                        Modifier.weight(1f),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) {
                        Icon(Icons.Default.Nfc, null, Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Band Group")
                    }
                } else {
                    OutlinedButton(
                        onClick = {},
                        Modifier.weight(1f),
                        enabled = false,
                    ) {
                        Icon(Icons.Default.Nfc, null, Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Band ($ageDays/${BANDING_AGE_DAYS}d)")
                    }
                }
            }
        }
    }
}

// =====================================================================
// Graduated Group Card (completed/lost)
// =====================================================================

@Composable
fun GraduatedGroupCard(group: ChickGroupDto, bloodlineName: String?) {
    val mortalityPct = if (group.initialCount > 0) ((group.initialCount - group.currentCount).toFloat() / group.initialCount * 100) else 0f

    Card(
        Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)),
        elevation = CardDefaults.cardElevation(0.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(14.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(bloodlineName ?: "Group #${group.id}", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("${group.currentCount}/${group.initialCount} chicks · ${group.status} · Hatched ${group.hatchDate}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (mortalityPct > 0) {
                Text("%.0f%% loss".format(mortalityPct), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

// =====================================================================
// Shared composables
// =====================================================================

@Composable
fun ClutchStatusBadge(status: String?, isHatching: Boolean) {
    val displayStatus = when { isHatching -> "Hatching!"; status != null -> status.replaceFirstChar { it.uppercase() }; else -> "Unknown" }
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
    Text(displayStatus, style = MaterialTheme.typography.labelLarge, color = textColor,
        modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(bgColor).padding(horizontal = 8.dp, vertical = 3.dp))
}

@Composable
fun ClutchStat(value: String, label: String, subtitle: String? = null) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onSurface)
        Text(label, style = MaterialTheme.typography.bodyMedium)
        if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
fun IncubationProgressBar(progress: Float, daysElapsed: Int) {
    Column {
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
            Text("Day $daysElapsed", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
            Text("of $INCUBATION_DAYS", style = MaterialTheme.typography.bodyMedium)
        }
        Spacer(Modifier.height(4.dp))
        LinearProgressIndicator(
            progress = { progress },
            modifier = Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp)),
            color = when { progress >= 1f -> AlertYellow; progress >= 0.82f -> DustyRose; else -> SageGreen },
            trackColor = MaterialTheme.colorScheme.surfaceVariant,
        )
    }
}

@Composable
fun MilestoneMarkers() {
    data class Milestone(val day: Int, val label: String)
    val milestones = listOf(Milestone(1, "Set"), Milestone(7, "Candle"), Milestone(10, "Candle"), Milestone(14, "Lockdown"), Milestone(17, "Hatch"))
    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
        milestones.forEach { m ->
            Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.width(48.dp)) {
                Box(Modifier.size(6.dp).clip(CircleShape).background(SageGreen))
                Spacer(Modifier.height(2.dp))
                Text("D${m.day}", fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                Text(m.label, fontSize = 9.sp, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
            }
        }
    }
}

private fun parseDate(dateStr: String?): LocalDate? {
    if (dateStr == null) return null
    return try {
        LocalDate.parse(dateStr, DateTimeFormatter.ISO_LOCAL_DATE)
    } catch (_: Exception) {
        try { LocalDate.parse(dateStr.take(10), DateTimeFormatter.ISO_LOCAL_DATE) } catch (_: Exception) { null }
    }
}
