@file:Suppress(
    "ASSIGNED_BUT_NEVER_ACCESSED_VARIABLE",
    "UNUSED_VALUE",
    "CanBeVal",
    "UnusedVariable"
)
@file:OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)

package com.quailsync.app.ui.screens

import android.util.Log
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.material3.FilterChip
import androidx.compose.runtime.mutableStateListOf
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
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Lineage
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.ChickGroupDto
import com.quailsync.app.data.Clutch
import com.quailsync.app.data.CreateChickGroupRequest
import com.quailsync.app.data.CreateLineageRequest
import com.quailsync.app.data.CreateClutchRequest
import com.quailsync.app.data.MortalityRequest
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.UpdateClutchRequest
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.DustyRose
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

private const val INCUBATION_DAYS = 17L

// =====================================================================
// ViewModel
// =====================================================================

class ClutchViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

    private val _clutches = MutableStateFlow<List<Clutch>>(emptyList())
    val clutches: StateFlow<List<Clutch>> = _clutches.asStateFlow()
    private val _lineages = MutableStateFlow<List<Lineage>>(emptyList())
    val lineages: StateFlow<List<Lineage>> = _lineages.asStateFlow()
    private val _chickGroups = MutableStateFlow<List<ChickGroupDto>>(emptyList())
    val chickGroups: StateFlow<List<ChickGroupDto>> = _chickGroups.asStateFlow()
    private val _brooders = MutableStateFlow<List<Brooder>>(emptyList())
    val brooders: StateFlow<List<Brooder>> = _brooders.asStateFlow()
    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()
    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    init { loadData() }

    fun refresh() { viewModelScope.launch { _isRefreshing.value = true; loadDataSuspend(); _isRefreshing.value = false } }
    private fun loadData() { viewModelScope.launch { loadDataSuspend() } }

    private suspend fun loadDataSuspend() {
        try {
            _clutches.value = api.getClutches()
            _lineages.value = try { api.getLineages() } catch (_: Exception) { emptyList() }
            _chickGroups.value = try { api.getChickGroups() } catch (_: Exception) { emptyList() }
            _brooders.value = try { api.getBrooders() } catch (_: Exception) { emptyList() }
        } catch (e: Exception) { Log.e("QuailSync", "Failed to load hatchery data", e) }
        finally { _isLoading.value = false }
    }

    suspend fun createClutch(request: CreateClutchRequest): Boolean {
        return try { api.createClutch(request); true } catch (e: Exception) { Log.e("QuailSync", "Create clutch failed", e); false }
    }

    suspend fun updateClutch(id: Int, request: UpdateClutchRequest): Boolean {
        return try { api.updateClutch(id, request); true } catch (e: Exception) { Log.e("QuailSync", "Update clutch failed", e); false }
    }

    suspend fun createChickGroup(request: CreateChickGroupRequest): Boolean {
        return try { api.createChickGroup(request); true } catch (e: Exception) { Log.e("QuailSync", "Create chick group failed", e); false }
    }

    fun deleteClutchById(id: Int, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try { api.deleteClutch(id); true } catch (e: Exception) { Log.e("QuailSync", "Delete clutch failed", e); false }
            onResult(ok)
        }
    }

    suspend fun logMortality(groupId: Int, count: Int, reason: String): Boolean {
        return try {
            api.logMortality(groupId, MortalityRequest(count, reason))
            loadDataSuspend()
            true
        } catch (e: Exception) { Log.e("QuailSync", "Log mortality failed", e); false }
    }

    fun deleteChickGroupById(id: Int, onResult: (Boolean) -> Unit) {
        viewModelScope.launch {
            val ok = try { api.deleteChickGroup(id); true } catch (e: Exception) { Log.e("QuailSync", "Delete chick group failed", e); false }
            onResult(ok)
        }
    }

    suspend fun createLineage(request: CreateLineageRequest): Lineage? {
        return try {
            val bl = api.createLineage(request)
            // Refresh lineages list so dropdown updates
            _lineages.value = try { api.getLineages() } catch (_: Exception) { _lineages.value }
            bl
        } catch (e: Exception) { Log.e("QuailSync", "Create lineage failed", e); null }
    }
}

// =====================================================================
// Hatchery Screen
// =====================================================================

