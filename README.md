# libmpv for Android

[mpv](https://mpv.io), FFmpeg and libplacebo built for the four Android ABIs (`arm64-v8a`,
`armeabi-v7a`, `x86`, `x86_64`) and published to Maven Central as one AAR,
`io.github.midasvo:libmpv`, with a small Kotlin wrapper, `dev.jdtech.mpv.MPVLib`. It is the player
engine of [Ephyra](https://github.com/midasvo/ephyra). Android 8.0 (API 26) or newer.

This is a fork of [jarnedemeulemeester/libmpv-android](https://github.com/jarnedemeulemeester/libmpv-android),
which is based on [mpv-android](https://github.com/mpv-android/mpv-android). It publishes its own
releases, so a libmpv update does not have to wait for an upstream release
([midasvo/ephyra#654](https://github.com/midasvo/ephyra/issues/654)). It has its own JNI wrapper and
its own build pipeline, and still takes upstream's changes through a weekly merge, resolved by hand
where the two differ ([RELEASING.md](RELEASING.md)).
Report problems with these builds [here](https://github.com/midasvo/libmpv-android/issues), not
upstream.

Landing page: <https://midasvo.nl/libmpv-android/>

## Use it

```kotlin
dependencies {
    implementation("io.github.midasvo:libmpv:<version>")
}
```

The latest version is on [Maven Central](https://central.sonatype.com/artifact/io.github.midasvo/libmpv)
and on the [releases page](https://github.com/midasvo/libmpv-android/releases). Maven Central needs
no extra repository.

```kotlin
import dev.jdtech.mpv.MPVLib

val mpv = MPVLib.create(context) ?: return
mpv.setOptionString("gpu-context", "android")
mpv.setOptionString("hwdec", "mediacodec")
mpv.init()

// once the Surface exists
mpv.attachSurface(holder.surface)
mpv.setOptionString("force-window", "yes")
mpv.setOptionString("vo", "gpu")
mpv.command(arrayOf("loadfile", url))

// when the player goes away
mpv.destroy()
```

`MPVLib` creates an mpv instance, attaches it to a `Surface`, sets options and properties, runs
commands, and passes mpv's events and log messages to observers (`addObserver`,
`addLogObserver`). Observer callbacks run on mpv's event thread. Everything else is mpv itself:
its [manual](https://mpv.io/manual/stable/) is the reference for options, properties and commands.

The Kotlin package is `dev.jdtech.mpv`, as upstream's ([ADR 0001](docs/adr/0001-keep-the-kotlin-package.md)).
Coming from `dev.jdtech.mpv:libmpv`, change the coordinate and nothing else. Do not depend on both:
they ship the same native libraries under the same file names.

<!-- API: the new API on main (mpv_node values, suspend functions, Flows, stream protocols) is described here once it is final. -->

## Released and unreleased

The latest release is **1.3.0**. Its AAR holds the C++ JNI wrapper inherited from upstream, and it
was built by the old single-job pipeline.

`main` has the following, which will be in the next release:

- **A JNI wrapper in Rust** ([ADR 0003](docs/adr/0003-rust-jni-wrapper.md)). The Kotlin API is
  unchanged. A call after `destroy()` throws `IllegalStateException` instead of touching freed
  memory, a call racing `destroy()` on another thread is safe, and a second `destroy()` does
  nothing. `destroy()` from an observer or log callback throws `IllegalStateException` (call it
  from another thread). A `null` string argument throws `NullPointerException`.
- **An mpv patch** that rounds the AudioTrack buffer to whole frames, so 22,050 Hz audio is no
  longer silent ([ephyra#659](https://github.com/midasvo/ephyra/issues/659)).
- **More checks in CI:** the dynamic dependencies of every library in every ABI, and instrumented
  tests that play a real file and check which decoders, demuxers and protocols the build contains.
- **A new build pipeline** ([ADR 0004](docs/adr/0004-build-pipeline.md)). Releases built by it also
  come with an SBOM and a provenance attestation.

## What is in the AAR

For each ABI: `libmpv.so` and the FFmpeg libraries as shared libraries (the other components are
statically linked into them), the wrapper `libplayer.so`, and the NDK's `libc++_shared.so`. The
components are listed under [Licensing](#licensing).

Versions are not repeated here, so they cannot go stale:

- **On `main`:** [`buildscripts/include/depinfo.sh`](buildscripts/include/depinfo.sh) is the one
  place every version is set. Patches to the sources are in [`buildscripts/patches/`](buildscripts/patches/).
- **In a release:** the release notes list every component version. Releases built by the new
  pipeline also list the tools they were built with and attach an SBOM, `libmpv-<version>.cdx.json`
  (CycloneDX).
- **On the landing page:** both, generated from the same files when the page is deployed.

## Building from source

The build runs on Linux and needs a JDK and [rustup](https://rustup.rs); `download.sh` installs the
rest.

```sh
cd buildscripts
./download.sh   # host tools, Android SDK and NDK, sources
./patch.sh
./build.sh      # native libraries, wrapper and AAR for all four ABIs
```

The AAR lands in `libmpv/build/outputs/aar/libmpv-release.aar`. [buildscripts/README.md](buildscripts/README.md)
covers single ABIs, rebuilding one component, Android Studio, the AAR check and how CI runs the
same scripts.

## Verifying a release

Each release attaches the AAR (the same file as on Maven Central), its signature
(`libmpv-<version>.aar.asc`) and the complete corresponding source. Check the signature with the
release signing key, fingerprint `DD02 C554 A0EB AF24 B72B 68B0 D84A 55E8 003C 6AB5`:

```sh
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys DD02C554A0EBAF24B72B68B0D84A55E8003C6AB5
gpg --verify libmpv-<version>.aar.asc libmpv-<version>.aar
```

Releases after 1.3.0, built by the new pipeline, also have a provenance attestation for the AAR
and the source tarball, which shows that they were built by this repository's `publish.yaml`, and
from which commit:

```sh
gh attestation verify libmpv-<version>.aar -R midasvo/libmpv-android
```

[RELEASING.md](RELEASING.md#verifying-a-release) has the details.

## Licensing

The source code in this repository is licensed under the [MIT license](LICENSE).

The AAR is effectively **GPL-3.0-or-later**: `libmpv.so` links FFmpeg and other GPL components. An
app that ships it must comply with that licence: show the licences, and offer the source. Each
release attaches its complete corresponding source as `libmpv-<version>-sources.tar.xz`.

The components and their licences (SPDX identifiers where there is one):

<!-- licence-table -->
| Component | Licence |
| --- | --- |
| mpv | `GPL-2.0-or-later` |
| FFmpeg | `GPL-3.0-or-later` |
| libplacebo | `LGPL-2.1-or-later` |
| libass | `ISC` |
| lua | `MIT` |
| curl | `curl` |
| mbedtls | `Apache-2.0 OR GPL-2.0-or-later` |
| freetype | `FTL OR GPL-2.0-or-later` |
| fontconfig | `fontconfig license (MIT-style)` |
| fribidi | `LGPL-2.1-or-later` |
| harfbuzz | `MIT-Modern-Variant` |
| libunibreak | `Zlib` |
| dav1d | `BSD-2-Clause` |
| libxml2 | `MIT` |
| libc++_shared (from the NDK) | `Apache-2.0 WITH LLVM-exception` |
| libplayer (the wrapper, this repository) | `MIT` |
<!-- /licence-table -->

The licences come from [`buildscripts/sbom.sh`](buildscripts/sbom.sh), which also writes them into
each release's SBOM; `buildscripts/check-docs.sh` fails when this table and `sbom.sh` disagree.
HarfBuzz calls its licence "Old MIT", which SPDX names `MIT-Modern-Variant`. Mbed TLS is offered
under Apache-2.0 or GPL-2.0-or-later, at the user's choice. The wrapper's Rust dependencies are
listed in the SBOM and vendored in the source tarball of releases built by the new pipeline.
