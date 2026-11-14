# Building

## Download dependencies

`download.sh` will take care of installing the Android SDK, NDK and downloading the sources.

If you're running on Debian/Ubuntu or RHEL/Fedora it will also install the necessary dependencies for you.

```shell
./download.sh
```

If you already have the Android SDK installed you can symlink `android-sdk-linux` to your SDK root
before running the script, it will still install the necessary SDK packages.

A matching NDK version inside the SDK will be picked up automatically or downloaded/installed otherwise.

The dependencies fetched as release tarballs (lua, curl, libunibreak) are verified against SHA-256 checksums kept next to their versions in `include/depinfo.sh` (`sha256_*`); when you bump one of those versions, update its checksum too. `gas-preprocessor` is pinned to a commit (`v_gas_preprocessor`) in the same file.

## Patching

```shell
./patch.sh
```

Run `patch.sh` to apply the custom patches to the dependencies. These patches are located in the `patches` folder.

**Note** running `patch.sh` resets the dependencies to a clean state.

## Build

```shell
./build.sh
```

Run `build.sh` with `--clean` to clean the build directories before building.

This builds for all architectures (`armeabi-v7a` `arm64-v8a` `x86` `x86_64`) by default.

If you want to build only for a specific arch, build the native part like this:
```shell
./build.sh --arch arm64 mpv
./build.sh --arch x86_64 mpv
```

You also need to tell gradle to only build said architectures. See [Specify ABIs](https://developer.android.com/studio/projects/gradle-external-native-builds#specify-abi) for more information
```kotlin
android {
  ...
  defaultConfig {
    ...
    ndk {
      abiFilters += listOf("x86_64", "arm64-v8a")
    }
  }
}
```

Finally you can build the AAR:
```shell
./build.sh -n
```

# Developing

## Getting logs

```shell
adb logcat # get all logs, useful when drivers/vendor libs output to logcat
adb logcat -s "mpv" # get only mpv logs
```

## Rebuilding a single component

If you've made changes to a single component (e.g. ffmpeg or mpv) and want a new build you can of course just run ./build.sh but it's also possible to just build a single component like this:

```shell
./build.sh -n ffmpeg
# optional: add --clean to build from a clean state
```

Note this will build the component for all architectures, specify `--arch` to build for a single arch.

Afterwards, build mpv-android:

```shell
./build.sh -n
```

## Using Android Studio

You can use Android Studio to develop the Kotlin part of the codebase. Before using it, make sure to build the project at least once by following the steps in the **Build** section.

You should point Android Studio to existing SDK installation at `libmpv-android/buildscripts/sdk/android-sdk-linux`. Then click "Open an existing Android Studio project" and select the repository root (`libmpv-android`). The library module is `libmpv`.

If Android Studio complains about project sync failing to find the NDK, go to "File -> Project Structure -> SDK Location" and set "Android NDK Location" to `libmpv-android/buildscripts/sdk/android-sdk-linux/ndk/<version>` (the version is `v_ndk` in `buildscripts/include/depinfo.sh`).

Note that if you build from Android Studio only the Kotlin part and the JNI wrapper are built. If you make any changes to libraries (ffmpeg, mpv, ...) or the native wrapper code (`libmpv/src/main/cpp/*`), first rebuild native code with:

```shell
./build.sh -n
```

then build the project from Android Studio.

Also, debugging native code does not work from within the studio at the moment.

## Verifying the AAR

After building the AAR, `check-aar.sh` checks it:

```shell
./check-aar.sh [path-to-aar]   # default: libmpv/build/outputs/aar/libmpv-release.aar
```

It fails with a list of every problem if an ABI is missing one of the expected libraries (`libmpv.so`, `libplayer.so`, the FFmpeg libraries, `libc++_shared.so`) or has an empty one, or if a 64-bit `.so` has `LOAD` segments aligned below 16 KiB (needed for 16 KiB page size devices). On success it prints the file count and size per ABI. It needs `unzip` and `readelf` (or `llvm-readelf`).

# Credits, notes, etc

These build scripts were created by @sfan5 and modified by @jarnedemeulemeester.