@Composable
fun ClutchScreen(
    viewModel: ClutchViewModel = viewModel(),
    onBandGroup: (ChickGroupDto) -> Unit = {},
) {
    val clutches by viewModel.clutches.collectAsState()
    val lineages by viewModel.lineages.collectAsState()
    val chickGroups by viewModel.chickGroups.collectAsState()
    val broodersList by viewModel.brooders.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()

    val lineageMap = remember(lineages) { lineages.associateBy { it.id } }
    val brooderMap = remember(broodersList) { broodersList.associateBy { it.id } }
    val clutchGroupMap = remember(chickGroups) { chickGroups.filter { it.clutchId != null }.associateBy { it.clutchId } }

    // Split clutches into "still relevant" (incubating, hatching, or hatched
    // within the last 14 days) and "old hatched" (>14 days since hatch). The
    // recently-hatched ones stay in the main list so users can review them
    // briefly after hatch day; the rest collapse into a "Completed Clutches"
    // expandable so the active list stays focused.
    val today = remember { LocalDate.now() }
    val clutchPartition = remember(clutches, today) {
        val active = mutableListOf<Clutch>()
        val oldHatched = mutableListOf<Clutch>()
        for (c in clutches) {
            val isHatched = c.status?.lowercase() in listOf("hatched", "completed", "complete")
            if (!isHatched) {
                active.add(c)
                continue
            }
            val hatchDate = effectiveHatchDate(c)
            val daysSinceHatch = hatchDate?.let { ChronoUnit.DAYS.between(it, today) }
            if (daysSinceHatch != null && daysSinceHatch <= 14) active.add(c) else oldHatched.add(c)
        }
        // Within the active list: incubating/hatching first (soonest expected
        // hatch first), then recently-hatched (most recent hatch first).
        active.sortWith(
            compareBy<Clutch> { if (it.status?.lowercase() in listOf("hatched", "completed", "complete")) 1 else 0 }
                .thenBy { c ->
                    if (c.status?.lowercase() in listOf("hatched", "completed", "complete")) {
                        // Sort recently-hatched descending by hatch date (most recent first).
                        // Negate epoch-day so ascending sort yields descending dates.
                        effectiveHatchDate(c)?.toEpochDay()?.let { -it } ?: Long.MAX_VALUE
                    } else {
                        // Sort incubating ascending by expected hatch date (soonest first).
                        effectiveHatchDate(c)?.toEpochDay() ?: Long.MAX_VALUE
                    }
                }
        )
        oldHatched.sortByDescending { effectiveHatchDate(it)?.toEpochDay() ?: Long.MIN_VALUE }
        active to oldHatched
    }
    val activeClutches = clutchPartition.first
    val oldHatchedClutches = clutchPartition.second
    val activeGroups = remember(chickGroups) { chickGroups.filter { it.status == "Active" }.sortedByDescending { it.hatchDate } }
    val graduatedGroups = remember(chickGroups) { chickGroups.filter { it.status != "Active" }.sortedByDescending { it.hatchDate } }
    var showCompletedClutches by remember { mutableStateOf(false) }
    var showGraduatedGroups by remember { mutableStateOf(false) }

    var showAddClutch by remember { mutableStateOf(false) }
    var candlingClutch by remember { mutableStateOf<Clutch?>(null) }
    var hatchClutch by remember { mutableStateOf<Clutch?>(null) }
    // Post-hatch chick group creation
    var createGroupForClutch by remember { mutableStateOf<Pair<Clutch, Int>?>(null) } // (clutch, hatchedCount)
    var editClutch by remember { mutableStateOf<Clutch?>(null) }
    var deleteClutch by remember { mutableStateOf<Clutch?>(null) }
    var editGroup by remember { mutableStateOf<ChickGroupDto?>(null) }
    var deleteGroup by remember { mutableStateOf<ChickGroupDto?>(null) }
    var mortalityGroup by remember { mutableStateOf<ChickGroupDto?>(null) }
    var showAddChooser by remember { mutableStateOf(false) }
    var showAddChickGroup by remember { mutableStateOf(false) }

    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    Column(modifier = Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Text("Hatchery", style = MaterialTheme.typography.headlineMedium)
            Row {
                if (isRefreshing) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = SageGreen)
                } else {
                    IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
                }
                IconButton(onClick = { showAddChooser = true }) { Icon(Icons.Default.Add, "Add", tint = SageGreen) }
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
                        Text("No clutches or chick groups yet.", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(8.dp))
                        Button(onClick = { showAddClutch = true }, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) {
                            Icon(Icons.Default.Add, null, Modifier.size(18.dp)); Spacer(Modifier.width(6.dp)); Text("Add Clutch")
                        }
                    }
                }
            }
            else -> {
                LazyColumn(
                    modifier = Modifier.testTag("hatchery_list"),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    if (activeClutches.isNotEmpty()) {
                        item { Text("Clutches", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        items(activeClutches, key = { "clutch-${it.id}" }) { clutch ->
                            val group = clutchGroupMap[clutch.id]
                            val brooderName = group?.brooderId?.let { brooderMap[it]?.name }
                            ClutchCard(clutch, clutch.lineageName ?: lineageMap[clutch.lineageId]?.name, brooderName,
                                onCandle = { candlingClutch = clutch },
                                onRecordHatch = { hatchClutch = clutch },
                                onEdit = { editClutch = clutch },
                                onDelete = { deleteClutch = clutch },
                                modifier = Modifier.testTag("hatchery_clutch_card_${clutch.id}"))
                        }
                    }
                    if (activeGroups.isNotEmpty()) {
                        item { Spacer(Modifier.height(4.dp)); HorizontalDivider(); Spacer(Modifier.height(4.dp)); Text("Chick Groups", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.testTag("hatchery_chick_groups")) }
                        items(activeGroups, key = { "group-${it.id}" }) { group ->
                            ChickGroupCard(group, group.brooderId?.let { brooderMap[it]?.name },
                                onEdit = { editGroup = group }, onDelete = { deleteGroup = group },
                                onLogMortality = { mortalityGroup = group },
                                onBandGroup = { onBandGroup(group) })
                        }
                    }
                    if (oldHatchedClutches.isNotEmpty()) {
                        item {
                            Spacer(Modifier.height(4.dp)); HorizontalDivider(); Spacer(Modifier.height(4.dp))
                            CollapsibleSectionHeader(
                                title = "Completed Clutches",
                                count = oldHatchedClutches.size,
                                expanded = showCompletedClutches,
                                onToggle = { showCompletedClutches = !showCompletedClutches },
                                testTag = "hatchery_completed_clutches_header",
                            )
                        }
                        if (showCompletedClutches) {
                            items(oldHatchedClutches, key = { "old-clutch-${it.id}" }) { clutch ->
                                val group = clutchGroupMap[clutch.id]
                                val brooderName = group?.brooderId?.let { brooderMap[it]?.name }
                                ClutchCard(clutch, clutch.lineageName ?: lineageMap[clutch.lineageId]?.name, brooderName,
                                    onCandle = { candlingClutch = clutch },
                                    onRecordHatch = { hatchClutch = clutch },
                                    onEdit = { editClutch = clutch },
                                    onDelete = { deleteClutch = clutch },
                                    modifier = Modifier.testTag("hatchery_clutch_card_${clutch.id}"))
                            }
                        }
                    }
                    if (graduatedGroups.isNotEmpty()) {
                        item {
                            Spacer(Modifier.height(4.dp)); HorizontalDivider(); Spacer(Modifier.height(4.dp))
                            CollapsibleSectionHeader(
                                title = "Graduated Groups",
                                count = graduatedGroups.size,
                                expanded = showGraduatedGroups,
                                onToggle = { showGraduatedGroups = !showGraduatedGroups },
                                testTag = "hatchery_graduated_groups_header",
                            )
                        }
                        if (showGraduatedGroups) {
                            items(graduatedGroups, key = { "done-${it.id}" }) { group ->
                                GraduatedGroupCard(group, group.housingId?.let { brooderMap[it]?.name })
                            }
                        }
                    }
                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }

    // --- Dialogs ---

    if (showAddChooser) {
        AlertDialog(
            onDismissRequest = { showAddChooser = false },
            title = { Text("What would you like to add?") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    // The Hatchery "+" toolbar button opens this chooser dialog
                    // first; the tagged button below is the actual "Add Clutch"
                    // entry point that opens the form. E2e tests tap "+", then
                    // tap this tagged button to reach AddClutchDialog.
                    Button(
                        onClick = { showAddChooser = false; showAddClutch = true },
                        modifier = Modifier.fillMaxWidth().testTag("hatchery_add_clutch"),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) {
                        Icon(Icons.Default.Egg, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(8.dp))
                        Text("Add Clutch")
                    }
                    OutlinedButton(
                        onClick = { showAddChooser = false; showAddChickGroup = true },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("\uD83D\uDC25", fontSize = 16.sp)
                        Spacer(Modifier.width(8.dp))
                        Text("Add Chick Group")
                    }
                }
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { showAddChooser = false }) { Text("Cancel") } },
        )
    }

    if (showAddChickGroup) {
        AddStandaloneChickGroupDialog(
            brooders = broodersList,
            viewModel = viewModel,
            onDismiss = { showAddChickGroup = false },
            onSuccess = {
                showAddChickGroup = false
                Toast.makeText(context, "Chick group created!", Toast.LENGTH_SHORT).show()
                viewModel.refresh()
            },
        )
    }

    if (showAddClutch) {
        AddClutchDialog(viewModel, onDismiss = { showAddClutch = false }, onSuccess = {
            showAddClutch = false
            Toast.makeText(context, "Clutch created!", Toast.LENGTH_SHORT).show()
            viewModel.refresh()
        })
    }

    if (candlingClutch != null) {
        CandlingDialog(candlingClutch!!, viewModel, onDismiss = { candlingClutch = null }, onSuccess = { fertile ->
            candlingClutch = null
            Toast.makeText(context, "Candling recorded: $fertile fertile", Toast.LENGTH_SHORT).show()
            viewModel.refresh()
        })
    }

    if (hatchClutch != null) {
        RecordHatchDialog(hatchClutch!!, viewModel, onDismiss = { hatchClutch = null }, onSuccess = { hatched ->
            val clutch = hatchClutch!!
            hatchClutch = null
            Toast.makeText(context, "Hatch recorded: $hatched chicks!", Toast.LENGTH_SHORT).show()
            viewModel.refresh()
            if (hatched > 0) createGroupForClutch = Pair(clutch, hatched)
        })
    }

    if (createGroupForClutch != null) {
        val (clutch, count) = createGroupForClutch!!
        CreateChickGroupDialog(clutch, count, lineageMap, broodersList, viewModel,
            onDismiss = { createGroupForClutch = null },
            onSuccess = {
                createGroupForClutch = null
                Toast.makeText(context, "Chick group created!", Toast.LENGTH_SHORT).show()
                viewModel.refresh()
            })
    }

    if (editClutch != null) {
        EditClutchDialog(editClutch!!, viewModel = viewModel,
            onDismiss = { editClutch = null },
            onSuccess = { editClutch = null; Toast.makeText(context, "Clutch updated", Toast.LENGTH_SHORT).show(); viewModel.refresh() })
    }

    if (deleteClutch != null) {
        val clutchToDelete = deleteClutch!!
        AlertDialog(
            onDismissRequest = { deleteClutch = null },
            title = { Text("Delete Clutch?") },
            text = { Text("Delete Clutch #${clutchToDelete.id}? This will remove the clutch and all associated data.") },
            confirmButton = {
                Button(onClick = {
                    deleteClutch = null
                    viewModel.deleteClutchById(clutchToDelete.id) { ok ->
                        if (ok) { Toast.makeText(context, "Clutch deleted", Toast.LENGTH_SHORT).show(); viewModel.refresh() }
                        else Toast.makeText(context, "Delete failed", Toast.LENGTH_SHORT).show()
                    }
                }, colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFCC4444))) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { deleteClutch = null }) { Text("Cancel") } },
        )
    }

    if (editGroup != null) {
        EditChickGroupDialog(editGroup!!, brooders = broodersList,
            onDismiss = { editGroup = null },
            onSuccess = { editGroup = null; Toast.makeText(context, "Group updated", Toast.LENGTH_SHORT).show(); viewModel.refresh() })
    }

    if (deleteGroup != null) {
        val groupToDelete = deleteGroup!!
        AlertDialog(
            onDismissRequest = { deleteGroup = null },
            title = { Text("Delete Chick Group?") },
            text = { Text("Delete Chick Group #${groupToDelete.id}? This cannot be undone.") },
            confirmButton = {
                Button(onClick = {
                    deleteGroup = null
                    viewModel.deleteChickGroupById(groupToDelete.id) { ok ->
                        if (ok) { Toast.makeText(context, "Group deleted", Toast.LENGTH_SHORT).show(); viewModel.refresh() }
                        else Toast.makeText(context, "Delete failed", Toast.LENGTH_SHORT).show()
                    }
                }, colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFCC4444))) { Text("Delete") }
            },
            dismissButton = { TextButton(onClick = { deleteGroup = null }) { Text("Cancel") } },
        )
    }

    if (mortalityGroup != null) {
        val group = mortalityGroup!!
        var count by remember { mutableStateOf("1") }
        var reason by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { mortalityGroup = null },
            title = { Text("Log Mortality — Group #${group.id}") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("${group.currentCount} chicks currently alive", style = MaterialTheme.typography.bodyMedium)
                    OutlinedTextField(
                        value = count, onValueChange = { count = it.filter { c -> c.isDigit() } },
                        label = { Text("Number lost") }, modifier = Modifier.fillMaxWidth(), singleLine = true,
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    )
                    OutlinedTextField(
                        value = reason, onValueChange = { reason = it },
                        label = { Text("Reason") }, placeholder = { Text("e.g. Failure to thrive") },
                        modifier = Modifier.fillMaxWidth(), singleLine = true,
                    )
                }
            },
            confirmButton = {
                Button(
                    onClick = {
                        val n = count.toIntOrNull() ?: 0
                        if (n > 0 && reason.isNotBlank()) {
                            val gid = group.id
                            mortalityGroup = null
                            scope.launch {
                                val ok = viewModel.logMortality(gid, n, reason)
                                if (ok) Toast.makeText(context, "Logged $n loss${if (n != 1) "es" else ""}", Toast.LENGTH_SHORT).show()
                                else Toast.makeText(context, "Failed to log mortality", Toast.LENGTH_SHORT).show()
                            }
                        }
                    },
                    enabled = (count.toIntOrNull() ?: 0) > 0 && reason.isNotBlank(),
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) { Text("Confirm") }
            },
            dismissButton = { TextButton(onClick = { mortalityGroup = null }) { Text("Cancel") } },
        )
    }
}

