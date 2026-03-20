package com.quailsync.app.ui.screens

import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.util.Log
import android.view.ViewGroup
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CameraAlt
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Tab
import androidx.compose.material3.TabRow
import androidx.compose.material3.TabRowDefaults.SecondaryIndicator
import androidx.compose.material3.TabRowDefaults.tabIndicatorOffset
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import com.quailsync.app.data.Brooder
import com.quailsync.app.data.Camera
import com.quailsync.app.data.CreateCameraRequest
import com.quailsync.app.data.QuailSyncApi
import com.quailsync.app.data.UpdateBrooderRequest
import com.quailsync.app.ui.theme.SageGreen
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody

// =====================================================================
// Unified camera item — either from /api/cameras or a brooder's camera_url
// =====================================================================

data class CameraItem(
    val name: String,
    val subtitle: String?,
    val streamUrl: String?,
    val source: CameraSource,
)

sealed class CameraSource {
    data class Standalone(val camera: Camera) : CameraSource()
    data class BrooderCamera(val brooder: Brooder) : CameraSource()
}

// =====================================================================
// ViewModel
// =====================================================================

class CameraViewModel : ViewModel() {
    private val api = QuailSyncApi.create()

    private val _cameraItems = MutableStateFlow<List<CameraItem>>(emptyList())
    val cameraItems: StateFlow<List<CameraItem>> = _cameraItems.asStateFlow()

    private val _brooders = MutableStateFlow<List<Brooder>>(emptyList())
    val brooders: StateFlow<List<Brooder>> = _brooders.asStateFlow()

    private val _isLoading = MutableStateFlow(true)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    private val _isRefreshing = MutableStateFlow(false)
    val isRefreshing: StateFlow<Boolean> = _isRefreshing.asStateFlow()

    private val _saveError = MutableStateFlow<String?>(null)
    val saveError: StateFlow<String?> = _saveError.asStateFlow()

    init { loadAll() }

    fun refresh() {
        viewModelScope.launch {
            _isRefreshing.value = true
            loadAllSuspend()
            _isRefreshing.value = false
        }
    }

    private fun loadAll() { viewModelScope.launch { loadAllSuspend() } }

    private suspend fun loadAllSuspend() {
        val items = mutableListOf<CameraItem>()

        // Fetch standalone cameras
        try {
            val cameras = api.getCameras()
            Log.d("QuailSync", "Cameras loaded: ${cameras.size}")
            cameras.forEach { c ->
                items.add(CameraItem(
                    name = c.name,
                    subtitle = c.brooderName ?: c.location,
                    streamUrl = c.feedUrl ?: c.url,
                    source = CameraSource.Standalone(c),
                ))
            }
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load cameras", e)
        }

        // Fetch brooders with camera_url
        try {
            val brooders = api.getBrooders()
            _brooders.value = brooders
            Log.d("QuailSync", "Brooders loaded: ${brooders.size}")
            brooders.filter { it.cameraUrl != null }.forEach { b ->
                // Avoid duplicates if the camera is also in /api/cameras
                val alreadyListed = items.any { item ->
                    val url = item.streamUrl ?: ""
                    url == b.cameraUrl
                }
                if (!alreadyListed) {
                    items.add(CameraItem(
                        name = "${b.name} Camera",
                        subtitle = b.name,
                        streamUrl = b.cameraUrl,
                        source = CameraSource.BrooderCamera(b),
                    ))
                }
            }
        } catch (e: Exception) {
            Log.e("QuailSync", "Failed to load brooders for cameras", e)
        }

        _cameraItems.value = items
        _isLoading.value = false
    }

    fun setBrooderCameraUrl(brooderId: Int, url: String) {
        _saveError.value = null
        viewModelScope.launch {
            try {
                api.updateBrooder(brooderId, UpdateBrooderRequest(cameraUrl = url))
                Log.d("QuailSync", "Set camera_url on brooder $brooderId: $url")
                loadAllSuspend()
            } catch (e: retrofit2.HttpException) {
                val body = e.response()?.errorBody()?.string()
                _saveError.value = "HTTP ${e.code()}: ${body ?: "Unknown error"}"
                Log.e("QuailSync", "Failed to set brooder camera: ${_saveError.value}", e)
            } catch (e: Exception) {
                _saveError.value = "Failed: ${e.message}"
                Log.e("QuailSync", "Failed to set brooder camera", e)
            }
        }
    }

