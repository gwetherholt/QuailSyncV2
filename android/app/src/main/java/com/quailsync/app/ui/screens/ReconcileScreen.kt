package com.quailsync.app.ui.screens

import android.app.Application
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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.HelpOutline
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.runtime.remember
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
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Bird
import com.quailsync.app.data.NfcService
import com.quailsync.app.data.ObservedBirdDto
import com.quailsync.app.data.ObservedTraitsDto
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ReconcileRequest
import com.quailsync.app.data.ReconcileResponse
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.formatLineages
import com.quailsync.app.ui.components.BandColorPicker
import com.quailsync.app.ui.theme.AlertGreen
import com.quailsync.app.ui.theme.AlertYellow
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

// =====================================================================
// Dropped-tag reconciliation wizard
//
// "I found a leg band on the hutch floor — whose is it?" A guided,
// read-only flow: scan the dropped tag(s), describe the present unbanded
// birds, and let the server deduce which bird each band belongs to. Nothing
// is written back; the keeper physically re-attaches the existing tag.
//
// Scoped to one breeding group (one male + N females). See the backend
// module crates/quailsync-server/src/routes/reconcile.rs and
// docs/dropped_tag_deduction.md.
// =====================================================================

enum class ReconcileStep { FoundTags, Observe, Deduce }

/** A dropped tag the keeper scanned, plus the bird its stored mapping points
 *  at (populated asynchronously after the NFC lookup resolves). */
data class FoundTag(val tagId: String, val bird: Bird? = null)

/** One present unbanded bird, as the keeper describes it. `sex` is "Male" /
 *  "Female" / null (= not sure); `bandColor`/`bloodline` blank = not observed. */
data class Observation(
    val refId: String,
    val sex: String? = null,
    val bandColor: String = "",
    val bloodline: String? = null,
)

