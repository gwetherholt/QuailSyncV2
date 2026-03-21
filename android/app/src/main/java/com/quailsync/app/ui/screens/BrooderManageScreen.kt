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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Pets
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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import com.quailsync.app.data.Bird
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
    var allGroups by remember { mutableStateOf<List<ChickGroupDto>>(emptyList()) }
    var residentBirds by remember { mutableStateOf<List<Bird>>(emptyList()) }
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
            targetTemp = try { api.getBrooderTargetTemp(brooderId) } catch (e: Exception) { Log.e("QuailSync", "targetTemp failed", e); null }

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
            residentBirds = try {
                val r = api.getBrooderResidents(brooderId)
                r.individualBirds
            } catch (e: Exception) {
                Log.e("QuailSync", "residents failed (treating as empty)", e)
                emptyList()
            }
            Log.d("QuailSync", "BrooderManage: loaded. allGroups=${allGroups.size}, birds=${residentBirds.size}")
        } catch (e: Exception) {
            Log.e("QuailSync", "Load failed", e)
            error = e.message
        }
        isLoading = false
    }

    Column(Modifier.fillMaxSize().padding(16.dp).verticalScroll(rememberScrollState())) {
        // Header
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.Default.ArrowBack, "Back") }
            Spacer(Modifier.width(8.dp))
            Text("Manage Brooder #$brooderId", style = MaterialTheme.typography.headlineMedium)
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
        if (targetTemp != null) {
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
                        Text("%.0f°F".format(tt.targetTempF), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Bold)
                    }
                    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                        Text("Range", style = MaterialTheme.typography.bodyMedium)
                        Text("%.0f–%.0f°F".format(tt.minTempF, tt.maxTempF), style = MaterialTheme.typography.bodyMedium)
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
                    val currentWeek = tt.week.coerceIn(1, 6)
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
                Text("Residents", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))

                // Show the current chick group from our derived state
                val groupsInBrooder = allGroups.filter { it.brooderId == brooderId && it.status == "Active" }

                if (groupsInBrooder.isEmpty() && residentBirds.isEmpty()) {
                    Text("No residents", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                groupsInBrooder.forEach { g ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(32.dp).clip(CircleShape).background(SageGreenLight), contentAlignment = Alignment.Center) {
                            Text("${g.currentCount}", style = MaterialTheme.typography.labelLarge, color = Color.White, fontWeight = FontWeight.Bold)
                        }
                        Spacer(Modifier.width(10.dp))
                        Column {
                            Text("Chick Group #${g.id}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            Text("${g.currentCount} of ${g.initialCount} chicks, hatched ${g.hatchDate}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }

                if (groupsInBrooder.isNotEmpty() && residentBirds.isNotEmpty()) {
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                }

                residentBirds.forEach { b ->
                    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(32.dp).clip(CircleShape).background(parseBandColor(b.bandColor)), contentAlignment = Alignment.Center) {
                            Icon(Icons.Default.Pets, null, tint = Color.White, modifier = Modifier.size(18.dp))
                        }
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(b.bandId ?: "Bird #${b.id}", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                            Text("${b.sex?.replaceFirstChar { it.uppercase() } ?: "Unknown"} · ${b.status?.replaceFirstChar { it.uppercase() } ?: ""}",
                                style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
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
}
