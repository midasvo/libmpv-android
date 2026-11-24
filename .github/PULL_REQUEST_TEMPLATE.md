## What and why

<!-- What changes, and why. Link the issue if there is one. -->

## Checklist

<!-- Tick what applies; say why for anything that does not. RELEASING.md has the details. -->

- [ ] **Public API:** `libmpv/api/libmpv.api` is unchanged, or updated on purpose with
  `buildscripts/check-api.sh --update`. A removed or changed line needs a major version (a class
  only gaining an interface does not).
- [ ] **Examples:** a changed or new README example is changed in
  `libmpv/src/test/java/dev/jdtech/mpv/docs/DocExamples.kt` first and copied into the README with
  its `<!-- example:<name> -->` marker (`buildscripts/check-docs.sh`).
- [ ] **Licences:** a dependency added, removed or relicensed is in the table in
  `buildscripts/sbom.sh` and in the README's licence table.
- [ ] **Dependency bumps:** `buildscripts/update-checksums.sh <name>` was run for curl, lua or
  libunibreak, and for a git tag, so `include/tag-commits.txt` has its commit.
- [ ] **Docs:** README, RELEASING.md or an ADR updated where this changes what they say.

## Tested

<!-- What you ran, and on what. Say what was not tested, and why. For example:
     cargo test, ./gradlew :libmpv:testDebugUnitTest,
     buildscripts/local-test.sh --serial <device> (API, ABI),
     a build of the native libraries, playback in an app. -->

- Tested:
- Not tested:
