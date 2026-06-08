package com.quailsync.app.ui.screens

import android.app.Application
import android.widget.Toast
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
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Nfc
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
import androidx.compose.ui.unit.dp
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.BreedingGroupDto
import com.quailsync.app.data.CreateBreedingGroupRequest
import com.quailsync.app.data.CullBatchRequest
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

// =====================================================================
// ViewModel
// =====================================================================

class BreedingViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    private val _groups = MutableStateFlow<List<BreedingGroupDto>>(emptyList())
    val groups: StateFlow<List<BreedingGroupDto>> = _groups.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    init { loadAll() }

    fun refresh() { loadAll() }

    private fun loadAll() {
        viewModelScope.launch {
            _isLoading.value = true
            _birds.value = try { api.getBirds() } catch (_: Exception) { emptyList() }
            _groups.value = try { api.getBreedingGroups() } catch (_: Exception) { emptyList() }
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
fun BreedingScreen(
    viewModel: BreedingViewModel = viewModel(),
    onBack: () -> Unit = {},
    initialTab: Int = 0,
    onReconcileGroup: (Int) -> Unit = {},
) {
    var tabIndex by remember { mutableIntStateOf(initialTab.coerceIn(0, 1)) }
    // Cull List was removed — the Flock screen's cull-mode toggle now owns
    // the cull workflow. Deep-links to the old `?tab=0` (Cull List) now land
    // on Breeding Groups; that's intentional, not a redirect bug.
    val tabs = listOf("Breeding Groups", "Pair Check")
    val isLoading by viewModel.isLoading.collectAsState()

    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 8.dp),
            Arrangement.Start, Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
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
                0 -> BreedingGroupsTab(viewModel, onReconcileGroup)
                1 -> PairCheckTab(viewModel)
            }
        }
    }
}

// The former "Tab 1: Cull List" lived here. Removed when the cull workflow
// moved to the Flock screen's Cull Mode toggle — see FlockScreen for the
// guardrail-based selection UI.

// =====================================================================
// Tab 1: Breeding Groups
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BreedingGroupsTab(viewModel: BreedingViewModel, onReconcileGroup: (Int) -> Unit = {}) {
    val groups by viewModel.groups.collectAsState()
    val birds by viewModel.birds.collectAsState()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    // Explicit MutableState (not `by` delegate) so writes are setter calls;
    // the Kotlin flow-analyser otherwise flags the lambda assignments below as
    // `UNUSED_VALUE` (it can't see Compose's recomposition-time reads).
    val showCreateDialog = remember { mutableStateOf(false) }

    val activeBirds = birds.filter { it.status?.lowercase() == "active" }
    val birdMap = birds.associateBy { it.id }

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
                            if (male != null && male.lineages.isNotEmpty()) {
                                Text(" (${com.quailsync.app.data.formatLineages(male.lineages)})", style = MaterialTheme.typography.labelSmall, color = SageGreen)
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
                                if (f.lineages.isNotEmpty()) {
                                    Text(" (${com.quailsync.app.data.formatLineages(f.lineages)})", style = MaterialTheme.typography.labelSmall, color = SageGreen)
                                }
                            }
                        }
                        if (group.notes != null) {
                            Spacer(Modifier.height(4.dp))
                            Text(group.notes, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        Spacer(Modifier.height(8.dp))
                        // Found a band on the hutch floor? Deduce whose it is.
                        TextButton(
                            onClick = { onReconcileGroup(group.id) },
                            modifier = Modifier.align(Alignment.End),
                        ) {
                            Icon(Icons.Default.Nfc, null, Modifier.size(18.dp), tint = SageGreen)
                            Spacer(Modifier.width(4.dp))
                            Text("Found a dropped band?", color = SageGreen, style = MaterialTheme.typography.labelMedium)
                        }
                    }
                }
            }
            item { Spacer(Modifier.height(8.dp)) }
        }

        // Create button
        Button(
            onClick = { showCreateDialog.value = true },
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) { Text("Create Breeding Group") }
    }

    if (showCreateDialog.value) {
        CreateBreedingGroupDialog(
            males = activeBirds.filter { it.sex?.lowercase() == "male" && it.id !in assignedBirdIds },
            females = activeBirds.filter { it.sex?.lowercase() == "female" && it.id !in assignedBirdIds },
            onConfirm = { name, maleId, femaleIds, notes ->
                showCreateDialog.value = false
                scope.launch {
                    try {
                        viewModel.createGroup(name, maleId, femaleIds, notes.ifBlank { null })
                        Toast.makeText(context, "Group created", Toast.LENGTH_SHORT).show()
                    } catch (e: Exception) {
                        Toast.makeText(context, "Failed: ${e.message}", Toast.LENGTH_SHORT).show()
                    }
                }
            },
            onDismiss = { showCreateDialog.value = false },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun CreateBreedingGroupDialog(
    males: List<Bird>,
    females: List<Bird>,
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
                            val bl = com.quailsync.app.data.formatLineages(m.lineages, emptyText = "")
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
                        val bl = com.quailsync.app.data.formatLineages(f.lineages, emptyText = "")
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
// Tab 2: Pair Check
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PairCheckTab(viewModel: BreedingViewModel) {
    val birds by viewModel.birds.collectAsState()
    val scope = rememberCoroutineScope()

    val activeBirds = birds.filter { it.status?.lowercase() == "active" }
    val males = activeBirds.filter { it.sex?.lowercase() == "male" }
    val females = activeBirds.filter { it.sex?.lowercase() == "female" }

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
                        val bl = com.quailsync.app.data.formatLineages(b.lineages, emptyText = "")
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
                    val bl = com.quailsync.app.data.formatLineages(m.lineages, emptyText = "")
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
                        val bl = com.quailsync.app.data.formatLineages(b.lineages, emptyText = "")
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
                    val bl = com.quailsync.app.data.formatLineages(f.lineages, emptyText = "")
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
