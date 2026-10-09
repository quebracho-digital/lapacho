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
        versionCode = 43
        versionName = "0.1.42-release-key"
        // The only ABIs the Rust bridge is built for (a device and the
        // emulator). Without this, JNA ships six more and a phone on one of
        // those would install the app and then crash loading the bridge.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    // Every published APK is signed with the release key (certificate SHA-256
    // in the README): Android installs an update only over the same key. CI
    // and minisforum hand it over in LAPACHO_RELEASE_KEYSTORE(_PASSWORD).
    // Without them a release build falls back to the debug key, fine for your
    // own phone; CI refuses to publish one.
    signingConfigs {
        System.getenv("LAPACHO_RELEASE_KEYSTORE")?.let { path ->
            create("release") {
                storeFile = file(path)
                storePassword = System.getenv("LAPACHO_RELEASE_KEYSTORE_PASSWORD")
                keyAlias = "lapacho"
                keyPassword = storePassword
            }
        }
    }

    // Published APKs are release builds: a debuggable one lets anyone with USB
    // access `run-as` into the app's data or attach a debugger and read the
    // clipboard in memory.
    buildTypes {
        getByName("release") {
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
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
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    // Runtime the uniffi Kotlin bindings call the Rust library through.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    testImplementation("junit:junit:4.13.2")
}
