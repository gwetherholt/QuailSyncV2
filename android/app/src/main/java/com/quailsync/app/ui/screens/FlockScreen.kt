package com.quailsync.app.ui.screens

import android.util.Log
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
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Refresh
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
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
            val birdList = api.getBirds()
            Log.d("QuailSync", "Birds loaded: ${birdList.size}")
            _birds.value = birdList

            val bloodlineList = try {
                api.getBloodlines()
            } catch (e: Exception) {
                Log.e("QuailSync", "Failed to load bloodlines", e)
                emptyList()
            }
            Log.d("QuailSync", "Bloodlines loaded: ${bloodlineList.size}")
            _bloodlines.value = bloodlineList
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load birds", e)
        } finally {
            _isLoading.value = false
        }
    }

    suspend fun getBirdWeights(birdId: Int): List<BirdWeight> {
        return try {
            api.getBirdWeights(birdId)
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load weights for bird $birdId", e)
            emptyList()
        }
    }
}

sealed class FlockFilter {
    data object All : FlockFilter()
    data object Males : FlockFilter()
    data object Females : FlockFilter()
    data class ByBloodline(val bloodlineId: Int, val name: String) : FlockFilter()
}

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
            is FlockFilter.ByBloodline -> birds.filter {
                it.bloodlineId == (selectedFilter as FlockFilter.ByBloodline).bloodlineId
            }
        }
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
                text = "Flock",
                style = MaterialTheme.typography.headlineMedium,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    text = "${filteredBirds.size} bird${if (filteredBirds.size != 1) "s" else ""}",
                    style = MaterialTheme.typography.bodyMedium,
                )
                Spacer(modifier = Modifier.width(8.dp))
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
        }

        if (!isLoading || birds.isNotEmpty()) {
            FlockFilterChips(
                bloodlines = bloodlines,
                selectedFilter = selectedFilter,
                onFilterSelected = { selectedFilter = it },
            )
        }

        when {
            isLoading && birds.isEmpty() -> {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator(color = SageGreen)
                }
            }
            birds.isEmpty() -> {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(
                            imageVector = Icons.Default.Pets,
                            contentDescription = null,
                            modifier = Modifier.size(64.dp),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Spacer(modifier = Modifier.height(16.dp))
                        Text(
                            text = "No birds registered yet.\nAdd birds from the web dashboard or CLI.",
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
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    items(filteredBirds, key = { it.id }) { bird ->
                        BirdCard(
                            bird = bird,
                            bloodlineName = bird.bloodlineName
                                ?: bloodlineMap[bird.bloodlineId]?.name,
                            onClick = { selectedBird = bird },
                        )
                    }
                    item { Spacer(modifier = Modifier.height(8.dp)) }
                }
            }
        }
    }

    if (selectedBird != null) {
        BirdDetailDialog(
            bird = selectedBird!!,
            bloodlineName = selectedBird!!.bloodlineName
                ?: bloodlineMap[selectedBird!!.bloodlineId]?.name,
            viewModel = viewModel,
            onDismiss = { selectedBird = null },
        )
    }
}

@Composable
fun FlockFilterChips(
    bloodlines: List<Bloodline>,
    selectedFilter: FlockFilter,
    onFilterSelected: (FlockFilter) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        FilterChip(
            selected = selectedFilter is FlockFilter.All,
            onClick = { onFilterSelected(FlockFilter.All) },
            label = { Text("All") },
            colors = FilterChipDefaults.filterChipColors(
                selectedContainerColor = SageGreen,
                selectedLabelColor = Color.White,
            ),
        )
        FilterChip(
            selected = selectedFilter is FlockFilter.Males,
            onClick = { onFilterSelected(FlockFilter.Males) },
            label = { Text("Males") },
            colors = FilterChipDefaults.filterChipColors(
                selectedContainerColor = SageGreen,
                selectedLabelColor = Color.White,
            ),
        )
        FilterChip(
            selected = selectedFilter is FlockFilter.Females,
            onClick = { onFilterSelected(FlockFilter.Females) },
            label = { Text("Females") },
            colors = FilterChipDefaults.filterChipColors(
                selectedContainerColor = SageGreen,
                selectedLabelColor = Color.White,
            ),
        )
        bloodlines.forEach { bloodline ->
            FilterChip(
                selected = selectedFilter is FlockFilter.ByBloodline &&
                    (selectedFilter as FlockFilter.ByBloodline).bloodlineId == bloodline.id,
                onClick = {
                    onFilterSelected(FlockFilter.ByBloodline(bloodline.id, bloodline.name))
                },
                label = { Text(bloodline.name) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = SageGreen,
                    selectedLabelColor = Color.White,
                ),
            )
        }
    }
}

