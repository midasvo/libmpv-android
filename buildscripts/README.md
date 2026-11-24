# Building

The build runs on Linux (CI uses Ubuntu 24.04). It has three parts:

1. **Native libraries**: libmpv, FFmpeg and their dependencies, one prefix per ABI (`prefix/<abi>`).
2. **The JNI wrapper**: the Rust crate in `libmpv/src/main/rust`, built into `libplayer.so` by `scripts/player.sh`, which also puts every library the AAR ships for that ABI into `libmpv/src/main/jniLibs/<abi>`: the shared libraries from the prefix, `libplayer.so` and the NDK's `libc++_shared.so`, all stripped.
3. **The AAR**: Gradle packages `jniLibs` and the Kotlin code. It builds no native code and needs no NDK.

## Requirements

- A JDK (the CI uses Temurin 25).
- [rustup](https://rustup.rs). The Rust version and the four Android targets are pinned in `libmpv/src/main/rust/rust-toolchain.toml`; rustup installs them the first time cargo runs there (or run `rustup toolchain install` in that directory). No cargo-ndk is needed: `player.sh` points cargo at the NDK's clang.
- The host tools (autoconf, libtool, ninja, nasm, gperf, pkg-config and meson, pinned as `v_meson` in `include/depinfo.sh`). `download.sh` installs them on Debian/Ubuntu and RHEL/Fedora.

## Download dependencies

`download.sh` installs the host tools (`include/install-system-deps.sh`), the Android SDK packages and the NDK (`include/download-sdk.sh`), and downloads the sources (`include/download-deps.sh`).

```shell
./download.sh
```

If you already have the Android SDK installed you can symlink `sdk/android-sdk-linux` to your SDK root
before running the script, it will still install the necessary SDK packages.

A matching NDK version inside the SDK will be picked up automatically or downloaded/installed otherwise.

The dependencies fetched as release tarballs (lua, curl, libunibreak) are verified against SHA-256 checksums kept next to their versions in `include/depinfo.sh` (`sha256_*`); when you bump one of those versions, update its checksum too. `gas-preprocessor` is pinned to a commit (`v_gas_preprocessor`) in the same file. The Rust crates are pinned, with checksums, by `libmpv/src/main/rust/Cargo.lock`, and the build uses `--locked`.

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

This builds everything for all four ABIs (`armeabi-v7a` `arm64-v8a` `x86` `x86_64`): the native libraries, then the wrapper, then the AAR (`libmpv/build/outputs/aar/libmpv-release.aar`). Run `build.sh` with `--clean` to clean the build directories before building.

To build a single ABI, pass `--arch`. Gradle packages whatever is in `jniLibs`, so an AAR built this way only contains the ABIs you built (no `abiFilters` needed):

```shell
./build.sh --arch x86_64
```

The targets are the components in the dependency tree at the bottom of `include/depinfo.sh` (`mpv-android` → `player` → `mpv` → ...). CI builds them in separate jobs, and you can do the same:

```shell
./build.sh --arch arm64 mpv         # native libraries only, into prefix/arm64-v8a
./build.sh -n --arch arm64 player   # the wrapper, from what is already in prefix/arm64-v8a
./build.sh -n                       # the wrapper for every ABI, then the AAR
```

`-n` skips the dependencies of the target. For the default target (`mpv-android`) it still rebuilds the wrapper for each ABI, so `./build.sh -n` is what you run after changing the wrapper or the Kotlin code.

## Generating the SBOM

```shell
./sbom.sh <version> > libmpv-<version>.cdx.json
```

writes a CycloneDX SBOM of the bundled native components: every library in `depinfo.sh` with version, licence and source, the NDK's libc++, the wrapper (licence from its `Cargo.toml`) and the crates in its `Cargo.lock`. Licences and source locations live in the table at the top of `sbom.sh`; when you add a dependency to `depinfo.sh`, add it there too (the script fails on a `v_*` entry it does not know). CI runs it on every build and attaches the result to each release.

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

Afterwards, rebuild the wrapper (which also copies the new libraries into `jniLibs`) and the AAR:

```shell
./build.sh -n
```

## Using Android Studio

You can use Android Studio to develop the Kotlin part of the codebase. Before using it, build the project at least once by following the steps in the **Build** section, so that `libmpv/src/main/jniLibs` is filled.

You should point Android Studio to existing SDK installation at `libmpv-android/buildscripts/sdk/android-sdk-linux`. Then click "Open an existing Android Studio project" and select the repository root (`libmpv-android`). The library module is `libmpv`.

Android Studio (Gradle) only builds the Kotlin part and packages the libraries in `jniLibs`. If you change any native code, the libraries (ffmpeg, mpv, ...) or the wrapper (`libmpv/src/main/rust`), first rebuild with:

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

It fails with a list of every problem if an ABI is missing one of the expected libraries (`libmpv.so`, `libplayer.so`, the FFmpeg libraries, `libc++_shared.so`) or has an empty one, if any `.so` of any ABI has a `DT_NEEDED` entry that is neither another library in the same ABI directory nor one of the Android system libraries on the script's allowlist (`system_libs`, limited to what the libraries need today and all present at API 26), or if a 64-bit `.so` has `LOAD` segments aligned below 16 KiB (needed for 16 KiB page size devices). The dependency check covers every ABI because CI only loads x86_64 on the emulator; a new system dependency fails it until someone confirms the library exists on API 26 devices and adds it to the allowlist. On success it prints the file count and size per ABI. It needs `unzip` and `readelf` (or `llvm-readelf`).

## CI

`.github/workflows/build.yaml` runs the same scripts, split into jobs: one per ABI for the native libraries (`build.sh --arch <a> mpv`, in parallel), then one that builds the wrapper and the AAR (`build.sh -n --arch <a> player`, Gradle), the instrumented tests, and a job that packs the corresponding source. Each ABI's prefix is cached, keyed on `include/`, `build.sh`, `download.sh`, `patch.sh`, `scripts/` (except `player.sh` and `mpv-android.sh`) and `patches/`, so a change to anything else (the wrapper, Kotlin, Gradle, the workflows) skips the native build. Release builds never use the cache. To throw every cached prefix away, bump `NATIVE_CACHE_EPOCH` in `build.yaml`.

`include/hash-sources.sh` hashes the downloaded and patched sources; CI uses it to check that the source tarball of a release holds exactly what each ABI was built from.

# Credits, notes, etc

These build scripts were created by @sfan5 and modified by @jarnedemeulemeester.
