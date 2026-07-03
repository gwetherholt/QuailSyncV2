package com.quailsync.app.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.quailsync.app.data.FlockDiversity
import com.quailsync.app.data.LineageProbability
import com.quailsync.app.data.QuailSyncApi

// ---------------------------------------------------------------------------
// Genetics UI (Phase 6): lineage colors, profile bars, confidence meters,
// risk pills, and the flock-wide new-blood banner. The palette mirrors the web
// dashboard so a lineage shows the same color on every surface.
// ---------------------------------------------------------------------------

private val LINEAGE_PALETTE = listOf(
    Color(0xFFB7C4A0), Color(0xFFD4A0A0), Color(0xFFA0B8D4), Color(0xFFD4C4A0),
    Color(0xFFC4A0D4), Color(0xFFA0D4C4), Color(0xFFD4A0B8), Color(0xFFB8D4A0),
    Color(0xFFD4B8A0), Color(0xFFA0C4D4),
)
private val TraceColor = Color(0xFFCFCABF)

/** Consistent color for a lineage, keyed by its ID (not list position). */
fun lineageColor(id: Int): Color {
    val n = LINEAGE_PALETTE.size
    return LINEAGE_PALETTE[((id % n) + n) % n]
}

private data class Capped(val shown: List<LineageProbability>, val trace: Double)

private fun capDist(dist: List<LineageProbability>, cap: Int): Capped {
    val sorted = dist.sortedByDescending { it.probability }
    return Capped(sorted.take(cap), sorted.drop(cap).sumOf { it.probability })
}

/** Fetch the display cap (top-N lineages) from settings; falls back to 4. */
suspend fun fetchDisplayCap(api: QuailSyncApi): Int =
    try {
        (api.getGeneticsSettings()["genetics.display_cap"] ?: "4").toIntOrNull() ?: 4
    } catch (e: Exception) {
        4
    }

/** Fetch the avoid overlap threshold as a fraction (e.g. 0.35); falls back to 0.35. */
suspend fun fetchAvoidThreshold(api: QuailSyncApi): Double =
    try {
        ((api.getGeneticsSettings()["genetics.threshold.avoid"] ?: "35").toIntOrNull() ?: 35) / 100.0
    } catch (e: Exception) {
        0.35
    }

/** Horizontal stacked distribution bar; groups lineages beyond `cap` as "trace". */
@Composable
fun DistributionBar(
    dist: List<LineageProbability>,
    modifier: Modifier = Modifier,
    cap: Int = 4,
    height: Int = 22,
) {
    if (dist.isEmpty()) {
        Box(
            modifier
                .fillMaxWidth()
                .height(height.dp)
                .clip(RoundedCornerShape(6.dp))
                .background(Color(0xFFF0ECE4)),
            contentAlignment = Alignment.Center,
        ) { Text("No profile", fontSize = 11.sp, color = Color(0xFF999999)) }
        return
    }
    val c = capDist(dist, cap)
    Row(
        modifier
            .fillMaxWidth()
            .height(height.dp)
            .clip(RoundedCornerShape(6.dp)),
    ) {
        c.shown.forEach { seg ->
            Box(
                Modifier
                    .fillMaxHeight()
                    .weight(seg.probability.toFloat().coerceAtLeast(0.0001f))
                    .background(lineageColor(seg.lineageId)),
                contentAlignment = Alignment.Center,
            ) {
                if (seg.probability >= 0.14) {
                    Text("${(seg.probability * 100).toInt()}%", fontSize = 10.sp, fontWeight = FontWeight.Bold, color = Color(0xFF4A453D))
                }
            }
        }
        if (c.trace > 0.0001) {
            Box(
                Modifier.fillMaxHeight().weight(c.trace.toFloat()).background(TraceColor),
                contentAlignment = Alignment.Center,
            ) {
                if (c.trace >= 0.14) Text("${(c.trace * 100).toInt()}%", fontSize = 10.sp, color = Color(0xFF555555))
            }
        }
    }
}