@Composable
fun BirdCard(bird: Bird, bloodlineName: String?, onClick: () -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface,
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 2.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Band color circle
            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(parseBandColor(bird.bandColor)),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = bird.id.toString(),
                    style = MaterialTheme.typography.labelLarge,
                    color = Color.White,
                    fontWeight = FontWeight.Bold,
                )
            }

            Spacer(modifier = Modifier.width(12.dp))

            Column(modifier = Modifier.weight(1f)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text(
                        text = bird.bandId ?: "Bird #${bird.id}",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    StatusBadge(status = bird.status)
                }
                Spacer(modifier = Modifier.height(4.dp))
                Row(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(
                        text = formatSex(bird.sex),
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    if (bloodlineName != null) {
                        Text(
                            text = bloodlineName,
                            style = MaterialTheme.typography.bodyMedium,
                            color = SageGreen,
                        )
                    }
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    if (bird.hatchDate != null) {
                        Text(
                            text = "Hatched ${bird.hatchDate}",
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                    if (bird.latestWeight != null) {
                        Text(
                            text = "%.0fg".format(bird.latestWeight),
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = FontWeight.Medium,
                            color = SageGreen,
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun StatusBadge(status: String?) {
    val displayStatus = status?.replaceFirstChar { it.uppercase() } ?: "Unknown"
    val bgColor = when (status?.lowercase()) {
        "active" -> SageGreenLight
        "culled", "deceased" -> Color(0xFFE0B0B0)
        else -> MaterialTheme.colorScheme.surfaceVariant
    }
    val textColor = when (status?.lowercase()) {
        "active" -> Color(0xFF2D4A1E)
        "culled", "deceased" -> Color(0xFF6B2D2D)
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }
    Text(
        text = displayStatus,
        style = MaterialTheme.typography.labelLarge,
        color = textColor,
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .background(bgColor)
            .padding(horizontal = 8.dp, vertical = 2.dp),
    )
}

@Composable
fun BirdDetailDialog(
    bird: Bird,
    bloodlineName: String?,
    viewModel: FlockViewModel,
    onDismiss: () -> Unit,
) {
    var weights by remember { mutableStateOf<List<BirdWeight>>(emptyList()) }
    var weightsLoaded by remember { mutableStateOf(false) }

    androidx.compose.runtime.LaunchedEffect(bird.id) {
        weights = viewModel.getBirdWeights(bird.id)
        weightsLoaded = true
    }

    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Card(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surface,
            ),
        ) {
            LazyColumn(
                modifier = Modifier.padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                item {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(
                                modifier = Modifier
                                    .size(40.dp)
                                    .clip(CircleShape)
                                    .background(parseBandColor(bird.bandColor)),
                                contentAlignment = Alignment.Center,
                            ) {
                                Text(
                                    text = bird.id.toString(),
                                    style = MaterialTheme.typography.labelLarge,
                                    color = Color.White,
                                    fontWeight = FontWeight.Bold,
                                )
                            }
                            Spacer(modifier = Modifier.width(12.dp))
                            Text(
                                text = bird.bandId ?: "Bird #${bird.id}",
                                style = MaterialTheme.typography.headlineMedium,
                            )
                        }
                        IconButton(onClick = onDismiss) {
                            Icon(
                                imageVector = Icons.Default.Close,
                                contentDescription = "Close",
                            )
                        }
                    }
                }

                item { Spacer(modifier = Modifier.height(4.dp)) }

                item {
                    DetailRow("Status", bird.status?.replaceFirstChar { it.uppercase() } ?: "Unknown")
                }
                item { DetailRow("Sex", formatSex(bird.sex)) }
                if (bloodlineName != null) {
                    item { DetailRow("Bloodline", bloodlineName) }
                }
                if (bird.species != null) {
                    item { DetailRow("Species", bird.species) }
                }
                if (bird.hatchDate != null) {
                    item { DetailRow("Hatch Date", bird.hatchDate) }
                }
                if (bird.sireId != null) {
                    item { DetailRow("Sire", "Bird #${bird.sireId}") }
                }
                if (bird.damId != null) {
                    item { DetailRow("Dam", "Bird #${bird.damId}") }
                }
                if (bird.brooderId != null) {
                    item { DetailRow("Brooder", "#${bird.brooderId}") }
                }

                if (bird.notes != null) {
                    item {
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = "Notes",
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = bird.notes,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }

                item {
                    Spacer(modifier = Modifier.height(8.dp))
                    HorizontalDivider()
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Weight History",
                        style = MaterialTheme.typography.titleMedium,
                    )
                }

                if (!weightsLoaded) {
                    item {
                        Box(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(16.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            CircularProgressIndicator(
                                color = SageGreen,
                                modifier = Modifier.size(24.dp),
                                strokeWidth = 2.dp,
                            )
                        }
                    }
                } else if (weights.isEmpty()) {
                    item {
                        Text(
                            text = "No weight records",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                } else {
                    items(weights) { w ->
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(
                                text = w.recordedAt ?: "—",
                                style = MaterialTheme.typography.bodyMedium,
                            )
                            Text(
                                text = "%.1f g".format(w.weightGrams),
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = FontWeight.Medium,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun DetailRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
        )
    }
}

private fun formatSex(sex: String?): String {
    return when (sex?.lowercase()) {
        "male", "m" -> "Male"
        "female", "f" -> "Female"
        else -> "Unknown"
    }
}

private fun parseBandColor(color: String?): Color {
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