class ReconcileViewModel(
    application: Application,
    private val nfcService: NfcService,
    val groupId: Int,
) : AndroidViewModel(application) {
    private val api = QuailSyncApi.create(ServerConfig.getServerUrl(application))

    private val _step = MutableStateFlow(ReconcileStep.FoundTags)
    val step: StateFlow<ReconcileStep> = _step.asStateFlow()

    private val _foundTags = MutableStateFlow<List<FoundTag>>(emptyList())
    val foundTags: StateFlow<List<FoundTag>> = _foundTags.asStateFlow()

    private val _observations = MutableStateFlow<List<Observation>>(emptyList())
    val observations: StateFlow<List<Observation>> = _observations.asStateFlow()

    private val _lineageOptions = MutableStateFlow<List<String>>(emptyList())
    val lineageOptions: StateFlow<List<String>> = _lineageOptions.asStateFlow()

    private val _groupName = MutableStateFlow("")
    val groupName: StateFlow<String> = _groupName.asStateFlow()

    private val _deducing = MutableStateFlow(false)
    val deducing: StateFlow<Boolean> = _deducing.asStateFlow()

    private val _result = MutableStateFlow<ReconcileResponse?>(null)
    val result: StateFlow<ReconcileResponse?> = _result.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    init {
        nfcService.enterReconcileMode()
        loadGroup()
        // Collect read-only scans captured while on the Found-tags step.
        viewModelScope.launch {
            nfcService.lastScan.collect { scan ->
                if (scan != null && _step.value == ReconcileStep.FoundTags) {
                    upsertFoundTag(scan.tagId, scan.bird)
                }
            }
        }
    }

    override fun onCleared() {
        nfcService.exitReconcileMode()
        super.onCleared()
    }

    private fun loadGroup() {
        viewModelScope.launch {
            try {
                val group = api.getBreedingGroups().find { it.id == groupId }
                val birds = api.getBirds()
                if (group != null) {
                    _groupName.value = group.name
                    val memberIds = (listOf(group.maleId) + group.femaleIds).toSet()
                    _lineageOptions.value = birds
                        .filter { it.id in memberIds }
                        .flatMap { it.lineages }
                        .map { it.name }
                        .distinct()
                        .sorted()
                }
            } catch (_: Exception) { /* options are a nicety; deduction still works */ }
        }
    }

    private fun upsertFoundTag(tagId: String, bird: Bird?) {
        val list = _foundTags.value.toMutableList()
        val idx = list.indexOfFirst { it.tagId == tagId }
        if (idx >= 0) {
            // Second emission of the same tap carries the resolved bird.
            if (bird != null && list[idx].bird == null) list[idx] = list[idx].copy(bird = bird)
        } else {
            list.add(FoundTag(tagId, bird))
        }
        _foundTags.value = list
    }

    /** Manual fallback for devices without NFC (and for testing): look the tag
     *  up the same way a scan would. */
    fun addTagManually(raw: String) {
        val tagId = raw.trim().uppercase()
        if (tagId.isBlank() || _foundTags.value.any { it.tagId == tagId }) return
        upsertFoundTag(tagId, null)
        viewModelScope.launch {
            try {
                upsertFoundTag(tagId, api.getBirdByNfcTag(tagId))
            } catch (_: Exception) { /* leave unresolved — likely lands in unmatched */ }
        }
    }

    fun removeTag(tagId: String) {
        _foundTags.value = _foundTags.value.filterNot { it.tagId == tagId }
    }

    /** Whose band is this, for display? Falls back to the raw id. */
    fun tagLabel(tagId: String): String {
        val bird = _foundTags.value.find { it.tagId == tagId }?.bird ?: return tagId
        return birdLabel(bird)
    }

    fun toFoundTags() {
        _step.value = ReconcileStep.FoundTags
    }

    fun toObserve() {
        // Seed one observation card per dropped band — the usual case is that
        // the birds in front of you are exactly the ones that lost a band.
        if (_observations.value.isEmpty()) {
            _observations.value = _foundTags.value.indices.map { i ->
                Observation(refId = "Bird ${i + 1}")
            }
        }
        _step.value = ReconcileStep.Observe
    }

    fun addObservation() {
        val n = _observations.value.size + 1
        _observations.value = _observations.value + Observation(refId = "Bird $n")
    }

    fun removeObservation(index: Int) {
        _observations.value = _observations.value.toMutableList().also { it.removeAt(index) }
    }

    fun updateObservation(index: Int, transform: (Observation) -> Observation) {
        _observations.value = _observations.value.toMutableList().also {
            it[index] = transform(it[index])
        }
    }

    fun deduce() {
        _step.value = ReconcileStep.Deduce
        _deducing.value = true
        _error.value = null
        _result.value = null
        viewModelScope.launch {
            try {
                val request = ReconcileRequest(
                    orphanTagIds = _foundTags.value.map { it.tagId },
                    observedBirds = _observations.value.map { o ->
                        ObservedBirdDto(
                            refId = o.refId,
                            sex = o.sex,
                            bloodline = o.bloodline?.ifBlank { null },
                            traits = ObservedTraitsDto(bandColor = o.bandColor.ifBlank { null }),
                        )
                    },
                )
                _result.value = api.reconcileTags(groupId, request)
            } catch (e: Exception) {
                _error.value = friendlyMessage(e)
            } finally {
                _deducing.value = false
            }
        }
    }

    private fun friendlyMessage(e: Exception): String {
        if (e is retrofit2.HttpException) {
            if (e.code() == 404) return "That breeding group no longer exists."
            return "Couldn't reach the server (${e.code()}). Try again."
        }
        return e.message ?: "Something went wrong. Try again."
    }

    companion object {
        fun birdLabel(bird: Bird): String {
            val color = bird.bandColor?.takeIf { it.isNotBlank() } ?: "Bird"
            val lineage = bird.lineages.takeIf { it.isNotEmpty() }?.let { " · ${formatLineages(it)}" } ?: ""
            return "$color #${bird.id}$lineage"
        }
    }
}

