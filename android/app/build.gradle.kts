plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.quailsync.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.quailsync.app"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"

        buildConfigField("String", "BASE_URL", "\"https://quailsync.tail01d133.ts.net\"")

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    // The Android 14+ foreground-service contract is satisfied in the
    // manifest (FOREGROUND_SERVICE + FOREGROUND_SERVICE_DATA_SYNC +
    // foregroundServiceType="dataSync" on MonitoringService). Lint's pairing
    // check still warns intermittently because it can't always see the
    // pairing across manifest elements. The matching tools:ignore in
    // AndroidManifest.xml handles most cases; this disables the same checks
    // at the Gradle level as a safety net for IDE inspection versions whose
    // ID isn't in the manifest's ignore list.
    lint {
        disable += setOf(
            "ForegroundServicePermission",
            "ForegroundServiceType",
            "MissingForegroundServiceType",
            "SpecialUseFgsType",
        )
    }
}

// Force AndroidX Test artifacts to Android 15-compatible versions, including
// any transitive copies pulled in by older libraries (Compose BOM, etc.). The
// `InputManager.getInstance` crash on API 35 happens when an older espresso
// sneaks in transitively; pinning here overrides that resolution.
//
// Note: this block lives at the top level rather than inside `android { }`
// because `configurations` is on Project, not on the AndroidExtension —
// putting it inside the android block doesn't compile.
configurations.all {
    resolutionStrategy {
        force("androidx.test.espresso:espresso-core:3.6.1")
        force("androidx.test:runner:1.6.2")
        force("androidx.test:core:1.6.1")
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.09.00")
    implementation(composeBom)

    // Compose
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.8.2")

    // Navigation
    implementation("androidx.navigation:navigation-compose:2.7.6")

    // Lifecycle + ViewModel
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.7.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.7.0")

    // Retrofit + Gson
    implementation("com.squareup.retrofit2:retrofit:2.9.0")
    implementation("com.squareup.retrofit2:converter-gson:2.9.0")

    // OkHttp
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("com.squareup.okhttp3:logging-interceptor:4.12.0")

    // Coil — async image loading for remote bird photos (Compose). The server
    // is reached over a system-trusted Tailscale ts.net cert, so Coil's default
    // ImageLoader works without custom OkHttp/trust wiring.
    implementation("io.coil-kt:coil-compose:2.7.0")

    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")

    // Core
    implementation("androidx.core:core-ktx:1.12.0")

    // WorkManager
    implementation("androidx.work:work-runtime-ktx:2.9.0")

    // Material Design Components (for XML theme)
    implementation("com.google.android.material:material:1.11.0")

    // ML Kit Barcode Scanning
    implementation("com.google.mlkit:barcode-scanning:17.3.0")

    // CameraX (for QR scanner viewfinder)
    val cameraxVersion = "1.3.1"
    implementation("androidx.camera:camera-core:$cameraxVersion")
    implementation("androidx.camera:camera-camera2:$cameraxVersion")
    implementation("androidx.camera:camera-lifecycle:$cameraxVersion")
    implementation("androidx.camera:camera-view:$cameraxVersion")

    // Debug
    debugImplementation("androidx.compose.ui:ui-tooling")

    // Instrumented UI tests (Compose) — versions pinned explicitly because
    // reusing `composeBom` in the androidTest configuration doesn't reliably
    // propagate versions in this AGP build, leaving the test artifacts
    // unresolved at compile time. 1.6.8 matches Compose BOM 2024.06.00.
    val composeTestVersion = "1.6.8"
    androidTestImplementation("androidx.compose.ui:ui-test:$composeTestVersion")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4:$composeTestVersion")
    debugImplementation("androidx.compose.ui:ui-test-manifest:$composeTestVersion")
    // AndroidX Test — versions bumped to Android 15 (API 35) compatible
    // releases. Older 1.1.5 / 1.5.2 pulls in a manifest that fails to merge
    // on API 35.
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    // ext:junit *should* pull JUnit 4 in transitively, but it doesn't here —
    // declare it explicitly so org.junit.* resolves in TestHelper.kt.
    androidTestImplementation("junit:junit:4.13.2")
    androidTestImplementation("com.squareup.okhttp3:okhttp:4.12.0")
}
