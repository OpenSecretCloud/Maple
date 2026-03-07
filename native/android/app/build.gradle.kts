import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

fun String.toKotlinStringLiteral(): String =
    "\"${replace("\\", "\\\\").replace("\"", "\\\"")}\""

val keystoreProperties = Properties()
val keystorePropertiesFile = rootProject.file("keystore.properties")

if (keystorePropertiesFile.exists()) {
    keystorePropertiesFile.inputStream().use(keystoreProperties::load)
}

val openSecretApiUrl = providers.gradleProperty("openSecretApiUrl")
    .orElse(providers.environmentVariable("OPEN_SECRET_API_URL"))
    .getOrElse("")

val mapleVersionName = providers.gradleProperty("mapleVersionName")
    .orElse(providers.environmentVariable("MAPLE_ANDROID_VERSION_NAME"))
    .orNull
    ?: "3.0.0"

val mapleVersionCode = providers.gradleProperty("mapleVersionCode")
    .orElse(providers.environmentVariable("MAPLE_ANDROID_VERSION_CODE"))
    .orNull
    ?.toInt()
    ?: 300000000

android {
    namespace = "cloud.opensecret.maple"
    compileSdk = 35
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "cloud.opensecret.maple"
        minSdk = 26
        targetSdk = 35
        versionCode = mapleVersionCode
        versionName = mapleVersionName
        buildConfigField("String", "OPEN_SECRET_API_URL", openSecretApiUrl.toKotlinStringLiteral())
    }

    signingConfigs {
        if (keystorePropertiesFile.exists()) {
            create("release") {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("password")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("password")
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".dev"
            versionNameSuffix = "-dev"
        }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
            )
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        resources.excludes.addAll(
            listOf("/META-INF/{AL2.0,LGPL2.1}", "META-INF/DEPENDENCIES"),
        )
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

tasks.register("ensureUniffiGenerated") {
    doLast {
        val out = file("src/main/java/cloud/opensecret/maple/rust/maple_core.kt")
        if (!out.exists()) {
            throw GradleException("Missing UniFFI Kotlin bindings. Run `rmp bindings kotlin` first.")
        }
    }
}

tasks.named("preBuild") {
    dependsOn("ensureUniffiGenerated")
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.06.00")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.3")

    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")

    debugImplementation("androidx.compose.ui:ui-tooling")

    // UniFFI JNA
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    // Secure credential storage
    implementation("androidx.security:security-crypto:1.1.0-alpha06")
}