    fun createStandaloneCamera(name: String, url: String, location: String?) {
        _saveError.value = null
        viewModelScope.launch {
            try {
                api.createCamera(CreateCameraRequest(name = name, feedUrl = url, location = location))
                Log.d("QuailSync", "Created standalone camera: $name")
                loadAllSuspend()
            } catch (e: retrofit2.HttpException) {
                val body = e.response()?.errorBody()?.string()
                _saveError.value = "HTTP ${e.code()}: ${body ?: "Unknown error"}"
                Log.e("QuailSync", "Failed to create camera: ${_saveError.value}", e)
            } catch (e: Exception) {
                _saveError.value = "Failed: ${e.message}"
                Log.e("QuailSync", "Failed to create camera", e)
            }
        }
    }

    fun deleteCamera(item: CameraItem) {
        Log.d("QuailSync", "deleteCamera called: name='${item.name}', source=${item.source::class.simpleName}")
        val baseUrl = com.quailsync.app.BuildConfig.BASE_URL.trimEnd('/')
        viewModelScope.launch {
            try {
                val (code, respBody) = when (val src = item.source) {
                    is CameraSource.Standalone -> {
                        val url = "$baseUrl/api/cameras/${src.camera.id}"
                        Log.d("QuailSync", "DELETE $url (standalone '${src.camera.name}')")
                        withContext(Dispatchers.IO) {
                            val req = okhttp3.Request.Builder().url(url).delete().build()
                            val resp = okhttp3.OkHttpClient().newCall(req).execute()
                            val body = resp.body?.string()
                            Log.d("QuailSync", "Delete response: ${resp.code} body=$body")
                            Pair(resp.code, body)
                        }
                    }
                    is CameraSource.BrooderCamera -> {
                        val url = "$baseUrl/api/brooders/${src.brooder.id}"
                        val json = """{"camera_url": null}"""
                        Log.d("QuailSync", "PUT $url body=$json (brooder '${src.brooder.name}')")
                        withContext(Dispatchers.IO) {
                            val body = json.toRequestBody("application/json".toMediaType())
                            val req = okhttp3.Request.Builder().url(url).put(body).build()
                            val resp = okhttp3.OkHttpClient().newCall(req).execute()
                            val respBody = resp.body?.string()
                            Log.d("QuailSync", "Clear camera response: ${resp.code} body=$respBody")
                            Pair(resp.code, respBody)
                        }
                    }
                }
                if (code in 200..299) {
                    Log.d("QuailSync", "Delete OK, refreshing camera list")
                    loadAllSuspend()
                } else {
                    Log.e("QuailSync", "Delete failed: HTTP $code body=$respBody")
                    _saveError.value = "Delete failed: HTTP $code"
                }
            } catch (e: Exception) {
                Log.e("QuailSync", "Delete failed", e)
                _saveError.value = "Delete failed: ${e.message}"
            }
        }
    }

    fun clearSaveError() { _saveError.value = null }
}

// =====================================================================
// Camera Screen
// =====================================================================

