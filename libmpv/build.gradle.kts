import com.vanniktech.maven.publish.AndroidSingleVariantLibrary
import com.vanniktech.maven.publish.JavadocJar
import com.vanniktech.maven.publish.SourcesJar

plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.maven.publish)
}

android {
    namespace = "dev.jdtech.mpv"
    compileSdk = 37
    buildToolsVersion = "37.0.0"

    defaultConfig {
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("proguard-rules.pro")
    }

    // Gradle builds no native code. The build scripts (buildscripts/scripts/player.sh) put
    // every .so of every ABI, already stripped, into src/main/jniLibs/<abi>, and Gradle
    // packages them byte for byte: no NDK is needed here, and the AAR does not depend on
    // which NDK the machine running Gradle happens to have.
    packaging {
        jniLibs {
            keepDebugSymbols += "**/*.so"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // The host unit tests (src/test) cover the Kotlin code that needs neither JNI nor mpv;
    // android.util.Log, which it logs with, returns defaults there instead of throwing.
    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

dependencies {
    // api: Flow and suspend functions are in the public API (MpvAsync.kt, MpvFlows.kt)
    api(libs.kotlinx.coroutines.core)

    testImplementation(libs.junit)

    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
    // Dispatchers.Main, for the tests that call the suspend functions on the main thread
    androidTestImplementation(libs.kotlinx.coroutines.android)
}

mavenPublishing {
    publishToMavenCentral(automaticRelease = true)
    signAllPublications()

    configure(
        platform = AndroidSingleVariantLibrary(
            javadocJar = JavadocJar.Empty(),
            sourcesJar = SourcesJar.Sources(),
            variant = "release",
        )
    )

    // The version comes from the tag: publish.yaml passes -PVERSION_NAME=<tag without the v>.
    coordinates(
        groupId = "io.github.midasvo",
        artifactId = "libmpv",
        version = providers.gradleProperty("VERSION_NAME").getOrElse("0.0.0-SNAPSHOT")
    )

    pom {
        name = "libmpv-android"
        description = "libmpv for Android (midasvo fork of jarnedemeulemeester/libmpv-android)"
        inceptionYear = "2023"
        url = "https://github.com/midasvo/libmpv-android"
        licenses {
            license {
                name = "GNU General Public License v3.0 or later"
                url = "https://www.gnu.org/licenses/gpl-3.0.txt"
            }
        }
        developers {
            developer {
                id = "midasvo"
                name = "Midas van Oene"
                url = "https://github.com/midasvo"
            }
            developer {
                id = "jarnedemeulemeester"
                name = "Jarne Demeulemeester"
                email = "jarnedemeulemeester@gmail.com"
                roles.add("original author")
            }
        }
        scm {
            url = "https://github.com/midasvo/libmpv-android"
            connection = "scm:git:https://github.com/midasvo/libmpv-android.git"
            developerConnection = "scm:git:ssh://git@github.com/midasvo/libmpv-android.git"
        }
        issueManagement {
            system = "GitHub"
            url = "https://github.com/midasvo/libmpv-android/issues"
        }
    }
}
