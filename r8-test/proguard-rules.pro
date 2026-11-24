# Only the test's entry point. Everything of the library is kept by its consumer rules
# (libmpv/proguard-rules.pro), and only by them: that is what this module tests.
-keep class dev.jdtech.mpv.r8test.Probe { public *; }

# What the test APK needs from the libraries it shares with the app: it does not bring its
# own copy of them, so R8 must not remove from the app what only the test uses (androidx.test
# uses the Kotlin standard library). Not the library under test, which this leaves alone.
-keep class kotlin.** { *; }
