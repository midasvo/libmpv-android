# The test APK. AGP runs R8 on it too when the app under test is minified, to point its
# references at the app's renamed classes. Nothing in it needs shrinking or renaming.
-dontshrink
-dontoptimize
-dontobfuscate
# compile-time annotations androidx.test refers to
-dontwarn com.google.errorprone.annotations.**
