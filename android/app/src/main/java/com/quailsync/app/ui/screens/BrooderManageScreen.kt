package com.quailsync.app.ui.screens

import android.util.Log
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Group
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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.quailsync.app.data.Bird
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.ChickGroupDto
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.TargetTempResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BrooderManageScreen(brooderId: Int, onBack: () -> Unit) {
    val context = LocalContext.current
    val serverUrl = remember { ServerConfig.getServerUrl(context) }
    val api = remember { QuailSyncApi.create(serverUrl) }
    val scope = rememberCoroutineScope()
    var targetTemp by remember { mutableStateOf<TargetTempResponse?>(null) }
    // The Brooder DTO itself, so we know housing_type / name for the header
    // and conditional rendering (issue #11).
    var brooder by remember { mutableStateOf<Brooder?>(null) }
    var headcount by remember { mutableStateOf<com.quailsync.app.data.HeadcountResponse?>(null) }
    var allGroups by remember { mutableStateOf<List<ChickGroupDto>>(emptyList()) }
    var residentBirds by remember { mutableStateOf<List<Bird>>(emptyList()) }
    // Issue #14: the residents endpoint now returns graduated chick groups
    // assigned to this unit via chick_groups.housing_id. Tracked separately
    // from `allGroups` (which is the global /api/chick-groups list used by
    // the "Chick Group Assignment" dropdown above).
    var residentGroups by remember { mutableStateOf<List<ChickGroupDto>>(emptyList()) }
    // Issue #13: full bird list (for the "Assign Birds" picker — filtered to
    // unhoused active birds when the dialog opens). Fetched on each load.
    var allBirds by remember { mutableStateOf<List<Bird>>(emptyList()) }
    // Picker dialog visibility. Explicit MutableState because the lambda-write
    // `showAssignDialog = false` inside dialog callbacks would trip Kotlin's
    // UNUSED_VALUE flow analyser otherwise — same precedent as elsewhere.
    val showAssignDialog = remember { mutableStateOf(false) }
    // Issue #14: separate dialog for "Assign Graduated Group". Mirrors the
    // dashboard's picker — only shown when there's at least one Graduated
    // group that isn't already in this hutch.
    val showAssignGroupDialog = remember { mutableStateOf(false) }
    var isLoading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var refreshKey by remember { mutableStateOf(0) }

    // Derived: which group is currently in this brooder
    val currentGroup = allGroups.find { it.brooderId == brooderId && it.status == "Active" }

    // Dropdown state
    var groupExpanded by remember { mutableStateOf(false) }
    var selectedGroupId by remember { mutableStateOf<Int?>(null) }

    // Load data
    LaunchedEffect(brooderId, refreshKey) {
        isLoading = refreshKey == 0
        error = null
        Log.d("QuailSync", "BrooderManage: loading data for brooder $brooderId (refresh=$refreshKey)")
        try {
            brooder = try { api.getBrooders().find { it.id == brooderId } } catch (e: Exception) {
                Log.e("QuailSync", "brooder lookup failed", e); null
            }
            targetTemp = try { api.getBrooderTargetTemp(brooderId) } catch (e: Exception) { Log.e("QuailSync", "targetTemp failed", e); null }
            headcount = try {
                val hc = api.getHeadcountLatest(brooderId)
                Log.d("QuailSync", "Headcount for brooder $brooderId: count=${hc.count} ts=${hc.timestamp}")
                hc
            } catch (e: Exception) {
                Log.e("QuailSync", "Headcount fetch failed for brooder $brooderId", e)
                null
            }

            // Fetch chick groups — log raw JSON for debugging
            allGroups = try {
                // First log the raw JSON to see exact field names
                val rawJson = withContext(Dispatchers.IO) {
                    val url = "${serverUrl.trimEnd('/')}/api/chick-groups"
                    val req = okhttp3.Request.Builder().url(url).get().build()
                    val resp = okhttp3.OkHttpClient().newCall(req).execute()
                    resp.body?.string()
                }
                Log.d("QuailSync", "BrooderManage: raw /api/chick-groups JSON: $rawJson")

                // Now parse via Retrofit
                val groups = api.getChickGroups()
                Log.d("QuailSync", "BrooderManage: parsed ${groups.size} chick groups:")
                groups.forEach { g ->
                    Log.d("QuailSync", "  Group #${g.id}: brooderId=${g.brooderId}, status='${g.status}', count=${g.currentCount}, hatch=${g.hatchDate}")
                }
                val match = groups.find { it.brooderId == brooderId && it.status == "Active" }
                Log.d("QuailSync", "  -> Looking for brooderId==$brooderId (type=${brooderId::class.simpleName}), match=${match?.let { "Group #${it.id} (brooderId=${it.brooderId}, type=${it.brooderId?.let { v -> v::class.simpleName }})" } ?: "NONE"}")
                groups
            } catch (e: Exception) {
                Log.e("QuailSync", "chickGroups failed", e)
                emptyList()
            }

            // Residents — may return non-JSON
            try {
                val r = api.getBrooderResidents(brooderId)
                residentBirds = r.individualBirds
                residentGroups = r.chickGroups
            } catch (e: Exception) {
                Log.e("QuailSync", "residents failed (treating as empty)", e)
                residentBirds = emptyList()
                residentGroups = emptyList()
            }
            allBirds = try { api.getBirds() } catch (e: Exception) {
                Log.e("QuailSync", "getBirds for picker failed", e); emptyList()
            }
            Log.d("QuailSync", "BrooderManage: loaded. allGroups=${allGroups.size}, birds=${residentBirds.size}")
        } catch (e: Exception) {
            Log.e("QuailSync", "Load failed", e)
            error = e.message
        }
        isLoading = false
    }

    // Housing type pulled from the Brooder DTO once loaded — drives the
    // header label (issue #11), the temp-schedule visibility (no sensors on
    // hutches), and the residents-pluralisation (eggs/chicks/birds).
    val housingType = brooder?.housingType?.lowercase() ?: "brooder"
    val isHutch = housingType == "hutch"

    Column(Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState())) {
        // Header — "Incubator: Nurture Right 360" / "Hutch: Outdoor Hutch" etc.
        // when the brooder DTO has loaded; falls back to the legacy framing
        // while the fetch is in flight.
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
            Spacer(Modifier.width(8.dp))
            val headerText = brooder?.let {
                val typeLabel = when (housingType) {
                    "incubator" -> "Incubator"
                    "hutch"     -> "Hutch"
                    else        -> "Brooder"
                }
                "$typeLabel: ${it.name}"
            } ?: "Manage Brooder #$brooderId"
            Text(headerText, style = MaterialTheme.typography.headlineMedium)
        }

        if (isLoading) {
            Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                CircularProgressIndicator(color = SageGreen)
            }
            return@Column
        }

        Spacer(Modifier.height(16.dp))

        // =====================================================
        // Temperature schedule
        // =====================================================
        // Hutches don't have environmental sensors — skip the whole temp block.
        if (targetTemp != null && !isHutch) {
            val tt = targetTemp!!
            Card(
                Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                elevation = CardDefaults.cardElevation(2.dp),
            ) {
                Column(Modifier.padding(16.dp)) {
                    Text("Temperature Schedule", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(8.dp))
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                        Text("Target", style = MaterialTheme.typography.bodyMedium)
                        Text(tt.targetTempF?.let { "%.0f°F".format(it) } ?: "—", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Bold)
                    }
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                        Text("Range", style = MaterialTheme.typography.bodyMedium)
                        Text(
                            if (tt.minTempF != null && tt.maxTempF != null) "%.0f–%.0f°F".format(tt.minTempF, tt.maxTempF) else "—",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                    if (tt.ageDays != null) {
                        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                            Text("Chick age", style = MaterialTheme.typography.bodyMedium)
                            Text("Day ${tt.ageDays}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                        }
                    }
                    Spacer(Modifier.height(4.dp))
                    Text(tt.scheduleLabel, style = MaterialTheme.typography.labelMedium, color = SageGreen)

                    Spacer(Modifier.height(12.dp))
                    // Nullable week → no week highlighted (Int == Int? never matches null).
                    val currentWeek = tt.week?.coerceIn(1, 6)
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceEvenly) {
                        listOf("W1\n97°" to 1, "W2\n92°" to 2, "W3\n87°" to 3, "W4\n82°" to 4, "W5\n77°" to 5, "W6+\n72°" to 6).forEach { (label, week) ->
                            val isCurrent = week == currentWeek && tt.ageDays != null
                            Box(
                                Modifier.size(42.dp).clip(CircleShape)
                                    .background(if (isCurrent) SageGreen else SageGreenLight.copy(alpha = 0.3f)),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(label, style = MaterialTheme.typography.labelSmall,
                                    color = if (isCurrent) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
                                    textAlign = TextAlign.Center)
                            }
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // =====================================================
        // Chick Headcount (YOLO inference)
        // =====================================================
        Card(
            Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(2.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                // Housing-type-aware count label: eggs / chicks / birds.
                val (countTitle, residentSingular, residentIcon) = when (housingType) {
                    "incubator" -> Triple("Egg Count",   "egg",   "\uD83E\uDD5A") // \uD83E\uDD5A
                    "hutch"     -> Triple("Bird Count",  "bird",  "\uD83D\uDC26") // \uD83D\uDC26
                    else        -> Triple("Chick Count", "chick", "\uD83D\uDC25") // \uD83D\uDC25
                }
                Text(countTitle, style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))
                val hc = headcount
                val hcAgo = remember(hc?.timestamp) { formatTimeAgo(hc?.timestamp) }
                if (hc?.count != null) {
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                        val label = residentSingular + if (hc.count != 1) "s" else ""
                        Text(
                            "$residentIcon ${hc.count} $label detected",
                            style = MaterialTheme.typography.bodyLarge,
                            fontWeight = FontWeight.SemiBold,
                            color = SageGreen,
                        )
                        if (hcAgo != null) {
                            Text(hcAgo, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                } else {
                    Text("No headcount data", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // =====================================================
        // Group assignment
        // =====================================================
        Card(
            Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(2.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                Text("Chick Group Assignment", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))

                if (currentGroup != null) {
                    Text("Current: Group #${currentGroup.id} (${currentGroup.currentCount} chicks, hatched ${currentGroup.hatchDate})",
                        style = MaterialTheme.typography.bodyMedium)
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(
                        onClick = {
                            Log.d("QuailSync", "Unassign tapped: brooder=$brooderId, group=${currentGroup.id}")
                            scope.launch {
                                try {
                                    val url = "${serverUrl.trimEnd('/')}/api/brooders/$brooderId/assign-group"
                                    Log.d("QuailSync", "DELETE $url")
                                    val code = withContext(Dispatchers.IO) {
                                        val req = okhttp3.Request.Builder().url(url).delete().build()
                                        val resp = okhttp3.OkHttpClient().newCall(req).execute()
                                        val body = resp.body?.string()
                                        Log.d("QuailSync", "Unassign response: ${resp.code} body=$body")
                                        resp.code
                                    }
                                    if (code in 200..299) {
                                        Toast.makeText(context, "Group unassigned from brooder", Toast.LENGTH_SHORT).show()
                                        error = null
                                    } else {
                                        error = "Unassign failed: HTTP $code"
                                        Toast.makeText(context, "Unassign failed: HTTP $code", Toast.LENGTH_SHORT).show()
                                    }
                                } catch (e: Exception) {
                                    Log.e("QuailSync", "Unassign failed", e)
                                    error = "Unassign failed: ${e.message}"
                                    Toast.makeText(context, "Unassign failed: ${e.message}", Toast.LENGTH_SHORT).show()
                                }
                                refreshKey++
                            }
                        },
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = AlertRed),
                    ) { Text("Unassign Group") }
                } else {
                    Text("No chick group assigned", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                Spacer(Modifier.height(12.dp))

                // Dropdown + Assign button
                val availableGroups = allGroups.filter { it.status == "Active" && (it.brooderId == null || it.brooderId == brooderId) }

                if (availableGroups.isNotEmpty()) {
                    val selectedGroup = availableGroups.find { it.id == selectedGroupId }
                    val displayText = selectedGroup?.let {
                        "Group #${it.id} — ${it.currentCount} chicks"
                    } ?: ""

                    ExposedDropdownMenuBox(groupExpanded, { groupExpanded = it }) {
                        OutlinedTextField(
                            value = displayText,
                            onValueChange = {},
                            readOnly = true,
                            label = { Text("Select chick group") },
                            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(groupExpanded) },
                            modifier = Modifier.menuAnchor().fillMaxWidth(),
                        )
                        ExposedDropdownMenu(groupExpanded, { groupExpanded = false }) {
                            availableGroups.forEach { g ->
                                DropdownMenuItem(
                                    text = { Text("Group #${g.id} — ${g.currentCount} chicks, hatched ${g.hatchDate}") },
                                    onClick = {
                                        Log.d("QuailSync", "Dropdown: selected group ${g.id}")
                                        selectedGroupId = g.id
                                        groupExpanded = false
                                    },
                                )
                            }
                        }
                    }

                    Spacer(Modifier.height(8.dp))

                    Button(
                        onClick = {
                            val gid = selectedGroupId ?: return@Button
                            Log.d("QuailSync", "Assign button tapped: group=$gid -> brooder=$brooderId")
                            scope.launch {
                                try {
                                    val url = "${serverUrl.trimEnd('/')}/api/brooders/$brooderId/assign-group"
                                    val jsonBody = """{"group_id": $gid}"""
                                    Log.d("QuailSync", "PUT $url body=$jsonBody")
                                    val (code, respBody) = withContext(Dispatchers.IO) {
                                        val body = jsonBody.toRequestBody("application/json".toMediaType())
                                        val req = okhttp3.Request.Builder().url(url).put(body).build()
                                        val resp = okhttp3.OkHttpClient().newCall(req).execute()
                                        val b = resp.body?.string()
                                        Log.d("QuailSync", "Assign response: ${resp.code} body=$b")
                                        Pair(resp.code, b)
                                    }
                                    if (code in 200..299) {
                                        Log.d("QuailSync", "Assign OK: group $gid -> brooder $brooderId")
                                        Toast.makeText(context, "Group #$gid assigned to brooder", Toast.LENGTH_SHORT).show()
                                        selectedGroupId = null
                                        error = null
                                    } else {
                                        error = "Assign failed: HTTP $code — $respBody"
                                        Toast.makeText(context, "Assign failed: HTTP $code", Toast.LENGTH_SHORT).show()
                                    }
                                } catch (e: Exception) {
                                    Log.e("QuailSync", "Assign failed", e)
                                    error = "Assign failed: ${e.message}"
                                    Toast.makeText(context, "Assign failed: ${e.message}", Toast.LENGTH_SHORT).show()
                                }
                                refreshKey++
                            }
                        },
                        enabled = selectedGroupId != null,
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) {
                        Text("Assign Group to Brooder")
                    }
                } else if (currentGroup == null) {
                    Text("No active chick groups available", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // =====================================================
        // Residents
        // =====================================================
        Card(
            Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(2.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Text("Residents", style = MaterialTheme.typography.titleMedium)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        // Issue #14 — assign a previously-graduated group to
                        // this hutch. Hutches only; brooders/incubators don't
                        // own graduated groups so the button would be useless.
                        if (isHutch) {
                            TextButton(onClick = { showAssignGroupDialog.value = true }) {
                                Icon(Icons.Default.Group, null, Modifier.size(16.dp), tint = SageGreen)
                                Spacer(Modifier.width(4.dp))
                                Text("Assign Group", color = SageGreen)
                            }
                        }
                        // Issue #13 — assign unhoused adult birds to this unit.
                        TextButton(onClick = { showAssignDialog.value = true }) {
                            Icon(Icons.Default.Add, null, Modifier.size(16.dp), tint = SageGreen)
                            Spacer(Modifier.width(4.dp))
                            Text("Assign Birds", color = SageGreen)
                        }
                    }
                }
                Spacer(Modifier.height(8.dp))

                // `residentGroups` already includes Active groups (for brooders)
                // and Graduated groups (for hutches) — server-side filtering at
                // /api/brooders/{id}/residents (issue #14).
                val groupsInBrooder = residentGroups

                if (groupsInBrooder.isEmpty() && residentBirds.isEmpty()) {
                    Text("No residents", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                groupsInBrooder.forEach { g ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(32.dp).clip(CircleShape).background(SageGreenLight), contentAlignment = Alignment.Center) {
                            Text("${g.currentCount}", style = MaterialTheme.typography.labelLarge, color = Color.White, fontWeight = FontWeight.Bold)
                        }
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            val role = if (g.status == "Graduated") "Graduated" else "Chick"
                            Text("$role Group #${g.id}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            Text("${g.currentCount} of ${g.initialCount ?: "?"} birds, hatched ${g.hatchDate}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        // Issue #14 — detach a graduated group from this hutch.
                        // Only shown on the hutch path (Graduated status); for
                        // Active chick groups, the existing "Unassign Group"
                        // button at the top handles it.
                        if (isHutch && g.status == "Graduated") {
                            IconButton(onClick = {
                                scope.launch {
                                    try {
                                        // Retrofit/Gson default config strips
                                        // null fields, so a plain
                                        // updateChickGroup(id, mapOf("housing_id" to null))
                                        // would send {} and the server-side
                                        // "is field present?" check would miss
                                        // it. Use a raw PUT with explicit JSON.
                                        val url = "${serverUrl.trimEnd('/')}/api/chick-groups/${g.id}"
                                        withContext(Dispatchers.IO) {
                                            val body = """{"housing_id": null}""".toRequestBody("application/json".toMediaType())
                                            val req = okhttp3.Request.Builder().url(url).put(body).build()
                                            okhttp3.OkHttpClient().newCall(req).execute().close()
                                        }
                                        // Also unhouse every bird that came
                                        // from this group, so they don't get
                                        // orphaned in the hutch's bird list.
                                        val groupBirdIds = residentBirds
                                            .filter { it.chickGroupId == g.id }
                                            .map { it.id }
                                        if (groupBirdIds.isNotEmpty()) {
                                            api.unassignBirdsFromHousing(
                                                brooderId,
                                                com.quailsync.app.data.BirdAssignmentRequest(groupBirdIds),
                                            )
                                        }
                                        refreshKey++
                                    } catch (e: Exception) {
                                        Log.e("QuailSync", "detach group ${g.id} failed", e)
                                    }
                                }
                            }) {
                                Icon(Icons.Default.Close, contentDescription = "Detach group", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }

                if (groupsInBrooder.isNotEmpty() && residentBirds.isNotEmpty()) {
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                }

                residentBirds.forEach { b ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(32.dp).clip(CircleShape).background(parseBandColor(b.bandColor)), contentAlignment = Alignment.Center) {
                            Text("\uD83D\uDC25", fontSize = 14.sp)
                        }
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(b.bandId ?: "Bird #${b.id}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            Text("${b.sex?.replaceFirstChar { it.uppercase() } ?: "Unknown"} · ${b.status?.replaceFirstChar { it.uppercase() } ?: ""}",
                                style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        // Issue #13 — remove this bird from the housing unit.
                        IconButton(onClick = {
                            scope.launch {
                                try {
                                    api.unassignBirdsFromHousing(
                                        brooderId,
                                        com.quailsync.app.data.BirdAssignmentRequest(listOf(b.id)),
                                    )
                                    refreshKey++
                                } catch (e: Exception) {
                                    Log.e("QuailSync", "unassign bird ${b.id} failed", e)
                                }
                            }
                        }) {
                            Icon(Icons.Default.Close, contentDescription = "Remove from housing",
                                tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }

        // Error display
        if (error != null) {
            Spacer(Modifier.height(8.dp))
            Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(8.dp),
                colors = CardDefaults.cardColors(containerColor = Color(0xFFFFEBEE))) {
                Text(error!!, Modifier.padding(12.dp), color = AlertRed, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }

    // Issue #13 — picker dialog. Shown when the Residents card's "Assign
    // Birds" button is tapped. Lists unhoused active birds with checkboxes;
    // confirming POSTs to /api/brooders/{id}/assign-birds and refreshes.
    if (showAssignDialog.value) {
        val unhoused = allBirds.filter {
            it.housingId == null && it.status?.lowercase() == "active"
        }
        val selected = remember { mutableStateListOf<Int>() }
        AlertDialog(
            onDismissRequest = {
                showAssignDialog.value = false
                selected.clear()
            },
            title = { Text("Assign Birds") },
            text = {
                if (unhoused.isEmpty()) {
                    Text(
                        "No unhoused birds available.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState())) {
                        unhoused.forEach { b ->
                            val checked = selected.contains(b.id)
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .padding(vertical = 2.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Checkbox(
                                    checked = checked,
                                    onCheckedChange = {
                                        if (it) selected.add(b.id) else selected.remove(b.id)
                                    },
                                )
                                Spacer(Modifier.width(4.dp))
                                Column(Modifier.weight(1f)) {
                                    Text(
                                        b.bandId ?: "Bird #${b.id}",
                                        style = MaterialTheme.typography.bodyMedium,
                                    )
                                    val sub = listOfNotNull(
                                        b.sex?.replaceFirstChar { it.uppercase() },
                                        b.bandColor?.let { "band: $it" },
                                    ).joinToString(" · ")
                                    if (sub.isNotEmpty()) {
                                        Text(
                                            sub,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            },
            confirmButton = {
                Button(
                    onClick = {
                        val toAssign = selected.toList()
                        scope.launch {
                            try {
                                api.assignBirdsToHousing(
                                    brooderId,
                                    com.quailsync.app.data.BirdAssignmentRequest(toAssign),
                                )
                                showAssignDialog.value = false
                                selected.clear()
                                refreshKey++
                            } catch (e: Exception) {
                                Log.e("QuailSync", "assign birds failed", e)
                            }
                        }
                    },
                    enabled = selected.isNotEmpty(),
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) { Text("Assign (${selected.size})") }
            },
            dismissButton = {
                TextButton(onClick = {
                    showAssignDialog.value = false
                    selected.clear()
                }) { Text("Cancel") }
            },
        )
    }

    // Issue #14 — "Assign Graduated Group" picker. Hutch-only. Lists every
    // Graduated chick group whose housing_id is null OR points at another
    // hutch (lets the user move groups between hutches). Single-select to
    // keep the model simple; the user can re-open the dialog to add another.
    if (showAssignGroupDialog.value) {
        val available = allGroups.filter { it.status == "Graduated" && it.housingId != brooderId }
        val pickedId = remember { mutableStateOf<Int?>(null) }
        AlertDialog(
            onDismissRequest = { showAssignGroupDialog.value = false; pickedId.value = null },
            title = { Text("Assign Graduated Group") },
            text = {
                if (available.isEmpty()) {
                    Text(
                        "No unassigned graduated groups available.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Column(Modifier.heightIn(max = 360.dp).verticalScroll(rememberScrollState())) {
                        available.forEach { g ->
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .padding(vertical = 2.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Checkbox(
                                    checked = pickedId.value == g.id,
                                    onCheckedChange = { if (it) pickedId.value = g.id else pickedId.value = null },
                                )
                                Spacer(Modifier.width(4.dp))
                                Column(Modifier.weight(1f)) {
                                    Text("Group #${g.id}", style = MaterialTheme.typography.bodyMedium)
                                    val lineageLabel = if (g.lineages.isNotEmpty()) {
                                        com.quailsync.app.data.formatLineages(g.lineages, maxShown = 2)
                                    } else "(no lineage)"
                                    val whereLabel = g.housingId?.let { " · in hutch #$it" } ?: ""
                                    Text(
                                        "$lineageLabel · ${g.currentCount} birds$whereLabel",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                    }
                }
            },
            confirmButton = {
                Button(
                    onClick = {
                        val gid = pickedId.value ?: return@Button
                        scope.launch {
                            try {
                                val resp = api.assignGraduatedGroupToHousing(
                                    brooderId,
                                    com.quailsync.app.data.AssignGraduatedGroupRequest(gid),
                                )
                                Toast.makeText(
                                    context,
                                    "Group #${resp.groupId} assigned (${resp.birdsUpdated} birds)",
                                    Toast.LENGTH_SHORT,
                                ).show()
                                showAssignGroupDialog.value = false
                                pickedId.value = null
                                refreshKey++
                            } catch (e: retrofit2.HttpException) {
                                // Surface the server's message — e.g. a group that
                                // hasn't been banded yet has no individual bird
                                // records and can't be housed in a hutch.
                                val body = e.response()?.errorBody()?.string()
                                Log.e("QuailSync", "assign-graduated-group failed: $body", e)
                                Toast.makeText(
                                    context,
                                    body?.takeIf { it.isNotBlank() } ?: "Failed: ${e.message}",
                                    Toast.LENGTH_LONG,
                                ).show()
                            } catch (e: Exception) {
                                Log.e("QuailSync", "assign-graduated-group failed", e)
                                Toast.makeText(
                                    context,
                                    "Failed: ${e.message}",
                                    Toast.LENGTH_SHORT,
                                ).show()
                            }
                        }
                    },
                    enabled = pickedId.value != null,
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) { Text("Assign") }
            },
            dismissButton = {
                TextButton(onClick = { showAssignGroupDialog.value = false; pickedId.value = null }) { Text("Cancel") }
            },
        )
    }

}

private fun formatTimeAgo(timestamp: String?): String? {
    if (timestamp == null) return null
    return try {
        val instant = java.time.Instant.parse(timestamp)
        val ago = java.time.Duration.between(instant, java.time.Instant.now()).toMinutes()
        when {
            ago < 1 -> "just now"
            ago < 60 -> "${ago}m ago"
            else -> "${ago / 60}h ago"
        }
    } catch (_: Exception) {
        null
    }
}
