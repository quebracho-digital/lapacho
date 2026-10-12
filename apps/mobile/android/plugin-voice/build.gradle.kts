plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val rustOut = layout.buildDirectory.dir("generated/rust")

android {
    namespace = "digital.quebracho.lapacho.plugin.voice"
    compileSdk = 35

    defaultConfig {
        applicationId = "digital.quebracho.lapacho.plugin.voice"
        minSdk = 26
        targetSdk = 35
        // Its own version, like Lapacho Terms.
        versionCode = 3
        versionName = "0.1.2"
        // sherpa-onnx ships these two ABIs' libraries; see voice-bridge.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    // Same key as Lapacho's published APKs (see app/build.gradle.kts).
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

// lapacho-voice (the desktop's dictation) for Android, sherpa-onnx's
// libraries, and the uniffi Kotlin bindings.
val voiceBridge = tasks.register<Exec>("voiceBridge") {
    workingDir = rootDir.resolve("voice-bridge")
    commandLine("./build-android.sh", rustOut.get().asFile.path)
}
tasks.named("preBuild") { dependsOn(voiceBridge) }

dependencies {
    // Runtime the uniffi Kotlin bindings call the Rust library through.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
}
