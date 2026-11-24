# ADR 0004: One build definition, one job per ABI, Gradle only packages

- **Status:** accepted
- **Date:** 2026-11-24
- **Context links:** #49 (the change), ADR 0003

## Context

Up to and including 1.3.0, CI was one job that built the native libraries for the four ABIs one
after the other, then ran Gradle, which also compiled the C++ wrapper through CMake and the NDK.
`publish.yaml` repeated that job's steps by hand, so a release ran a copy of the build that pull
requests never ran. Every pull request paid for the whole native build, even one that only changed
Kotlin or a workflow.

What has to stay true:

- The AAR that is released is the AAR that was checked and tested, byte for byte. Maven Central
  never lets a version be replaced.
- The source tarball of a release is the complete corresponding source of that build (GPL).
- A release is built from scratch, from pinned inputs, and says what it was built with.
- `main` has a branch ruleset that requires one check named `Build`.

## Decision

`.github/workflows/build.yaml` is the only build definition. Its header comment shows the jobs:

- **`lint`** runs shellcheck on the scripts and actionlint on the workflows before anything
  expensive starts. (Added later, with the jobs below that say so.)
- **`native`, one job per ABI, in parallel.** Each runs `build.sh --arch <arch> mpv` and hands on
  the shared libraries of its prefix, a hash of the sources it built from
  (`include/hash-sources.sh`) and the versions of the host tools.
- **`assemble`** builds the Rust wrapper for each ABI with `build.sh -n --arch <arch> player`.
  `scripts/player.sh` puts everything the AAR ships for that ABI, stripped, into
  `libmpv/src/main/jniLibs/<abi>`. Gradle then only packages: no CMake, no NDK, no native code. The
  job writes the SBOM (`sbom.sh`) and runs the checks that need no device: the wrapper's `cargo`
  checks and host tests, `check-aar.sh`, `check-jni.sh`, the API check, the JVM unit tests and
  `check-docs.sh` (RELEASING.md lists them).
- **`test`** runs the instrumented tests on three emulators: API 34 x86_64 (which also runs the R8
  test app), API 26 x86_64 (minSdk) and API 29 x86 (32-bit). Each loads only its own ABI.
- **`reproducible`** bundles the AAR again from the same `jniLibs` in a fresh job and fails unless
  the bytes match. It was part of `test` at first; on its own, its failure no longer hides the test
  results.
- **`sources`** packs the corresponding source, with the Rust crates vendored, and fails unless its
  sources hash the same as what every ABI was built from.
- **`Build`** runs always and is green only if every job above succeeded. It is the required check.

`publish.yaml` calls `build.yaml` with `release: true` and adds only what a release needs on top:
signing, the provenance attestation, the draft GitHub release and the Maven Central upload. It
bundles the AAR once more to sign it and refuses to continue unless the bytes are those of the
tested AAR. Run by hand, it is a dry run that uses no environment and no secret.

The cache rules:

- Each ABI's native build is cached. The key holds `NATIVE_CACHE_EPOCH`, the ABI, the runner OS,
  the NDK and meson versions, and a hash of everything that decides the native libraries:
  `buildscripts/include/**`, `build.sh`, `download.sh`, `patch.sh`, `scripts/**` (except
  `player.sh` and `mpv-android.sh`, which run after the prefix is built) and `patches/**`. A change
  to anything else (the wrapper, Kotlin, Gradle, the workflows) restores the cache and skips the
  native build.
- Only what later jobs use is cached: `prefix/<abi>/lib/*.so`, `sources.sha256` and `tools.txt`.
  The two text files describe the build that filled the cache, not the run that restored it.
- Release builds never read or write any cache, the Gradle cache included.
- `NATIVE_CACHE_EPOCH` in `build.yaml` throws every cached native build away when it is bumped.

## Consequences

- A pull request that does not touch the native build skips it. One that does builds the four ABIs
  at the same time. Measured before the extra emulators were added: about 11 to 12 minutes for a
  whole run without the native cache, about 6 with it.
- A release builds the same jobs as every pull request, with no cache. A failure that only a release
  would show can only come from the release-only steps in `publish.yaml`. The dry run exercises all
  of those except the attestation, the draft release and the upload.
- The reproducibility check runs on every build, so a change that makes the AAR non-reproducible
  fails its pull request, not a tag.
- The cache key does not cover the exact runner image version or the host tool versions that come
  with it (autoconf, nasm and the rest from apt). A runner image update can change those without a
  rebuild. Every release rebuilds without the cache, and its notes list the tool versions.
- If an upstream project moves a tag, a cached build no longer matches what a fresh download gets.
  The download step catches that first (`include/tag-commits.txt` records each tag's commit), and
  the `sources` job as a second line. Either says to bump `NATIVE_CACHE_EPOCH`.
- Building the wrapper outside Gradle means Android Studio cannot rebuild it. After changing the
  wrapper, run `./build.sh -n` (`buildscripts/README.md`).
- The jobs hand files to each other as artifacts, which do not keep file modes. `player.sh`,
  the `reproducible` job and `publish.yaml` all set the same mode (0644), or the AAR would differ.

## Alternatives considered

- **Keep one job and cache it.** Simpler, but every cache miss still builds four ABIs in sequence,
  and a release would still take the longest path.
- **Keep CMake in Gradle for the wrapper.** It worked for C++. For Rust it needs a Gradle plugin or
  a custom task calling cargo, Gradle would need the NDK again, and the wrapper would be built in a
  different way locally and in the per-ABI jobs. A shell script that the build scripts call, like
  every other component, is one way for both.
- **A separate release workflow** with its own steps, as before. Rejected: it is how the release
  path drifted from what pull requests test.
- **A custom container image with the toolchain.** It would fix the host tool versions too, but it
  has to be built, published, versioned and updated, and its own sources become part of what a
  release depends on. The jobs also rely on what the GitHub runner provides: the preinstalled
  Android SDK (`download-sdk.sh` links it with `IN_CI=1`) and KVM for the emulator. The runner image
  is pinned (`ubuntu-26.04` now; it was `ubuntu-24.04` when this was decided), meson is pinned in
  `depinfo.sh`, and the release notes record the rest. The cache already removes most of the time a container would save.

## Revisit when

- A runner image update breaks or changes the native build in a way the pinned versions do not
  catch: then pinning the host tools (or a container) is worth its cost.
- The per-ABI jobs stop being faster than one job would be, or the cache misses most of the time.
