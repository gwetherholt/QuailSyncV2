package com.quailsync.app.ui.screens

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
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Pets
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bloodline
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.ChickGroupDto
import com.quailsync.app.data.Clutch
import com.quailsync.app.data.CreateChickGroupRequest
import com.quailsync.app.data.CreateClutchRequest
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
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

private const val INCUBATION_DAYS = 17L
private const val BANDING_AGE_DAYS = 28

// =====================================================================
// ViewModel
// =====================================================================

class ClutchViewModel(application: Application) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

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

    fun refresh() { viewModelScope.launch { _isRefreshing.value = true; loadDataSuspend(); _isRefreshing.value = false } }
    private fun loadData() { viewModelScope.launch { loadDataSuspend() } }

    private suspend fun loadDataSuspend() {
        try {
            _clutches.value = api.getClutches()
            _bloodlines.value = try { api.getBloodlines() } catch (_: Exception) { emptyList() }
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
            api.logMortality(groupId, com.quailsync.app.data.MortalityRequest(count, reason))
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

    suspend fun createBloodline(request: com.quailsync.app.data.CreateBloodlineRequest): Bloodline? {
        return try {
            val bl = api.createBloodline(request)
            // Refresh bloodlines list so dropdown updates
            _bloodlines.value = try { api.getBloodlines() } catch (_: Exception) { _bloodlines.value }
            bl
        } catch (e: Exception) { Log.e("QuailSync", "Create bloodline failed", e); null }
    }
}

// =====================================================================
// Hatchery Screen
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
        clutches.sortedWith(compareBy<Clutch> { when (it.status?.lowercase()) { "incubating", "active", "set" -> 0; "hatching" -> 1; else -> 2 } }.thenByDescending { it.setDate })
    }
    val activeGroups = remember(chickGroups) { chickGroups.filter { it.status == "Active" }.sortedByDescending { it.hatchDate } }
    val graduatedGroups = remember(chickGroups) { chickGroups.filter { it.status != "Active" }.sortedByDescending { it.hatchDate } }

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
                IconButton(onClick = { showAddClutch = true }) { Icon(Icons.Default.Add, "Add Clutch", tint = SageGreen) }
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
                LazyColumn(contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (sortedClutches.isNotEmpty()) {
                        item { Text("Clutches", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        items(sortedClutches, key = { "clutch-${it.id}" }) { clutch ->
                            val group = clutchGroupMap[clutch.id]
                            val brooderName = group?.brooderId?.let { brooderMap[it]?.name }
                            ClutchCard(clutch, clutch.bloodlineName ?: bloodlineMap[clutch.bloodlineId]?.name, brooderName,
                                onCandle = { candlingClutch = clutch },
                                onRecordHatch = { hatchClutch = clutch },
                                onEdit = { editClutch = clutch },
                                onDelete = { deleteClutch = clutch })
                        }
                    }
                    if (activeGroups.isNotEmpty()) {
                        item { Spacer(Modifier.height(4.dp)); HorizontalDivider(); Spacer(Modifier.height(4.dp)); Text("Chick Groups", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        items(activeGroups, key = { "group-${it.id}" }) { group ->
                            ChickGroupCard(group, bloodlineMap[group.bloodlineId]?.name, group.brooderId?.let { brooderMap[it]?.name },
                                onEdit = { editGroup = group }, onDelete = { deleteGroup = group },
                                onLogMortality = { mortalityGroup = group })
                        }
                    }
                    if (graduatedGroups.isNotEmpty()) {
                        item { Spacer(Modifier.height(4.dp)); HorizontalDivider(); Spacer(Modifier.height(4.dp)); Text("Completed", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        items(graduatedGroups, key = { "done-${it.id}" }) { group -> GraduatedGroupCard(group, bloodlineMap[group.bloodlineId]?.name) }
                    }
                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }

    // --- Dialogs ---

    if (showAddClutch) {
        AddClutchDialog(bloodlines, viewModel, onDismiss = { showAddClutch = false }, onSuccess = {
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
        CreateChickGroupDialog(clutch, count, bloodlineMap, broodersList, viewModel,
            onDismiss = { createGroupForClutch = null },
            onSuccess = {
                createGroupForClutch = null
                Toast.makeText(context, "Chick group created!", Toast.LENGTH_SHORT).show()
                viewModel.refresh()
            })
    }

    if (editClutch != null) {
        EditClutchDialog(editClutch!!, liveBloodlines = bloodlines, viewModel = viewModel,
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
        EditChickGroupDialog(editGroup!!, brooders = broodersList, viewModel = viewModel,
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
fun AddClutchDialog(bloodlines: List<Bloodline>, viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    // Use live bloodlines from the ViewModel so new ones appear immediately
    val liveBloodlines by viewModel.bloodlines.collectAsState()

    var selectedBloodlineId by remember { mutableStateOf<Int?>(null) }
    var eggsSet by remember { mutableStateOf("") }
    var notes by remember { mutableStateOf("") }
    var expanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }

    // Inline new bloodline fields
    var showNewBloodline by remember { mutableStateOf(false) }
    var newBlName by remember { mutableStateOf("") }
    var newBlSource by remember { mutableStateOf("") }
    var newBlNotes by remember { mutableStateOf("") }
    var creatingBl by remember { mutableStateOf(false) }

    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val context = LocalContext.current

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add Clutch") },
        text = {
            Column(
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                // Bloodline dropdown
                ExposedDropdownMenuBox(expanded, { expanded = it }) {
                    OutlinedTextField(
                        value = selectedBloodlineId?.let { id -> liveBloodlines.find { it.id == id }?.name ?: "" } ?: "",
                        onValueChange = {}, readOnly = true, label = { Text("Bloodline") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
                        modifier = Modifier.menuAnchor().fillMaxWidth(),
                    )
                    ExposedDropdownMenu(expanded, { expanded = false }) {
                        liveBloodlines.forEach { bl ->
                            DropdownMenuItem(text = { Text(bl.name) }, onClick = { selectedBloodlineId = bl.id; expanded = false })
                        }
                    }
                }

                // New bloodline section
                if (!showNewBloodline) {
                    TextButton(onClick = { showNewBloodline = true }) {
                        Icon(Icons.Default.Add, null, Modifier.size(16.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("New Bloodline")
                    }
                } else {
                    Card(
                        Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(8.dp),
                        colors = CardDefaults.cardColors(containerColor = SageGreenLight.copy(alpha = 0.1f)),
                    ) {
                        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text("New Bloodline", style = MaterialTheme.typography.titleSmall)
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
                                    onClick = { showNewBloodline = false },
                                    Modifier.weight(1f),
                                ) { Text("Cancel") }
                                Button(
                                    onClick = {
                                        creatingBl = true
                                        scope.launch {
                                            val bl = viewModel.createBloodline(com.quailsync.app.data.CreateBloodlineRequest(
                                                name = newBlName.trim(),
                                                source = newBlSource.trim().ifBlank { "" },
                                                notes = newBlNotes.trim().ifBlank { null },
                                            ))
                                            creatingBl = false
                                            if (bl != null) {
                                                selectedBloodlineId = bl.id
                                                showNewBloodline = false
                                                newBlName = ""; newBlSource = ""; newBlNotes = ""
                                                Toast.makeText(context, "Bloodline '${bl.name}' created!", Toast.LENGTH_SHORT).show()
                                            } else {
                                                Toast.makeText(context, "Failed to create bloodline", Toast.LENGTH_SHORT).show()
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
                                bloodlineId = selectedBloodlineId, eggsSet = count,
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
    val scope = androidx.compose.runtime.rememberCoroutineScope()

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
    val scope = androidx.compose.runtime.rememberCoroutineScope()

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
    clutch: Clutch, hatchedCount: Int, bloodlineMap: Map<Int, Bloodline>, brooders: List<Brooder>,
    viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit,
) {
    var selectedBrooderId by remember { mutableStateOf<Int?>(null) }
    var brooderExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Create Chick Group?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("$hatchedCount chicks hatched from ${clutch.bloodlineName ?: bloodlineMap[clutch.bloodlineId]?.name ?: "Clutch #${clutch.id}"}.",
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
            Button(
                onClick = {
                    saving = true
                    scope.launch {
                        val ok = viewModel.createChickGroup(CreateChickGroupRequest(
                            clutchId = clutch.id, bloodlineId = clutch.bloodlineId ?: 1,
                            brooderId = selectedBrooderId, initialCount = hatchedCount,
                            hatchDate = LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE),
                        ))
                        if (ok) onSuccess() else saving = false
                    }
                },
                enabled = !saving,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Creating..." else "Create Group") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Skip") } },
    )
}

// =====================================================================
// Clutch Card
// =====================================================================

@Composable
fun ClutchCard(clutch: Clutch, bloodlineName: String?, brooderName: String? = null, onCandle: () -> Unit = {}, onRecordHatch: () -> Unit = {}, onEdit: () -> Unit = {}, onDelete: () -> Unit = {}) {
    val today = remember { LocalDate.now() }
    val setDate = remember(clutch.setDate) { parseDate(clutch.setDate) }
    val daysElapsed = remember(setDate, today) { setDate?.let { ChronoUnit.DAYS.between(it, today).toInt() } }
    val expectedHatchDate = remember(setDate) { setDate?.plusDays(INCUBATION_DAYS) }
    val daysUntilHatch = remember(expectedHatchDate, today) { expectedHatchDate?.let { ChronoUnit.DAYS.between(today, it).toInt() } }
    val progress = remember(daysElapsed) { if (daysElapsed == null) 0f else (daysElapsed.toFloat() / INCUBATION_DAYS).coerceIn(0f, 1f) }
    val isComplete = clutch.status?.lowercase() in listOf("hatched", "completed", "complete")
    val isHatching = daysElapsed != null && daysElapsed >= INCUBATION_DAYS && !isComplete
    val isIncubating = clutch.status?.lowercase() in listOf("incubating", "active", "set")

    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface), elevation = CardDefaults.cardElevation(2.dp)) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(bloodlineName ?: "Clutch #${clutch.id}", style = MaterialTheme.typography.titleLarge)
                    if (bloodlineName != null) Text("Clutch #${clutch.id}", style = MaterialTheme.typography.bodyMedium)
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
fun ChickGroupCard(group: ChickGroupDto, bloodlineName: String?, brooderName: String?, onEdit: () -> Unit = {}, onDelete: () -> Unit = {}, onLogMortality: () -> Unit = {}) {
    val today = remember { LocalDate.now() }
    val hatchDate = remember(group.hatchDate) { parseDate(group.hatchDate) }
    val ageDays = remember(hatchDate, today) { hatchDate?.let { ChronoUnit.DAYS.between(it, today).toInt() } ?: 0 }
    val mortalityPct = if (group.initialCount > 0) ((group.initialCount - group.currentCount).toFloat() / group.initialCount * 100) else 0f
    val canBand = ageDays >= BANDING_AGE_DAYS

    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface), elevation = CardDefaults.cardElevation(2.dp)) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(bloodlineName ?: "Group #${group.id}", style = MaterialTheme.typography.titleLarge)
                    Text("Group #${group.id}", style = MaterialTheme.typography.bodyMedium)
                }
                Row(verticalAlignment = Alignment.CenterVertically) {
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
            if (brooderName != null) { Spacer(Modifier.height(8.dp)); Text("Brooder: $brooderName", style = MaterialTheme.typography.bodyMedium, color = SageGreen) }
            Spacer(Modifier.height(12.dp))
            Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = onLogMortality, Modifier.weight(1f)) { Icon(Icons.Default.Pets, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Log Mortality") }
                if (canBand) {
                    Button(onClick = {}, Modifier.weight(1f), colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) { Icon(Icons.Default.Nfc, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Band Group") }
                } else {
                    OutlinedButton(onClick = {}, Modifier.weight(1f), enabled = false) { Icon(Icons.Default.Nfc, null, Modifier.size(16.dp)); Spacer(Modifier.width(4.dp)); Text("Band ($ageDays/${BANDING_AGE_DAYS}d)") }
                }
            }
        }
    }
}

// =====================================================================
// Graduated Group Card
// =====================================================================

@Composable
fun GraduatedGroupCard(group: ChickGroupDto, bloodlineName: String?) {
    val mortalityPct = if (group.initialCount > 0) ((group.initialCount - group.currentCount).toFloat() / group.initialCount * 100) else 0f
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp), colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)), elevation = CardDefaults.cardElevation(0.dp)) {
        Row(Modifier.fillMaxWidth().padding(14.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(bloodlineName ?: "Group #${group.id}", style = MaterialTheme.typography.titleMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("${group.currentCount}/${group.initialCount} chicks · ${group.status} · Hatched ${group.hatchDate}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (mortalityPct > 0) Text("%.0f%% loss".format(mortalityPct), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

// =====================================================================
// Edit Clutch Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun EditClutchDialog(clutch: Clutch, liveBloodlines: List<Bloodline>, viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    var setDate by remember { mutableStateOf(clutch.setDate ?: "") }
    var eggsFertile by remember { mutableStateOf(clutch.totalFertile?.toString() ?: "") }
    var eggsHatched by remember { mutableStateOf(clutch.totalHatched?.toString() ?: "") }
    var status by remember { mutableStateOf(clutch.status ?: "Incubating") }
    var notes by remember { mutableStateOf(clutch.notes ?: "") }
    var statusExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()

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
                Text("${clutch.bloodlineName ?: "Clutch"} — ${clutch.totalEggs ?: "?"} eggs set", style = MaterialTheme.typography.bodyMedium)

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
fun EditChickGroupDialog(group: ChickGroupDto, brooders: List<Brooder>, viewModel: ClutchViewModel, onDismiss: () -> Unit, onSuccess: () -> Unit) {
    var currentCount by remember { mutableStateOf(group.currentCount.toString()) }
    var selectedBrooderId by remember { mutableStateOf(group.brooderId) }
    var notes by remember { mutableStateOf(group.notes ?: "") }
    var brooderExpanded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
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
                        kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                            val body = json.toByteArray().let { okhttp3.RequestBody.create(null, it) }
                            val req = okhttp3.Request.Builder().url("$baseUrl/api/chick-groups/${group.id}").put(body)
                                .addHeader("Content-Type", "application/json").build()
                            okhttp3.OkHttpClient().newCall(req).execute()
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
