# Security

## Reporting a vulnerability

Please do not put the details of a vulnerability in a public issue or pull request.

Private vulnerability reporting is not enabled for this repository yet. Until it is, open an
[issue](https://github.com/midasvo/libmpv-android/issues/new) that says only that you have a
security report for this fork, and how to reach you. The maintainer will reply there and agree a
private channel with you before you send the details.

Include, once the channel is agreed:

- the version of `io.github.midasvo:libmpv` (or the commit), the ABI and the Android version;
- what an attacker controls (a media file, a stream URL, a subtitle, a call from the app) and what
  they get;
- a file or the steps that show it, and the mpv log (`adb logcat -s mpv`) if it helps.

## What is in scope here

- **The JNI wrapper**: `libplayer.so`, the Rust crate in `libmpv/src/main/rust`, and the Kotlin
  API in `libmpv/src/main/java` (handles, lifecycle, the custom stream protocols, string and node
  conversion).
- **The build and release pipeline**: the build scripts and patches in `buildscripts/`, the
  workflows in `.github/workflows/`, how dependencies are downloaded and verified, the signing and
  the provenance attestation.
- **The published AAR**: what it bundles and how it is built, for example a dependency left at a
  version with a known vulnerability, or a build option that turns a protection off.

## What belongs upstream

A vulnerability in mpv, FFmpeg or another bundled library is that project's to fix. Report it to
them, as their policy asks, and tell us here (publicly once it is public) so that the next release
picks up the fix:

- mpv: [its security policy](https://github.com/mpv-player/mpv/blob/master/SECURITY.md)
- FFmpeg: [its security page](https://ffmpeg.org/security.html)
- curl: [its security policy](https://github.com/curl/curl/blob/master/SECURITY.md)
- Mbed TLS: [its security policy](https://github.com/Mbed-TLS/mbedtls/blob/development/SECURITY.md)
- HarfBuzz: [its security policy](https://github.com/harfbuzz/harfbuzz/blob/main/SECURITY.md)
- libplacebo, dav1d, libass, FreeType, fontconfig, FriBidi, libxml2, Lua and libunibreak: the
  project's own tracker or contact; each release's SBOM (`libmpv-<version>.cdx.json`) names the
  source of every component.

The build scripts come from [jarnedemeulemeester/libmpv-android](https://github.com/jarnedemeulemeester/libmpv-android)
and [mpv-android](https://github.com/mpv-android/mpv-android). A problem in a part this fork shares
with them is in scope here too; say whether you have told them.

The Rust crates of the wrapper are checked against the RustSec advisory database every day
(`.github/workflows/audit.yaml`).

## How fixes ship

A version on Maven Central can never be replaced, so a fix always ships as a new release, with its
notes saying what was fixed and which versions are affected. Older versions are not patched.
Releases are signed and, after 1.3.0, carry a provenance attestation:
[RELEASING.md](RELEASING.md#verifying-a-release) shows how to check both.
