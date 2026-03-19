package com.quailsync.app.ui.screens

import android.util.Log
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
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
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.NfcScanResult
import com.quailsync.app.data.NfcService
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertRed
import com.quailsync.app.ui.theme.DustyRose
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.time.format.DateTimeFormatter

class NfcViewModel(val nfcService: NfcService) : ViewModel() {
    private val api = QuailSyncApi.create()

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    init {
        loadBirds()
    }

    private fun loadBirds() {
        viewModelScope.launch {
            try {
                _birds.value = api.getBirds()
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load birds for NFC", e)
            }
        }
    }

    fun lookupBirdByNfc(tagId: String, payload: String?) {
        viewModelScope.launch {
            // Try payload first (e.g. "BIRD-42"), then raw tag ID
            val lookupId = when {
                payload?.startsWith("BIRD-") == true -> payload
                else -> tagId
            }
            try {
                val bird = api.getBirdByNfcTag(lookupId)
                nfcService.updateScanWithBird(tagId, bird)
                Log.d("QuailSync", "NFC lookup found bird: ${bird.id} ${bird.bandId}")
            } catch (e: Exception) {
                // Also try extracting numeric ID from BIRD-N format
                if (payload?.startsWith("BIRD-") == true) {
                    try {
                        val birdId = payload.removePrefix("BIRD-").toInt()
                        val bird = _birds.value.find { it.id == birdId }
                        if (bird != null) {
                            nfcService.updateScanWithBird(tagId, bird)
                            Log.d("QuailSync", "NFC matched bird from local cache: ${bird.id}")
                            return@launch
                        }
                    } catch (_: NumberFormatException) {}
                }
                Log.d("QuailSync", "NFC lookup: no bird found for $lookupId")
            }
        }
    }

    fun startWriteMode(birdId: Int) {
        nfcService.enterWriteMode("BIRD-$birdId")
    }

    fun cancelWriteMode() {
        nfcService.cancelWriteMode()
    }
}

@Composable
fun NfcScreen(nfcService: NfcService, viewModel: NfcViewModel) {
    val lastScan by nfcService.lastScan.collectAsState()
    val scanHistory by nfcService.scanHistory.collectAsState()
    val writeMode by nfcService.writeMode.collectAsState()
    val pendingWriteData by nfcService.pendingWriteData.collectAsState()
    val writeResult by nfcService.writeResult.collectAsState()
    val isAvailable by nfcService.isAvailable.collectAsState()
    val birds by viewModel.birds.collectAsState()

    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Text(
                text = "NFC Scanner",
                style = MaterialTheme.typography.headlineMedium,
            )
        }

        if (!isAvailable) {
            item {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = Color(0xFFFFF3E0)),
                    shape = RoundedCornerShape(12.dp),
                ) {
                    Text(
                        text = "NFC is not available or not enabled on this device. Enable NFC in system settings to scan bird tags.",
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(16.dp),
                        color = Color(0xFF6D4C00),
                    )
                }
            }
        }

        // Scan prompt area
        item {
            NfcScanArea(writeMode = writeMode, pendingWriteData = pendingWriteData)
        }

        // Write result feedback
        if (writeResult != null) {
            item {
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(
                        containerColor = if (writeResult!!.success) Color(0xFFE8F5E9) else Color(0xFFFFEBEE),
                    ),
                    shape = RoundedCornerShape(12.dp),
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = writeResult!!.message,
                            style = MaterialTheme.typography.bodyMedium,
                            color = if (writeResult!!.success) Color(0xFF2E7D32) else Color(0xFFC62828),
                            modifier = Modifier.weight(1f),
                        )
                        IconButton(onClick = { nfcService.clearWriteResult() }) {
                            Icon(
                                Icons.Default.Close,
                                contentDescription = "Dismiss",
                                modifier = Modifier.size(18.dp),
                            )
                        }
                    }
                }
            }
        }

        // Last scan result
        if (lastScan != null) {
            item {
                Text(
                    text = "Last Scan",
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            item {
                NfcResultCard(scan = lastScan!!)
            }
        }

        // Write tag section
        item {
            HorizontalDivider()
            Spacer(modifier = Modifier.height(4.dp))
            WriteTagSection(
                birds = birds,
                writeMode = writeMode,
                onStartWrite = { viewModel.startWriteMode(it) },
                onCancel = { viewModel.cancelWriteMode() },
            )
        }

        // Scan history
        if (scanHistory.size > 1) {
            item {
                HorizontalDivider()
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = "Scan History",
                    style = MaterialTheme.typography.titleMedium,
                )
            }
            // Skip the first item (it's the lastScan already shown above)
            items(scanHistory.drop(1)) { scan ->
                NfcHistoryItem(scan = scan)
            }
        }

        item { Spacer(modifier = Modifier.height(8.dp)) }
    }
}

@Composable
fun NfcScanArea(writeMode: Boolean, pendingWriteData: String?) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (writeMode) DustyRose.copy(alpha = 0.15f)
                else SageGreenLight.copy(alpha = 0.15f),
        ),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            PulsingNfcIcon(writeMode = writeMode)

            Spacer(modifier = Modifier.height(16.dp))

            Text(
                text = if (writeMode) "Hold phone near tag to write"
                    else "Hold phone near NFC tag to scan",
                style = MaterialTheme.typography.titleMedium,
                textAlign = TextAlign.Center,
            )

            if (writeMode && pendingWriteData != null) {
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Writing: $pendingWriteData",
                    style = MaterialTheme.typography.bodyMedium,
                    color = DustyRose,
                    fontWeight = FontWeight.Medium,
                )
            }
        }
    }
}

