package com.quailsync.app.ui.screens

import android.Manifest
import android.content.pm.PackageManager
import android.util.Log
import android.util.Size
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.OptIn
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import java.util.concurrent.Executors

@Composable
fun QrScannerDialog(
    onQrScanned: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    val context = LocalContext.current
    var hasCameraPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
        )
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted -> hasCameraPermission = granted }

    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Card(
            Modifier.fillMaxWidth(0.9f),
            shape = RoundedCornerShape(16.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            Column(
                Modifier.padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text("Scan Brooder QR Code", style = MaterialTheme.typography.titleMedium)
                Spacer(Modifier.height(8.dp))
                Text(
                    "Point at a brooder QR code (format: brooder-1-texas)",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(12.dp))

                if (hasCameraPermission) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .aspectRatio(4f / 3f)
                            .clip(RoundedCornerShape(8.dp))
                    ) {
                        CameraPreviewWithScanner(onQrScanned = onQrScanned)
                    }
                } else {
                    Text("Camera permission required", style = MaterialTheme.typography.bodyMedium)
                    Spacer(Modifier.height(8.dp))
                    OutlinedButton(onClick = { permissionLauncher.launch(Manifest.permission.CAMERA) }) {
                        Text("Grant Permission")
                    }
                }

                Spacer(Modifier.height(12.dp))
                OutlinedButton(onClick = onDismiss) { Text("Cancel") }
            }
        }
    }
}

@OptIn(ExperimentalGetImage::class)
@Composable
private fun CameraPreviewWithScanner(onQrScanned: (String) -> Unit) {
    val context = LocalContext.current
    val lifecycleOwner = context as LifecycleOwner
    var scanned by remember { mutableStateOf(false) }

    val previewView = remember { PreviewView(context) }
    val executor = remember { Executors.newSingleThreadExecutor() }

    DisposableEffect(Unit) {
        val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
        cameraProviderFuture.addListener({
            val cameraProvider = cameraProviderFuture.get()

            val preview = Preview.Builder().build().also {
                it.surfaceProvider = previewView.surfaceProvider
            }

            val options = BarcodeScannerOptions.Builder()
                .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
                .build()
            val scanner = BarcodeScanning.getClient(options)

            val analysis = ImageAnalysis.Builder()
                .setTargetResolution(Size(1280, 720))
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()

            analysis.setAnalyzer(executor) { imageProxy ->
                processImage(imageProxy, scanner) { qrValue ->
                    if (!scanned) {
                        scanned = true
                        onQrScanned(qrValue)
                    }
                }
            }

            try {
                cameraProvider.unbindAll()
                cameraProvider.bindToLifecycle(
                    lifecycleOwner,
                    CameraSelector.DEFAULT_BACK_CAMERA,
                    preview,
                    analysis,
                )
            } catch (e: Exception) {
                Log.e("QuailSync", "Camera bind failed", e)
            }
        }, ContextCompat.getMainExecutor(context))

        onDispose {
            try {
                cameraProviderFuture.get().unbindAll()
            } catch (_: Exception) {}
        }
    }

    AndroidView(
        factory = { previewView },
        modifier = Modifier.fillMaxSize(),
    )
}

@OptIn(ExperimentalGetImage::class)
private fun processImage(
    imageProxy: ImageProxy,
    scanner: com.google.mlkit.vision.barcode.BarcodeScanner,
    onFound: (String) -> Unit,
) {
    val mediaImage = imageProxy.image
    if (mediaImage == null) {
        imageProxy.close()
        return
    }

    val inputImage = InputImage.fromMediaImage(mediaImage, imageProxy.imageInfo.rotationDegrees)
    scanner.process(inputImage)
        .addOnSuccessListener { barcodes ->
            for (barcode in barcodes) {
                val value = barcode.rawValue ?: continue
                if (value.startsWith("brooder-", ignoreCase = true)) {
                    onFound(value)
                    break
                }
            }
        }
        .addOnFailureListener { e ->
            Log.w("QuailSync", "QR scan failed: ${e.message}")
        }
        .addOnCompleteListener {
            imageProxy.close()
        }
}
