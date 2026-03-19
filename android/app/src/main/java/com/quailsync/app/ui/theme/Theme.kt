package com.quailsync.app.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

val SageGreen = Color(0xFF7D8B6A)
val SageGreenLight = Color(0xFFA8B496)
val SageGreenDark = Color(0xFF546142)
val WarmBeige = Color(0xFFF5F0E8)
val DarkBrown = Color(0xFF3D3229)
val DarkBrownLight = Color(0xFF6A5D52)
val Cream = Color(0xFFFEFCF9)
val DustyRose = Color(0xFFD4A0A0)
val AlertRed = Color(0xFFCC4444)
val AlertYellow = Color(0xFFCCA844)
val AlertGreen = Color(0xFF6A8B5E)

private val QuailSyncColorScheme = lightColorScheme(
    primary = SageGreen,
    onPrimary = Color.White,
    primaryContainer = SageGreenLight,
    onPrimaryContainer = SageGreenDark,
    secondary = DustyRose,
    onSecondary = Color.White,
    background = WarmBeige,
    onBackground = DarkBrown,
    surface = Cream,
    onSurface = DarkBrown,
    surfaceVariant = WarmBeige,
    onSurfaceVariant = DarkBrownLight,
    outline = SageGreenLight,
)

private val QuailSyncTypography = Typography(
    headlineLarge = TextStyle(
        fontWeight = FontWeight.Bold,
        fontSize = 28.sp,
        color = DarkBrown,
    ),
    headlineMedium = TextStyle(
        fontWeight = FontWeight.SemiBold,
        fontSize = 22.sp,
        color = DarkBrown,
    ),
    titleLarge = TextStyle(
        fontWeight = FontWeight.SemiBold,
        fontSize = 18.sp,
        color = DarkBrown,
    ),
    titleMedium = TextStyle(
        fontWeight = FontWeight.Medium,
        fontSize = 16.sp,
        color = DarkBrown,
    ),
    bodyLarge = TextStyle(
        fontSize = 16.sp,
        color = DarkBrown,
    ),
    bodyMedium = TextStyle(
        fontSize = 14.sp,
        color = DarkBrownLight,
    ),
    labelLarge = TextStyle(
        fontWeight = FontWeight.Medium,
        fontSize = 14.sp,
    ),
)

@Composable
fun QuailSyncTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = QuailSyncColorScheme,
        typography = QuailSyncTypography,
        content = content,
    )
}
