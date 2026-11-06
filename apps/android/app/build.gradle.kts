import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}
// google-services.json is not in git. Without it the app builds and is never woken by Firebase.
if (file("google-services.json").exists()) apply(plugin = "com.google.gms.google-services")
android {
    namespace = "com.flashmind.beep"
    compileSdk = 36
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "com.flashmind.beep"
        minSdk = 30
        targetSdk = 36
        // CI passes a growing number, since Play refuses a version code it has seen before.
        versionCode = providers.gradleProperty("beepVersionCode").orNull?.toInt() ?: 5
        versionName = "0.1.13"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86") }
        // The server this build talks to, so a new install needs no setup. `just bake-profile`
        // writes it; -PbeepServerUrl overrides it.
        val profile = rootProject.file("../../.dev/android-assets/testing-profile.json")
            .takeIf { it.exists() }?.readText()
        val serverUrl = providers.gradleProperty("beepServerUrl").orNull
            ?: profile?.let { Regex("\"server_url\"\\s*:\\s*\"([^\"]+)\"").find(it)?.groupValues?.get(1) }
            ?: ""
        buildConfigField("String", "SERVER_URL", "\"$serverUrl\"")
    }
    androidResources { localeFilters += "en" }
    // CI writes keystore.properties from secrets. Without it a release build is signed with
    // the debug key, which lets it install over the debug build.
    val keystore = rootProject.file("keystore.properties")
    if (keystore.exists()) {
        val props = Properties().apply { keystore.inputStream().use { load(it) } }
        signingConfigs.create("release") {
            storeFile = file(props.getProperty("storeFile"))
            storePassword = props.getProperty("storePassword")
            keyAlias = props.getProperty("keyAlias")
            keyPassword = props.getProperty("keyPassword")
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }
    sourceSets.getByName("main").java.srcDir("build/generated/uniffi")
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    packaging { jniLibs.useLegacyPackaging = true }
    testOptions { unitTests.isReturnDefaultValues = true }
}
dependencies {
    implementation(platform("androidx.compose:compose-bom:2025.05.01"))
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.0")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.glance:glance-appwidget:1.1.1")
    implementation("androidx.glance:glance-material3:1.1.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-play-services:1.9.0")
    implementation("com.google.android.gms:play-services-auth-blockstore:16.4.0")
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
    implementation("com.google.android.gms:play-services-location:21.3.0")
    implementation("com.google.zxing:core:3.5.3")
    implementation("androidx.camera:camera-camera2:1.4.2")
    implementation("androidx.camera:camera-lifecycle:1.4.2")
    implementation("androidx.camera:camera-view:1.4.2")
    implementation(platform("com.google.firebase:firebase-bom:34.19.0"))
    implementation("com.google.firebase:firebase-messaging")
    // Block Store pulls in an old Fragment that breaks registerForActivityResult.
    implementation("androidx.fragment:fragment-ktx:1.8.6")
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("ru.noties:jlatexmath-android:0.2.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20250107")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
}
val buildNative by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir.parentFile.parentFile)
    commandLine("sh", "scripts/build-android-native.sh")
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    inputs.files(fileTree("../../../crates/proto") { exclude("target/**") })
    inputs.files(fileTree("../../../crates/android-bridge") { exclude("target/**") })
    inputs.files(fileTree("../../../crates/live") { exclude("target/**") })
    inputs.file("../../../Cargo.lock")
    outputs.dir("src/main/jniLibs")
    outputs.dir("build/generated/uniffi")
}
tasks.named("preBuild").configure { dependsOn(buildNative) }
