# ADR 0001: Keep the `dev.jdtech.mpv` Kotlin package

- **Status:** accepted
- **Date:** 2026-11-14
- **Context links:** midasvo/ephyra#654 (why the fork exists), #1 (fork epic), #5 (coordinates)

## Context

This fork publishes its own artifact, `io.github.midasvo:libmpv`, instead of upstream's
`dev.jdtech.mpv:libmpv`. That raised the question whether the Kotlin package should follow,
for example `dev.jdtech.mpv` → `io.github.midasvo.mpv`, so that the code also says it is ours.

Things that matter:

- **Ephyra did the same split.** It changed the identifier the outside world sees
  (`applicationId = "nl.midasvo.findroid.ce"`) but kept upstream Findroid's code package and
  `namespace` (`dev.jdtech.jellyfin`) to keep merging upstream cheap. The Maven coordinate is this
  library's equivalent of an application ID.
- **The native code is bound to the package.** The JNI wrapper in `libmpv/src/main/cpp`
  (`main.cpp`, `property.cpp`, `render.cpp`, `jni_utils.cpp`, `jni_utils.h`) names
  `dev/jdtech/mpv/MPVLib` in 37 places: the exported `Java_dev_jdtech_mpv_MPVLib_*` function
  names and the class lookups. A rename touches all of them, plus `MPVLib.kt`, the Gradle
  `namespace` and the ProGuard rules.
- **Those are the files upstream changes.** Wrapper features such as `requestLogMessages` in 1.1.0
  land in exactly these files. The fork follows upstream through a weekly merge PR
  (`sync-upstream.yaml`), and its cost depends on how few lines differ (see RELEASING.md).
- **Nobody sees the package.** Users see the Maven coordinate. The one case where both libraries
  meet, both on one app's classpath, is not fixed by a rename: both still ship `libmpv.so` and the
  FFmpeg libraries under the same file names, so the APK clashes either way.

## Decision

Keep the Kotlin package `dev.jdtech.mpv` and the Gradle `namespace` unchanged. The fork's identity
is its Maven coordinate `io.github.midasvo:libmpv`, its POM (URL, SCM, developers) and its
releases.

## Consequences

- Upstream wrapper changes merge without conflicts in the C++ and Kotlin sources.
- Apps switch from `dev.jdtech.mpv:libmpv` by changing only the dependency coordinate: no import
  changes (Ephyra's switch, midasvo/ephyra#655, changed no imports).
- An app must not depend on both artifacts at once. That was already impossible because of the
  native library names, and it doesn't need to be supported.
- Stack traces and crash reports show `dev.jdtech.mpv`. The library version (from the release
  notes, or the AAR) says which build it was.

## Alternatives considered

- **Rename to `io.github.midasvo.mpv` (or `nl.midasvo.mpv`).** About an hour of mechanical work,
  plus a major version (2.0.0) because it breaks every user's imports. Rejected: it turns most
  future upstream syncs into manual merges, for a purely cosmetic gain.
- **Rename only the Kotlin side, keep the JNI names through `RegisterNatives`.** Fewer generated
  names change, but it still rewrites the class lookups and adds code upstream doesn't have.
  Rejected for the same reason.

## Revisit when

- the fork stops following upstream (no more sync PRs), or
- the wrapper grows an API of its own that diverges from upstream's.

Then the merge cost is gone. Rename in a major release, and say so in the release notes.