@Composable
fun ReconcileScreen(
    groupId: Int,
    nfcService: NfcService,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    val app = context.applicationContext as Application
    val vm: ReconcileViewModel = viewModel(
        key = "reconcile-$groupId",
        factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T =
                ReconcileViewModel(app, nfcService, groupId) as T
        },
    )

    val step by vm.step.collectAsState()
    val groupName by vm.groupName.collectAsState()

    Column(Modifier.fillMaxSize()) {
        // Header
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
            Column(Modifier.weight(1f)) {
                Text("Find a dropped band", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                if (groupName.isNotBlank()) {
                    Text(groupName, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        StepIndicator(step)

        when (step) {
            ReconcileStep.FoundTags -> FoundTagsStep(vm)
            ReconcileStep.Observe -> ObserveStep(vm)
            ReconcileStep.Deduce -> DeduceStep(vm, onBack)
        }
    }
}

@Composable
private fun StepIndicator(step: ReconcileStep) {
    val index = when (step) {
        ReconcileStep.FoundTags -> 0
        ReconcileStep.Observe -> 1
        ReconcileStep.Deduce -> 2
    }
    val labels = listOf("Found tags", "Observe birds", "Result")
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        LinearProgressIndicator(
            progress = { (index + 1) / 3f },
            modifier = Modifier.fillMaxWidth().height(6.dp).clip(RoundedCornerShape(3.dp)),
            color = SageGreen,
            trackColor = SageGreenLight.copy(alpha = 0.3f),
        )
        Spacer(Modifier.height(4.dp))
        Text(
            "Step ${index + 1} of 3 · ${labels[index]}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
    }
}

// ---------------------------------------------------------------------
// Step 1 — Found tags
// ---------------------------------------------------------------------

@Composable
private fun FoundTagsStep(vm: ReconcileViewModel) {
    val foundTags by vm.foundTags.collectAsState()
    var manualEntry by remember { mutableStateOf("") }

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            contentPadding = PaddingValues(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            modifier = Modifier.weight(1f),
        ) {
            item {
                Card(
                    Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(16.dp),
                    colors = CardDefaults.cardColors(containerColor = SageGreenLight.copy(alpha = 0.18f)),
                ) {
                    Column(Modifier.padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(Icons.Default.Nfc, null, Modifier.size(40.dp), tint = SageGreen)
                        Spacer(Modifier.height(8.dp))
                        Text("Tap each band you found", style = MaterialTheme.typography.titleMedium)
                        Text(
                            "Hold each dropped tag to the back of the phone. We only read it — nothing is written.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                        )
                    }
                }
            }

            if (foundTags.isEmpty()) {
                item {
                    Text(
                        "No tags scanned yet.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(vertical = 8.dp),
                    )
                }
            } else {
                item {
                    Text(
                        "We're looking for ${foundTags.size} bird${if (foundTags.size != 1) "s" else ""}:",
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
                items(foundTags, key = { it.tagId }) { ft ->
                    Card(
                        Modifier.fillMaxWidth(),
                        shape = RoundedCornerShape(12.dp),
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                        elevation = CardDefaults.cardElevation(2.dp),
                    ) {
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(Modifier.weight(1f)) {
                                Text(
                                    ft.bird?.let { ReconcileViewModel.birdLabel(it) } ?: "Unknown bird",
                                    style = MaterialTheme.typography.bodyLarge,
                                )
                                Text(
                                    "Tag ${ft.tagId}",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                            IconButton(onClick = { vm.removeTag(ft.tagId) }) {
                                Icon(Icons.Default.Close, "Remove", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }

            // Manual entry fallback (no-NFC devices / testing).
            item {
                Spacer(Modifier.height(4.dp))
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        value = manualEntry,
                        onValueChange = { manualEntry = it },
                        label = { Text("Enter a tag id") },
                        singleLine = true,
                        modifier = Modifier.weight(1f),
                    )
                    Spacer(Modifier.width(8.dp))
                    TextButton(
                        onClick = { vm.addTagManually(manualEntry); manualEntry = "" },
                        enabled = manualEntry.isNotBlank(),
                    ) {
                        Icon(Icons.Default.Add, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Add")
                    }
                }
            }
        }

        Button(
            onClick = { vm.toObserve() },
            enabled = foundTags.isNotEmpty(),
            modifier = Modifier.fillMaxWidth().padding(16.dp).height(52.dp),
            colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
        ) { Text("Next: describe the birds", fontSize = 16.sp) }
    }
}

// ---------------------------------------------------------------------
// Step 2 — Observe present birds
// ---------------------------------------------------------------------

@Composable
private fun ObserveStep(vm: ReconcileViewModel) {
    val observations by vm.observations.collectAsState()
    val lineageOptions by vm.lineageOptions.collectAsState()

    Column(Modifier.fillMaxSize()) {
        LazyColumn(
            contentPadding = PaddingValues(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            modifier = Modifier.weight(1f),
        ) {
            item {
                Text(
                    "Describe each unbanded bird in front of you. Anything you're unsure of, leave as \"Not sure\" — we only rule a bird out on what you're certain of.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            itemsIndexed(observations, key = { _, o -> o.refId }) { index, obs ->
                ObservationCard(
                    obs = obs,
                    lineageOptions = lineageOptions,
                    canRemove = observations.size > 1,
                    onRemove = { vm.removeObservation(index) },
                    onChange = { transform -> vm.updateObservation(index, transform) },
                )
            }
            item {
                OutlinedButton(
                    onClick = { vm.addObservation() },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Default.Add, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Add another bird")
                }
            }
        }

        Row(Modifier.fillMaxWidth().padding(16.dp)) {
            OutlinedButton(
                onClick = { vm.toFoundTags() },
                modifier = Modifier.weight(1f).height(52.dp),
            ) { Text("Back") }
            Spacer(Modifier.width(12.dp))
            Button(
                onClick = { vm.deduce() },
                modifier = Modifier.weight(1f).height(52.dp),
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text("Deduce", fontSize = 16.sp) }
        }
    }
}

@Composable
private fun ObservationCard(
    obs: Observation,
    lineageOptions: List<String>,
    canRemove: Boolean,
    onRemove: () -> Unit,
    onChange: ((Observation) -> Observation) -> Unit,
) {
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(14.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text(obs.refId, style = MaterialTheme.typography.titleMedium)
                if (canRemove) {
                    IconButton(onClick = onRemove) {
                        Icon(Icons.Default.Close, "Remove bird", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }

            // Sex (hard attribute)
            Text("Sex", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(4.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SexChip("Male", obs.sex == "Male", Modifier.weight(1f)) { onChange { it.copy(sex = "Male") } }
                SexChip("Female", obs.sex == "Female", Modifier.weight(1f)) { onChange { it.copy(sex = "Female") } }
                SexChip("Not sure", obs.sex == null, Modifier.weight(1f)) { onChange { it.copy(sex = null) } }
            }

            Spacer(Modifier.height(10.dp))
            // Band color (soft attribute, used only for ranking)
            BandColorPicker(
                value = obs.bandColor,
                onValueChange = { newColor -> onChange { it.copy(bandColor = newColor) } },
                label = "Band color (if you can see it)",
                modifier = Modifier.fillMaxWidth(),
            )

            // Bloodline (hard attribute) — only offered when the group has known lineages.
            if (lineageOptions.isNotEmpty()) {
                Spacer(Modifier.height(10.dp))
                Text("Bloodline", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.height(4.dp))
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    SexChip("Not sure", obs.bloodline == null, Modifier) { onChange { it.copy(bloodline = null) } }
                    lineageOptions.forEach { name ->
                        SexChip(name, obs.bloodline == name, Modifier) { onChange { it.copy(bloodline = name) } }
                    }
                }
            }
        }
    }
}

@Composable
private fun SexChip(label: String, selected: Boolean, modifier: Modifier = Modifier, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        modifier = modifier,
        colors = if (selected) {
            ButtonDefaults.outlinedButtonColors(containerColor = SageGreenLight.copy(alpha = 0.3f), contentColor = SageGreen)
        } else {
            ButtonDefaults.outlinedButtonColors()
        },
    ) {
        Text(label, fontSize = 13.sp, fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal)
    }
}

// ---------------------------------------------------------------------
// Step 3 — Deduce / result
// ---------------------------------------------------------------------

@Composable
private fun DeduceStep(vm: ReconcileViewModel, onDone: () -> Unit) {
    val deducing by vm.deducing.collectAsState()
    val result by vm.result.collectAsState()
    val error by vm.error.collectAsState()

    Column(Modifier.fillMaxSize()) {
        Box(Modifier.weight(1f)) {
            when {
                deducing -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        CircularProgressIndicator(color = SageGreen)
                        Spacer(Modifier.height(12.dp))
                        Text("Working it out…", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }

                error != null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(Modifier.padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(error!!, color = MaterialTheme.colorScheme.error, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(12.dp))
                        OutlinedButton(onClick = { vm.deduce() }) { Text("Try again") }
                    }
                }

                result != null -> ResultList(vm, result!!)
            }
        }

        Row(Modifier.fillMaxWidth().padding(16.dp)) {
            OutlinedButton(
                onClick = { vm.toObserve() },
                modifier = Modifier.weight(1f).height(52.dp),
            ) { Text("Back") }
            Spacer(Modifier.width(12.dp))
            Button(
                onClick = onDone,
                modifier = Modifier.weight(1f).height(52.dp),
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text("Done", fontSize = 16.sp) }
        }
    }
}

@Composable
private fun ResultList(vm: ReconcileViewModel, response: ReconcileResponse) {
    LazyColumn(
        contentPadding = PaddingValues(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        items(response.results, key = { it.refId }) { res ->
            val outcome = res.outcome
            val neutral = MaterialTheme.colorScheme.onSurfaceVariant
            when (outcome.kind) {
                "resolved" -> ResultCard(
                    accent = AlertGreen,
                    icon = Icons.Default.CheckCircle,
                    title = res.refId,
                    body = buildString {
                        append("This is ${vm.tagLabel(outcome.tagId ?: "")}'s band")
                        if (outcome.confidence == "forced") {
                            append(" (the only one left once the others were placed)")
                        }
                        append(". Re-attach it and you're done.")
                    },
                )

                "ambiguous" -> ResultCard(
                    accent = AlertYellow,
                    icon = Icons.AutoMirrored.Filled.HelpOutline,
                    title = res.refId,
                    body = "Narrowed it down — check the band color to tell these apart:",
                    candidates = outcome.candidates.map { c ->
                        "${vm.tagLabel(c.tagId)}  ·  ${(c.score * 100).toInt()}% match"
                    },
                )

                else -> ResultCard(
                    accent = neutral,
                    icon = Icons.AutoMirrored.Filled.HelpOutline,
                    title = res.refId,
                    body = "None of the dropped bands match this bird.",
                )
            }
        }

        if (response.unmatchedTags.isNotEmpty()) {
            item {
                Card(
                    Modifier.fillMaxWidth(),
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f)),
                ) {
                    Column(Modifier.padding(14.dp)) {
                        Text("Not from this group", style = MaterialTheme.typography.labelLarge)
                        Spacer(Modifier.height(4.dp))
                        response.unmatchedTags.forEach { tag ->
                            Text("• Tag $tag", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ResultCard(
    accent: Color,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    body: String,
    candidates: List<String> = emptyList(),
) {
    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = accent.copy(alpha = 0.12f)),
    ) {
        Column(Modifier.padding(14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, Modifier.size(22.dp), tint = accent)
                Spacer(Modifier.width(8.dp))
                Text(title, style = MaterialTheme.typography.titleMedium, color = accent, fontWeight = FontWeight.SemiBold)
            }
            Spacer(Modifier.height(6.dp))
            Text(body, style = MaterialTheme.typography.bodyMedium)
            candidates.forEach { c ->
                Spacer(Modifier.height(4.dp))
                Text("• $c", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurface)
            }
        }
    }
}
