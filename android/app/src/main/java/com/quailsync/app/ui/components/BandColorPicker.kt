package com.quailsync.app.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** Visual representation of a band-color preset. */
sealed class SwatchKind {
    data class Solid(val color: Color) : SwatchKind()
    data object Rainbow : SwatchKind()
    data object BlackAndWhite : SwatchKind()
    /** Sentinel for the "Other" entry that reveals a custom-text field. */
    data object Other : SwatchKind()
}

/** A single dropdown entry. */
data class BandColorOption(val name: String, val swatch: SwatchKind)

/** Common preset palette. The "Other" entry is the escape hatch for any
 *  band color not listed — selecting it reveals a free-text TextField. */
val BAND_COLOR_PRESETS: List<BandColorOption> = listOf(
    BandColorOption("Red", SwatchKind.Solid(Color(0xFFE53935))),
    BandColorOption("Blue", SwatchKind.Solid(Color(0xFF1E88E5))),
    BandColorOption("Green", SwatchKind.Solid(Color(0xFF43A047))),
    BandColorOption("Yellow", SwatchKind.Solid(Color(0xFFFDD835))),
    BandColorOption("Orange", SwatchKind.Solid(Color(0xFFFB8C00))),
    BandColorOption("Purple", SwatchKind.Solid(Color(0xFF8E24AA))),
    BandColorOption("Pink", SwatchKind.Solid(Color(0xFFEC407A))),
    BandColorOption("White", SwatchKind.Solid(Color(0xFFF5F5F5))),
    BandColorOption("Black", SwatchKind.Solid(Color(0xFF212121))),
    BandColorOption("Brown", SwatchKind.Solid(Color(0xFF6D4C41))),
    BandColorOption("Rainbow", SwatchKind.Rainbow),
    BandColorOption("Black & White", SwatchKind.BlackAndWhite),
    BandColorOption("Other", SwatchKind.Other),
)

/**
 * Round swatch indicator. Solid colors render as a single-color circle;
 * Rainbow uses a horizontal multi-color gradient; Black & White renders as
 * a half-and-half disk; Other is a grey circle with a "?" centered.
 */
@Composable
fun Swatch(kind: SwatchKind, size: Dp = 18.dp) {
    val border = BorderStroke(1.dp, Color.Gray.copy(alpha = 0.35f))
    when (kind) {
        is SwatchKind.Solid -> Box(
            Modifier
                .size(size)
                .clip(CircleShape)
                .background(kind.color)
                .border(border, CircleShape),
        )
        SwatchKind.Rainbow -> Box(
            Modifier
                .size(size)
                .clip(CircleShape)
                .background(
                    Brush.horizontalGradient(
                        listOf(
                            Color(0xFFE53935),
                            Color(0xFFFB8C00),
                            Color(0xFFFDD835),
                            Color(0xFF43A047),
                            Color(0xFF1E88E5),
                            Color(0xFF8E24AA),
                        ),
                    ),
                )
                .border(border, CircleShape),
        )
        SwatchKind.BlackAndWhite -> Box(
            Modifier
                .size(size)
                .clip(CircleShape)
                .border(border, CircleShape),
        ) {
            Row(Modifier.fillMaxSize()) {
                Box(Modifier.weight(1f).fillMaxHeight().background(Color(0xFF212121)))
                Box(Modifier.weight(1f).fillMaxHeight().background(Color(0xFFF5F5F5)))
            }
        }
        SwatchKind.Other -> Box(
            Modifier
                .size(size)
                .clip(CircleShape)
                .background(Color(0xFFE0E0E0))
                .border(border, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                "?",
                fontSize = (size.value * 0.6f).sp,
                color = Color(0xFF616161),
                fontWeight = FontWeight.Bold,
            )
        }
    }
}

/**
 * Band color picker. Dropdown of common colors with swatches; selecting
 * "Other" reveals a free-text TextField where the user can type any value.
 *
 * Behavior:
 *  - When `value` matches a preset name (case-sensitive), the dropdown
 *    shows that preset and no custom field.
 *  - When `value` is non-empty and doesn't match any preset, the dropdown
 *    shows "Other" and the custom TextField is revealed pre-filled with
 *    `value`. This is the "edit existing custom band color" case.
 *  - When `value` is blank, the dropdown is empty and no custom field is
 *    shown until the user explicitly picks "Other".
 *  - Caller's `onValueChange(String)` is the single source of truth — it
 *    receives either a preset name (e.g. `"Yellow"`) or the raw custom
 *    text. The literal string `"Other"` is never persisted.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BandColorPicker(
    value: String,
    onValueChange: (String) -> Unit,
    label: String = "Band color",
    modifier: Modifier = Modifier,
) {
    val matchedPreset = BAND_COLOR_PRESETS
        .firstOrNull { it.name == value && it.swatch !is SwatchKind.Other }

    // `otherMode` is internal state because there's no way to distinguish
    // "Other selected, no custom text typed yet" from "no selection at all"
    // using just the parent's `value` alone — both are empty strings.
    var otherMode by remember {
        mutableStateOf(value.isNotEmpty() && matchedPreset == null)
    }

    // Re-sync internal mode if the parent feeds a new value (e.g. when an
    // edit dialog opens with a previously-saved custom value).
    LaunchedEffect(value) {
        if (matchedPreset != null) otherMode = false
        else if (value.isNotEmpty()) otherMode = true
    }

    val displayPreset = matchedPreset ?: if (otherMode) {
        BAND_COLOR_PRESETS.first { it.swatch is SwatchKind.Other }
    } else null

    var expanded by remember { mutableStateOf(false) }

    Column(modifier) {
        ExposedDropdownMenuBox(expanded = expanded, onExpandedChange = { expanded = it }) {
            OutlinedTextField(
                value = displayPreset?.name ?: "",
                onValueChange = {},
                readOnly = true,
                label = { Text(label) },
                leadingIcon = displayPreset?.let { p -> { Swatch(p.swatch) } },
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
                modifier = Modifier.menuAnchor().fillMaxWidth(),
            )
            ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                BAND_COLOR_PRESETS.forEach { p ->
                    DropdownMenuItem(
                        leadingIcon = { Swatch(p.swatch) },
                        text = { Text(p.name) },
                        onClick = {
                            expanded = false
                            if (p.swatch is SwatchKind.Other) {
                                otherMode = true
                                // If they were on a preset and now want Other,
                                // clear so the custom field starts blank.
                                if (matchedPreset != null) onValueChange("")
                            } else {
                                otherMode = false
                                onValueChange(p.name)
                            }
                        },
                    )
                }
            }
        }
        if (otherMode) {
            Spacer(Modifier.height(4.dp))
            OutlinedTextField(
                value = value,
                onValueChange = onValueChange,
                label = { Text("Custom band color") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