@Composable
fun CameraScreen(viewModel: CameraViewModel = viewModel()) {
    val cameraItems by viewModel.cameraItems.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val isRefreshing by viewModel.isRefreshing.collectAsState()
    var showAddDialog by remember { mutableStateOf(false) }

    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
            Arrangement.SpaceBetween, Alignment.CenterVertically,
        ) {
            Text("Cameras", style = MaterialTheme.typography.headlineMedium)
            Row {
                if (isRefreshing) {
                    CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp, color = SageGreen)
                } else {
                    IconButton(onClick = { viewModel.refresh() }) { Icon(Icons.Default.Refresh, "Refresh") }
                }
                IconButton(onClick = { showAddDialog = true }) { Icon(Icons.Default.Add, "Add Camera", tint = SageGreen) }
            }
        }

        when {
            isLoading && cameraItems.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = SageGreen) }
            }
            cameraItems.isEmpty() -> {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Icon(Icons.Default.VideocamOff, null, Modifier.size(64.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.height(16.dp))
                        Text("No cameras configured.", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(8.dp))
                        Button(onClick = { showAddDialog = true }, colors = ButtonDefaults.buttonColors(containerColor = SageGreen)) {
                            Icon(Icons.Default.Add, null, Modifier.size(18.dp))
                            Spacer(Modifier.size(6.dp))
                            Text("Add Camera")
                        }
                    }
                }
            }
            else -> {
                LazyColumn(contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                    items(cameraItems) { item -> CameraCard(item, onDelete = { viewModel.deleteCamera(item) }) }
                    item { Spacer(Modifier.height(8.dp)) }
                }
            }
        }
    }

    if (showAddDialog) {
        AddCameraDialog(viewModel) { showAddDialog = false }
    }
}

// =====================================================================
// Camera Card
// =====================================================================

@Composable
fun CameraCard(item: CameraItem, onDelete: () -> Unit = {}) {
    val context = LocalContext.current
    var showFullScreen by remember { mutableStateOf(false) }
    var showDeleteConfirm by remember { mutableStateOf(false) }
    val snapshotUrl = item.streamUrl?.replace("/stream", "/snapshot")

    if (showDeleteConfirm) {
        val sourceType = when (item.source) {
            is CameraSource.Standalone -> "standalone #${item.source.camera.id}"
            is CameraSource.BrooderCamera -> "brooder #${item.source.brooder.id} (${item.source.brooder.name})"
        }
        val sourceLabel = when (item.source) {
            is CameraSource.Standalone -> "This will remove the camera '${item.name}' from the system."
            is CameraSource.BrooderCamera -> "This will clear the camera URL from ${item.source.brooder.name}."
        }
        Log.d("QuailSync", "Delete confirm dialog shown for: $sourceType")
        AlertDialog(
            onDismissRequest = { showDeleteConfirm = false },
            title = { Text("Remove Camera?") },
            text = { Text(sourceLabel) },
            confirmButton = {
                Button(
                    onClick = {
                        Log.d("QuailSync", "Delete confirmed for: $sourceType")
                        showDeleteConfirm = false
                        onDelete()
                    },
                    colors = ButtonDefaults.buttonColors(containerColor = androidx.compose.ui.graphics.Color(0xFFCC4444)),
                ) { Text("Remove") }
            },
            dismissButton = {
                OutlinedButton(onClick = { Log.d("QuailSync", "Delete cancelled"); showDeleteConfirm = false }) { Text("Cancel") }
            },
        )
    }

    Card(
        Modifier.fillMaxWidth(), shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        elevation = CardDefaults.cardElevation(2.dp),
    ) {
        Column(Modifier.padding(16.dp)) {
            Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(item.name, style = MaterialTheme.typography.titleLarge)
                    if (item.subtitle != null) Text(item.subtitle, style = MaterialTheme.typography.bodyMedium)
                }
                Row {
                    Icon(Icons.Default.CameraAlt, null, tint = SageGreen)
                    IconButton(onClick = {
                        Log.d("QuailSync", "Delete icon tapped for camera: '${item.name}'")
                        showDeleteConfirm = true
                    }) {
                        Icon(Icons.Default.Delete, "Delete", tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }

            Spacer(Modifier.height(12.dp))

            if (item.streamUrl != null) {
                MjpegStreamView(item.streamUrl, Modifier.fillMaxWidth().aspectRatio(16f / 9f).clip(RoundedCornerShape(8.dp)))
                Spacer(Modifier.height(12.dp))
                Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(
                        onClick = {
                            if (snapshotUrl != null) context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(snapshotUrl)))
                        },
                        Modifier.weight(1f),
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = SageGreen),
                    ) {
                        Icon(Icons.Default.PhotoCamera, null, Modifier.size(18.dp))
                        Spacer(Modifier.size(6.dp))
                        Text("Snapshot")
                    }
                    Button(
                        onClick = { showFullScreen = true },
                        Modifier.weight(1f),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) {
                        Icon(Icons.Default.Fullscreen, null, Modifier.size(18.dp))
                        Spacer(Modifier.size(6.dp))
                        Text("Full Screen")
                    }
                }
            } else {
                CameraOfflinePlaceholder()
            }
        }
    }

    if (showFullScreen && item.streamUrl != null) {
        FullScreenStreamDialog(item.streamUrl, item.name) { showFullScreen = false }
    }
}

