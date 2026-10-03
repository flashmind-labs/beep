plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}
android {
    namespace = "com.flashmind.beep"
    compileSdk = 36
    ndkVersion = "28.2.13676358"
    defaultConfig {
        applicationId = "com.flashmind.beep"
        minSdk = 30
        targetSdk = 36
        versionCode = 5
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += "arm64-v8a" }
    }
    sourceSets.getByName("debug").assets.srcDir("../../.dev/android-assets")
    sourceSets.getByName("main").java.srcDir("build/generated/uniffi")
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    packaging { jniLibs.useLegacyPackaging = false }
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
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20250107")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
}
val buildNative by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir.parentFile)
    commandLine("sh", "scripts/build-android-native.sh")
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    inputs.files(fileTree("../../crates/proto") { exclude("target/**") })
    inputs.files(fileTree("../../crates/android-bridge") { exclude("target/**") })
    inputs.file("../../Cargo.lock")
    outputs.file("src/main/jniLibs/arm64-v8a/libbeep_android.so")
    outputs.dir("build/generated/uniffi")
}
tasks.named("preBuild").configure { dependsOn(buildNative) }
