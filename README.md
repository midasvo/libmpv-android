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

<!-- example-unchecked: a Gradle build script -->
```kotlin
dependencies {
    implementation("io.github.midasvo:libmpv:<version>")
}
```

The latest version is on [Maven Central](https://central.sonatype.com/artifact/io.github.midasvo/libmpv)
and on the [releases page](https://github.com/midasvo/libmpv-android/releases). Maven Central needs
no extra repository. Since 1.4.0 the library also brings
`kotlinx-coroutines-core` with it (see [More of mpv's API](#more-of-mpvs-api-since-140)).

<!-- example-unchecked: the 1.3.0 API, which has no region in DocExamples.kt -->
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
`addLogObserver`). Observer callbacks run on mpv's event thread (named `mpv-event` in traces,
since 1.4.0). The setters (`setPropertyInt`, `setPropertyString`, ...) log an mpv error rather than
report it; see [Errors](#more-of-mpvs-api-since-140) for the calls that throw it. Everything else is mpv itself:
its [manual](https://mpv.io/manual/stable/) is the reference for options, properties and commands.

The Kotlin package is `dev.jdtech.mpv`, as upstream's ([ADR 0001](docs/adr/0001-keep-the-kotlin-package.md)).
Coming from `dev.jdtech.mpv:libmpv`, change the coordinate and nothing else. Do not depend on both:
they ship the same native libraries under the same file names.

## More of mpv's API (since 1.4.0)

1.4.0 adds structured values, suspend functions, flows, custom protocols, `AutoCloseable` and a
logcat level, as extension functions on `MPVLib` in the same package ([ADR 0005](docs/adr/0005-api-on-top-of-mpvlib.md)).
Nothing above changes: the 1.3.0 API is the same, and its observers get what they got before.
The new functions report mpv's errors as `MpvException` (with mpv's code in `error`), where the
classic ones return a code or `null`. All of them throw `IllegalStateException` after `destroy()`.

With this API the library depends on `kotlinx-coroutines-core` (an `api` dependency; the version
is in [`gradle/libs.versions.toml`](gradle/libs.versions.toml)). Your app gets it with the
library.

**Structured values.** `MpvNode` is mpv's `mpv_node`: maps, arrays and typed scalars instead of
JSON text. Its accessors return `null` for a node of another kind, so a lookup reads like a path.

<!-- example:structured-values -->
```kotlin
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.commandNode
import dev.jdtech.mpv.getPropertyNode
import dev.jdtech.mpv.setPropertyNode

val tracks = mpv.getPropertyNode("track-list").list.orEmpty()
val audio = tracks.filter { it["type"]?.string == "audio" }
audio.firstOrNull()?.get("id")?.let { mpv.setPropertyNode("aid", it) }

val result = mpv.commandNode(MpvNode.command("loadfile", url, "append"))
val entryId = result["playlist_entry_id"]?.long
```

Use these instead of `getPropertyString` on a structured property (`track-list`,
`chapter-list`, `metadata`), and instead of `command` when you need a command's result, typed
arguments, or its error. They block like the classic calls.

**Errors.** The classic setters (`setPropertyInt`, `setPropertyDouble`, `setPropertyBoolean`,
`setPropertyString`) log an mpv error and return nothing. `setPropertyNode` and
`setPropertyAsync` set the same properties and throw the error as `MpvException`, with mpv's code
in `error`:

<!-- example:errors -->
```kotlin
import dev.jdtech.mpv.MpvException
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.setPropertyNode

try {
    mpv.setPropertyNode("speed", MpvNode.of(1.5))
} catch (e: MpvException) {
    showError(e.error) // an MPVLib.MpvError code, e.g. MPV_ERROR_PROPERTY_FORMAT
}
```

**Suspend functions.** `commandAsync`, `getPropertyAsync` and `setPropertyAsync` use mpv's own
async API: the calling thread is never blocked, so they are safe on the main thread. Cancelling
the coroutine stops waiting (and aborts commands that support it); if the instance is destroyed
while one is pending, it throws `IllegalStateException`.

<!-- example:suspend-functions -->
```kotlin
import dev.jdtech.mpv.MpvNode
import dev.jdtech.mpv.commandAsync
import dev.jdtech.mpv.getPropertyAsync
import dev.jdtech.mpv.setPropertyAsync
import kotlinx.coroutines.launch

scope.launch {
    mpv.commandAsync(MpvNode.command("loadfile", url))
    mpv.setPropertyAsync("pause", MpvNode.of(false))
    val volume = mpv.getPropertyAsync("volume").double
}
```

`getBooleanPropertyAsync`, `getLongPropertyAsync`, `getDoublePropertyAsync` and
`getStringPropertyAsync` read a property in that type, without unwrapping an `MpvNode`. mpv
converts the value as for the property flows (so `getLongPropertyAsync` gives a time in whole
seconds); a property that cannot be read in that type throws `MpvException`
(`MPV_ERROR_PROPERTY_FORMAT`).

<!-- example:typed-async -->
```kotlin
import dev.jdtech.mpv.getBooleanPropertyAsync
import dev.jdtech.mpv.getDoublePropertyAsync
import dev.jdtech.mpv.getStringPropertyAsync
import kotlinx.coroutines.launch

scope.launch {
    val paused: Boolean = mpv.getBooleanPropertyAsync("pause")
    val position: Double = mpv.getDoublePropertyAsync("time-pos")
    val title: String = mpv.getStringPropertyAsync("media-title")
}
```

Prefer them on the main thread and wherever mpv may be busy (opening a file); the blocking calls
are fine on a background thread and inside the observer callbacks.

**Flows.** `booleanPropertyFlow`, `longPropertyFlow`, `doublePropertyFlow`, `stringPropertyFlow`
and `nodePropertyFlow` emit a property's value (first the current one, `null` or `MpvNode.None`
while it is unavailable) and each change; `eventFlow()` emits mpv's events with their data as
`MpvPlayerEvent`. They are cold: observation starts when collection starts and ends when it
ends. A property flow is conflated (a slow collector gets the latest value). The event flow
buffers 256 events per collector; beyond that it drops the newest and emits one
`MpvPlayerEvent.Dropped(count)`. When the instance is destroyed, the flows complete.

<!-- example:flows -->
```kotlin
import dev.jdtech.mpv.MPVLib
import dev.jdtech.mpv.MpvPlayerEvent
import dev.jdtech.mpv.doublePropertyFlow
import dev.jdtech.mpv.eventFlow
import kotlinx.coroutines.launch

scope.launch {
    mpv.doublePropertyFlow("time-pos").collect { position -> showPosition(position) }
}
scope.launch {
    mpv.eventFlow().collect { event ->
        when (event) {
            is MpvPlayerEvent.EndFile ->
                if (event.reason == MPVLib.MpvEndFileReason.MPV_END_FILE_REASON_ERROR) showError(event.error)
            is MpvPlayerEvent.Dropped -> reloadState()
            else -> {}
        }
    }
}
```

Prefer flows in coroutine code, where collection follows a lifecycle (a `ViewModel` scope, say)
and stops observing on its own. `observeProperty` and the `EventObserver` callbacks remain for
callback code and Java; `unobserveProperty(name)` now undoes `observeProperty`.

**Custom protocols.** `registerStreamProtocol(protocol, provider)` makes mpv open
`protocol://...` URIs through your Kotlin code: the `MpvStreamProvider` opens an `MpvStream`, which
implements `read` and `close` (and `seek`, `size` and `cancel` where it can).

<!-- example:custom-protocols -->
```kotlin
import android.net.Uri
import dev.jdtech.mpv.MpvStream
import dev.jdtech.mpv.registerStreamProtocol
import java.io.FileNotFoundException

// plays content:// URIs as "local://content://..." through the app's ContentResolver
mpv.registerStreamProtocol("local") { uri ->
    val input = contentResolver.openInputStream(Uri.parse(uri.removePrefix("local://")))
        ?: throw FileNotFoundException(uri)
    object : MpvStream {
        override fun read(buffer: ByteArray, offset: Int, length: Int) = input.read(buffer, offset, length)
        override fun close() = input.close()
    }
}
mpv.command(arrayOf("loadfile", "local://$contentUri"))
```

Use it when mpv cannot open the source itself: your own HTTP client (authentication, caching), a
`ContentResolver`, data in memory. The provider and the streams usually run on mpv's threads; a
synchronous command that opens such a URI itself runs them on the calling thread. They must not
call the same `MPVLib` instance: that throws `IllegalStateException`. The KDoc in `MpvStreams.kt`
has the rules.

**Closing.** `MPVLib` implements `AutoCloseable`, and `close()` is `destroy()`, so `use` destroys
the instance when the block ends, also when it throws:

<!-- example:auto-close -->
```kotlin
import dev.jdtech.mpv.MPVLib

val version = MPVLib.create(context)?.use { mpv ->
    mpv.setOptionString("vo", "null")
    mpv.init()
    mpv.getPropertyString("mpv-version")
} // destroyed here, also if the block throws
```

**Logcat.** libplayer writes every mpv log message to logcat (tag `mpv`) by default.
`setLogcatLevel(level)`, with one of `MPVLib.MpvLogLevel`, reduces that echo, and
`MPV_LOG_LEVEL_NONE` stops it. The `LogObserver`s still get every message at the level set with
`requestLogMessages`. Verbose messages include the URLs mpv opens, so an app that plays
authenticated URLs can keep its log to itself:

<!-- example:logcat-level -->
```kotlin
import dev.jdtech.mpv.MPVLib
import dev.jdtech.mpv.setLogcatLevel

// keep only warnings and errors in logcat; the LogObservers still get every message
mpv.setLogcatLevel(MPVLib.MpvLogLevel.MPV_LOG_LEVEL_WARN)
mpv.addLogObserver(object : MPVLib.LogObserver {
    override fun logMessage(prefix: String, level: Int, text: String) {
        appLog += "[$prefix] $text"
    }
})
```

Every public function and type has KDoc, and releases after 1.3.0 publish a sources jar, so an IDE
shows both. The examples in this section are compiled with the unit tests
(`libmpv/src/test/java/dev/jdtech/mpv/docs/DocExamples.kt`), and `buildscripts/check-docs.sh`
checks that the README shows them unchanged. The exact public API is in
[`libmpv/api/libmpv.api`](libmpv/api/libmpv.api).

## Released and unreleased

The latest release is **1.4.0**, the first built by the new pipeline. Since 1.3.0 it has:

- **A JNI wrapper in Rust** ([ADR 0003](docs/adr/0003-rust-jni-wrapper.md)). The Kotlin API is
  unchanged. A call after `destroy()` throws `IllegalStateException` instead of touching freed
  memory, a call racing `destroy()` on another thread is safe, and a second `destroy()` does
  nothing. `destroy()` from an observer or log callback throws `IllegalStateException` (call it
  from another thread). A `null` string argument throws `NullPointerException`.
- **More of mpv's API** ([above](#more-of-mpvs-api-since-140), [ADR 0005](docs/adr/0005-api-on-top-of-mpvlib.md)):
  structured values (`MpvNode`), suspend functions on mpv's async API (also typed:
  `getDoublePropertyAsync` and the rest), flows of properties and events, custom protocols
  implemented in Kotlin, `unobserveProperty`, `AutoCloseable`, and `setLogcatLevel`. All additive.
  The event thread is named `mpv-event`, and the release publishes a sources jar. The
  library now depends on `kotlinx-coroutines-core`.
- **An mpv patch** that rounds the AudioTrack buffer to whole frames, so 22,050 Hz audio is no
  longer silent ([ephyra#659](https://github.com/midasvo/ephyra/issues/659)).
- **An FFmpeg patch** so https works when the server asks for an optional client certificate
  (Caddy `verify_if_given`, nginx `ssl_verify_client optional`) ([#56](https://github.com/midasvo/libmpv-android/issues/56)).
- **More checks in CI:** the dynamic dependencies of every library in every ABI, the wrapper's JNI
  exports against the Kotlin declarations, and instrumented tests on three emulators (Android 8.0,
  32-bit x86, and a current one) that play real files and subtitles, check which decoders, demuxers
  and protocols the build contains, and run an app that uses the library minified by R8.
- **A new build pipeline** ([ADR 0004](docs/adr/0004-build-pipeline.md)). Releases built by it also
  come with an SBOM and a provenance attestation.

`main` has nothing a user would notice beyond 1.4.0 yet.

## What is in the AAR

For each ABI: `libmpv.so` and the FFmpeg libraries as shared libraries (the other components are
statically linked into them), the wrapper `libplayer.so`, and the NDK's `libc++_shared.so`. The
components are listed under [Licensing](#licensing).

Versions are not repeated here, so they cannot go stale:

- **On `main`:** [`buildscripts/include/depinfo.sh`](buildscripts/include/depinfo.sh) is the one
  place every version is set. Patches to the sources are in [`buildscripts/patches/`](buildscripts/patches/).
- **In a release:** the release notes list every component version. Releases built by the new
  pipeline also list the tools they were built with and attach an SBOM, `libmpv-<version>.cdx.json`
  (CycloneDX) of the native components, the wrapper's Rust crates, and the JVM dependencies every
  app gets with the AAR (`kotlinx-coroutines-core`, `kotlin-stdlib`, from its POM).
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

To run the instrumented tests on an emulator or device without building the native libraries
(also on Windows, in Git Bash), `buildscripts/local-test.sh` takes them from a published AAR,
builds the wrapper for the device's ABI with cargo and the NDK, and runs the connected tests on
the device you name:

```sh
buildscripts/local-test.sh --serial emulator-5554          # native libraries of the latest release
buildscripts/local-test.sh --serial emulator-5554 --aar libmpv/build/outputs/aar/libmpv-release.aar --r8
```

It needs adb, the NDK version from `depinfo.sh`, a JDK, rustup and `gh` (for the download), and
does not start an emulator or install anything. Its header has the details.

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