// =====================================================================
// Add Camera Dialog
// =====================================================================

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddCameraDialog(viewModel: CameraViewModel, onDismiss: () -> Unit) {
    val brooders by viewModel.brooders.collectAsState()
    val saveError by viewModel.saveError.collectAsState()
    var tabIndex by remember { mutableIntStateOf(0) }

    // Brooder camera fields
    var selectedBrooderId by remember { mutableStateOf<Int?>(null) }
    var brooderCameraUrl by remember { mutableStateOf("") }
    var brooderExpanded by remember { mutableStateOf(false) }

    // Standalone camera fields
    var cameraName by remember { mutableStateOf("") }
    var cameraUrl by remember { mutableStateOf("") }
    var cameraLocation by remember { mutableStateOf("") }

    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Card(
            Modifier.fillMaxWidth().padding(16.dp),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            Column(Modifier.padding(20.dp)) {
                Row(Modifier.fillMaxWidth(), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Text("Add Camera", style = MaterialTheme.typography.headlineMedium)
                    IconButton(onClick = onDismiss) { Icon(Icons.Default.Close, "Close") }
                }

                Spacer(Modifier.height(8.dp))

                TabRow(
                    selectedTabIndex = tabIndex,
                    containerColor = MaterialTheme.colorScheme.surface,
                    indicator = { tabPositions ->
                        if (tabIndex < tabPositions.size) {
                            SecondaryIndicator(Modifier.tabIndicatorOffset(tabPositions[tabIndex]), color = SageGreen)
                        }
                    },
                ) {
                    Tab(tabIndex == 0, { tabIndex = 0 }) { Text("Brooder Camera", Modifier.padding(12.dp)) }
                    Tab(tabIndex == 1, { tabIndex = 1 }) { Text("Standalone", Modifier.padding(12.dp)) }
                }

                Spacer(Modifier.height(16.dp))

                if (tabIndex == 0) {
                    // Brooder camera
                    Text("Assign a camera stream URL to a brooder.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(12.dp))

                    ExposedDropdownMenuBox(brooderExpanded, { brooderExpanded = it }) {
                        OutlinedTextField(
                            value = selectedBrooderId?.let { id -> brooders.find { it.id == id }?.name ?: "Brooder #$id" } ?: "",
                            onValueChange = {}, readOnly = true,
                            label = { Text("Select brooder") },
                            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(brooderExpanded) },
                            modifier = Modifier.menuAnchor().fillMaxWidth(),
                        )
                        ExposedDropdownMenu(brooderExpanded, { brooderExpanded = false }) {
                            brooders.forEach { b ->
                                val existing = if (b.cameraUrl != null) " (has camera)" else ""
                                DropdownMenuItem(
                                    text = { Text("${b.name}$existing") },
                                    onClick = { selectedBrooderId = b.id; brooderExpanded = false },
                                )
                            }
                        }
                    }

                    Spacer(Modifier.height(8.dp))

                    OutlinedTextField(
                        value = brooderCameraUrl,
                        onValueChange = { brooderCameraUrl = it },
                        label = { Text("Stream URL") },
                        placeholder = { Text("http://192.168.0.114:8080/stream") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )

                    Spacer(Modifier.height(12.dp))

                    Button(
                        onClick = {
                            selectedBrooderId?.let { viewModel.setBrooderCameraUrl(it, brooderCameraUrl) }
                            onDismiss()
                        },
                        enabled = selectedBrooderId != null && brooderCameraUrl.isNotBlank(),
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) { Text("Save Camera URL") }
                } else {
                    // Standalone camera
                    Text("Add a camera not tied to a specific brooder.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(12.dp))

                    OutlinedTextField(
                        value = cameraName,
                        onValueChange = { cameraName = it },
                        label = { Text("Camera name") },
                        placeholder = { Text("Barn Overview") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )

                    Spacer(Modifier.height(8.dp))

                    OutlinedTextField(
                        value = cameraUrl,
                        onValueChange = { cameraUrl = it },
                        label = { Text("Stream URL") },
                        placeholder = { Text("http://192.168.0.114:8080/stream") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )

                    Spacer(Modifier.height(8.dp))

                    OutlinedTextField(
                        value = cameraLocation,
                        onValueChange = { cameraLocation = it },
                        label = { Text("Location (optional)") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                    )

                    Spacer(Modifier.height(12.dp))

                    Button(
                        onClick = {
                            viewModel.createStandaloneCamera(cameraName, cameraUrl, cameraLocation.ifBlank { null })
                            onDismiss()
                        },
                        enabled = cameraName.isNotBlank() && cameraUrl.isNotBlank(),
                        modifier = Modifier.fillMaxWidth(),
                        colors = ButtonDefaults.buttonColors(containerColor = SageGreen),
                    ) { Text("Add Camera") }
                }

                if (saveError != null) {
                    Spacer(Modifier.height(8.dp))
                    Text(saveError!!, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

// =====================================================================
// MJPEG Stream WebView
// =====================================================================

@Composable
fun MjpegStreamView(url: String, modifier: Modifier = Modifier) {
    var hasError by remember { mutableStateOf(false) }
    var isLoading by remember { mutableStateOf(true) }

    // MJPEG streams must be loaded as <img src> — loading the URL directly
    // in a WebView shows garbled multipart text. Wrap in a minimal HTML page.
    LaunchedEffect(url) { delay(5000L); isLoading = false }

    val html = remember(url) {
        """
        <html><head>
        <meta name="viewport" content="width=device-width,initial-scale=1">
        <style>body{margin:0;padding:0;background:#000;display:flex;align-items:center;justify-content:center;height:100vh}
        img{width:100%;height:auto;object-fit:contain}</style>
        </head><body>
        <img src="${url.replace("\"", "&quot;")}" alt="Camera stream">
        </body></html>
        """.trimIndent()
    }

    Box(modifier = modifier) {
        if (hasError) {
            CameraOfflinePlaceholder()
        } else {
            AndroidView(
                factory = { ctx ->
                    WebView(ctx).apply {
                        layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                        settings.javaScriptEnabled = false
                        settings.loadWithOverviewMode = true
                        settings.useWideViewPort = true
                        settings.blockNetworkImage = false
                        setBackgroundColor(android.graphics.Color.BLACK)
                        webViewClient = object : WebViewClient() {
                            override fun onPageFinished(view: WebView?, url: String?) { isLoading = false }
                            override fun onReceivedError(view: WebView?, request: WebResourceRequest?, error: WebResourceError?) {
                                if (request?.isForMainFrame == true) { hasError = true; isLoading = false }
                            }
                        }
                        loadDataWithBaseURL(null, html, "text/html", "utf-8", null)
                    }
                },
                modifier = Modifier.fillMaxSize(),
            )
            if (isLoading) {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(color = SageGreen, modifier = Modifier.size(32.dp), strokeWidth = 2.dp)
                }
            }
        }
    }
}

@Composable
fun CameraOfflinePlaceholder() {
    Box(
        Modifier.fillMaxWidth().aspectRatio(16f / 9f).clip(RoundedCornerShape(8.dp)).background(MaterialTheme.colorScheme.surfaceVariant),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(Icons.Default.VideocamOff, null, Modifier.size(40.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(8.dp))
            Text("Camera unreachable", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
fun FullScreenStreamDialog(url: String, cameraName: String, onDismiss: () -> Unit) {
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            Column(Modifier.fillMaxSize()) {
                Row(Modifier.fillMaxWidth().padding(8.dp), Arrangement.SpaceBetween, Alignment.CenterVertically) {
                    Text(cameraName, style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(start = 8.dp))
                    OutlinedButton(onClick = onDismiss) { Text("Close") }
                }
                MjpegStreamView(url, Modifier.fillMaxSize().padding(8.dp).clip(RoundedCornerShape(8.dp)))
            }
        }
    }
}
