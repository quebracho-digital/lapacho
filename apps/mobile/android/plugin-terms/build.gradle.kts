plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The request's words come from lapacho-core, the same file the desktop and
// Lapacho's built-in plugin use, copied in as an asset at build time.
val requestAsset = layout.buildDirectory.dir("generated/requestAsset")
val copyRequest = tasks.register<Copy>("copyTermsRequest") {
    from(rootDir.resolve("../../../crates/lapacho-core/src/terms_request.txt"))
    into(requestAsset)
}
tasks.named("preBuild") { dependsOn(copyRequest) }

android {
    namespace = "digital.quebracho.lapacho.plugin.terms"
    compileSdk = 35

    defaultConfig {
        applicationId = "digital.quebracho.lapacho.plugin.terms"
        minSdk = 26
        targetSdk = 35
        // Lapacho's version, read from its build: one release, one version
        // for the three apps, so Obtainium can match an installed plugin to
        // the release tag (with versions of its own it never could).
        versionCode = lapachoVersion("versionCode = (\\d+)").toInt()
        versionName = lapachoVersion("versionName = \"(.*)\"")
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
    sourceSets["main"].assets.srcDir(requestAsset)
}

dependencies {
    testImplementation("junit:junit:4.13.2")
    // Android's org.json is a stub in JVM tests; the real library stands in.
    testImplementation("org.json:json:20240303")
}

fun lapachoVersion(pattern: String): String =
    Regex(pattern).find(file("../app/build.gradle.kts").readText())!!.groupValues[1]
