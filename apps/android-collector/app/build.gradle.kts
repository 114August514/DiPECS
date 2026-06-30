import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.dipecs.collector"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.dipecs.collector"
        minSdk = 26
        targetSdk = 35
        versionCode = 3
        versionName = "0.3.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // ── Signing configs ──────────────────────────────────────────
    // 1. release     — debug keystore (default, for normal app)
    // 2. platform    — Android platform certificate (for system app)
    //
    // To enable platform signing, create
    //   apps/android-collector/signing/platform.properties
    // with:
    //   platform.storeFile=/path/to/platform.keystore
    //   platform.storePassword=android
    //   platform.keyAlias=platform
    //   platform.keyPassword=android
    //   platform.certificateFile=/path/to/platform.x509.pem
    //
    // The platform.keystore is generated from AOSP build/target/product/security/platform.pk8 + platform.x509.pem.
    signingConfigs {
        create("release") {
            storeFile = file(
                System.getenv("ANDROID_KEYSTORE_PATH")
                    ?: "${System.getProperty("user.home")}/.android/debug.keystore",
            )
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD") ?: "android"
            keyAlias = System.getenv("ANDROID_KEY_ALIAS") ?: "androiddebugkey"
            keyPassword = System.getenv("ANDROID_KEY_PASSWORD") ?: "android"
        }

        val platformPropsFile = rootProject.file("signing/platform.properties")
        if (findProperty("DIPECS_PLATFORM_SIGNING") == "true" || platformPropsFile.exists()) {
            register("platform") {
                val props = Properties()
                if (platformPropsFile.exists()) {
                    platformPropsFile.inputStream().use { props.load(it) }
                }
                storeFile = file(
                    props.getProperty("platform.storeFile")
                        ?: error("platform.storeFile must be set in signing/platform.properties"),
                )
                storePassword = props.getProperty("platform.storePassword", "android")
                keyAlias = props.getProperty("platform.keyAlias", "platform")
                keyPassword = props.getProperty("platform.keyPassword", "android")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
        }
        // Use `-PDIPECS_PLATFORM_SIGNING=true` to build with platform certificate.
        // ./gradlew assemblePlatform -PDIPECS_PLATFORM_SIGNING=true
        if (findProperty("DIPECS_PLATFORM_SIGNING") == "true" && signingConfigs.names.contains("platform")) {
            register("platform") {
                initWith(buildTypes.getByName("release"))
                signingConfig = signingConfigs.getByName("platform")
                isMinifyEnabled = false
            }
        }
    }

    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("androidx.security:security-crypto:1.1.0-alpha06")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")

    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("org.json:json:20240303")
}
