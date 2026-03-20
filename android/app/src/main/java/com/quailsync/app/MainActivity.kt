package com.quailsync.app

import android.content.Intent
import android.nfc.NfcAdapter
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Dashboard
import androidx.compose.material.icons.filled.Egg
import androidx.compose.material.icons.filled.Nfc
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.sp
import androidx.navigation.NavDestination.Companion.hierarchy
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import com.quailsync.app.data.NfcService
import com.quailsync.app.ui.screens.BatchState
import com.quailsync.app.ui.screens.CameraScreen
import com.quailsync.app.ui.screens.ClutchScreen
import com.quailsync.app.ui.screens.DashboardScreen
import com.quailsync.app.ui.screens.FlockScreen
import com.quailsync.app.ui.screens.NfcScreen
import com.quailsync.app.ui.screens.NfcViewModel
import com.quailsync.app.ui.theme.QuailSyncTheme

sealed class Screen(val route: String, val label: String, val icon: ImageVector) {
    data object Dashboard : Screen("dashboard", "Dashboard", Icons.Default.Dashboard)
    data object Cameras : Screen("cameras", "Cameras", Icons.Default.Videocam)
    data object Flock : Screen("flock", "Flock", Icons.Default.Pets)
    data object Nfc : Screen("nfc", "NFC", Icons.Default.Nfc)
    data object Clutches : Screen("clutches", "Clutches", Icons.Default.Egg)
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        nfcAdapter = NfcAdapter.getDefaultAdapter(this)
        nfcService.checkAvailability(nfcAdapter)
        nfcViewModel = NfcViewModel(nfcService)

        handleNfcIntent(intent)

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

    private fun handleNfcIntent(intent: Intent) {
        val wasBatchWriting = nfcViewModel.batchState.value is BatchState.AwaitingTagWrite

        val (scanResult, writeAttempt) = nfcService.handleIntent(intent) ?: return

        // --- Write mode was active ---
        if (writeAttempt != null) {
            when (writeAttempt) {
                is NfcService.WriteAttemptResult.Written -> {
                    Toast.makeText(this, "Wrote ${scanResult.payload ?: scanResult.tagId}", Toast.LENGTH_SHORT).show()
                    if (wasBatchWriting) {
                        nfcViewModel.onBatchTagWritten(scanResult.tagId, true)
                    }
                }
                is NfcService.WriteAttemptResult.Conflict -> {
                    // Look up the existing bird to populate the dialog
                    nfcViewModel.lookupConflictBird(writeAttempt.conflict)
                    if (wasBatchWriting) {
                        // Batch is paused — dialog will show, user confirms or cancels
                        nfcViewModel.setBatchPausedForConflict(true)
                    }
                }
                is NfcService.WriteAttemptResult.Failed -> {
                    Toast.makeText(this, writeAttempt.message, Toast.LENGTH_SHORT).show()
                    if (wasBatchWriting) {
                        nfcViewModel.onBatchTagWritten(scanResult.tagId, false)
                    }
                }
            }
            // Navigate to NFC tab if not already there
            navController?.navigate(Screen.Nfc.route) {
                popUpTo(navController!!.graph.findStartDestination().id) { saveState = true }
                launchSingleTop = true
                restoreState = true
            }
            return
        }

        // --- Normal read mode ---
        nfcViewModel.lookupBirdByNfc(scanResult.tagId, scanResult.payload)

        val toastMsg = if (scanResult.payload?.startsWith("BIRD-") == true) {
            "Scanned: ${scanResult.payload}"
        } else {
            "NFC tag: ${scanResult.tagId}"
        }
        Toast.makeText(this, toastMsg, Toast.LENGTH_SHORT).show()

        navController?.navigate(Screen.Nfc.route) {
            popUpTo(navController!!.graph.findStartDestination().id) { saveState = true }
            launchSingleTop = true
            restoreState = true
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
        bottomBar = {
            NavigationBar {
                bottomNavItems.forEach { screen ->
                    NavigationBarItem(
                        icon = { Icon(screen.icon, contentDescription = screen.label) },
                        label = { Text(screen.label, fontSize = 11.sp) },
                        selected = currentDestination?.hierarchy?.any { it.route == screen.route } == true,
                        onClick = {
                            navController.navigate(screen.route) {
                                popUpTo(navController.graph.findStartDestination().id) {
                                    saveState = true
                                }
                                launchSingleTop = true
                                restoreState = true
                            }
                        },
                        colors = NavigationBarItemDefaults.colors(
                            selectedIconColor = com.quailsync.app.ui.theme.SageGreen,
                            selectedTextColor = com.quailsync.app.ui.theme.SageGreen,
                            indicatorColor = com.quailsync.app.ui.theme.SageGreenLight.copy(alpha = 0.3f),
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
            composable(Screen.Dashboard.route) { DashboardScreen() }
            composable(Screen.Cameras.route) { CameraScreen() }
            composable(Screen.Flock.route) { FlockScreen() }
            composable(Screen.Nfc.route) { NfcScreen(nfcService = nfcService, viewModel = nfcViewModel) }
            composable(Screen.Clutches.route) { ClutchScreen() }
        }
    }
}
