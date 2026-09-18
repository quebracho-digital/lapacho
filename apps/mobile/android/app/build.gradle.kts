plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "digital.quebracho.lapacho.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "digital.quebracho.lapacho"
        minSdk = 26
        targetSdk = 35
        versionCode = 7
        versionName = "0.1.6-latino"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation(project(":storage"))
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    testImplementation("junit:junit:4.13.2")
}
