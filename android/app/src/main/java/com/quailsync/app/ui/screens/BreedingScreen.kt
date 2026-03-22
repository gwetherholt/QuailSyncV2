package com.quailsync.app.ui.screens

import android.app.Application
import android.util.Log
import android.widget.Toast
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
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
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
import androidx.compose.material3.SecondaryTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.Bloodline
import com.quailsync.app.data.BreedingGroupDto
import com.quailsync.app.data.CreateBreedingGroupRequest
import com.quailsync.app.data.CullBatchRequest
import com.quailsync.app.data.CullRecommendation
import com.quailsync.app.data.InbreedingCheckResult
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

// =====================================================================
// ViewModel
// =====================================================================

class BreedingViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    private val _bloodlines = MutableStateFlow<List<Bloodline>>(emptyList())
    val bloodlines: StateFlow<List<Bloodline>> = _bloodlines.asStateFlow()

    private val _groups = MutableStateFlow<List<BreedingGroupDto>>(emptyList())
    val groups: StateFlow<List<BreedingGroupDto>> = _groups.asStateFlow()

    private val _cullRecs = MutableStateFlow<List<CullRecommendation>>(emptyList())
    val cullRecs: StateFlow<List<CullRecommendation>> = _cullRecs.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    init { loadAll() }

    fun refresh() { loadAll() }

    private fun loadAll() {
        viewModelScope.launch {
            _isLoading.value = true
            _birds.value = try { api.getBirds() } catch (_: Exception) { emptyList() }
            _bloodlines.value = try { api.getBloodlines() } catch (_: Exception) { emptyList() }
            _groups.value = try { api.getBreedingGroups() } catch (_: Exception) { emptyList() }
            _cullRecs.value = try { api.getCullRecommendations() } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load cull recs", e); emptyList()
            }
            _isLoading.value = false
        }
    }

    suspend fun cullBatch(birdIds: List<Int>, reason: String, method: String, notes: String?, date: String): Int {
        val resp = api.cullBatch(CullBatchRequest(birdIds, reason, method, notes, date))
        loadAll()
        return resp.updated
    }

    suspend fun createGroup(name: String, maleId: Int, femaleIds: List<Int>, notes: String?): BreedingGroupDto {
        val group = api.createBreedingGroup(
            CreateBreedingGroupRequest(name, maleId, femaleIds, LocalDate.now().toString(), notes)
        )
        loadAll()
        return group
    }

    suspend fun checkInbreeding(maleId: Int, femaleId: Int): InbreedingCheckResult {
        return api.checkInbreeding(maleId, femaleId)
    }
}

// =====================================================================
// Breeding Screen
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BreedingScreen(viewModel: BreedingViewModel = viewModel(), onBack: () -> Unit = {}) {
    var tabIndex by remember { mutableIntStateOf(0) }
    val tabs = listOf("Cull List", "Breeding Groups", "Pair Check")
    val isLoading by viewModel.isLoading.collectAsState()

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 8.dp),
            Arrangement.Start, Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.Default.ArrowBack, "Back") }
            Text("Breeding & Culling", style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
            IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
        }

        SecondaryTabRow(selectedTabIndex = tabIndex) {
            tabs.forEachIndexed { i, title ->
                Tab(selected = tabIndex == i, onClick = { tabIndex = i }, text = { Text(title) })
            }
        }

        if (isLoading) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = SageGreen)
            }
        } else {
            when (tabIndex) {
                0 -> CullListTab(viewModel)
                1 -> BreedingGroupsTab(viewModel)
                2 -> PairCheckTab(viewModel)
            }
        }
    }
}

// =====================================================================
// Tab 1: Cull List
// =====================================================================

