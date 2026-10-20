plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}
android {
    namespace = "io.ytmlisten.player"
    compileSdk = 36
    defaultConfig { minSdk = 26; consumerProguardFiles("consumer-rules.pro") }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_1_8; targetCompatibility = JavaVersion.VERSION_1_8 }
    kotlinOptions { jvmTarget = "1.8" }
}
dependencies {
    implementation("com.google.android.gms:play-services-auth:22.0.0")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("com.fasterxml.jackson.core:jackson-databind:2.15.3")
    implementation(project(":tauri-android"))
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}
