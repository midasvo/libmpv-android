# Releasing

This fork publishes `io.github.midasvo:libmpv` to Maven Central for [Ephyra](https://github.com/midasvo/ephyra).
It started as a copy of [jarnedemeulemeester/libmpv-android](https://github.com/jarnedemeulemeester/libmpv-android)
with a few changes. It now has its own JNI wrapper (Rust, [ADR 0003](docs/adr/0003-rust-jni-wrapper.md))
and its own build pipeline ([ADR 0004](docs/adr/0004-build-pipeline.md)), and still takes upstream's
changes through a weekly merge.

## Keeping up with upstream

Upstream is where most dependency bumps and build script fixes come from, and it is worth
following. There is no rule any more to keep the diff to upstream small: where the fork is better
off with its own version of a file, it has one, and the sync is resolved by hand. Prefer upstream's
layout where there is no reason to differ, because that part of the sync stays free.

`sync-upstream.yaml` runs every Monday, or by hand: **Actions → Sync upstream → Run workflow**.

- If upstream has new commits and they merge cleanly, it merges them into the `upstream-sync`
  branch, opens or updates a PR to `main`, and starts `build.yaml` on it.
- If the merge conflicts, it opens an issue titled "Upstream sync needs a manual merge" that lists
  the files (see "Upstream conflicts" below).
- Merge the PR with **Create a merge commit**, never squash or rebase, so upstream's history stays
  an ancestor of `main` and the next sync only brings what is new.

A clean merge is not a correct one. Read the upstream commits listed in the PR, and check each
against the table below: some upstream changes merge without a conflict but do not apply here.

| Upstream changes | What to do here |
|---|---|
| A version in `buildscripts/include/depinfo.sh` | Usually merges. If it conflicts, keep the higher version on each line, and its `sha256_*`. Upstream's `v_cmake` does not exist here (Gradle runs no CMake): drop it. |
| `libmpv/src/main/cpp/**` (its C++ wrapper) | That directory does not exist here, so git reports a modify/delete conflict. Read the change, port it to the Rust crate in `libmpv/src/main/rust` by hand (or decide that it does not apply, and say why in the merge commit), then keep the directory deleted with `git rm -r libmpv/src/main/cpp`. Add a test for the behaviour if it has none. |
| `MPVLib.kt` | Merge by hand; ours also holds the runtime of the new API (`runtime`, `handle()`, the `on*` methods libplayer calls) and implements `AutoCloseable`. A new `external fun` needs a matching export in `player/src/exports.rs` (same `Java_dev_jdtech_mpv_MPVLib_*` name and JNI signature): `buildscripts/check-jni.sh` fails the build until it has one. |
| `buildscripts/scripts/<name>.sh`, `build.sh`, `download-deps.sh`, `download-sdk.sh` | Usually merges. The fork has removed the lines that symlink libraries into `jniLibs` (`ln -sf ... "$native_dir"` in `mpv.sh` and `ffmpeg.sh`) and the CMake install: `scripts/player.sh` fills `jniLibs`. Keep them out if upstream touches them. |
| A new dependency (a new `v_*`, script and `dep_*` entry) | Add it to the component table in `buildscripts/sbom.sh` (which fails on a `v_*` it does not know) and to the licence table in `README.md` (`buildscripts/check-docs.sh` fails until it is there). If it is fetched as a tarball, it needs a `sha256_*` too. |
| `libmpv/build.gradle.kts` | Keep ours: our `coordinates(...)` and `pom { }`, no `externalNativeBuild`. The usual conflict is upstream bumping `version = "x.y.z"` in `coordinates(...)`: keep our line, which reads `VERSION_NAME`. |
| `.github/workflows/build.yaml`, `publish.yaml` | Ours have a different structure. Take the intent of the change (a newer action, a new step) and write it into our jobs by hand. |
| `README.md` | Keep ours. |
| `renovate.json` | Take upstream's new managers, keep our two (meson, curl). |
| Gradle, the Gradle wrapper, `gradle/libs.versions.toml` | Usually merge. Renovate bumps them on both sides. |
| `settings.gradle.kts`, the root `build.gradle.kts`, `gradle.properties` | Keep our additions: `include(":r8-test")` (the R8 test app), the Android application plugin it needs, and the Gradle JVM arguments (`org.gradle.jvmargs`, which R8 needs for its Metaspace). |

A change to anything in the native cache key (see "The native build cache") makes the sync PR
rebuild all four ABIs.

### Upstream conflicts

```sh
git fetch https://github.com/jarnedemeulemeester/libmpv-android.git main
git switch -c upstream-sync origin/main
git merge FETCH_HEAD
git status            # "deleted by us" marks upstream changes to files the fork removed
# resolve each file as the table above says, for example after porting a wrapper change:
git rm -r libmpv/src/main/cpp
git commit            # the merge commit; say what was ported or left out
git push -u origin upstream-sync   # then open a PR, merge it with a merge commit
```

GitHub deletes a pull request's branch when it is merged (a repository setting), so the next sync
starts a fresh `upstream-sync`.

A PR opened by a person runs `build.yaml` as usual. Close the conflict issue when it is merged.

## What the fork changes

By area, compared with upstream:

- **Identity and publishing.** `libmpv/build.gradle.kts`: `coordinates(...)` (the version comes
  from `-PVERSION_NAME`) and the `pom { }` block. The Kotlin package stays `dev.jdtech.mpv`
  ([ADR 0001](docs/adr/0001-keep-the-kotlin-package.md)). Every release attaches its corresponding
  source.
- **The wrapper.** `libplayer.so` is the Rust crate in `libmpv/src/main/rust`
  ([ADR 0003](docs/adr/0003-rust-jni-wrapper.md)); upstream's `libmpv/src/main/cpp` is deleted.
  `MPVLib.kt` adds `commandWithResult()` (returns the mpv error code; `command()` stays `Unit` for
  binary compatibility) and `getPropertyLong`, keeps observers in copy-on-write lists, logs an
  observer that throws instead of stopping the others, still delivers `MPV_EVENT_END_FILE` to
  `event(int)` after upstream's `eventEndFile(reason, error)` (upstream sends it only to
  `eventEndFile`, which would break observers such as Ephyra's player), and documents the threading
  rules, including that `destroy()` from a callback throws. It also holds the per-instance runtime
  of the new API.
- **The API on top of `MPVLib`** ([ADR 0005](docs/adr/0005-api-on-top-of-mpvlib.md)): structured
  values, suspend functions, flows and custom protocols, as extension functions in new files
  (`MpvNode.kt`, `MpvNodes.kt`, `MpvAsync.kt`, `MpvFlows.kt`, `MpvPlayerEvent.kt`, `MpvStreams.kt`,
  `MpvException.kt`, `internal/`), with `kotlinx-coroutines-core` as an `api` dependency and
  ProGuard rules for the classes libplayer calls by name. The public API is checked against
  `libmpv/api/libmpv.api`.
- **The native build.** `buildscripts/patches/mpv/`: mpv source patches, applied by `patch.sh` with
  `git apply`. `0001-ao_audiotrack-align-buffer-to-frames.patch` rounds the AudioTrack buffer to whole
  frames, so 22,050 Hz audio is no longer silent ([ephyra#659](https://github.com/midasvo/ephyra/issues/659));
  drop it once a released mpv includes a fix. `scripts/mpv.sh` builds with `-Dbuild-date=false`, so
  `libmpv.so` is reproducible. `depinfo.sh` and `download-deps.sh`: sha256-verified tarballs, the
  curl version format (`8_23_0`), a pinned gas-preprocessor and a pinned meson (`v_meson`, installed
  by `include/install-system-deps.sh`). `scripts/player.sh` builds the wrapper and fills `jniLibs`;
  `scripts/mpv-android.sh` only runs Gradle. `include/tag-commits.txt` records the commit of every
  dependency tag. New scripts: `api-diff.sh`, `check-aar.sh`, `check-api.sh`, `check-docs.sh`,
  `check-jni.sh`, `local-test.sh`, `sbom.sh`, `site-data.sh`, `test-summary.sh`,
  `update-checksums.sh`, `include/hash-sources.sh`.
- **CI and releases.** `build.yaml` and `publish.yaml` ([ADR 0004](docs/adr/0004-build-pipeline.md)),
  `audit.yaml`, `sync-upstream.yaml`, `pages.yaml`, `.github/actionlint.yaml`, the `.shellcheckrc`
  files, `.gitattributes`, the issue and pull request templates and `SECURITY.md`. `renovate.json`
  adds managers for meson and curl and holds Lua below 5.3 and mbedtls below 4.
- **Tests.** The instrumented tests in `libmpv/src/androidTest` (lifecycle, playback of generated
  files and subtitle fixtures, what the build contains, the node, async, flow, stream and logging
  API, calls from the main thread), the JVM unit tests in `libmpv/src/test` (including
  `docs/DocExamples.kt`, the README's examples), the `r8-test` module (an app that uses the library
  minified by R8), and their dependencies in `gradle/libs.versions.toml`. The wrapper's host tests
  are in the crate (`cargo test`).
- **Documentation and site.** `README.md`, this file, `docs/adr/`, `buildscripts/README.md`, and
  `site/` (the landing page at <https://midasvo.nl/libmpv-android/>).

## Checks on every build

`build.yaml` runs these on every pull request, on `main` and in every release build. A release
cannot pass with one of them failing. The jobs: `lint`, then `native` (one per ABI), then
`assemble`, then `test` (three emulators) and `reproducible`; `sources` after `native`; and `Build`,
which needs all of them. Every job writes what it checked to its job summary (test counts, the
native cache, the AAR's SHA-256, `libplayer.so` sizes).

| Job | Check | What it catches |
|---|---|---|
| Lint | `renovate.json` parses; shellcheck on every `*.sh`; actionlint on the workflows | shell errors (they fail; warnings are only reported for now), workflow mistakes |
| Assemble | `cargo fmt --check`, `cargo clippy` (host and Android), `cargo test` in `libmpv/src/main/rust` | formatting, lints and the wrapper's host unit tests |
| Assemble | `buildscripts/check-aar.sh` | a missing library, a system dependency not on its allowlist, 64-bit libraries not aligned for 16 KiB pages, a `libplayer.so` above its size limit for the ABI (`libplayer_max_kib`) |
| Assemble | `buildscripts/check-jni.sh` | an `external fun` without its `Java_*` export in `libplayer.so`, or an export nothing declares, in any ABI |
| Assemble | `buildscripts/check-api.sh` | a change to the public API that `libmpv/api/libmpv.api` does not show (next section) |
| Assemble | `./gradlew :libmpv:testDebugUnitTest` | the JVM unit tests of the Kotlin code that needs neither JNI nor mpv; it also compiles the README's examples |
| Assemble | the SBOM check, and `buildscripts/check-docs.sh` | a component without version, source or licence; a README licence table that disagrees with `sbom.sh`; a README example that differs from its compiled region |
| Instrumented tests | `connectedDebugAndroidTest` on API 34 x86_64, API 26 x86_64 (minSdk) and API 29 x86 (32-bit); on API 34 also `:r8-test:connectedReleaseAndroidTest` | the wrapper and the API against a real mpv, on the oldest supported Android and on a 32-bit ABI; a consumer ProGuard rule missing under R8. The job also fails when the reports hold a failure or no tests, even if Gradle succeeded |
| Reproducible AAR | the AAR bundled again from the same `jniLibs` | an AAR that is not byte for byte reproducible, which `publish.yaml` would refuse |
| Corresponding source | the source tarball hashed against every ABI's build | a tarball that is not the source of the build |
| Build | all of the above succeeded | the required check on `main` |

Outside `build.yaml`: `audit.yaml` checks the wrapper's `Cargo.lock` against the RustSec advisory
database daily and when the crates change. It is not part of `Build` and does not block a pull
request or a release; a red run means an advisory to look at.

To run the instrumented tests without CI, on an emulator or device you name, use
`buildscripts/local-test.sh` (see its header, and "Building from source" in the README).

## The API contract

`libmpv/api/libmpv.api` is the public API of the library: every public class, method and field
of the release AAR's classes, as `javap` prints them, sorted (`dev.jdtech.mpv.internal` and
compiler-generated names left out). `buildscripts/check-api.sh` dumps the AAR that CI just built and
fails with a diff when it differs from the file.

- **Added lines** are new API: compatible, a minor version.
- **A class header line that changes only by gaining an interface** (as
  `public final class dev.jdtech.mpv.MPVLib implements java.lang.AutoCloseable` did) is an addition
  too: a minor version. `api-diff.sh` counts it as a changed line and calls it breaking, and
  `publish.yaml` then warns that the major version did not change; check the line and go on.
- **Any other removed or changed line**, header lines included, breaks apps compiled against an
  earlier release: a major version, by the rule under "Cutting a release". Prefer to add and
  deprecate instead.
- **To accept a deliberate change,** build the AAR and update the file in the same pull request,
  so the diff shows up in review:
  ```sh
  ./gradlew :libmpv:assembleRelease
  ./buildscripts/check-api.sh --update
  git diff libmpv/api/libmpv.api
  ```
  It needs a JDK (`javap` and `jar`). The dump depends only on the Kotlin and Java classes, so an
  AAR built without native libraries gives the same file. `.gitattributes` checks the dump out with
  LF line endings everywhere, Windows included.

The release notes get an API section from `buildscripts/api-diff.sh`: the lines added and removed
since the previous release tag. The first release after 1.3.0 has nothing to compare with (1.3.0
has no dump), and says so.

The README's Kotlin examples are held to the API too. Each is a region of
`libmpv/src/test/java/dev/jdtech/mpv/docs/DocExamples.kt`, compiled by the unit tests, and the
README marks it with `<!-- example:<name> -->` on the line before the block. `check-docs.sh` fails
when a block and its region differ, when a Kotlin block has no marker, or when a marker names no
region. Change an example in `DocExamples.kt` first, then copy it into the README. A block that is
not checked on purpose (the Gradle snippet, the 1.3.0 example) is marked
`<!-- example-unchecked: <why> -->`.

An upstream sync that changes `MPVLib`'s public methods changes the dump too: decide whether to
take the change, and what it means for the version, before updating the file.

## Renovate

Renovate runs on this fork with upstream's `renovate.json` plus two managers: meson, which only
this fork pins, and curl, which upstream does not track (curl's tags look like `curl-8_23_0`, so
`depinfo.sh` stores `8_23_0`). Upstream runs its own config, so most dependency bumps arrive twice:
once from Renovate here, once through the sync PR.

- **Checksums and tag records.** A bump of curl, lua or libunibreak fails at the download step
  until the matching `sha256_*` in `depinfo.sh` is updated. Run
  `buildscripts/update-checksums.sh <name>` on the Renovate branch and commit the result: it
  downloads the tarball, checks it against what the project publishes (lua.org's checksum list, the
  GitHub release's asset digest for libunibreak, curl's GPG signature) and writes the new checksum.
  For a git-tag dependency it records the commit the new tag resolves to in
  `include/tag-commits.txt`; a bumped tag without a record only warns, so run it for those too.
  `update-checksums.sh --check` changes nothing and fails when a record is out of date.
- **A tag that moved.** If an upstream moves a recorded tag, the download fails with "tag ... is
  commit X, but include/tag-commits.txt recorded Y". Find out why before trusting it. To accept the
  new commit, run `update-checksums.sh --accept-moved-tags <name>` and bump `NATIVE_CACHE_EPOCH`
  (cached builds were made from the old commit).
- **Held back on purpose:** Lua stays below 5.3 (mpv supports 5.1 and 5.2 only) and mbedtls below 4
  (`packageRules` in `renovate.json`).
- **meson** (`v_meson`, from PyPI) is installed with `pip install meson==<version>`. CI fails if a
  different meson ends up on the `PATH`. meson is part of the cache key, so a bump rebuilds all four
  ABIs.
- **Rust crates.** Renovate's built-in Cargo manager proposes bumps of `Cargo.toml` and
  `Cargo.lock`, which pins every crate with its checksum (the build uses `--locked`). A crate bump
  does not touch the native cache: only the wrapper is rebuilt. The new crate lands in the SBOM and
  in the vendored source of the next release.
- **The Rust toolchain and the runner image.** Renovate also bumps the channel in
  `libmpv/src/main/rust/rust-toolchain.toml` and the `runs-on` image in the workflows. Neither is in
  the native cache key by name, but the runner image is (`ImageOS`), so an image bump rebuilds all
  four ABIs. Check the wrapper's size and the host tests after a toolchain bump, and the tool
  versions in the build log after an image bump.
- **Not tracked:** the NDK, SDK and platform versions in `depinfo.sh`. Upstream's bumps of those
  arrive through the sync.
- **The same version on both sides** merges without a conflict, whichever lands first. **Different
  versions** (Renovate here is a release ahead) conflict on one line of `depinfo.sh`: keep the
  higher one.
- **Review Renovate PRs here like any change:** CI builds all four ABIs and runs the instrumented
  tests. A bump of mpv, FFmpeg or libplacebo is a release candidate: read its notes for the player
  (see Ephyra's `libmpv-release` skill). For mpv, also check `libmpv/src/main/rust/player/src/ffi.rs`
  against the new `mpv/client.h`: those declarations are written by hand.

## The native build cache

Each ABI's native build (`build.sh --arch <arch> mpv`) is cached by `build.yaml`. The key is
`native-<NATIVE_CACHE_EPOCH>-<abi>-<runner OS>-ndk<v_ndk>-meson<v_meson>-<hash>`, where the hash
covers `buildscripts/include/**`, `build.sh`, `download.sh`, `patch.sh`, `scripts/**` (except
`player.sh` and `mpv-android.sh`) and `patches/**`. Anything else, such as the wrapper, Kotlin,
Gradle or the workflows, does not touch the native build.

- Only `prefix/<abi>/lib/*.so`, `sources.sha256` and `tools.txt` are cached. The last two describe
  the build that filled the cache.
- Release builds never read or write the cache. Pull requests see their own caches and `main`'s.
- The `sources` job hashes a fresh download of the sources and fails if any ABI was built from
  something else. A moved tag is normally caught before that, by `include/tag-commits.txt` in the
  download step (see "Renovate").
- **`NATIVE_CACHE_EPOCH`** (in `build.yaml`'s `env`) is part of every key. Bump it to throw every
  cached native build away: when the `sources` job says so, when an upstream re-tagged a release, or
  when you want a full rebuild on the current runner image (its host tools are not in the key).

## Cutting a release

Versions are our own semver, starting at `1.1.0`. Raise the major version for a breaking API change
(a line removed or changed in `libmpv/api/libmpv.api`, or a change in behaviour that existing code
relies on), the minor version for new API (lines only added to the dump) or dependency bumps, and
the patch version for build or wrapper fixes. Each release's notes list the exact component
versions. `git diff vA.B.C -- libmpv/api/libmpv.api` against the last release shows which it is.

1. Make sure `main` is green, and merge a pending upstream sync if you want it in.
2. Rehearse it. A dry run of version 0.0.0 runs every Wednesday at 06:00 UTC on `main`; check that
   the last one passed, or start one with the real version: **Actions → Publish → Run workflow**,
   or
   ```sh
   gh workflow run publish.yaml -R midasvo/libmpv-android --ref main -f version=X.Y.Z
   ```
   A dry run runs every step of a release with the same commands, except that it signs with a
   throwaway key, publishes to the runner's local Maven repository, and creates no attestation and
   no release. It uses no environment and no secret. The version must not be on Maven Central yet.
   Read the release notes in the run's summary.
3. Tag and push the version. Maven Central decides what "already released" means (`publish.yaml`
   refuses those), so take the last version from there, not from the GitHub releases, which can
   include a draft from a failed run:
   ```sh
   curl -s https://repo1.maven.org/maven2/io/github/midasvo/libmpv/maven-metadata.xml | grep '<release>'
   git switch main && git pull
   git tag -a vX.Y.Z -m "libmpv X.Y.Z"
   git push origin vX.Y.Z
   ```
4. `publish.yaml` runs:
   - **Check tag** refuses tags that are not `vX.Y.Z`, whose commit is not on `main`, or whose
     version is already on Maven Central.
   - **Build** is `build.yaml` with `release: true`: the same jobs as every pull request, without any
     cache. It bundles, checks and tests the AAR, writes the SBOM and packs the corresponding source.
   - **Publish** runs in the `release` environment (only `v*` tags can deploy). It bundles the AAR
     again from the same `jniLibs` to sign it and stops unless the bytes are those of the tested AAR.
     It attests the build provenance of the AAR and the source tarball, writes the release notes
     (component and tool versions, the API changes since the previous release from `api-diff.sh`,
     the licence and how to verify),
     and creates the **draft** GitHub release with `libmpv-<version>.aar`, its `.asc`, the SBOM
     `libmpv-<version>.cdx.json` and `libmpv-<version>-sources.tar.xz`. Only then does it upload to
     Maven Central with automatic release, and it fails if the uploaded AAR is not the one on the
     draft. If a run fails before the upload, re-run it: it finds its own draft and updates it.
5. Check the deployment on [central.sonatype.com → Deployments](https://central.sonatype.com/publishing/deployments).
   It appears on `repo1.maven.org` within about 30 minutes, and in search later.
6. Check the draft release notes, add what changed for the player, and publish the release.
7. Verify it as a user would (next section).
8. Update the "Released and unreleased" section of `README.md` and the release text on the site
   (`site/index.html`, marked `release-text`, including its `data-written-for` version). Once the
   new API is released, drop "on `main`" from its README heading and the site's `#api` block, and
   say so in the release notes. The site's version numbers and component table are
   generated: they follow Maven Central on the next deploy of the site, which runs daily, on a push
   to `site/`, or by hand with `gh workflow run pages.yaml -R midasvo/libmpv-android --ref main`.
9. Bump Ephyra: `libmpv` in `gradle/libs.versions.toml`, then run `docs/mpv-checklist.md` there.

A published version can never be replaced on Maven Central. If something is wrong, release the next
patch version.

**Not proven yet.** No release has been made with this pipeline; 1.3.0 was the last one made with the
old one. The dry run covers the build, the signing, the release notes and the Maven publication
(to the runner's local repository); it ran once on `main`, on 2026-11-24, and passed. What only a real release exercises: the `release` environment and its secrets, the
provenance attestation, `gh release create` of the draft, and the upload to Maven Central. Watch the
first release closely and check the attestation (step 7).

## Verifying a release

Each release attaches the AAR under its Maven Central name, `libmpv-<version>.aar`, with its
signature. The AAR is the same file as on Maven Central, and its sha256 is in the release notes.

```sh
gh release download vX.Y.Z -R midasvo/libmpv-android
sha256sum libmpv-X.Y.Z.aar   # compare with the release notes

# the signature (every release)
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys DD02C554A0EBAF24B72B68B0D84A55E8003C6AB5
gpg --verify libmpv-X.Y.Z.aar.asc libmpv-X.Y.Z.aar

# the provenance (releases after 1.3.0)
gh attestation verify libmpv-X.Y.Z.aar -R midasvo/libmpv-android \
  --signer-workflow midasvo/libmpv-android/.github/workflows/publish.yaml
gh attestation verify libmpv-X.Y.Z-sources.tar.xz -R midasvo/libmpv-android \
  --signer-workflow midasvo/libmpv-android/.github/workflows/publish.yaml
```

`gpg --verify` must report a good signature from `libmpv-android release signing (midasvo)` with the
fingerprint `DD02 C554 A0EB AF24 B72B 68B0 D84A 55E8 003C 6AB5`. The attestation says that the file
was built by `publish.yaml` in this repository, and from which commit and tag. It is checked by the
file's digest, so it works the same for the AAR downloaded from Maven Central.

Releases after 1.3.0 also attach `libmpv-<version>.cdx.json`, a CycloneDX SBOM of the bundled
components: each with version, licence and source (git tag or tarball checksum), the patches applied
to it, and the Rust crates of the wrapper. It also lists the JVM dependencies every app gets with
the AAR (`kotlinx-coroutines-core`, `kotlin-stdlib`), read from the POM Gradle generates for Maven
Central; those are not bundled in the AAR.

## The landing page

`site/` is deployed to <https://midasvo.nl/libmpv-android/> by `pages.yaml`, only from `main`. The
page is static. Its version numbers and the component table come from `site/data.js`, which
`pages.yaml` generates with `buildscripts/site-data.sh` at deploy time: the versions on `main` from
`depinfo.sh`, the versions of the latest release on Maven Central from `depinfo.sh` at its tag, and
the licences from `sbom.sh`. `data.js` is not committed. Without it, for example when you open
`site/index.html` from disk, the page shows links instead of versions. To see it with data:

```sh
buildscripts/site-data.sh > site/data.js   # ignored by git (site/.gitignore)
```

`pages.yaml` has two jobs. The first generates the site; on the daily run it compares every file
with the deployed one and stops there when nothing changed. The second deploys, and retries once
after 30 seconds if the deployment fails. A push to `site/` (or to the files the data comes from)
and a run by hand always deploy.

The release text in `index.html` (what is in the latest release and what is only on `main`) is
written by hand; update it with each release (step 8). It names the release it was written for in
`data-written-for`. When `data.js` names a newer release, the page hides that text and links to the
new release's notes instead, so the page never describes an old release as the latest.

## Secrets and signing key

The `release` environment holds:

| Secret | What |
|---|---|
| `OSSRH_USERNAME`, `OSSRH_PASSWORD` | Central Portal user token (the names date from OSSRH) |
| `SIGNING_KEY` | ASCII-armoured private key |
| `SIGNING_KEY_ID` | Last 8 hex digits of the fingerprint |
| `SIGNING_PASSWORD` | Key passphrase |

The dry run uses none of them. The provenance attestation needs no secret: it uses the job's OIDC
token (`id-token: write`).

The signing key is a dedicated RSA-4096 key with the user ID `libmpv-android release signing (midasvo)`,
fingerprint `DD02 C554 A0EB AF24 B72B 68B0 D84A 55E8 003C 6AB5`, and a three-year expiry. It is
published on keyserver.ubuntu.com and keys.openpgp.org; `gpg --keyserver hkps://keyserver.ubuntu.com --search-keys "libmpv-android release signing"`
shows the fingerprint and expiry. The private key, passphrase and revocation certificate are kept
offline by the maintainer, not in this repository.

When generating a key on Windows, strip `\r` as well as `\n` from generated passphrases: Git Bash's
MinGW `openssl` ends its output with CRLF, and a passphrase ending in `\r` does not survive the trip
into an Actions secret (signing then fails with `checksum mismatch`).

**Rotating the key:** create a new key, upload the public key to both keyservers, and replace the
three `SIGNING_*` secrets. Update the fingerprint in this file and in `README.md`. Old releases stay
valid. To extend instead, run `gpg --quick-set-expire <fpr> 3y`, then re-upload the public key and
re-export `SIGNING_KEY`. If the key leaks, publish the revocation certificate to the keyservers and
rotate.

**Rotating the Portal token:** generate a new user token on central.sonatype.com and replace
`OSSRH_USERNAME` and `OSSRH_PASSWORD`.

## Licence

The AAR is effectively GPL-3.0-or-later. Every release attaches its complete corresponding source
(`libmpv-<version>-sources.tar.xz`: this repository at the tag plus every dependency as downloaded
and patched; for releases after 1.3.0 also the wrapper's Rust crates, vendored under
`libmpv/src/main/rust/vendor`). CI checks on every build that the tarball holds exactly the sources
each ABI was built from. Keep it that way. Apps that ship the AAR, Ephyra included, must show the
licences and point to that source.

The licence of each component is set once, in the table at the top of `buildscripts/sbom.sh`. The
licence table in `README.md` repeats it for readers; `buildscripts/check-docs.sh` fails when the two
disagree. When a component is added or relicensed, update both.