@Composable
fun PulsingNfcIcon(writeMode: Boolean) {
    val transition = rememberInfiniteTransition(label = "nfc_pulse")
    val scale by transition.animateFloat(
        initialValue = 1f,
        targetValue = 1.15f,
        animationSpec = infiniteRepeatable(
            animation = tween(1000, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "nfc_scale",
    )
    val ringAlpha by transition.animateFloat(
        initialValue = 0.4f,
        targetValue = 0f,
        animationSpec = infiniteRepeatable(
            animation = tween(1500, easing = LinearEasing),
            repeatMode = RepeatMode.Restart,
        ),
        label = "nfc_ring",
    )

    val color = if (writeMode) DustyRose else SageGreen

    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier.size(120.dp),
    ) {
        // Outer pulsing ring
        Box(
            modifier = Modifier
                .size(120.dp)
                .scale(scale)
                .alpha(ringAlpha)
                .border(3.dp, color, CircleShape),
        )
        // Inner circle with icon
        Box(
            modifier = Modifier
                .size(80.dp)
                .clip(CircleShape)
                .background(color.copy(alpha = 0.12f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = if (writeMode) Icons.Default.Edit else Icons.Default.Nfc,
                contentDescription = if (writeMode) "Write mode" else "Scan mode",
                modifier = Modifier.size(40.dp),
                tint = color,
            )
        }
    }
}

@Composable
fun NfcResultCard(scan: NfcScanResult) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "Tag: ${scan.tagId}",
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.Medium,
                )
                Text(
                    text = scan.timestamp.format(DateTimeFormatter.ofPattern("HH:mm:ss")),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            if (scan.payload != null) {
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = "Data: ${scan.payload}",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }

            if (scan.bird != null) {
                Spacer(modifier = Modifier.height(12.dp))
                NfcBirdInfo(bird = scan.bird)
            } else if (scan.payload?.startsWith("BIRD-") == true) {
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "Looking up bird...",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
fun NfcBirdInfo(bird: Bird) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        colors = CardDefaults.cardColors(
            containerColor = AlertGreen.copy(alpha = 0.1f),
        ),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(parseBandColor(bird.bandColor)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Default.Pets,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(20.dp),
                )
            }
            Spacer(modifier = Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = bird.bandId ?: "Bird #${bird.id}",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (bird.sex != null) {
                        Text(
                            text = bird.sex.replaceFirstChar { it.uppercase() },
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                    if (bird.bloodlineName != null) {
                        Text(
                            text = bird.bloodlineName,
                            style = MaterialTheme.typography.bodyMedium,
                            color = SageGreen,
                        )
                    }
                    if (bird.status != null) {
                        Text(
                            text = bird.status.replaceFirstChar { it.uppercase() },
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun WriteTagSection(
    birds: List<Bird>,
    writeMode: Boolean,
    onStartWrite: (Int) -> Unit,
    onCancel: () -> Unit,
) {
    var selectedBirdId by remember { mutableStateOf<Int?>(null) }
    var expanded by remember { mutableStateOf(false) }

    Column {
        Text(
            text = "Write Tag",
            style = MaterialTheme.typography.titleMedium,
        )
        Spacer(modifier = Modifier.height(8.dp))

        if (writeMode) {
            OutlinedButton(
                onClick = onCancel,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = AlertRed),
            ) {
                Icon(Icons.Default.Close, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(6.dp))
                Text("Cancel Write Mode")
            }
        } else {
            // Bird selector dropdown
            ExposedDropdownMenuBox(
                expanded = expanded,
                onExpandedChange = { expanded = it },
            ) {
                OutlinedTextField(
                    value = selectedBirdId?.let { id ->
                        birds.find { it.id == id }?.let { b ->
                            "${b.bandId ?: "Bird #${b.id}"} — ${b.bloodlineName ?: b.sex ?: ""}"
                        } ?: "Bird #$id"
                    } ?: "",
                    onValueChange = {},
                    readOnly = true,
                    label = { Text("Select bird to write") },
                    trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
                    modifier = Modifier
                        .menuAnchor()
                        .fillMaxWidth(),
                )
                ExposedDropdownMenu(
                    expanded = expanded,
                    onDismissRequest = { expanded = false },
                ) {
                    birds.forEach { bird ->
                        DropdownMenuItem(
                            text = {
                                Text("${bird.bandId ?: "Bird #${bird.id}"} — ${bird.bloodlineName ?: ""}")
                            },
                            onClick = {
                                selectedBirdId = bird.id
                                expanded = false
                            },
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(8.dp))

            Button(
                onClick = { selectedBirdId?.let { onStartWrite(it) } },
                enabled = selectedBirdId != null,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) {
                Icon(Icons.Default.Edit, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(6.dp))
                Text("Write BIRD-${selectedBirdId ?: "?"} to Tag")
            }
        }
    }
}

@Composable
fun NfcHistoryItem(scan: NfcScanResult) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    text = scan.tagId,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.Medium,
                )
                if (scan.payload != null) {
                    Text(
                        text = scan.payload,
                        style = MaterialTheme.typography.bodyMedium,
                        color = SageGreen,
                    )
                }
            }
            if (scan.bird != null) {
                Text(
                    text = "${scan.bird.bandId ?: "Bird #${scan.bird.id}"} — ${scan.bird.bloodlineName ?: ""}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Text(
            text = scan.timestamp.format(DateTimeFormatter.ofPattern("HH:mm")),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
