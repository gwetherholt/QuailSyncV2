package com.quailsync.app.ui.screens

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.Log
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.BirdWeight
import com.quailsync.app.data.Bloodline
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File

// =====================================================================
// ViewModel
// =====================================================================

class FlockViewModel : ViewModel() {
    private val api = QuailSyncApi.create()

    private val _birds = MutableStateFlow<List<Bird>>(emptyList())
    val birds: StateFlow<List<Bird>> = _birds.asStateFlow()

    private val _bloodlines = MutableStateFlow<List<Bloodline>>(emptyList())
    val bloodlines: StateFlow<List<Bloodline>> = _bloodlines.asStateFlow()

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
            val birdList = api.getBirds()
            Log.d("QuailSync", "Birds loaded: ${birdList.size}")
            _birds.value = birdList
            val bloodlineList = try { api.getBloodlines() } catch (e: Exception) { Log.e("QuailSync", "Failed to load bloodlines", e); emptyList() }
            _bloodlines.value = bloodlineList
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load birds", e)
        } finally {
            _isLoading.value = false
        }
    }

    suspend fun getBirdWeights(birdId: Int): List<BirdWeight> {
        return try { api.getBirdWeights(birdId) } catch (e: Exception) { Log.e("QuailSync", "Failed to load weights for bird $birdId", e); emptyList() }
    }

    fun uploadBirdPhoto(birdId: Int, uri: Uri, context: android.content.Context) {
        viewModelScope.launch {
            // Always save to the standard local path first
            try {
                val dir = File(context.filesDir, "bird_photos").also { it.mkdirs() }
                val dest = File(dir, "bird_${birdId}.jpg")
                context.contentResolver.openInputStream(uri)?.use { input ->
                    dest.outputStream().use { input.copyTo(it) }
                }
                Log.d("QuailSync", "Photo saved locally: ${dest.absolutePath}")
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to save photo locally", e)
            }
            // Try server upload
            try {
                val bytes = context.contentResolver.openInputStream(uri)?.readBytes() ?: return@launch
                val part = okhttp3.MultipartBody.Part.createFormData(
                    "photo", "bird_${birdId}.jpg", bytes.toRequestBody("image/jpeg".toMediaType()),
                )
                api.uploadBirdPhoto(birdId, part)
                Log.d("QuailSync", "Photo uploaded for bird $birdId")
            } catch (e: Exception) {
                Log.e("QuailSync", "Photo upload failed (saved locally)", e)
            }
        }
    }

    /** Save a bitmap directly (used from TakePicturePreview). */
    fun saveBirdPhotoBitmap(birdId: Int, bitmap: Bitmap, context: android.content.Context) {
        viewModelScope.launch {
            try {
                val dir = File(context.filesDir, "bird_photos").also { it.mkdirs() }
                val file = File(dir, "bird_${birdId}.jpg")
                file.outputStream().use { bitmap.compress(Bitmap.CompressFormat.JPEG, 90, it) }
                Log.d("QuailSync", "Photo saved: ${file.absolutePath}")
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to save photo", e)
            }
        }
    }
}

// =====================================================================
// Helpers
// =====================================================================

sealed class FlockFilter {
    data object All : FlockFilter()
    data object Males : FlockFilter()
    data object Females : FlockFilter()
    data class ByBloodline(val bloodlineId: Int, val name: String) : FlockFilter()
}

private fun formatSex(sex: String?): String {
    return when (sex?.lowercase()) {
        "male", "m" -> "Male"
        "female", "f" -> "Female"
        else -> "Unknown"
    }
}

internal fun parseBandColor(color: String?): Color {
    if (color == null) return Color(0xFF9E9E9E)
    return try {
        val hex = color.removePrefix("#")
        Color(android.graphics.Color.parseColor("#$hex"))
    } catch (_: Exception) {
        when (color.lowercase()) {
            "red" -> Color(0xFFCC4444)
            "blue" -> Color(0xFF4477CC)
            "green" -> Color(0xFF6A8B5E)
            "yellow" -> Color(0xFFCCA844)
            "orange" -> Color(0xFFCC8844)
            "purple" -> Color(0xFF8855AA)
            "pink" -> Color(0xFFD4A0A0)
            "white" -> Color(0xFFCCCCCC)
            "black" -> Color(0xFF333333)
            else -> Color(0xFF9E9E9E)
        }
    }
}

/** Load a bird's photo bitmap from local storage. Returns null if not found. */
@Composable
fun rememberBirdPhoto(birdId: Int, refreshKey: Int = 0): Bitmap? {
    val context = LocalContext.current
    return remember(birdId, refreshKey) {
        val file = File(context.filesDir, "bird_photos/bird_${birdId}.jpg")
        if (file.exists()) {
            try { BitmapFactory.decodeFile(file.absolutePath) } catch (_: Exception) { null }
        } else null
    }
}

// =====================================================================
// Flock Screen
// =====================================================================

