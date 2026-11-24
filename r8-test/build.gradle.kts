// An app that uses the library the way a release build of an app does: minified and
// optimised by R8, with the library's consumer rules (libmpv/proguard-rules.pro) and
// nothing else keeping its classes. Its instrumented test (connectedReleaseAndroidTest) runs
// every path on which libplayer.so calls into Kotlin by name; a class or member those rules
// fail to keep makes it fail. Not published, and not part of the AAR.
plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "dev.jdtech.mpv.r8test"
    compileSdk = 37
    buildToolsVersion = "37.0.0"

    defaultConfig {
        applicationId = "dev.jdtech.mpv.r8test"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "1"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    // Instrumented against the release build type: AGP runs R8 on a debuggable build without
    // renaming anything, which would hide a missing keep rule.
    testBuildType = "release"
    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            testProguardFiles("test-proguard-rules.pro")
            // the test APK must be signed with the app's key; the debug key is enough here
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":libmpv"))

    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
}