@Composable
private fun CullListTab(viewModel: BreedingViewModel) {
    val cullRecs by viewModel.cullRecs.collectAsState()
    val birds by viewModel.birds.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    val selectedIds = remember { mutableStateListOf<Int>() }
    var showCullDialog by remember { mutableStateOf(false) }

    val birdMap = birds.associateBy { it.id }
    val bloodlineMap = bloodlines.associateBy { it.id }
    val today = LocalDate.now()

    Column(Modifier.fillMaxSize()) {
        if (cullRecs.isEmpty()) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(Icons.Default.Check, null, Modifier.size(48.dp), tint = AlertGreen)
                    Spacer(Modifier.height(8.dp))
                    Text("No cull recommendations", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text("Flock ratios look good!", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.weight(1f),
            ) {
                items(cullRecs, key = { it.birdId }) { rec ->
                    val bird = birdMap[rec.birdId]
                    val bloodline = bird?.bloodlineId?.let { bloodlineMap[it] }
                    val age = bird?.hatchDate?.let {
                        try {
                            ChronoUnit.DAYS.between(LocalDate.parse(it.take(10)), today).toInt()
                        } catch (_: Exception) { null }
                    }
                    val isSelected = rec.birdId in selectedIds

                    val priorityColor = when (rec.priority) {
                        "high" -> AlertRed
                        "medium" -> AlertYellow
                        else -> MaterialTheme.colorScheme.onSurfaceVariant
                    }

                    Card(
                        Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(10.dp),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                        elevation = CardDefaults.cardElevation(1.dp),
                    ) {
                        Row(
                            Modifier.padding(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(
                                checked = isSelected,
                                onCheckedChange = {
                                    if (it) selectedIds.add(rec.birdId) else selectedIds.remove(rec.birdId)
                                },
                                colors = CheckboxDefaults.colors(checkedColor = SageGreen),
                            )
                            // Priority dot
                            Box(Modifier.size(8.dp).clip(CircleShape).background(priorityColor))
                            Spacer(Modifier.width(10.dp))
                            Column(Modifier.weight(1f)) {
                                Row {
                                    Text(
                                        bird?.bandColor?.let { "[$it]" } ?: "Bird #${rec.birdId}",
                                        style = MaterialTheme.typography.bodyLarge,
                                        fontWeight = FontWeight.SemiBold,
                                    )
                                    Spacer(Modifier.width(8.dp))
                                    Text(
                                        bird?.sex ?: "",
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                Text(
                                    rec.reasonLabel,
                                    style = MaterialTheme.typography.labelMedium,
                                    color = priorityColor,
                                )
                                Row {
                                    if (bloodline != null) {
                                        Text(bloodline.name, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                        Spacer(Modifier.width(8.dp))
                                    }
                                    if (age != null) {
                                        Text("${age}d old", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    }
                                    bird?.latestWeight?.let { w ->
                                        Spacer(Modifier.width(8.dp))
                                        Text("${w.toInt()}g", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Bottom action bar
            if (selectedIds.isNotEmpty()) {
                Card(
                    Modifier.fillMaxWidth().padding(16.dp),
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    elevation = CardDefaults.cardElevation(4.dp),
                ) {
                    Row(
                        Modifier.padding(12.dp).fillMaxWidth(),
                        Arrangement.SpaceBetween, Alignment.CenterVertically,
                    ) {
                        Text("${selectedIds.size} selected", style = MaterialTheme.typography.titleMedium)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = { selectedIds.clear() }) { Text("Clear") }
                            Button(
                                onClick = { showCullDialog = true },
                                colors = ButtonDefaults.buttonColors(containerColor = AlertRed),
                            ) {
                                Icon(Icons.Default.Delete, null, Modifier.size(18.dp))
                                Spacer(Modifier.width(4.dp))
                                Text("Cull Selected")
                            }
                        }
                    }
                }
            }
        }
    }

    if (showCullDialog) {
        CullDialog(
            count = selectedIds.size,
            onConfirm = { method, notes ->
                showCullDialog = false
                val ids = selectedIds.toList()
                val reason = cullRecs.find { it.birdId == ids.firstOrNull() }?.reasonKey ?: "excess_male"
                scope.launch {
                    try {
                        val updated = viewModel.cullBatch(ids, reason, method, notes.ifBlank { null }, LocalDate.now().toString())
                        Toast.makeText(context, "Culled $updated bird${if (updated != 1) "s" else ""}", Toast.LENGTH_SHORT).show()
                        selectedIds.clear()
                    } catch (e: Exception) {
                        Toast.makeText(context, "Cull failed: ${e.message}", Toast.LENGTH_SHORT).show()
                    }
                }
            },
            onDismiss = { showCullDialog = false },
        )
    }
}

@Composable
private fun CullDialog(count: Int, onConfirm: (method: String, notes: String) -> Unit, onDismiss: () -> Unit) {
    var method by remember { mutableStateOf("Butchered") }
    var notes by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Cull $count bird${if (count != 1) "s" else ""}?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("This will update their status. This action cannot be easily undone.", style = MaterialTheme.typography.bodyMedium)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf("Butchered", "Culled").forEach { m ->
                        OutlinedButton(
                            onClick = { method = m },
                            colors = if (method == m)
                                ButtonDefaults.outlinedButtonColors(containerColor = SageGreen.copy(alpha = 0.12f), contentColor = SageGreen)
                            else ButtonDefaults.outlinedButtonColors(),
                        ) { Text(m) }
                    }
                }
                OutlinedTextField(
                    value = notes, onValueChange = { notes = it },
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
// Tab 2: Breeding Groups
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BreedingGroupsTab(viewModel: BreedingViewModel) {
    val groups by viewModel.groups.collectAsState()
    val birds by viewModel.birds.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    var showCreateDialog by remember { mutableStateOf(false) }

    val activeBirds = birds.filter { it.status?.lowercase() == "active" }
    val birdMap = birds.associateBy { it.id }
    val bloodlineMap = bloodlines.associateBy { it.id }

    // Birds already assigned to a group
    val assignedBirdIds = groups.flatMap { listOf(it.maleId) + it.femaleIds }.toSet()

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier.weight(1f),
        ) {
            if (groups.isEmpty()) {
                item {
                    Box(Modifier.fillParentMaxSize(), contentAlignment = Alignment.Center) {
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Icon(Icons.Default.Groups, null, Modifier.size(48.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            Spacer(Modifier.height(8.dp))
                            Text("No breeding groups", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text("Create one to start managing pairings", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
            items(groups, key = { it.id }) { group ->
                val male = birdMap[group.maleId]
                val females = group.femaleIds.mapNotNull { birdMap[it] }

                Card(
                    Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    elevation = CardDefaults.cardElevation(2.dp),
                ) {
                    Column(Modifier.padding(14.dp)) {
                        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                            Text(group.name, style = MaterialTheme.typography.titleMedium)
                            Text(
                                "1M : ${females.size}F",
                                style = MaterialTheme.typography.labelMedium,
                                color = if (females.size in 3..5) AlertGreen else AlertYellow,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                        Spacer(Modifier.height(8.dp))
                        // Male
                        Row {
                            Text("Male: ", style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.SemiBold)
                            Text(
                                male?.let { "${it.bandColor ?: "Bird"} #${it.id}" } ?: "#${group.maleId}",
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            male?.bloodlineId?.let { bid ->
                                bloodlineMap[bid]?.let { bl ->
                                    Text(" (${bl.name})", style = MaterialTheme.typography.labelSmall, color = SageGreen)
                                }
                            }
                        }
                        // Females
                        females.forEach { f ->
                            Row {
                                Text("  F: ", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                Text(
                                    "${f.bandColor ?: "Bird"} #${f.id}",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                f.bloodlineId?.let { bid ->
                                    bloodlineMap[bid]?.let { bl ->
                                        Text(" (${bl.name})", style = MaterialTheme.typography.labelSmall, color = SageGreen)
                                    }
                                }
                            }
                        }
                        if (group.notes != null) {
                            Spacer(Modifier.height(4.dp))
                            Text(group.notes, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
            item { Spacer(Modifier.height(8.dp)) }
        }

        // Create button
        Button(
            onClick = { showCreateDialog = true },
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) { Text("Create Breeding Group") }
    }

    if (showCreateDialog) {
        CreateBreedingGroupDialog(
            males = activeBirds.filter { it.sex?.lowercase() == "male" && it.id !in assignedBirdIds },
            females = activeBirds.filter { it.sex?.lowercase() == "female" && it.id !in assignedBirdIds },
            bloodlineMap = bloodlineMap,
            onConfirm = { name, maleId, femaleIds, notes ->
                showCreateDialog = false
                scope.launch {
                    try {
                        viewModel.createGroup(name, maleId, femaleIds, notes.ifBlank { null })
                        Toast.makeText(context, "Group created", Toast.LENGTH_SHORT).show()
                    } catch (e: Exception) {
                        Toast.makeText(context, "Failed: ${e.message}", Toast.LENGTH_SHORT).show()
                    }
                }
            },
            onDismiss = { showCreateDialog = false },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CreateBreedingGroupDialog(
    males: List<Bird>,
    females: List<Bird>,
    bloodlineMap: Map<Int, Bloodline>,
    onConfirm: (name: String, maleId: Int, femaleIds: List<Int>, notes: String) -> Unit,
    onDismiss: () -> Unit,
) {
    var name by remember { mutableStateOf("") }
    var selectedMaleId by remember { mutableStateOf<Int?>(null) }
    val selectedFemaleIds = remember { mutableStateListOf<Int>() }
    var notes by remember { mutableStateOf("") }
    var maleExpanded by remember { mutableStateOf(false) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Create Breeding Group") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = name, onValueChange = { name = it },
                    label = { Text("Group name") }, modifier = Modifier.fillMaxWidth(), singleLine = true,
                )

                // Male selection
                ExposedDropdownMenuBox(maleExpanded, { maleExpanded = it }) {
                    OutlinedTextField(
                        value = selectedMaleId?.let { id -> males.find { it.id == id }?.let { b -> "${b.bandColor ?: "Bird"} #${b.id}" } } ?: "",
                        onValueChange = {}, readOnly = true,
                        label = { Text("Male (1)") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(maleExpanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(maleExpanded, { maleExpanded = false }) {
                        males.forEach { m ->
                            val bl = m.bloodlineId?.let { bloodlineMap[it]?.name } ?: ""
                            DropdownMenuItem(
                                text = { Text("${m.bandColor ?: "Bird"} #${m.id} $bl") },
                                onClick = { selectedMaleId = m.id; maleExpanded = false },
                            )
                        }
                    }
                }

                // Female multi-select
                Text("Females (select 3-5):", style = MaterialTheme.typography.labelMedium)
                Column {
                    females.take(20).forEach { f ->
                        val checked = f.id in selectedFemaleIds
                        val bl = f.bloodlineId?.let { bloodlineMap[it]?.name } ?: ""
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(
                                checked = checked,
                                onCheckedChange = { if (it) selectedFemaleIds.add(f.id) else selectedFemaleIds.remove(f.id) },
                                colors = CheckboxDefaults.colors(checkedColor = SageGreen),
                            )
                            Text("${f.bandColor ?: "Bird"} #${f.id} $bl", style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }

                OutlinedTextField(
                    value = notes, onValueChange = { notes = it },
                    label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth(), maxLines = 2,
                )
            }
        },
        confirmButton = {
            Button(
                onClick = { selectedMaleId?.let { onConfirm(name, it, selectedFemaleIds.toList(), notes) } },
                enabled = name.isNotBlank() && selectedMaleId != null && selectedFemaleIds.size in 3..5,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text("Create") }
        },
        dismissButton = { OutlinedButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Tab 3: Pair Check
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PairCheckTab(viewModel: BreedingViewModel) {
    val birds by viewModel.birds.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val scope = rememberCoroutineScope()

    val activeBirds = birds.filter { it.status?.lowercase() == "active" }
    val males = activeBirds.filter { it.sex?.lowercase() == "male" }
    val females = activeBirds.filter { it.sex?.lowercase() == "female" }
    val bloodlineMap = bloodlines.associateBy { it.id }

    var selectedMaleId by remember { mutableStateOf<Int?>(null) }
    var selectedFemaleId by remember { mutableStateOf<Int?>(null) }
    var maleExpanded by remember { mutableStateOf(false) }
    var femaleExpanded by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<InbreedingCheckResult?>(null) }
    var checking by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Column(
        Modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Check if a male-female pairing is genetically safe.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)

        // Male dropdown
        ExposedDropdownMenuBox(maleExpanded, { maleExpanded = it }) {
            OutlinedTextField(
                value = selectedMaleId?.let { id ->
                    males.find { it.id == id }?.let { b ->
                        val bl = b.bloodlineId?.let { bloodlineMap[it]?.name } ?: ""
                        "${b.bandColor ?: "Bird"} #${b.id} $bl"
                    }
                } ?: "",
                onValueChange = {}, readOnly = true,
                label = { Text("Select Male") },
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(maleExpanded) },
                modifier = Modifier.menuAnchor().fillMaxWidth(),
            )
            ExposedDropdownMenu(maleExpanded, { maleExpanded = false }) {
                males.forEach { m ->
                    val bl = m.bloodlineId?.let { bloodlineMap[it]?.name } ?: ""
                    DropdownMenuItem(
                        text = { Text("${m.bandColor ?: "Bird"} #${m.id} $bl") },
                        onClick = { selectedMaleId = m.id; maleExpanded = false; result = null },
                    )
                }
            }
        }

        // Female dropdown
        ExposedDropdownMenuBox(femaleExpanded, { femaleExpanded = it }) {
            OutlinedTextField(
                value = selectedFemaleId?.let { id ->
                    females.find { it.id == id }?.let { b ->
                        val bl = b.bloodlineId?.let { bloodlineMap[it]?.name } ?: ""
                        "${b.bandColor ?: "Bird"} #${b.id} $bl"
                    }
                } ?: "",
                onValueChange = {}, readOnly = true,
                label = { Text("Select Female") },
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(femaleExpanded) },
                modifier = Modifier.menuAnchor().fillMaxWidth(),
            )
            ExposedDropdownMenu(femaleExpanded, { femaleExpanded = false }) {
                females.forEach { f ->
                    val bl = f.bloodlineId?.let { bloodlineMap[it]?.name } ?: ""
                    DropdownMenuItem(
                        text = { Text("${f.bandColor ?: "Bird"} #${f.id} $bl") },
                        onClick = { selectedFemaleId = f.id; femaleExpanded = false; result = null },
                    )
                }
            }
        }

        Button(
            onClick = {
                val mid = selectedMaleId ?: return@Button
                val fid = selectedFemaleId ?: return@Button
                checking = true; error = null; result = null
                scope.launch {
                    try {
                        result = viewModel.checkInbreeding(mid, fid)
                    } catch (e: Exception) {
                        error = e.message ?: "Check failed"
                    }
                    checking = false
                }
            },
            enabled = selectedMaleId != null && selectedFemaleId != null && !checking,
            modifier = Modifier.fillMaxWidth(),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) {
            if (checking) {
                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = Color.White)
                Spacer(Modifier.width(8.dp))
            }
            Text("Check Compatibility")
        }

        // Result card
        result?.let { r ->
            val color = when {
                r.coefficient < 0.03125 -> AlertGreen    // < 3.1% — very safe
                r.coefficient < 0.0625 -> AlertYellow     // < 6.25% — caution
                else -> AlertRed                           // >= 6.25% — unsafe
            }
            val label = when {
                r.coefficient < 0.03125 -> "Safe — Unrelated"
                r.coefficient < 0.0625 -> "Caution — Distant Relation"
                else -> "Unsafe — High Inbreeding Risk"
            }

            Card(
                Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(containerColor = color.copy(alpha = 0.1f)),
                elevation = CardDefaults.cardElevation(0.dp),
            ) {
                Column(Modifier.padding(16.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(16.dp).clip(CircleShape).background(color))
                        Spacer(Modifier.width(10.dp))
                        Text(label, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, color = color)
                    }
                    Spacer(Modifier.height(8.dp))
                    Text(
                        "Inbreeding coefficient: %.1f%%".format(r.coefficient * 100),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    if (!r.warning.isNullOrBlank()) {
                        Spacer(Modifier.height(4.dp))
                        Text(r.warning, style = MaterialTheme.typography.bodySmall, color = AlertRed)
                    }
                }
            }
        }

        error?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium, color = AlertRed)
        }
    }
}
