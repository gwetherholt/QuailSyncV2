package com.quailsync.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.nfc.NfcAdapter
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.quailsync.app.data.HatchCountdownWorker
import com.quailsync.app.data.MonitoringService
import com.quailsync.app.data.NfcService
import com.quailsync.app.data.NotificationHelper
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.ui.screens.BatchState
import com.quailsync.app.ui.screens.BrooderManageScreen
import com.quailsync.app.ui.screens.CameraScreen
import com.quailsync.app.ui.screens.ClutchScreen
import com.quailsync.app.ui.screens.DashboardScreen
import com.quailsync.app.ui.screens.FlockScreen
import com.quailsync.app.ui.screens.TelemetryScreen
import com.quailsync.app.ui.screens.NfcScreen
import com.quailsync.app.ui.screens.NfcViewModel
import com.quailsync.app.ui.theme.QuailSyncTheme
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight

sealed class Screen(val route: String, val label: String, val icon: ImageVector, val iconRes: Int? = null) {
    data object Dashboard : Screen("dashboard", "Dashboard", Icons.Default.Dashboard)
    data object Cameras : Screen("cameras", "Cameras", Icons.Default.Videocam)
    data object Flock : Screen("flock", "Flock", Icons.Default.Pets, iconRes = R.drawable.ic_bird)
    data object Nfc : Screen("nfc", "NFC", Icons.Default.Nfc)
    data object Clutches : Screen("clutches", "Hatchery", Icons.Default.Egg)
    data object Settings : Screen("settings", "Settings", Icons.Default.Settings)
    data object Telemetry : Screen("telemetry", "Telemetry", Icons.Default.Settings)
}

val bottomNavItems = listOf(
    Screen.Dashboard,
    Screen.Cameras,
    Screen.Flock,
    Screen.Nfc,
    Screen.Clutches,
)

class MainActivity : ComponentActivity() {
    private var nfcAdapter: NfcAdapter? = null
    private val nfcService = NfcService()
    private lateinit var nfcViewModel: NfcViewModel
    private var navController: NavHostController? = null

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (granted) {
            startMonitoringIfEnabled()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Create notification channels early
        NotificationHelper.createChannels(this)

        nfcAdapter = NfcAdapter.getDefaultAdapter(this)
        nfcService.checkAvailability(nfcAdapter)
        nfcViewModel = NfcViewModel(nfcService, ServerConfig.getServerUrl(this))

        handleNfcIntent(intent)

        // Request notification permission (Android 13+)
        requestNotificationPermission()

        // Schedule hatch countdown worker
        HatchCountdownWorker.schedule(this)

        setContent {
            QuailSyncTheme {
                QuailSyncApp(
                    nfcService = nfcService,
                    nfcViewModel = nfcViewModel,
                    onNavControllerReady = { navController = it },
                )
            }
        }
    }

    override fun onResume() {
        super.onResume()
        nfcService.checkAvailability(nfcAdapter)
        nfcService.enableForegroundDispatch(this, nfcAdapter)
    }

    override fun onPause() {
        super.onPause()
        nfcService.disableForegroundDispatch(this, nfcAdapter)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleNfcIntent(intent)
    }

    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED
            ) {
                notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                return
            }
        }
        startMonitoringIfEnabled()
    }

    private fun startMonitoringIfEnabled() {
        if (MonitoringService.isMonitoringEnabled(this)) {
            MonitoringService.start(this)
        }
    }

    private fun handleNfcIntent(intent: Intent) {
        val wasBatchWriting = nfcViewModel.batchState.value is BatchState.AwaitingTagWrite

        val (scanResult, writeAttempt) = nfcService.handleIntent(intent) ?: return

        if (writeAttempt != null) {
            when (writeAttempt) {
                is NfcService.WriteAttemptResult.Written -> {
                    Toast.makeText(this, "Wrote ${scanResult.payload ?: scanResult.tagId}", Toast.LENGTH_SHORT).show()
                    if (wasBatchWriting) nfcViewModel.onBatchTagWritten(scanResult.tagId, true)
                }
                is NfcService.WriteAttemptResult.Conflict -> {
                    nfcViewModel.lookupConflictBird(writeAttempt.conflict)
                    if (wasBatchWriting) nfcViewModel.setBatchPausedForConflict(true)
                }
                is NfcService.WriteAttemptResult.Failed -> {
                    Toast.makeText(this, writeAttempt.message, Toast.LENGTH_SHORT).show()
                    if (wasBatchWriting) nfcViewModel.onBatchTagWritten(scanResult.tagId, false)
                }
            }
            navController?.navigate(Screen.Nfc.route) {
                popUpTo(navController!!.graph.findStartDestination().id) { saveState = true }
                launchSingleTop = true; restoreState = true
            }
            return
        }

        nfcViewModel.lookupBirdByNfc(scanResult.tagId, scanResult.payload)
        val toastMsg = if (scanResult.payload?.startsWith("BIRD-") == true) "Scanned: ${scanResult.payload}" else "NFC tag: ${scanResult.tagId}"
        Toast.makeText(this, toastMsg, Toast.LENGTH_SHORT).show()

        navController?.navigate(Screen.Nfc.route) {
            popUpTo(navController!!.graph.findStartDestination().id) { saveState = true }
            launchSingleTop = true; restoreState = true
        }
    }
}

