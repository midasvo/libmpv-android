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
    ndkVersion = "30.0.16248370"

    defaultConfig {
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("proguard-rules.pro")
        externalNativeBuild {
            cmake {
                arguments += listOf(
                    "-DANDROID_STL=c++_shared",
                )
                cFlags += "-Werror"
                cppFlags += "-std=c++11"
            }
        }
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "4.1.2"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
}

mavenPublishing {
    publishToMavenCentral(automaticRelease = true)
    signAllPublications()

    configure(
        platform = AndroidSingleVariantLibrary(
            javadocJar = JavadocJar.Empty(),
            sourcesJar = SourcesJar.Empty(),
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