@Composable
fun FlockScreen(viewModel: FlockViewModel = viewModel()) {
    val birds by viewModel.birds.collectAsState()
    val bloodlines by viewModel.bloodlines.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    var selectedFilter by remember { mutableStateOf<FlockFilter>(FlockFilter.All) }
    var selectedBird by remember { mutableStateOf<Bird?>(null) }

    val bloodlineMap = remember(bloodlines) { bloodlines.associateBy { it.id } }

    val filteredBirds = remember(birds, selectedFilter) {
        when (selectedFilter) {
            FlockFilter.All -> birds
            FlockFilter.Males -> birds.filter { it.sex?.lowercase() == "male" }
            FlockFilter.Females -> birds.filter { it.sex?.lowercase() == "female" }
            is FlockFilter.ByBloodline -> birds.filter { it.bloodlineId == (selectedFilter as FlockFilter.ByBloodline).bloodlineId }
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Flock", style = MaterialTheme.typography.headlineMedium)
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("${filteredBirds.size} bird${if (filteredBirds.size != 1) "s" else ""}", style = MaterialTheme.typography.bodyMedium)
                Spacer(Modifier.width(8.dp))
                if (isRefreshing) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = SageGreen)
                } else {
                    IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
                }
            }
        }

        if (!isLoading || birds.isNotEmpty()) {
            FlockFilterChips(bloodlines, selectedFilter) { selectedFilter = it }
        }

        when {
            isLoading && birds.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = SageGreen) }
            }
            birds.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(Icons.Default.Pets, null, Modifier.size(64.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.height(16.dp))
                        Text("No birds registered yet.\nAdd birds from the web dashboard or CLI.", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    }
                }
            }
            else -> {
                LazyColumn(contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    items(filteredBirds, key = { it.id }) { bird ->
                        BirdCard(bird, bird.bloodlineName ?: bloodlineMap[bird.bloodlineId]?.name) { selectedBird = bird }
                    }
                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }

    if (selectedBird != null) {
        BirdDetailDialog(selectedBird!!, selectedBird!!.bloodlineName ?: bloodlineMap[selectedBird!!.bloodlineId]?.name, viewModel) { selectedBird = null }
    }
}

// =====================================================================
// Filter Chips
// =====================================================================

@Composable
fun FlockFilterChips(bloodlines: List<Bloodline>, selectedFilter: FlockFilter, onFilterSelected: (FlockFilter) -> Unit) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val chipColors = FilterChipDefaults.filterChipColors(selectedContainerColor = SageGreen, selectedLabelColor = Color.White)
        FilterChip(selectedFilter is FlockFilter.All, { onFilterSelected(FlockFilter.All) }, { Text("All") }, colors = chipColors)
        FilterChip(selectedFilter is FlockFilter.Males, { onFilterSelected(FlockFilter.Males) }, { Text("Males") }, colors = chipColors)
        FilterChip(selectedFilter is FlockFilter.Females, { onFilterSelected(FlockFilter.Females) }, { Text("Females") }, colors = chipColors)
        bloodlines.forEach { bl ->
            FilterChip(
                selectedFilter is FlockFilter.ByBloodline && (selectedFilter as FlockFilter.ByBloodline).bloodlineId == bl.id,
                { onFilterSelected(FlockFilter.ByBloodline(bl.id, bl.name)) },
                { Text(bl.name) }, colors = chipColors,
            )
        }
    }
}

// =====================================================================
// Bird Card — with photo thumbnail
// =====================================================================

@Composable
fun BirdCard(bird: Bird, bloodlineName: String?, onClick: () -> Unit) {
    val photo = rememberBirdPhoto(bird.id)

    Card(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Row(Modifier.fillMaxWidth().padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
            // Photo or band color circle
            Box(Modifier.size(42.dp).clip(CircleShape), contentAlignment = Alignment.Center) {
                if (photo != null) {
                    Image(
                        bitmap = photo.asImageBitmap(),
                        contentDescription = "Bird photo",
                        modifier = Modifier.fillMaxSize().clip(CircleShape),
                        contentScale = ContentScale.Crop,
                    )
                } else {
                    Box(
                        Modifier.fillMaxSize().background(parseBandColor(bird.bandColor)),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(bird.id.toString(), style = MaterialTheme.typography.labelLarge, color = Color.White, fontWeight = FontWeight.Bold)
                    }
                }
            }

            Spacer(Modifier.width(12.dp))

            Column(Modifier.weight(1f)) {
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                    Text(bird.bandId ?: "Bird #${bird.id}", style = MaterialTheme.typography.titleMedium)
                    StatusBadge(bird.status)
                }
                Spacer(Modifier.height(4.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(formatSex(bird.sex), style = MaterialTheme.typography.bodyMedium)
                    if (bloodlineName != null) Text(bloodlineName, style = MaterialTheme.typography.bodyMedium, color = SageGreen)
                }
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                    if (bird.hatchDate != null) Text("Hatched ${bird.hatchDate}", style = MaterialTheme.typography.bodyMedium)
                    if (bird.latestWeight != null) Text("%.0fg".format(bird.latestWeight), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium, color = SageGreen)
                }
            }
        }
    }
}