@Composable
fun QuailSyncApp(
    nfcService: NfcService,
    nfcViewModel: NfcViewModel,
    onNavControllerReady: (NavHostController) -> Unit,
) {
    val navController = rememberNavController()
    onNavControllerReady(navController)
    val navBackStackEntry by navController.currentBackStackEntryAsState()
    val currentDestination = navBackStackEntry?.destination

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        floatingActionButton = {
            if (currentDestination?.route == Screen.Dashboard.route) {
                androidx.compose.material3.FloatingActionButton(
                    onClick = {
                        navController.navigate(Screen.Settings.route) {
                            launchSingleTop = true
                        }
                    },
                    containerColor = SageGreen,
                    contentColor = androidx.compose.ui.graphics.Color.White,
                    modifier = Modifier.padding(bottom = 8.dp),
                ) {
                    Icon(Icons.Default.Settings, "Settings")
                }
            }
        },
        bottomBar = {
            NavigationBar {
                bottomNavItems.forEach { screen ->
                    NavigationBarItem(
                        icon = {
                            if (screen.iconRes != null) {
                                Icon(androidx.compose.ui.res.painterResource(screen.iconRes), contentDescription = screen.label)
                            } else {
                                Icon(screen.icon, contentDescription = screen.label)
                            }
                        },
                        label = { Text(screen.label, fontSize = 11.sp) },
                        selected = currentDestination?.hierarchy?.any { it.route == screen.route } == true,
                        onClick = {
                            navController.navigate(screen.route) {
                                popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                                launchSingleTop = true; restoreState = true
                            }
                        },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = SageGreen,
                            selectedTextColor = SageGreen,
                            indicatorColor = SageGreenLight.copy(alpha = 0.3f),
                        ),
                    )
                }
            }
        },
    ) { innerPadding ->
        NavHost(
            navController = navController,
            startDestination = Screen.Dashboard.route,
            modifier = Modifier.padding(innerPadding),
        ) {
            composable(Screen.Dashboard.route) {
                DashboardScreen(
                    onBrooderClick = { id -> navController.navigate("brooder/$id") },
                    onTelemetryClick = { navController.navigate(Screen.Telemetry.route) { launchSingleTop = true } },
                )
            }
            composable(Screen.Telemetry.route) {
                TelemetryScreen(
                    onBrooderClick = { id -> navController.navigate("brooder/$id") },
                    onBack = { navController.popBackStack() },
                )
            }
            composable(Screen.Cameras.route) { CameraScreen() }
            composable(Screen.Flock.route) { FlockScreen() }
            composable(Screen.Nfc.route) { NfcScreen(nfcService = nfcService, viewModel = nfcViewModel) }
            composable(Screen.Clutches.route) { ClutchScreen() }
            composable(Screen.Settings.route) { SettingsScreen() }
            composable("brooder/{id}") { backStackEntry ->
                val id = backStackEntry.arguments?.getString("id")?.toIntOrNull() ?: 0
                BrooderManageScreen(brooderId = id, onBack = { navController.popBackStack() })
            }
        }
    }
}

@Composable
fun SettingsScreen() {
    val context = LocalContext.current
    var monitoringEnabled by remember {
        mutableStateOf(MonitoringService.isMonitoringEnabled(context))
    }
    var serverUrl by remember {
        mutableStateOf(ServerConfig.getServerUrl(context))
    }
    var serverUrlSaved by remember { mutableStateOf(true) }

    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Text("Settings", style = MaterialTheme.typography.headlineMedium)
        Spacer(Modifier.height(16.dp))

        // Server Connection card
        androidx.compose.material3.Card(
            Modifier.fillMaxWidth(),
            shape = androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
            colors = androidx.compose.material3.CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = androidx.compose.material3.CardDefaults.cardElevation(2.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                Text("Server Connection", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(12.dp))
                Text(
                    "The URL of your QuailSync server. Changes take effect after restarting the app.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(8.dp))
                androidx.compose.material3.OutlinedTextField(
                    value = serverUrl,
                    onValueChange = { serverUrl = it; serverUrlSaved = false },
                    label = { Text("Server URL") },
                    placeholder = { Text(ServerConfig.DEFAULT_URL) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                androidx.compose.material3.Button(
                    onClick = {
                        ServerConfig.setServerUrl(context, serverUrl)
                        serverUrlSaved = true
                        android.widget.Toast.makeText(context, "Server URL saved — restart app to apply", android.widget.Toast.LENGTH_SHORT).show()
                    },
                    enabled = !serverUrlSaved,
                    colors = androidx.compose.material3.ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) {
                    Text(if (serverUrlSaved) "Saved" else "Save")
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // Notifications card
        androidx.compose.material3.Card(
            Modifier.fillMaxWidth(),
            shape = androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
            colors = androidx.compose.material3.CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = androidx.compose.material3.CardDefaults.cardElevation(2.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                Text("Notifications", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(12.dp))

                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Background Monitoring", style = MaterialTheme.typography.bodyLarge)
                        Text("Monitor brooder temps & humidity in background. Alerts on threshold violations.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Switch(
                        checked = monitoringEnabled,
                        onCheckedChange = { monitoringEnabled = it; MonitoringService.setMonitoringEnabled(context, it) },
                        colors = SwitchDefaults.colors(checkedThumbColor = SageGreen, checkedTrackColor = SageGreenLight),
                    )
                }

                HorizontalDivider(Modifier.padding(vertical = 12.dp))
                Text("Alert Thresholds", style = MaterialTheme.typography.bodyLarge)
                Spacer(Modifier.height(4.dp))
                Text("CRITICAL: Temp < 60°F or > 75°F", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("WARNING: Temp < 65°F or > 72°F", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("WARNING: Humidity < 40% or > 80%", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Sensor offline: No data for 2 min", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)

                HorizontalDivider(Modifier.padding(vertical = 12.dp))
                Text("Hatch Reminders", style = MaterialTheme.typography.bodyLarge)
                Spacer(Modifier.height(4.dp))
                Text("Daily check at 8am for incubating clutches.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Alerts at: Day 7 (candle), Day 14 (lockdown), Day 16, Day 17+ (hatch)", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}
