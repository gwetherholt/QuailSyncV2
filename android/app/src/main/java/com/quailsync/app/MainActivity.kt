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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.quailsync.app.data.HatchCountdownWorker
import com.quailsync.app.data.MonitoringService
import com.quailsync.app.data.NfcService
import com.quailsync.app.data.NotificationHelper
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.ServerConfig
import com.quailsync.app.data.UpdateAppSettings
import com.quailsync.app.ui.screens.AlertsScreen
import com.quailsync.app.ui.screens.BatchState
import com.quailsync.app.ui.screens.BrooderManageScreen
import com.quailsync.app.ui.screens.CameraScreen
import com.quailsync.app.ui.screens.ClutchScreen
import com.quailsync.app.ui.screens.BreedingScreen
import com.quailsync.app.ui.screens.DashboardScreen
import com.quailsync.app.ui.screens.FlockScreen
import com.quailsync.app.ui.screens.TelemetryScreen
import com.quailsync.app.ui.screens.NfcScreen
import com.quailsync.app.ui.screens.NfcViewModel
import com.quailsync.app.ui.theme.QuailSyncTheme
import com.quailsync.app.ui.theme.SageGreen
import com.quailsync.app.ui.theme.SageGreenLight
import kotlinx.coroutines.launch

sealed class Screen(val route: String, val label: String, val icon: ImageVector, val iconRes: Int? = null) {
    data object Dashboard : Screen("dashboard", "Dashboard", Icons.Default.Dashboard)
    data object Cameras : Screen("cameras", "Cameras", Icons.Default.Videocam)
    data object Flock : Screen("flock", "Flock", Icons.Default.Egg, iconRes = R.drawable.ic_bird)
    data object Nfc : Screen("nfc", "NFC", Icons.Default.Nfc)
    data object Clutches : Screen("clutches", "Hatchery", Icons.Default.Egg)
    data object Settings : Screen("settings", "Settings", Icons.Default.Settings)
    data object Telemetry : Screen("telemetry", "Telemetry", Icons.Default.Settings)
    data object Breeding : Screen("breeding", "Breeding", Icons.Default.Egg)
    data object Alerts : Screen("alerts", "Alerts", Icons.Default.Settings)
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
        val batchStateAtEntry = nfcViewModel.batchState.value
        val wasBatchScanning = batchStateAtEntry is BatchState.AwaitingTagScan
        val wasBatchWriting = batchStateAtEntry is BatchState.AwaitingTagWrite

        // Batch flow owns the NFC intent during AwaitingTagScan / AwaitingTagWrite.
        // No conflict detection, no bird lookup, no shared logic with the
        // standalone scanner — just overwrite whatever's on the tap with the
        // active payload and capture its hardware uniqueId. The user is
        // holding a chick in one hand and a tag in the other; every tap must
        // Just Work. DB uniqueness across previously-tagged birds is enforced
        // server-side via clear_nfc_tag_from_others on the create-bird POST.
        if (wasBatchScanning || wasBatchWriting) {
            val writeData = nfcService.pendingWriteData.value ?: when (batchStateAtEntry) {
                is BatchState.AwaitingTagScan -> "QS-L${batchStateAtEntry.lineageId}"
                is BatchState.AwaitingTagWrite -> "BIRD-${batchStateAtEntry.pendingBird.id}"
                else -> null
            }
            if (writeData != null) {
                val tagId = nfcService.handleBatchIntent(intent, writeData)
                if (tagId != null) {
                    Toast.makeText(this, "Tag written", Toast.LENGTH_SHORT).show()
                    when {
                        wasBatchScanning -> nfcViewModel.onBatchTagScanned(tagId)
                        wasBatchWriting -> nfcViewModel.onBatchTagWritten(tagId, true)
                    }
                }
                // Failure path: handleBatchIntent set _writeResult with the
                // retry-friendly message — BatchAwaitingScanScreen's banner
                // surfaces it. No toast (banner is the canonical error UX),
                // stay on the same bird, write mode still active for retry.
                navController?.navigate(Screen.Nfc.route) {
                    popUpTo(navController!!.graph.findStartDestination().id) { saveState = true }
                    launchSingleTop = true; restoreState = true
                }
                return
            }
        }