@Composable
fun StatusBadge(status: String?) {
    val displayStatus = status?.replaceFirstChar { it.uppercase() } ?: "Unknown"
    val bgColor = when (status?.lowercase()) { "active" -> SageGreenLight; "culled", "deceased" -> Color(0xFFE0B0B0); else -> MaterialTheme.colorScheme.surfaceVariant }
    val textColor = when (status?.lowercase()) { "active" -> Color(0xFF2D4A1E); "culled", "deceased" -> Color(0xFF6B2D2D); else -> MaterialTheme.colorScheme.onSurfaceVariant }
    Text(displayStatus, style = MaterialTheme.typography.labelLarge, color = textColor, modifier = Modifier.clip(RoundedCornerShape(6.dp)).background(bgColor).padding(horizontal = 8.dp, vertical = 2.dp))
}

// =====================================================================
// Bird Detail Dialog — with profile photo and Take Photo button
// =====================================================================

@Composable
fun BirdDetailDialog(bird: Bird, bloodlineName: String?, viewModel: FlockViewModel, onDismiss: () -> Unit) {
    val context = LocalContext.current
    var weights by remember { mutableStateOf<List<BirdWeight>>(emptyList()) }
    var weightsLoaded by remember { mutableStateOf(false) }
    var photoRefreshKey by remember { mutableIntStateOf(0) }
    val photo = rememberBirdPhoto(bird.id, photoRefreshKey)

    val photoLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.TakePicturePreview(),
    ) { bitmap ->
        if (bitmap != null) {
            viewModel.saveBirdPhotoBitmap(bird.id, bitmap, context)
            photoRefreshKey++ // trigger recomposition to show new photo
        }
    }

    androidx.compose.runtime.LaunchedEffect(bird.id) {
        weights = viewModel.getBirdWeights(bird.id)
        weightsLoaded = true
    }

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Card(
            Modifier.fillMaxWidth().padding(16.dp),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            LazyColumn(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                // Photo + header
                item {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth()) {
                        // Close button row
                        Row(Modifier.fillMaxWidth(), Arrangement.End) {
                            IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, "Close") }
                        }

                        // Profile photo or placeholder
                        Box(
                            Modifier.size(100.dp).clip(CircleShape),
                            contentAlignment = Alignment.Center,
                        ) {
                            if (photo != null) {
                                Image(
                                    bitmap = photo.asImageBitmap(),
                                    contentDescription = "Bird photo",
                                    modifier = Modifier.fillMaxSize().clip(CircleShape),
                                    contentScale = ContentScale.Crop,
                                )
                            } else {
                                Box(
                                    Modifier.fillMaxSize().background(parseBandColor(bird.bandColor)),
                                    contentAlignment = Alignment.Center,
                                ) {
                                    Icon(Icons.Default.Pets, null, Modifier.size(48.dp), tint = Color.White)
                                }
                            }
                        }

                        Spacer(Modifier.height(8.dp))
                        Text(bird.bandId ?: "Bird #${bird.id}", style = MaterialTheme.typography.headlineMedium)
                        if (bloodlineName != null) Text(bloodlineName, style = MaterialTheme.typography.titleMedium, color = SageGreen)
                        Spacer(Modifier.height(4.dp))
                        StatusBadge(bird.status)

                        Spacer(Modifier.height(8.dp))
                        Button(
                            onClick = { photoLauncher.launch(null) },
                            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                        ) {
                            Icon(Icons.Default.CameraAlt, null, Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text(if (photo != null) "Update Photo" else "Take Photo")
                        }
                    }
                }

                item { HorizontalDivider() }

                // Details
                item { DetailRow("Sex", formatSex(bird.sex)) }
                if (bird.species != null) { item { DetailRow("Species", bird.species) } }
                if (bird.hatchDate != null) { item { DetailRow("Hatch Date", bird.hatchDate) } }
                if (bird.bandColor != null) { item { DetailRow("Band Color", bird.bandColor) } }
                if (bird.sireId != null) { item { DetailRow("Sire", "Bird #${bird.sireId}") } }
                if (bird.damId != null) { item { DetailRow("Dam", "Bird #${bird.damId}") } }
                if (bird.brooderId != null) { item { DetailRow("Brooder", "#${bird.brooderId}") } }

                if (bird.notes != null) {
                    item {
                        Spacer(Modifier.height(4.dp))
                        Text("Notes", style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(4.dp))
                        Text(bird.notes, style = MaterialTheme.typography.bodyMedium)
                    }
                }

                // Weight history
                item {
                    Spacer(Modifier.height(8.dp))
                    HorizontalDivider()
                    Spacer(Modifier.height(8.dp))
                    Text("Weight History", style = MaterialTheme.typography.titleMedium)
                }
                if (!weightsLoaded) {
                    item { Box(Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = SageGreen, modifier = Modifier.size(24.dp), strokeWidth = 2.dp) } }
                } else if (weights.isEmpty()) {
                    item { Text("No weight records", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                } else {
                    items(weights) { w ->
                        Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
                            Text(w.recordedAt ?: "—", style = MaterialTheme.typography.bodyMedium)
                            Text("%.1f g".format(w.weightGrams), style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun DetailRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
    }
}
