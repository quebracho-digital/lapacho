// P0 spike root build: no Rust/uniffi bridge yet (that's P1). Plain Kotlin +
// Android Keystore + SQLite, per docs/ARQUITECTURA_MOBILE_ANDROID.md §9.
plugins {
    id("com.android.application") version "8.7.2" apply false
    id("com.android.library") version "8.7.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
}