/** Color-swatch legend under a [DistributionBar]. */
@Composable
fun DistributionLegend(dist: List<LineageProbability>, modifier: Modifier = Modifier, cap: Int = 4) {
    if (dist.isEmpty()) return
    val c = capDist(dist, cap)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        c.shown.forEach { seg ->
            LegendChip(lineageColor(seg.lineageId), "${seg.lineageName.ifBlank { "#${seg.lineageId}" }} ${(seg.probability * 100).toInt()}%")
        }
        if (c.trace > 0.0001) LegendChip(TraceColor, "Trace ${(c.trace * 100).toInt()}%")
    }
}

@Composable
private fun LegendChip(color: Color, label: String) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        Box(Modifier.size(10.dp).clip(RoundedCornerShape(2.dp)).background(color))
        Text(label, fontSize = 12.sp, color = Color(0xFF555555))
    }
}

/** Confidence meter. Green > 70%, amber 50–70%, red < 50%. */
@Composable
fun ConfidenceMeter(confidence: Double, modifier: Modifier = Modifier) {
    val color = when {
        confidence > 0.7 -> Color(0xFF5B8A5B)
        confidence >= 0.5 -> Color(0xFFD9A441)
        else -> Color(0xFFC56B6B)
    }
    Row(modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(
            Modifier
                .weight(1f)
                .height(8.dp)
                .clip(RoundedCornerShape(4.dp))
                .background(Color(0xFFEEEEEE)),
        ) {
            Box(
                Modifier
                    .fillMaxHeight()
                    .fillMaxWidth(confidence.toFloat().coerceIn(0f, 1f))
                    .background(color),
            )
        }
        Text("${(confidence * 100).toInt()}%", fontSize = 11.sp, color = Color(0xFF666666))
    }
}

@Composable
fun GenBadge(gen: Int?, modifier: Modifier = Modifier) {
    Box(
        modifier
            .clip(RoundedCornerShape(10.dp))
            .background(Color(0xFFEFEAE1))
            .padding(horizontal = 7.dp, vertical = 2.dp),
    ) {
        Text("Gen ${gen ?: "?"}", fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = Color(0xFF6B6256))
    }
}

@Composable
fun RiskPill(level: String, pct: Int, modifier: Modifier = Modifier) {
    val (bg, fg) = when (level) {
        "safe" -> Color(0xFFE3F0E3) to Color(0xFF2E7D32)
        "avoid" -> Color(0xFFF7E0E0) to Color(0xFFC0392B)
        else -> Color(0xFFFBF0D9) to Color(0xFF9A7B10)
    }
    Box(
        modifier
            .clip(RoundedCornerShape(10.dp))
            .background(bg)
            .padding(horizontal = 8.dp, vertical = 3.dp),
    ) {
        Text("${level.uppercase()} $pct%", fontSize = 11.sp, fontWeight = FontWeight.Bold, color = fg)
    }
}

/** Flock-wide new-blood alert. Red when best pairing risk exceeds the avoid
 *  threshold, amber otherwise. Renders nothing when new blood isn't needed. */
@Composable
fun NewBloodBanner(diversity: FlockDiversity, avoidThreshold: Double, modifier: Modifier = Modifier) {
    if (!diversity.needsNewBlood) return
    val red = diversity.bestPairingRisk > avoidThreshold
    val bg = if (red) Color(0xFFF7E0E0) else Color(0xFFFBF0D9)
    val fg = if (red) Color(0xFF8B2A1E) else Color(0xFF7A5D0A)
    val title = if (red) "Action needed" else "Heads up"
    val msg = if (red) {
        "No safe pairings available. Introduce a new lineage before breeding further."
    } else {
        "Genetic diversity is narrowing — consider introducing new blood in the next 1-2 generations."
    }
    Box(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(bg)
            .padding(12.dp),
    ) {
        Text("🧬 $title: $msg", fontSize = 13.sp, color = fg)
    }
}
