// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import groovy.json.JsonSlurper

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

// Firebase reads its settings from string resources. The usual way to make
// them is the google-services plugin applied to the app module; here they
// are made from the same file, in this library, so that the app module
// stays as it is and a build without this plugin has no trace of Firebase.
//
// No file: the library builds, and the app reports that pushes are not set up.
fun firebaseSettings(): Map<String, String> {
    val file = rootProject.file("app/google-services.json")
    if (!file.exists()) {
        logger.lifecycle("veydan-push: no app/google-services.json, pushes will be unavailable")
        return emptyMap()
    }
    val appId = (rootProject.findProject(":app")?.extensions?.findByName("android")
        as? com.android.build.api.dsl.ApplicationExtension)?.defaultConfig?.applicationId

    @Suppress("UNCHECKED_CAST")
    val json = JsonSlurper().parse(file) as Map<String, Any?>
    val project = json["project_info"] as? Map<String, Any?> ?: emptyMap()
    val clients = json["client"] as? List<Map<String, Any?>> ?: emptyList()
    fun packageOf(client: Map<String, Any?>): String? {
        val info = client["client_info"] as? Map<String, Any?>
        val android = info?.get("android_client_info") as? Map<String, Any?>
        return android?.get("package_name") as? String
    }
    val client = clients.firstOrNull { appId != null && packageOf(it) == appId }
        ?: clients.singleOrNull()
        ?: throw GradleException(
            "veydan-push: app/google-services.json has no client for package $appId"
        )
    val info = client["client_info"] as? Map<String, Any?> ?: emptyMap()
    val keys = client["api_key"] as? List<Map<String, Any?>> ?: emptyList()

    val out = mutableMapOf<String, String>()
    fun put(name: String, value: Any?) {
        val text = value as? String
        if (!text.isNullOrEmpty()) out[name] = text
    }
    put("google_app_id", info["mobilesdk_app_id"])
    put("gcm_defaultSenderId", project["project_number"])
    put("project_id", project["project_id"])
    put("google_storage_bucket", project["storage_bucket"])
    put("google_api_key", keys.firstOrNull()?.get("current_key"))
    if (!out.containsKey("google_app_id") || !out.containsKey("google_api_key")) {
        throw GradleException("veydan-push: app/google-services.json lacks the app id or the api key")
    }
    return out
}

android {
    namespace = "net.veydan.push"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
        for ((name, value) in firebaseSettings()) {
            resValue("string", name, value)
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // The last line of releases built with Kotlin 2.0; the project compiles
    // with Kotlin 1.9, which reads metadata up to 2.0 and no further.
    implementation("com.google.firebase:firebase-messaging:24.1.2")
    // Named for the check "are Google services on this phone".
    implementation("com.google.android.gms:play-services-base:18.5.0")
    implementation(project(":tauri-android"))
}