// =====================================================================
// Add Clutch Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddClutchDialog(viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    // Use live lineages from the ViewModel so new ones appear immediately
    val liveLineages by viewModel.lineages.collectAsState()

    var selectedLineageId by remember { mutableStateOf<Int?>(null) }
    var eggsSet by remember { mutableStateOf("") }
    var notes by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }

    // Inline new lineage fields
    var showNewLineage by remember { mutableStateOf(false) }
    var newBlName by remember { mutableStateOf("") }
    var newBlSource by remember { mutableStateOf("") }
    var newBlNotes by remember { mutableStateOf("") }
    var creatingBl by remember { mutableStateOf(false) }

    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Clutch") },
        text = {
            Column(
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                // Lineage dropdown
                ExposedDropdownMenuBox(expanded, { expanded = it }) {
                    OutlinedTextField(
                        value = selectedLineageId?.let { id -> liveLineages.find { it.id == id }?.name ?: "" } ?: "",
                        onValueChange = {}, readOnly = true, label = { Text("Lineage") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(expanded, { expanded = false }) {
                        liveLineages.forEach { bl ->
                            DropdownMenuItem(text = { Text(bl.name) }, onClick = { selectedLineageId = bl.id; expanded = false })
                        }
                    }
                }

                // New lineage section
                if (!showNewLineage) {
                    TextButton(onClick = { showNewLineage = true }) {
                        Icon(Icons.Default.Add, null, Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("New Lineage")
                    }
                } else {
                    Card(
                        Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(8.dp),
                        colors = CardDefaults.cardColors(containerColor = SageGreenLight.copy(alpha = 0.1f)),
                    ) {
                        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text("New Lineage", style = MaterialTheme.typography.titleSmall)
                            OutlinedTextField(
                                value = newBlName, onValueChange = { newBlName = it },
                                label = { Text("Name") }, modifier = Modifier.fillMaxWidth(), singleLine = true,
                            )
                            OutlinedTextField(
                                value = newBlSource, onValueChange = { newBlSource = it },
                                label = { Text("Source (optional)") }, placeholder = { Text("e.g. Texas A&M") },
                                modifier = Modifier.fillMaxWidth(), singleLine = true,
                            )
                            OutlinedTextField(
                                value = newBlNotes, onValueChange = { newBlNotes = it },
                                label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth(), singleLine = true,
                            )
                            Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                                OutlinedButton(
                                    onClick = { showNewLineage = false },
                                    Modifier.weight(1f),
                                ) { Text("Cancel") }
                                Button(
                                    onClick = {
                                        creatingBl = true
                                        scope.launch {
                                            val bl = viewModel.createLineage(CreateLineageRequest(
                                                name = newBlName.trim(),
                                                source = newBlSource.trim().ifBlank { "" },
                                                notes = newBlNotes.trim().ifBlank { null },
                                            ))
                                            creatingBl = false
                                            if (bl != null) {
                                                selectedLineageId = bl.id
                                                showNewLineage = false
                                                newBlName = ""; newBlSource = ""; newBlNotes = ""
                                                Toast.makeText(context, "Lineage '${bl.name}' created!", Toast.LENGTH_SHORT).show()
                                            } else {
                                                Toast.makeText(context, "Failed to create lineage", Toast.LENGTH_SHORT).show()
                                            }
                                        }
                                    },
                                    Modifier.weight(1f),
                                    enabled = newBlName.isNotBlank() && !creatingBl,
                                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                                ) { Text(if (creatingBl) "Creating..." else "Create") }
                            }
                        }
                    }
                }

                // Eggs and notes
                OutlinedTextField(
                    value = eggsSet, onValueChange = { eggsSet = it.filter { c -> c.isDigit() } },
                    label = { Text("Eggs set") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = notes, onValueChange = { notes = it },
                    label = { Text("Notes (optional)") },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    saving = true
                    scope.launch {
                        val count = eggsSet.toIntOrNull() ?: 0
                        if (count > 0) {
                            val ok = viewModel.createClutch(CreateClutchRequest(
                                lineageId = selectedLineageId, eggsSet = count,
                                setDate = LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE),
                                notes = notes.ifBlank { null },
                            ))
                            if (ok) onSuccess() else saving = false
                        } else saving = false
                    }
                },
                enabled = (eggsSet.toIntOrNull() ?: 0) > 0 && !saving,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Saving..." else "Create Clutch") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Candling Dialog
// =====================================================================

@Composable
fun CandlingDialog(clutch: Clutch, viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: (Int) -> Unit) {
    var fertile by remember { mutableStateOf(clutch.totalEggs?.toString() ?: "") }
    var notes by remember { mutableStateOf("") }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Record Candling") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Clutch #${clutch.id} — ${clutch.totalEggs ?: "?"} eggs set", style = MaterialTheme.typography.bodyMedium)
                OutlinedTextField(value = fertile, onValueChange = { fertile = it.filter { c -> c.isDigit() } }, label = { Text("Fertile eggs") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                OutlinedTextField(value = notes, onValueChange = { notes = it }, label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    saving = true
                    scope.launch {
                        val count = fertile.toIntOrNull() ?: 0
                        val ok = viewModel.updateClutch(clutch.id, UpdateClutchRequest(
                            eggsFertile = count, notes = notes.ifBlank { null },
                        ))
                        if (ok) onSuccess(count) else saving = false
                    }
                },
                enabled = (fertile.toIntOrNull() ?: -1) >= 0 && !saving,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Saving..." else "Record") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Record Hatch Dialog
// =====================================================================

@Composable
fun RecordHatchDialog(clutch: Clutch, viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: (Int) -> Unit) {
    var hatched by remember { mutableStateOf("") }
    var stillborn by remember { mutableStateOf("0") }
    var quit by remember { mutableStateOf("0") }
    var infertile by remember { mutableStateOf("0") }
    var damaged by remember { mutableStateOf("0") }
    var hatchNotes by remember { mutableStateOf("") }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Record Hatch") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text("Clutch #${clutch.id} — ${clutch.totalEggs ?: "?"} eggs", style = MaterialTheme.typography.bodyMedium)
                OutlinedTextField(value = hatched, onValueChange = { hatched = it.filter { c -> c.isDigit() } }, label = { Text("Hatched") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(value = stillborn, onValueChange = { stillborn = it.filter { c -> c.isDigit() } }, label = { Text("Stillborn") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.weight(1f))
                    OutlinedTextField(value = quit, onValueChange = { quit = it.filter { c -> c.isDigit() } }, label = { Text("Quit") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.weight(1f))
                }
                Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                    OutlinedTextField(value = infertile, onValueChange = { infertile = it.filter { c -> c.isDigit() } }, label = { Text("Infertile") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.weight(1f))
                    OutlinedTextField(value = damaged, onValueChange = { damaged = it.filter { c -> c.isDigit() } }, label = { Text("Damaged") },
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.weight(1f))
                }
                OutlinedTextField(value = hatchNotes, onValueChange = { hatchNotes = it }, label = { Text("Notes (optional)") }, modifier = Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            Button(
                onClick = {
                    saving = true
                    scope.launch {
                        val count = hatched.toIntOrNull() ?: 0
                        val ok = viewModel.updateClutch(clutch.id, UpdateClutchRequest(
                            eggsHatched = count, status = "Hatched",
                            eggsStillborn = stillborn.toIntOrNull(), eggsQuit = quit.toIntOrNull(),
                            eggsInfertile = infertile.toIntOrNull(), eggsDamaged = damaged.toIntOrNull(),
                            hatchNotes = hatchNotes.ifBlank { null },
                        ))
                        if (ok) onSuccess(count) else saving = false
                    }
                },
                enabled = (hatched.toIntOrNull() ?: -1) >= 0 && !saving,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Saving..." else "Record Hatch") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Create Chick Group Dialog (post-hatch)
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CreateChickGroupDialog(
    clutch: Clutch, hatchedCount: Int, lineageMap: Map<Int, Lineage>, brooders: List<Brooder>,
    viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit,
) {
    var selectedBrooderId by remember { mutableStateOf<Int?>(null) }
    var brooderExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Create Chick Group?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("$hatchedCount chicks hatched from ${clutch.lineageName ?: lineageMap[clutch.lineageId]?.name ?: "Clutch #${clutch.id}"}.",
                    style = MaterialTheme.typography.bodyMedium)
                Text("Assign to a brooder:", style = MaterialTheme.typography.bodyMedium)
                ExposedDropdownMenuBox(brooderExpanded, { brooderExpanded = it }) {
                    OutlinedTextField(
                        value = selectedBrooderId?.let { id -> brooders.find { it.id == id }?.name ?: "" } ?: "",
                        onValueChange = {}, readOnly = true, label = { Text("Brooder (optional)") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(brooderExpanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(brooderExpanded, { brooderExpanded = false }) {
                        DropdownMenuItem(text = { Text("None") }, onClick = { selectedBrooderId = null; brooderExpanded = false })
                        brooders.forEach { b -> DropdownMenuItem(text = { Text(b.name) }, onClick = { selectedBrooderId = b.id; brooderExpanded = false }) }
                    }
                }
            }
        },
        confirmButton = {
            // Inherit the parent clutch's lineage as a singleton; if the clutch
            // has none, the user must set one on the clutch first — we no longer
            // silently fall back to lineage #1 (was the "auto-assign Fernbank" bug).
            val parentLineageId = clutch.lineageId
            Button(
                onClick = {
                    if (parentLineageId == null) return@Button
                    saving = true
                    scope.launch {
                        val ok = viewModel.createChickGroup(CreateChickGroupRequest(
                            clutchId = clutch.id,
                            lineageIds = listOf(parentLineageId),
                            brooderId = selectedBrooderId, initialCount = hatchedCount,
                            hatchDate = LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE),
                        ))
                        if (ok) onSuccess() else saving = false
                    }
                },
                enabled = !saving && parentLineageId != null,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) {
                Text(
                    when {
                        saving -> "Creating..."
                        parentLineageId == null -> "Set clutch lineage first"
                        else -> "Create Group"
                    }
                )
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Skip") } },
    )
}

// =====================================================================
// Clutch Card
// =====================================================================

@Composable
fun ClutchCard(clutch: Clutch, lineageName: String?, brooderName: String? = null, onCandle: () -> Unit = {}, onRecordHatch: () -> Unit = {}, onEdit: () -> Unit = {}, onDelete: () -> Unit = {}, modifier: Modifier = Modifier) {
    val today = remember { LocalDate.now() }
    val setDate = remember(clutch.setDate) { parseDate(clutch.setDate) }
    val daysElapsed = remember(setDate, today) { setDate?.let { ChronoUnit.DAYS.between(it, today).toInt() } }
    val expectedHatchDate = remember(setDate) { setDate?.plusDays(INCUBATION_DAYS) }
    val daysUntilHatch = remember(expectedHatchDate, today) { expectedHatchDate?.let { ChronoUnit.DAYS.between(today, it).toInt() } }
    val progress = remember(daysElapsed) { if (daysElapsed == null) 0f else (daysElapsed.toFloat() / INCUBATION_DAYS).coerceIn(0f, 1f) }
    val isComplete = clutch.status?.lowercase() in listOf("hatched", "completed", "complete")
    val isHatching = daysElapsed != null && daysElapsed >= INCUBATION_DAYS && !isComplete
    val isIncubating = clutch.status?.lowercase() in listOf("incubating", "active", "set")

    Card(modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface), elevation = CardDefaults.cardElevation(2.dp)) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(lineageName ?: "Clutch #${clutch.id}", style = MaterialTheme.typography.titleLarge)
                    if (lineageName != null) Text("Clutch #${clutch.id}", style = MaterialTheme.typography.bodyMedium)
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    ClutchStatusBadge(clutch.status, isHatching)
                    IconButton(onClick = onEdit, modifier = Modifier.size(32.dp)) { Icon(Icons.Default.Edit, "Edit", Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                    IconButton(onClick = onDelete, modifier = Modifier.size(32.dp)) { Icon(Icons.Default.Delete, "Delete", Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                }
            }

            Spacer(Modifier.height(12.dp))

            if (isComplete) {
                // Prominent scores for hatched clutches
                val eggs = clutch.totalEggs ?: 0
                val fertile = clutch.totalFertile
                val hatched = clutch.totalHatched
                val fertilityRate = if (eggs > 0 && fertile != null) (fertile.toFloat() / eggs * 100) else null
                // Hatch rate: hatched/fertile, fallback to hatched/eggs if no fertile data
                val hatchRate = if (hatched != null && hatched > 0) {
                    if (fertile != null && fertile > 0) (hatched.toFloat() / fertile * 100)
                    else if (eggs > 0) (hatched.toFloat() / eggs * 100)
                    else null
                } else null
                val hatchRateDenom = if (fertile != null && fertile > 0) fertile else eggs

                fun rateColor(rate: Float) = if (rate < 50f) AlertRed else if (rate < 70f) AlertYellow else AlertGreen

                Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                    clutch.totalEggs?.let { ClutchStat(it.toString(), "Eggs") }
                    fertilityRate?.let { rate ->
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Text("${"%.0f".format(rate)}%", fontSize = 22.sp, fontWeight = FontWeight.Bold, color = rateColor(rate))
                            Text("Fertility", style = MaterialTheme.typography.bodyMedium)
                            fertile?.let { Text("$it of $eggs", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        }
                    }
                    hatchRate?.let { rate ->
                        Column(horizontalAlignment = Alignment.CenterHorizontally) {
                            Text("${"%.0f".format(rate)}%", fontSize = 22.sp, fontWeight = FontWeight.Bold, color = rateColor(rate))
                            Text("Hatch Rate", style = MaterialTheme.typography.bodyMedium)
                            hatched?.let { Text("$it of $hatchRateDenom", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        }
                    }
                }

                // Detail breakdown — only show non-zero stats
                val details = listOfNotNull(
                    clutch.eggsStillborn?.takeIf { it > 0 }?.let { "$it stillborn" },
                    clutch.eggsInfertile?.takeIf { it > 0 }?.let { "$it infertile" },
                    clutch.eggsQuit?.takeIf { it > 0 }?.let { "$it quit" },
                    clutch.eggsDamaged?.takeIf { it > 0 }?.let { "$it damaged" },
                )
                if (details.isNotEmpty()) {
                    Spacer(Modifier.height(6.dp))
                    Text(
                        details.joinToString(" \u00b7 "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                if (!clutch.hatchNotes.isNullOrBlank()) {
                    Spacer(Modifier.height(4.dp))
                    Text(clutch.hatchNotes, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            } else {
                // Standard egg/fertile/hatched stats for incubating clutches
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                    clutch.totalEggs?.let { ClutchStat(it.toString(), "Eggs") }
                    clutch.totalFertile?.let { ClutchStat(it.toString(), "Fertile", clutch.totalEggs?.let { e -> "of $e" }) }
                    clutch.totalHatched?.let { ClutchStat(it.toString(), "Hatched", (clutch.totalFertile ?: clutch.totalEggs)?.let { e -> "of $e" }) }
                }
            }

            if (setDate != null) {
                Spacer(Modifier.height(14.dp))
                IncubationProgressBar(progress, daysElapsed ?: 0)
                Spacer(Modifier.height(6.dp))
                MilestoneMarkers()
                Spacer(Modifier.height(10.dp))
                Text(
                    when { isComplete -> "Hatched"; isHatching -> "Hatch day!"; daysUntilHatch == 1 -> "1 day until hatch"; daysUntilHatch != null && daysUntilHatch > 0 -> "$daysUntilHatch days until hatch"; daysElapsed != null -> "Day $daysElapsed of $INCUBATION_DAYS"; else -> "" },
                    style = MaterialTheme.typography.titleMedium, color = when { isHatching -> AlertYellow; isComplete -> AlertGreen; else -> SageGreen },
                    fontWeight = FontWeight.SemiBold, modifier = Modifier.fillMaxWidth(), textAlign = TextAlign.Center,
                )
            }

            if (brooderName != null) { Spacer(Modifier.height(6.dp)); Text("Brooder: $brooderName", style = MaterialTheme.typography.bodyMedium, color = SageGreen) }
            else if (isComplete) { Spacer(Modifier.height(6.dp)); Text("Not assigned to a brooder", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }

            if (clutch.setDate != null || expectedHatchDate != null) {
                Spacer(Modifier.height(8.dp))
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                    clutch.setDate?.let { Text("Set: $it", style = MaterialTheme.typography.bodyMedium) }
                    expectedHatchDate?.let { Text("Due: ${it.format(DateTimeFormatter.ISO_LOCAL_DATE)}", style = MaterialTheme.typography.bodyMedium) }
                }
            }

            // Action buttons for incubating clutches
            if (isIncubating || isHatching) {
                Spacer(Modifier.height(10.dp))
                Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = onCandle, Modifier.weight(1f)) { Text("Record Candling") }
                    Button(onClick = onRecordHatch, Modifier.weight(1f), colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text("Record Hatch") }
                }
            }
        }
    }
}

// =====================================================================
// Chick Group Card
// =====================================================================

@Composable
fun ChickGroupCard(group: ChickGroupDto, brooderName: String?, onEdit: () -> Unit = {}, onDelete: () -> Unit = {}, onLogMortality: () -> Unit = {}, onBandGroup: () -> Unit = {}) {
    val today = remember { LocalDate.now() }
    val hatchDate = remember(group.hatchDate) { parseDate(group.hatchDate) }
    val ageDays = remember(hatchDate, today) { hatchDate?.let { ChronoUnit.DAYS.between(it, today).toInt() } ?: 0 }
    val mortalityPct = if (group.initialCount > 0) ((group.initialCount - group.currentCount).toFloat() / group.initialCount * 100) else 0f
    val readyToTransition = group.isReadyToTransition

    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface), elevation = CardDefaults.cardElevation(2.dp)) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    // Title: comma-separated lineages, truncated to 3 + "+N" beyond.
                    // Falls back to the group id when no lineages are tagged.
                    val title = if (group.lineages.isNotEmpty()) {
                        com.quailsync.app.data.formatLineages(group.lineages)
                    } else {
                        "Group #${group.id}"
                    }
                    Text(title, style = MaterialTheme.typography.titleLarge)
                    Text("Group #${group.id}", style = MaterialTheme.typography.bodyMedium)
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (readyToTransition) {
                        Box(
                            Modifier
                                .clip(RoundedCornerShape(8.dp))
                                .background(AlertGreen)
                                .padding(horizontal = 10.dp, vertical = 4.dp)
                                .testTag("ready-to-band-badge"),
                            contentAlignment = Alignment.Center
                        ) {
                            Text("\u2713 Ready to band", style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = Color.White)
                        }
                        Spacer(Modifier.width(6.dp))
                    }
                    Box(Modifier.clip(RoundedCornerShape(8.dp)).background(SageGreenLight.copy(alpha = 0.3f)).padding(horizontal = 10.dp, vertical = 4.dp), contentAlignment = Alignment.Center) {
                        Text("Day $ageDays", style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold, color = SageGreen)
                    }
                    IconButton(onClick = onEdit, modifier = Modifier.size(32.dp)) { Icon(Icons.Default.Edit, "Edit", Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                    IconButton(onClick = onDelete, modifier = Modifier.size(32.dp)) { Icon(Icons.Default.Delete, "Delete", Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) }
                }
            }
            Spacer(Modifier.height(12.dp))
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) { Text("${group.currentCount}", fontSize = 22.sp, fontWeight = FontWeight.Bold); Text("Alive", style = MaterialTheme.typography.bodyMedium); Text("of ${group.initialCount}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                Column(horizontalAlignment = Alignment.CenterHorizontally) { Text("%.0f%%".format(mortalityPct), fontSize = 22.sp, fontWeight = FontWeight.Bold, color = if (mortalityPct > 20) AlertRed else if (mortalityPct > 10) AlertYellow else AlertGreen); Text("Mortality", style = MaterialTheme.typography.bodyMedium) }
                Column(horizontalAlignment = Alignment.CenterHorizontally) { Text("${ageDays / 7 + 1}", fontSize = 22.sp, fontWeight = FontWeight.Bold); Text("Week", style = MaterialTheme.typography.bodyMedium) }
            }
            if (brooderName != null) {
                Spacer(Modifier.height(8.dp))
                Text("Brooder: $brooderName", style = MaterialTheme.typography.bodyMedium, color = SageGreen)
                if (readyToTransition) {
                    Text("Fully feathered \u2014 ready to move to flock", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            } else if (readyToTransition) {
                Spacer(Modifier.height(8.dp))
                Text("Fully feathered \u2014 ready to move to flock", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(12.dp))
            Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = onLogMortality, Modifier.weight(1f)) { Text("\uD83D\uDC25", fontSize = 14.sp); Spacer(Modifier.width(4.dp)); Text("Log Mortality") }
                Button(
                    onClick = onBandGroup,
                    modifier = Modifier.weight(1f),
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    elevation = ButtonDefaults.buttonElevation(defaultElevation = if (readyToTransition) 6.dp else 2.dp)
                ) {
                    Icon(Icons.Default.Nfc, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Band Group")
                }
            }
        }
    }
}

// =====================================================================
// Graduated Group Card
// =====================================================================

@Composable
fun GraduatedGroupCard(group: ChickGroupDto, hutchName: String? = null) {
    val mortalityPct = if (group.initialCount > 0) ((group.initialCount - group.currentCount).toFloat() / group.initialCount * 100) else 0f
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)), elevation = CardDefaults.cardElevation(0.dp)) {
        Row(Modifier.fillMaxWidth().padding(14.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                val title = if (group.lineages.isNotEmpty()) {
                    com.quailsync.app.data.formatLineages(group.lineages)
                } else {
                    "Group #${group.id}"
                }
                Text(title, style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("${group.currentCount}/${group.initialCount} chicks · ${group.status} · Hatched ${group.hatchDate}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                val destination = hutchName ?: group.housingId?.let { "Hutch #$it" }
                if (destination != null) {
                    Text("Graduated to: $destination", style = MaterialTheme.typography.labelMedium, color = SageGreen)
                }
            }
            if (mortalityPct > 0) Text("%.0f%% loss".format(mortalityPct), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
fun CollapsibleSectionHeader(
    title: String,
    count: Int,
    expanded: Boolean,
    onToggle: () -> Unit,
    testTag: String,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .clickable { onToggle() }
            .padding(vertical = 6.dp)
            .testTag(testTag),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            "$title ($count)",
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(1f),
        )
        Icon(
            if (expanded) Icons.Default.ExpandLess else Icons.Default.ExpandMore,
            contentDescription = if (expanded) "Collapse" else "Expand",
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

// Server-provided `expectedHatchDate` if set, else derived from setDate. Used
// both as the "expected hatch" for incubating clutches (sort key) and as the
// best-effort actual hatch date for Hatched clutches (the entity has no
// dedicated actual-hatch field, so this is the closest stand-in for the
// 14-day "still recent" cutoff).
private fun effectiveHatchDate(clutch: Clutch): LocalDate? {
    parseDate(clutch.expectedHatchDate)?.let { return it }
    return parseDate(clutch.setDate)?.plusDays(INCUBATION_DAYS)
}

// =====================================================================
// Edit Clutch Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditClutchDialog(clutch: Clutch, viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    var setDate by remember { mutableStateOf(clutch.setDate ?: "") }
    var eggsFertile by remember { mutableStateOf(clutch.totalFertile?.toString() ?: "") }
    var eggsHatched by remember { mutableStateOf(clutch.totalHatched?.toString() ?: "") }
    var status by remember { mutableStateOf(clutch.status ?: "Incubating") }
    var notes by remember { mutableStateOf(clutch.notes ?: "") }
    var statusExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    // Calculate expected hatch date from set date
    val expectedHatch = remember(setDate) {
        try {
            val d = LocalDate.parse(setDate, DateTimeFormatter.ISO_LOCAL_DATE)
            d.plusDays(INCUBATION_DAYS).format(DateTimeFormatter.ISO_LOCAL_DATE)
        } catch (_: Exception) { null }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Edit Clutch #${clutch.id}") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("${clutch.lineageName ?: "Clutch"} — ${clutch.totalEggs ?: "?"} eggs set", style = MaterialTheme.typography.bodyMedium)

                // Set date
                OutlinedTextField(
                    value = setDate, onValueChange = { setDate = it },
                    label = { Text("Set Date (YYYY-MM-DD)") },
                    placeholder = { Text("2026-03-15") },
                    modifier = Modifier.fillMaxWidth(), singleLine = true,
                )
                if (expectedHatch != null) {
                    Text("Expected hatch: $expectedHatch", style = MaterialTheme.typography.labelMedium, color = SageGreen)
                }

                // Status
                ExposedDropdownMenuBox(statusExpanded, { statusExpanded = it }) {
                    OutlinedTextField(value = status, onValueChange = {}, readOnly = true, label = { Text("Status") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(statusExpanded) }, modifier = Modifier.menuAnchor().fillMaxWidth())
                    ExposedDropdownMenu(statusExpanded, { statusExpanded = false }) {
                        listOf("Incubating", "Hatched", "Failed").forEach { s ->
                            DropdownMenuItem(text = { Text(s) }, onClick = { status = s; statusExpanded = false })
                        }
                    }
                }

                OutlinedTextField(value = eggsFertile, onValueChange = { eggsFertile = it.filter { c -> c.isDigit() } }, label = { Text("Fertile eggs") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                OutlinedTextField(value = eggsHatched, onValueChange = { eggsHatched = it.filter { c -> c.isDigit() } }, label = { Text("Hatched count") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                OutlinedTextField(value = notes, onValueChange = { notes = it }, label = { Text("Notes") }, modifier = Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            Button(onClick = {
                saving = true
                scope.launch {
                    val ok = viewModel.updateClutch(clutch.id, UpdateClutchRequest(
                        setDate = setDate.ifBlank { null },
                        eggsFertile = eggsFertile.toIntOrNull(), eggsHatched = eggsHatched.toIntOrNull(),
                        status = status, notes = notes.ifBlank { null },
                    ))
                    if (ok) onSuccess() else saving = false
                }
            }, enabled = !saving, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text(if (saving) "Saving..." else "Save") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Edit Chick Group Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditChickGroupDialog(group: ChickGroupDto, brooders: List<Brooder>, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    var currentCount by remember { mutableStateOf(group.currentCount.toString()) }
    var selectedBrooderId by remember { mutableStateOf(group.brooderId) }
    var notes by remember { mutableStateOf(group.notes ?: "") }
    var brooderExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val baseUrl = ServerConfig.getServerUrl(context).trimEnd('/')

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Edit Group #${group.id}") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(value = currentCount, onValueChange = { currentCount = it.filter { c -> c.isDigit() } }, label = { Text("Current count") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
                ExposedDropdownMenuBox(brooderExpanded, { brooderExpanded = it }) {
                    OutlinedTextField(
                        value = selectedBrooderId?.let { id -> brooders.find { it.id == id }?.name ?: "Brooder #$id" } ?: "None",
                        onValueChange = {}, readOnly = true, label = { Text("Brooder") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(brooderExpanded) }, modifier = Modifier.menuAnchor().fillMaxWidth())
                    ExposedDropdownMenu(brooderExpanded, { brooderExpanded = false }) {
                        DropdownMenuItem(text = { Text("None") }, onClick = { selectedBrooderId = null; brooderExpanded = false })
                        brooders.forEach { b -> DropdownMenuItem(text = { Text(b.name) }, onClick = { selectedBrooderId = b.id; brooderExpanded = false }) }
                    }
                }
                OutlinedTextField(value = notes, onValueChange = { notes = it }, label = { Text("Notes") }, modifier = Modifier.fillMaxWidth())
            }
        },
        confirmButton = {
            Button(onClick = {
                saving = true
                scope.launch {
                    try {
                        // Use raw OkHttp since the chick group PUT might return non-JSON
                        val json = buildString {
                            append("{")
                            currentCount.toIntOrNull()?.let { append("\"current_count\":$it,") }
                            append("\"brooder_id\":${selectedBrooderId ?: "null"},")
                            append("\"notes\":${if (notes.isBlank()) "null" else "\"${notes.replace("\"", "\\\"")}\"" }")
                            append("}")
                        }
                        withContext(Dispatchers.IO) {
                            val body = json.toRequestBody("application/json".toMediaType())
                            val req = Request.Builder().url("$baseUrl/api/chick-groups/${group.id}").put(body)
                                .addHeader("Content-Type", "application/json").build()
                            OkHttpClient().newCall(req).execute()
                        }
                        onSuccess()
                    } catch (e: Exception) {
                        Log.e("QuailSync", "Edit chick group failed", e)
                        saving = false
                    }
                }
            }, enabled = !saving, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Text(if (saving) "Saving..." else "Save") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// =====================================================================
// Shared composables
// =====================================================================

@Composable fun ClutchStatusBadge(status: String?, isHatching: Boolean) {
    val displayStatus = when { isHatching -> "Hatching!"; status != null -> status.replaceFirstChar { it.uppercase() }; else -> "Unknown" }
    val bgColor = when { isHatching -> AlertYellow; status?.lowercase() in listOf("hatched", "completed", "complete") -> AlertGreen; status?.lowercase() in listOf("incubating", "active", "set") -> SageGreenLight; else -> MaterialTheme.colorScheme.surfaceVariant }
    val textColor = when { isHatching -> Color(0xFF4A3D00); status?.lowercase() in listOf("hatched", "completed", "complete") -> Color(0xFF1B3A14); status?.lowercase() in listOf("incubating", "active", "set") -> Color(0xFF2D4A1E); else -> MaterialTheme.colorScheme.onSurfaceVariant }
    Text(displayStatus, style = MaterialTheme.typography.labelLarge, color = textColor, modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(bgColor).padding(horizontal = 8.dp, vertical = 3.dp))
}

@Composable fun ClutchStat(value: String, label: String, subtitle: String? = null) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Text(value, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.onSurface)
        Text(label, style = MaterialTheme.typography.bodyMedium)
        if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable fun IncubationProgressBar(progress: Float, daysElapsed: Int) {
    Column {
        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) { Text("Day $daysElapsed", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium); Text("of $INCUBATION_DAYS", style = MaterialTheme.typography.bodyMedium) }
        Spacer(Modifier.height(4.dp))
        LinearProgressIndicator(progress = { progress }, modifier = Modifier.fillMaxWidth().height(10.dp).clip(RoundedCornerShape(5.dp)), color = when { progress >= 1f -> AlertYellow; progress >= 0.82f -> DustyRose; else -> SageGreen }, trackColor = MaterialTheme.colorScheme.surfaceVariant)
    }
}

@Composable fun MilestoneMarkers() {
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
    return try { LocalDate.parse(dateStr, DateTimeFormatter.ISO_LOCAL_DATE) }
    catch (_: Exception) { try { LocalDate.parse(dateStr.take(10), DateTimeFormatter.ISO_LOCAL_DATE) } catch (_: Exception) { null } }
}

// =====================================================================
// Add Standalone Chick Group Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddStandaloneChickGroupDialog(
    brooders: List<Brooder>,
    viewModel: ClutchViewModel,
    onDismiss: () -> Unit,
    onSuccess: () -> Unit,
) {
    var count by remember { mutableStateOf("") }
    var source by remember { mutableStateOf("") }
    var hatchDate by remember { mutableStateOf(LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE)) }
    var notes by remember { mutableStateOf("") }
    var selectedBrooderId by remember { mutableStateOf<Int?>(null) }
    // Multi-select lineage tags. Default is empty: users MUST pick at least one
    // before the Create button enables — replaces the silent fallback to
    // lineage #1 ("auto-assign Fernbank" bug).
    val selectedLineageIds = remember { mutableStateListOf<Int>() }
    var brooderExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    var showNewLineage by remember { mutableStateOf(false) }
    var newBlName by remember { mutableStateOf("") }
    var newBlSource by remember { mutableStateOf("") }
    val liveLineages by viewModel.lineages.collectAsState()
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("\uD83D\uDC25 Add Chick Group") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = count, onValueChange = { count = it.filter { c -> c.isDigit() } },
                    label = { Text("Number of chicks") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.fillMaxWidth(), singleLine = true,
                )
                OutlinedTextField(
                    value = source, onValueChange = { source = it },
                    label = { Text("Source / Origin") },
                    placeholder = { Text("e.g. Farmer pickup, Hatched in-house") },
                    modifier = Modifier.fillMaxWidth(), singleLine = true,
                )
                OutlinedTextField(
                    value = hatchDate, onValueChange = { hatchDate = it },
                    label = { Text("Hatch date (YYYY-MM-DD)") },
                    modifier = Modifier.fillMaxWidth(), singleLine = true,
                )

                // Brooder dropdown
                ExposedDropdownMenuBox(brooderExpanded, { brooderExpanded = it }) {
                    OutlinedTextField(
                        value = selectedBrooderId?.let { id -> brooders.find { it.id == id }?.name ?: "" } ?: "",
                        onValueChange = {}, readOnly = true,
                        label = { Text("Assign to brooder") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(brooderExpanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(brooderExpanded, { brooderExpanded = false }) {
                        brooders.forEach { b ->
                            DropdownMenuItem(
                                text = { Text(b.name) },
                                onClick = { selectedBrooderId = b.id; brooderExpanded = false },
                            )
                        }
                    }
                }

                // Multi-select lineage tags. Tap a chip to toggle.
                Column {
                    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                        Text(
                            "Lineages (pick at least one)",
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.weight(1f),
                        )
                        IconButton(onClick = { showNewLineage = true }) {
                            Icon(Icons.Default.Add, "New lineage", tint = SageGreen)
                        }
                    }
                    androidx.compose.foundation.layout.FlowRow(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        liveLineages.forEach { bl ->
                            val on = selectedLineageIds.contains(bl.id)
                            FilterChip(
                                selected = on,
                                onClick = {
                                    if (on) selectedLineageIds.remove(bl.id)
                                    else selectedLineageIds.add(bl.id)
                                },
                                label = { Text(bl.name) },
                            )
                        }
                    }
                }

                // Inline new lineage fields. Tap "Add lineage" to POST to
                // /api/lineages, which auto-toggles the new chip as selected and
                // clears the form so another can be added. The button label is
                // intentionally distinct from the dialog's "Create Group" so users
                // can tell the actions apart.
                if (showNewLineage) {
                    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(8.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant)) {
                        Column(Modifier.padding(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text("New Lineage", style = MaterialTheme.typography.labelMedium)
                            OutlinedTextField(value = newBlName, onValueChange = { newBlName = it }, label = { Text("Name") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
                            OutlinedTextField(value = newBlSource, onValueChange = { newBlSource = it }, label = { Text("Source") }, placeholder = { Text("e.g. Breeder name") }, modifier = Modifier.fillMaxWidth(), singleLine = true)
                            Row(Modifier.fillMaxWidth(), Arrangement.End, Alignment.CenterVertically) {
                                TextButton(onClick = { showNewLineage = false; newBlName = ""; newBlSource = "" }) { Text("Cancel") }
                                Button(
                                    onClick = {
                                        scope.launch {
                                            val bl = viewModel.createLineage(CreateLineageRequest(name = newBlName.trim(), source = newBlSource.trim()))
                                            if (bl != null) {
                                                selectedLineageIds.add(bl.id)
                                                showNewLineage = false; newBlName = ""; newBlSource = ""
                                            } else {
                                                Toast.makeText(context, "Failed to create lineage", Toast.LENGTH_SHORT).show()
                                            }
                                        }
                                    },
                                    enabled = newBlName.isNotBlank(),
                                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                                ) { Text("Add lineage") }
                            }
                        }
                    }
                }

                OutlinedTextField(
                    value = notes, onValueChange = { notes = it },
                    label = { Text("Notes (optional)") },
                    placeholder = { Text("e.g. Source: $source") },
                    modifier = Modifier.fillMaxWidth(), maxLines = 2,
                )
            }
        },
        confirmButton = {
            // Tolerant enabled rule: any existing chip selected OR a valid
            // inline lineage being typed but not yet "Add lineage"-d.
            val inlineFormValid = showNewLineage && newBlName.isNotBlank()
            val canCreate = (count.toIntOrNull() ?: 0) > 0 && !saving &&
                (selectedLineageIds.isNotEmpty() || inlineFormValid)
            Button(
                onClick = {
                    val n = count.toIntOrNull() ?: 0
                    if (n <= 0) return@Button
                    saving = true
                    scope.launch {
                        // If the user typed a new lineage in the inline form but
                        // didn't tap "Add lineage", create it now so we don't
                        // silently drop their input. On failure, surface the
                        // error and abort — never proceed with a chick group
                        // that's missing the lineage they just typed.
                        if (selectedLineageIds.isEmpty() && inlineFormValid) {
                            val bl = viewModel.createLineage(
                                CreateLineageRequest(
                                    name = newBlName.trim(),
                                    source = newBlSource.trim(),
                                )
                            )
                            if (bl == null) {
                                Toast.makeText(context, "Failed to create lineage — chick group not created", Toast.LENGTH_LONG).show()
                                saving = false
                                return@launch
                            }
                            selectedLineageIds.add(bl.id)
                            showNewLineage = false; newBlName = ""; newBlSource = ""
                        }

                        val fullNotes = buildString {
                            if (source.isNotBlank()) append("Source: $source")
                            if (notes.isNotBlank()) { if (isNotEmpty()) append(". "); append(notes) }
                        }.ifBlank { null }
                        val ok = viewModel.createChickGroup(
                            CreateChickGroupRequest(
                                clutchId = null,
                                lineageIds = selectedLineageIds.toList(),
                                brooderId = selectedBrooderId,
                                initialCount = n,
                                hatchDate = hatchDate,
                                notes = fullNotes,
                            )
                        )
                        if (ok) onSuccess() else saving = false
                    }
                },
                enabled = canCreate,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Creating..." else "Create Group") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
