plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val rustOut = layout.buildDirectory.dir("generated/rust")

android {
    namespace = "digital.quebracho.lapacho.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "digital.quebracho.lapacho"
        minSdk = 26
        targetSdk = 35
        versionCode = 24
        versionName = "0.1.23-descargas"
        // The only ABIs the Rust bridge is built for (a device and the
        // emulator). Without this, JNA ships six more and a phone on one of
        // those would install the app and then crash loading the bridge.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    sourceSets["main"].java.srcDir(rustOut.map { it.dir("kotlin") })
    sourceSets["main"].jniLibs.srcDir(rustOut.map { it.dir("jniLibs") })
}

// lapacho-core, compiled for Android, plus its uniffi Kotlin bindings. Needs
// the Rust toolchain with cargo-ndk and the Android targets installed.
val rustBridge = tasks.register<Exec>("rustBridge") {
    workingDir = rootDir.resolve("rust-bridge")
    commandLine("./build-android.sh", rustOut.get().asFile.path)
}
tasks.named("preBuild") { dependsOn(rustBridge) }

dependencies {
    implementation(project(":storage"))
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    // Runtime the uniffi Kotlin bindings call the Rust library through.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    testImplementation("junit:junit:4.13.2")
}