        // Standalone NFC screen / non-batch state: keep conflict detection +
        // bird lookup. A stray tap on someone else's tag here should not
        // silently clobber their bird's association.
        val (scanResult, writeAttempt) = nfcService.handleIntent(intent) ?: return

        if (writeAttempt != null) {
            when (writeAttempt) {
                is NfcService.WriteAttemptResult.Written -> {
                    Toast.makeText(this, "Wrote ${scanResult.payload ?: scanResult.tagId}", Toast.LENGTH_SHORT).show()
                }
                is NfcService.WriteAttemptResult.Conflict -> {
                    nfcViewModel.lookupConflictBird(writeAttempt.conflict)
                }
                is NfcService.WriteAttemptResult.Failed -> {
                    Toast.makeText(this, writeAttempt.message, Toast.LENGTH_SHORT).show()
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
                FloatingActionButton(
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
                    // Test-tag slug derived from the screen's stable route so
                    // the nav_* identifiers stay consistent with Screen.<>.route.
                    // ("clutches" maps to "hatchery" to match the visible label.)
                    val navTag = when (screen) {
                        Screen.Clutches -> "nav_hatchery"
                        else -> "nav_${screen.route}"
                    }
                    NavigationBarItem(
                        modifier = Modifier.testTag(navTag),
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
                    onAlertsClick = { navController.navigate(Screen.Alerts.route) { launchSingleTop = true } },
                )
            }
            composable(Screen.Alerts.route) {
                AlertsScreen(onBack = { navController.popBackStack() })
            }
            composable(Screen.Telemetry.route) {
                TelemetryScreen(
                    onBrooderClick = { id -> navController.navigate("brooder/$id") },
                    onBack = { navController.popBackStack() },
                )
            }
            // Route accepts an optional `?tab=N` query so callers can deep-link
            // directly to a tab (Flock's Cull and Breeding buttons do this).
            // The bare "breeding" path keeps working via the default value.
            composable(
                route = "${Screen.Breeding.route}?tab={tab}",
                arguments = listOf(navArgument("tab") { type = NavType.IntType; defaultValue = 0 }),
            ) { backStackEntry ->
                BreedingScreen(
                    initialTab = backStackEntry.arguments?.getInt("tab") ?: 0,
                    onBack = { navController.popBackStack() },
                )
            }
            composable(Screen.Cameras.route) { CameraScreen() }
            composable(Screen.Flock.route) {
                FlockScreen(
                    // Cull List was retired in favour of Flock's local Cull
                    // Mode, so we only deep-link to Breeding Groups now.
                    // Breeding Groups slid up to tab=0 after the removal.
                    onBreedingClick = {
                        navController.navigate("${Screen.Breeding.route}?tab=0") { launchSingleTop = true }
                    },
                )
            }
            composable(Screen.Nfc.route) { NfcScreen(nfcService = nfcService, viewModel = nfcViewModel) }
            composable(Screen.Clutches.route) {
                ClutchScreen(
                    onBandGroup = { group ->
                        // Seed the NFC batch with the group's current count and
                        // (first) lineage so users land directly on the per-bird
                        // entry screen instead of the empty Setup screen.
                        // Multi-lineage propagation through the batch flow is a
                        // separate follow-up — for now only the first lineage
                        // tag is carried into each new bird.
                        val firstLineageId = group.lineages.firstOrNull()?.id
                        if (firstLineageId != null && group.currentCount > 0) {
                            // Bug A: pass group.id so the NFC batch flow can
                            // flip status='Graduated' when banding finishes.
                            nfcViewModel.startBatchTagging(
                                count = group.currentCount,
                                lineageId = firstLineageId,
                                chickGroupId = group.id,
                            )
                        } else {
                            nfcViewModel.openBatchSetup()
                        }
                        navController.navigate(Screen.Nfc.route) {
                            popUpTo(navController.graph.findStartDestination().id) { saveState = true }
                            launchSingleTop = true; restoreState = true
                        }
                    },
                )
            }
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

    Column(
        Modifier
            .fillMaxSize()
            .padding(16.dp)
            .verticalScroll(rememberScrollState()),
    ) {
        Text("Settings", style = MaterialTheme.typography.headlineMedium)
        Spacer(Modifier.height(16.dp))

        // Server Connection card
        Card(
            Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(2.dp),
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
                OutlinedTextField(
                    value = serverUrl,
                    onValueChange = { serverUrl = it; serverUrlSaved = false },
                    label = { Text("Server URL") },
                    placeholder = { Text(ServerConfig.DEFAULT_URL) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(8.dp))
                Button(
                    onClick = {
                        ServerConfig.setServerUrl(context, serverUrl)
                        serverUrlSaved = true
                        Toast.makeText(context, "Server URL saved — restart app to apply", Toast.LENGTH_SHORT).show()
                    },
                    enabled = !serverUrlSaved,
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) {
                    Text(if (serverUrlSaved) "Saved" else "Save")
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // Breeding Configuration card. Drives the Flock-screen cull-mode
        // guardrail: minimum_males_needed = ceil(females / max_per_male)
        //                                   * desired_males_per_group.
        BreedingConfigCard()

        Spacer(Modifier.height(16.dp))

        // Notifications card
        Card(
            Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(12.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
            elevation = CardDefaults.cardElevation(2.dp),
        ) {
            Column(Modifier.padding(16.dp)) {
                Text("Notifications", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(12.dp))

                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text("Background Monitoring", style = MaterialTheme.typography.bodyLarge)
                        Text("Monitor brooder temps in background. Alerts when temperature is outside age-based range.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Switch(
                        checked = monitoringEnabled,
                        onCheckedChange = { monitoringEnabled = it; MonitoringService.setMonitoringEnabled(context, it) },
                        colors = SwitchDefaults.colors(checkedThumbColor = SageGreen, checkedTrackColor = SageGreenLight),
                    )
                }

                HorizontalDivider(Modifier.padding(vertical = 12.dp))
                Text("Alert Thresholds (age-based)", style = MaterialTheme.typography.bodyLarge)
                Spacer(Modifier.height(4.dp))
                Text("Temperature thresholds adjust by chick age:", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Week 1: 93-97°F, Week 2: 88-92°F, ..., Week 6+: 68-72°F", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("CRITICAL: >5°F outside range, WARNING: 2-5°F, INFO: 1-2°F", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Sensor offline: No data for 2 min", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)

                HorizontalDivider(Modifier.padding(vertical = 12.dp))
                Text("Hatch Reminders", style = MaterialTheme.typography.bodyLarge)
                Spacer(Modifier.height(4.dp))
                Text("Daily check at 8am for incubating clutches.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Alerts at: Day 7 (candle), Day 14 (lockdown), Day 16, Day 17+ (hatch)", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }

        Spacer(Modifier.height(16.dp))

        // Developer tools — only rendered when the server reports DEV_MODE=true.
        // On a production server the /api/dev/status route returns 404 and
        // the card never appears, so this is invisible to end users.
        DeveloperToolsCard()
    }
}

/**
 * Settings card for the breeding ratio that drives the Flock-screen
 * cull-mode guardrail. Reads `/api/settings` on first composition; Save
 * issues a PUT with the typed values. Both fields are positive ints; the
 * server enforces a 1..100 range and returns 400 if violated — we surface
 * that as a toast.
 */
@Composable
fun BreedingConfigCard() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val api = remember { QuailSyncApi.create(ServerConfig.getServerUrl(context)) }

    // String state so the user can clear the field while typing without us
    // snapping a zero in. We re-parse to Int at save time.
    var malesPerGroup by remember { mutableStateOf("") }
    var maxFemalesPerMale by remember { mutableStateOf("") }
    var loaded by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    var loadError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        try {
            val s = api.getSettings()
            malesPerGroup = s.desiredMalesPerGroup.toString()
            maxFemalesPerMale = s.maxFemalesPerMale.toString()
        } catch (e: Exception) {
            loadError = "Couldn't load settings — using local defaults"
            malesPerGroup = "1"
            maxFemalesPerMale = "5"
        } finally {
            loaded = true
        }
    }

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Text("Breeding Configuration", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Text(
                "Used by the Flock screen's Cull Mode to compute the minimum number of males the flock needs. Minimum = ceil(females ÷ max-per-male) × males-per-group.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            loadError?.let {
                Spacer(Modifier.height(4.dp))
                Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.error)
            }
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(
                value = malesPerGroup,
                onValueChange = { malesPerGroup = it.filter { c -> c.isDigit() }.take(3) },
                label = { Text("Males per breeding group") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true,
                enabled = loaded,
                modifier = Modifier.fillMaxWidth().testTag("settings_males_per_group"),
            )
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = maxFemalesPerMale,
                onValueChange = { maxFemalesPerMale = it.filter { c -> c.isDigit() }.take(3) },
                label = { Text("Max females per male") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true,
                enabled = loaded,
                modifier = Modifier.fillMaxWidth().testTag("settings_max_females_per_male"),
            )
            Spacer(Modifier.height(12.dp))
            val malesInt = malesPerGroup.toIntOrNull()
            val maxInt = maxFemalesPerMale.toIntOrNull()
            val valid = malesInt != null && maxInt != null &&
                malesInt in 1..100 && maxInt in 1..100
            Button(
                onClick = {
                    if (!valid) return@Button
                    saving = true
                    scope.launch {
                        try {
                            api.updateSettings(
                                UpdateAppSettings(
                                    desiredMalesPerGroup = malesInt,
                                    maxFemalesPerMale = maxInt,
                                )
                            )
                            Toast.makeText(context, "Breeding settings saved", Toast.LENGTH_SHORT).show()
                        } catch (e: Exception) {
                            Toast.makeText(context, "Save failed: ${e.message}", Toast.LENGTH_SHORT).show()
                        } finally {
                            saving = false
                        }
                    }
                },
                enabled = loaded && valid && !saving,
                colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
            ) { Text(if (saving) "Saving..." else "Save") }
        }
    }
}

/**
 * Probes /api/dev/status on first composition. A 200 response with
 * dev_mode=true renders the dev tools card; anything else (404, network
 * error, dev_mode=false) keeps it hidden, so production builds never see it.
 *
 * `has_backup` distinguishes "currently running production data" from
 * "currently running test data" — once seed has run, a backup exists, so
 * has_backup=true ≈ "test data active, original is preserved".
 */
@Composable
fun DeveloperToolsCard() {
    val context = LocalContext.current
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val api = remember { com.quailsync.app.data.QuailSyncApi.create(com.quailsync.app.data.ServerConfig.getServerUrl(context)) }

    // null = not yet checked; absent = server returned 404 / errored;
    // present = dev mode is on, render UI.
    var status by remember { mutableStateOf<com.quailsync.app.data.DevStatusResponse?>(null) }
    var statusChecked by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var lastMessage by remember { mutableStateOf<String?>(null) }

    suspend fun refreshStatus() {
        try {
            val resp = api.getDevStatus()
            status = if (resp.isSuccessful) resp.body() else null
        } catch (e: Exception) {
            android.util.Log.d("QuailSync", "DevTools: /api/dev/status unreachable — hiding card (${e.message})")
            status = null
        } finally {
            statusChecked = true
        }
    }

    androidx.compose.runtime.LaunchedEffect(Unit) { refreshStatus() }

    if (!statusChecked || status?.devMode != true) return

    Card(
        Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        // Distinct accent so the dev card never looks like a normal user-facing
        // setting (which historically led to users tapping "Restore" thinking
        // it was a recovery feature).
        colors = CardDefaults.cardColors(containerColor = androidx.compose.ui.graphics.Color(0xFFFFF8E1)),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Text("Developer Tools", style = MaterialTheme.typography.titleMedium)
                val (label, color) = if (status?.hasBackup == true)
                    "TEST DATA ACTIVE" to androidx.compose.ui.graphics.Color(0xFFC62828)
                else
                    "PRODUCTION DATA" to androidx.compose.ui.graphics.Color(0xFF2E7D32)
                Text(
                    label,
                    style = MaterialTheme.typography.labelSmall,
                    color = color,
                    fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                )
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "Server DEV_MODE is on. Seeding replaces the current DB with fixture data " +
                    "after backing it up; Restore swaps the backup back in.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            Spacer(Modifier.height(12.dp))

            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(
                    onClick = {
                        scope.launch {
                            busy = true
                            lastMessage = null
                            try {
                                val resp = api.seedDevData()
                                lastMessage = if (resp.isSuccessful) {
                                    "Test data loaded. Backup at ${resp.body()?.backup ?: "?"}."
                                } else "Seed failed: HTTP ${resp.code()}"
                                refreshStatus()
                            } catch (e: Exception) {
                                lastMessage = "Seed failed: ${e.message}"
                            } finally {
                                busy = false
                            }
                        }
                    },
                    enabled = !busy,
                    modifier = Modifier.weight(1f),
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) { Text("Load Test", fontSize = 12.sp) }

                Button(
                    onClick = {
                        scope.launch {
                            busy = true
                            lastMessage = null
                            try {
                                val resp = api.stressSeedDevData()
                                lastMessage = if (resp.isSuccessful) {
                                    "Stress test data loaded (60 birds, 10 lineages)."
                                } else "Stress seed failed: HTTP ${resp.code()}"
                                refreshStatus()
                            } catch (e: Exception) {
                                lastMessage = "Stress seed failed: ${e.message}"
                            } finally {
                                busy = false
                            }
                        }
                    },
                    enabled = !busy,
                    modifier = Modifier.weight(1f),
                    colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                ) { Text("Stress Test", fontSize = 12.sp) }
            }

            Spacer(Modifier.height(8.dp))

            Button(
                onClick = {
                    scope.launch {
                        busy = true
                        lastMessage = null
                        try {
                            val resp = api.restoreDevData()
                            lastMessage = when {
                                resp.isSuccessful -> "Production data restored."
                                resp.code() == 404 -> "No backup to restore — seed first."
                                else -> "Restore failed: HTTP ${resp.code()}"
                            }
                            refreshStatus()
                        } catch (e: Exception) {
                            lastMessage = "Restore failed: ${e.message}"
                        } finally {
                            busy = false
                        }
                    }
                },
                enabled = !busy && status?.hasBackup == true,
                modifier = Modifier.fillMaxWidth(),
                colors = ButtonDefaults.buttonColors(
                    containerColor = androidx.compose.ui.graphics.Color(0xFFC62828),
                ),
            ) { Text(if (status?.hasBackup == true) "Restore Production Data" else "No Backup To Restore") }

            if (busy) {
                Spacer(Modifier.height(8.dp))
                androidx.compose.material3.LinearProgressIndicator(
                    modifier = Modifier.fillMaxWidth(),
                    color = SageGreen,
                )
            }

            lastMessage?.let { msg ->
                Spacer(Modifier.height(8.dp))
                Text(
                    msg,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
